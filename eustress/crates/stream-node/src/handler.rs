//! Per-connection async task: reads `ClientFrame`s, dispatches to the
//! embedded `EustressStream`, and writes `ServerFrame` responses.
//!
//! Every connection logs in first: TLS 1.3, then `Challenge`, `Hello`, and
//! `Welcome` or `AuthFailed` ([`handle_connection`]). Dispatch then holds the
//! party to its grants and its subscription quota. QUIC runs its own login on
//! the stream it opens and then the same dispatcher ([`serve_party`]), so the
//! grant and quota checks live in one place for both transports.
//!
//! Reserved topics (`scene_deltas`, `agent_commands`) also need `admin = true`
//! to publish. That decides who may publish a scene delta at all; the
//! materializer that writes a Space to disk keeps its own `space://` path
//! checks on whatever a delta asks it to write.
//!
//! Requests that expect a reply are handled one at a time, in arrival order:
//! `run` dispatches each frame to completion, its reply written, before it
//! reads the next. Each gets exactly one reply. Only `Message` frames, replays
//! and live ones, are written from a subscription's forwarding task.
//!
//! Every write to the peer, a reply or a forwarded message, has a 10-second
//! deadline that covers waiting for the writer as well as the write; a write
//! that misses it closes the connection. The read and the dispatch are both
//! raced against the party's revocation, so a revoked party loses a
//! connection even while its dispatch waits on a peer that stopped reading.
//! On each reload the connection drops every subscription its party is no
//! longer granted.
//!
//! A subscription that ends on a live connection stops its forwarding task
//! through a stop flag, never by aborting it, since an aborted write can leave
//! half a frame on the wire. The task checks the flag after it has taken the
//! writer lock and before it writes: every frame it writes goes out before a
//! later `Subscribed`, which needs the same lock, and the client drops such a
//! frame as unregistered.
//!
//! A refused fire-and-forget publish sends nothing back: its sender reads no
//! reply, so an `Error` would be taken for the answer to its next
//! acknowledged request. The refusal is logged, rate-limited per source, and
//! counted: more than 100 inside one fixed 60-second window close the
//! connection, with one warn line that names the party.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::sync::{watch, Notify};
use tokio::task::AbortHandle;
use tokio::time::Instant;
use tracing::{debug, info, warn};

use eustress_stream::{EustressStream, OwnedMessage, SubscriberId};

use crate::auth::{
    self, Channel, HelloOutcome, NodeAuth, Party, PreLoginPermit, QuotaSlot, SessionGuard, ERR_BAD_REQUEST,
    ERR_FORBIDDEN, ERR_QUOTA, ERR_TOPIC_FULL,
};
use crate::error::NodeError;
use crate::protocol::{read_client_frame, write_server_frame, ClientFrame, ServerFrame, TopicStats};
use crate::transport;

/// What every connection on one node shares.
pub struct ConnectionContext {
    /// The node's in-process stream.
    pub stream: EustressStream,
    /// The node's authentication and authorization state.
    pub auth: Arc<NodeAuth>,
    /// Largest frame accepted after login.
    pub max_frame: usize,
    /// Per-subscription outbound channel capacity.
    pub channel_capacity: usize,
    /// Test hook, feature `test-hooks` only: the forwarding tasks' pause
    /// point. Set once, before connections are served.
    #[cfg(feature = "test-hooks")]
    pub forwarder_gate: std::sync::OnceLock<Arc<ForwarderGate>>,
}

impl ConnectionContext {
    pub fn new(stream: EustressStream, auth: Arc<NodeAuth>, max_frame: usize, channel_capacity: usize) -> Arc<Self> {
        Arc::new(ConnectionContext {
            stream,
            auth,
            max_frame,
            channel_capacity,
            #[cfg(feature = "test-hooks")]
            forwarder_gate: std::sync::OnceLock::new(),
        })
    }
}

/// Test hook, compiled only under the non-default feature `test-hooks`, which
/// no shipped build enables: a pause point in each forwarding task, between
/// receiving a message and taking the writer lock. Armed, it stops the next
/// task that receives a message there until it is released.
#[cfg(feature = "test-hooks")]
#[derive(Debug, Default)]
pub struct ForwarderGate {
    armed: AtomicBool,
    arrived: Notify,
    released: Notify,
}

#[cfg(feature = "test-hooks")]
impl ForwarderGate {
    /// Stop the next forwarding task that receives a message at the pause
    /// point.
    pub fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    /// Completes once a forwarding task waits at the pause point.
    pub async fn paused(&self) {
        self.arrived.notified().await;
    }

    /// Let the waiting task go on.
    pub fn release(&self) {
        self.released.notify_one();
    }

    /// The pause point a forwarding task passes.
    async fn pause_point(&self) {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.arrived.notify_one();
            self.released.notified().await;
        }
    }
}

/// Handle one accepted TCP connection.
///
/// `accepted_at` starts the 5-second login deadline, which covers the TLS
/// handshake. `permit` is the pre-login slot taken at accept; it is released
/// when the login ends, however it ends.
pub async fn handle_connection<IO>(
    stream: IO,
    peer: SocketAddr,
    permit: PreLoginPermit,
    ctx: Arc<ConnectionContext>,
    accepted_at: Instant,
) where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    debug!("connection from {:?}", peer);

    let deadline = accepted_at + auth::LOGIN_DEADLINE;
    let login = tokio::time::timeout_at(deadline, login_tcp(stream, peer, &ctx)).await;
    drop(permit);
    let (tls, party, session) = match login {
        Ok(Some(done)) => done,
        Ok(None) => return,
        Err(_) => {
            ctx.auth.log_login_failure(
                peer.ip(),
                Channel::Tcp,
                None,
                auth::REASON_AUTH_FAILED,
                "no Welcome within 5 seconds of accept",
            );
            return;
        }
    };

    info!(party = %party.id, source = %peer, channel = "tcp", "login succeeded");
    let (read_half, write_half) = tokio::io::split(tls);
    serve_party(read_half, write_half, peer, party, session, &ctx).await;
}

/// TLS accept, `Challenge`, `Hello`, then `Welcome` or `AuthFailed`. The
/// first frame must be `Hello`, and frames before `Welcome` are capped at
/// 64 KB. The party's session is registered before `Welcome` goes out.
async fn login_tcp<IO>(
    io: IO,
    peer: SocketAddr,
    ctx: &ConnectionContext,
) -> Option<(tokio_rustls::server::TlsStream<IO>, Arc<Party>, SessionGuard)>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let auth = &ctx.auth;
    let mut tls = match auth.tls().tcp_acceptor().accept(io).await {
        Ok(tls) => tls,
        Err(e) => {
            debug!(source = %peer, "TLS handshake failed: {e}");
            auth.log_login_failure(peer.ip(), Channel::Tcp, None, auth::REASON_AUTH_FAILED, "TLS handshake failed");
            return None;
        }
    };

    let exporter = {
        let (_, connection) = tls.get_ref();
        match transport::tcp_exporter(&**connection) {
            Ok(exporter) => exporter,
            Err(e) => {
                warn!(source = %peer, "cannot derive the login binding: {e}");
                return None;
            }
        }
    };

    let nonce = auth::fresh_nonce();
    let challenge = ServerFrame::Challenge { node_id: auth.node_id().to_string(), nonce };
    if write_server_frame(&mut tls, &challenge).await.is_err() {
        return None;
    }

    let first = match read_client_frame(&mut tls, auth::PRE_LOGIN_MAX_FRAME).await {
        Ok(frame) => frame,
        Err(e) => {
            let cause = match e {
                NodeError::FrameTooLarge(_) => "pre-login frame over 64 KB",
                NodeError::Decode(_) => "first frame does not decode",
                _ => "closed before Hello",
            };
            auth.log_login_failure(peer.ip(), Channel::Tcp, None, auth::REASON_AUTH_FAILED, cause);
            return None;
        }
    };
    let ClientFrame::Hello { protocol_version, party_id, signature } = first else {
        auth.log_login_failure(peer.ip(), Channel::Tcp, None, auth::REASON_AUTH_FAILED, "first frame was not Hello");
        let _ = tls.shutdown().await;
        return None;
    };

    match auth.check_hello(Channel::Tcp, &nonce, &exporter, protocol_version, &party_id, &signature) {
        HelloOutcome::Welcome(party) => {
            // The session is registered before `Welcome` goes out, and the
            // party is checked again right after, so a revocation that lands
            // during the login is never missed.
            let Some(session) = auth.register_session(&party) else {
                auth.log_login_failure(
                    peer.ip(),
                    Channel::Tcp,
                    Some(&party.id),
                    auth::REASON_AUTH_FAILED,
                    "party revoked during the login",
                );
                let reason = auth::REASON_AUTH_FAILED.to_string();
                let _ = write_server_frame(&mut tls, &ServerFrame::AuthFailed { reason }).await;
                let _ = tls.shutdown().await;
                return None;
            };
            let welcome = ServerFrame::Welcome { party_id: party.id.clone() };
            if write_server_frame(&mut tls, &welcome).await.is_err() {
                return None;
            }
            Some((tls, party, session))
        }
        HelloOutcome::Refused(refusal) => {
            auth.log_login_failure(
                peer.ip(),
                Channel::Tcp,
                refusal.enrolled_party.as_deref(),
                refusal.reason,
                refusal.cause,
            );
            let _ = write_server_frame(&mut tls, &ServerFrame::AuthFailed { reason: refusal.reason.to_string() }).await;
            let _ = tls.shutdown().await;
            None
        }
    }
}

/// Dispatch a logged-in party's frames until the peer closes, a write fails
/// or misses its deadline, or the party is revoked. Shared by TCP and QUIC.
/// `session` was registered by the login, before `Welcome`.
pub(crate) async fn serve_party<R, W>(
    read_half: R,
    write_half: W,
    peer: SocketAddr,
    party: Arc<Party>,
    session: SessionGuard,
    ctx: &ConnectionContext,
) where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let write_half = Arc::new(tokio::sync::Mutex::new(write_half));

    let mut handler = ConnectionHandler {
        reader: read_half,
        writer: Arc::clone(&write_half),
        stream: ctx.stream.clone(),
        subscriptions: HashMap::new(),
        max_frame: ctx.max_frame,
        channel_capacity: ctx.channel_capacity,
        party_id: party.id.clone(),
        key_fingerprint: party.key_fingerprint(),
        auth: Arc::clone(&ctx.auth),
        held: HashMap::new(),
        peer,
        no_ack_refusals: None,
        closing: false,
        parties_changed: ctx.auth.parties_changed(),
        close: Arc::new(Notify::new()),
        #[cfg(feature = "test-hooks")]
        forwarder_gate: ctx.forwarder_gate.get().cloned(),
    };

    if let Err(e) = handler.run(session).await {
        match e {
            NodeError::ConnectionClosed => debug!("connection closed by {:?}", peer),
            other => warn!("connection error from {:?}: {other}", peer),
        }
    }

    // Unsubscribe all active subscriptions on disconnect.
    for (topic, id) in handler.subscriptions.drain() {
        handler.stream.unsubscribe(&topic, id);
    }
    // Their quota slots go back to the party, the forwarding tasks stop, and
    // the peer sees the close. A peer that stopped reading cannot hold the
    // connection open: no task is left writing to it, and the close waits at
    // most a second.
    for (_, held) in handler.held.drain() {
        held.forwarder.abort();
    }
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        let mut writer = write_half.lock().await;
        let _ = writer.shutdown().await;
    })
    .await;

    debug!("connection from {:?} cleaned up", peer);
}

struct ConnectionHandler<R, W> {
    reader: R,
    writer: Arc<tokio::sync::Mutex<W>>,
    stream: EustressStream,
    subscriptions: HashMap<String, SubscriberId>,
    max_frame: usize,
    channel_capacity: usize,
    /// The party this connection logged in as.
    party_id: String,
    /// The node's live parties and the per-party subscription quotas.
    auth: Arc<NodeAuth>,
    /// What each live subscription holds: its quota slot and its forwarding
    /// task.
    held: HashMap<String, Held>,
    /// Where the connection comes from, for the rate-limited warn lines.
    peer: SocketAddr,
    /// The fixed window of refused fire-and-forget publishes: when it began,
    /// and how many refusals it holds.
    no_ack_refusals: Option<(Instant, u32)>,
    /// Set when the connection must close after the current frame.
    closing: bool,
    /// The fingerprint of the key this party logged in with. A reload that
    /// finds another key under the same id ends the connection.
    key_fingerprint: [u8; 32],
    /// Changes each time a reload puts a new set of parties in force; the
    /// connection then drops every subscription its party is no longer
    /// granted.
    parties_changed: watch::Receiver<u64>,
    /// Told by a forwarding task whose write failed or missed its deadline:
    /// the connection closes.
    close: Arc<Notify>,
    /// Test hook, feature `test-hooks` only.
    #[cfg(feature = "test-hooks")]
    forwarder_gate: Option<Arc<ForwarderGate>>,
}

/// What a live subscription holds besides its subscriber id.
struct Held {
    /// The party's quota slot, released when this is dropped.
    _slot: QuotaSlot,
    /// The task that writes the subscription's frames. Aborted only when the
    /// connection ends; a subscription that ends on a live connection stops
    /// its task through `stop` instead.
    forwarder: AbortHandle,
    /// The forwarding task's stop flag, which it reads under the writer lock.
    stop: Arc<AtomicBool>,
}

impl Held {
    /// End the subscription's forwarding: the task finishes the frame it may
    /// be writing, so the connection never carries half a frame, and writes
    /// nothing more, since it reads the flag under the writer lock before each
    /// frame. The quota slot goes back as this is dropped.
    fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl<R, W> ConnectionHandler<R, W>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send + 'static,
{
    async fn run(&mut self, mut session: SessionGuard) -> Result<(), NodeError> {
        let close = Arc::clone(&self.close);
        let mut watching_parties = true;
        loop {
            let frame = tokio::select! {
                biased;
                _ = session.revoked() => {
                    info!(party = %self.party_id, "connection closed: party revoked");
                    return Ok(());
                }
                _ = close.notified() => return Err(write_deadline_missed()),
                changed = self.parties_changed.changed(), if watching_parties => {
                    match changed {
                        Ok(()) => self.apply_grants(),
                        Err(_) => watching_parties = false,
                    }
                    if self.closing {
                        return Ok(());
                    }
                    continue;
                }
                frame = read_client_frame(&mut self.reader, self.max_frame) => frame?,
            };
            // The dispatch, its reply included, runs to completion before the
            // next frame is read. It is raced against revocation and against
            // a forwarder's failed write, so a dispatch waiting on a write to
            // a peer that stopped reading still ends with the connection.
            tokio::select! {
                biased;
                _ = session.revoked() => {
                    info!(party = %self.party_id, "connection closed: party revoked while a write waited");
                    return Ok(());
                }
                _ = close.notified() => return Err(write_deadline_missed()),
                result = self.dispatch(frame) => result?,
            }
            if self.closing {
                return Ok(());
            }
        }
    }

    /// A reload put a new set of parties in force: drop every subscription
    /// this party is no longer granted. A party that is gone, disabled, or
    /// holds another key now ends the connection; the revocation pass ends
    /// its session too.
    fn apply_grants(&mut self) {
        let set = self.auth.party_set();
        if !set.holds_key(&self.party_id, &self.key_fingerprint) {
            info!(party = %self.party_id, "connection closed: party no longer enabled under the key it logged in with");
            self.closing = true;
            return;
        }
        let Some(party) = set.active(&self.party_id).cloned() else {
            self.closing = true;
            return;
        };
        let removed: Vec<String> =
            self.subscriptions.keys().filter(|topic| !party.may_subscribe(topic)).cloned().collect();
        for topic in removed {
            info!(
                party = %self.party_id,
                topic = ?auth::truncate_for_log(&topic),
                "subscription ended: the party's grant for this topic was removed"
            );
            self.end_subscription(&topic);
        }
    }

    /// End one subscription: no new message enters it, its forwarding task
    /// stops once the frame it may be writing has gone out, and its quota slot
    /// goes back. What the task still holds is dropped.
    fn end_subscription(&mut self, topic: &str) {
        if let Some(id) = self.subscriptions.remove(topic) {
            self.stream.unsubscribe(topic, id);
        }
        if let Some(held) = self.held.remove(topic) {
            held.stop();
        }
    }

    async fn dispatch(&mut self, frame: ClientFrame) -> Result<(), NodeError> {
        // Grants are read from the parties in force now, so a narrowed grant
        // applies to a live connection at its next frame.
        let set = self.auth.party_set();
        let Some(party) = set
            .active(&self.party_id)
            .filter(|_| set.holds_key(&self.party_id, &self.key_fingerprint))
            .cloned()
        else {
            info!(party = %self.party_id, "connection closed: party no longer enabled");
            return Err(NodeError::AuthFailed(auth::REASON_AUTH_FAILED.to_string()));
        };

        match frame {
            ClientFrame::Publish { topic, payload } => {
                if !self.publish_allowed(&party, &topic) {
                    return self.refuse(ERR_FORBIDDEN, "publish refused: no grant for this topic").await;
                }
                let offset = self.stream
                    .producer(&topic)
                    .send_bytes(Bytes::from(payload));
                self.send(ServerFrame::Ack { offset }).await?;
            }

            ClientFrame::PublishBatch { messages } => {
                // All or nothing: one topic without a grant refuses the batch
                // before anything is written.
                if messages.iter().any(|(topic, _)| !self.publish_allowed(&party, topic)) {
                    return self
                        .refuse(ERR_FORBIDDEN, "publish refused: no grant for a topic in this batch; nothing was written")
                        .await;
                }
                let offsets: Vec<u64> = messages
                    .into_iter()
                    .map(|(topic, payload)| {
                        self.stream.producer(&topic).send_bytes(Bytes::from(payload))
                    })
                    .collect();
                self.send(ServerFrame::BatchAck { offsets }).await?;
            }

            // Fire-and-forget — no response sent.
            ClientFrame::PublishNoAck { topic, payload } => {
                // A refusal sends nothing back. This sender reads no reply to
                // a fire-and-forget frame, so an `Error` would be taken for
                // the answer to its next acknowledged request. The refusal is
                // logged instead, rate-limited per source, and counted toward
                // the connection's window.
                if !self.publish_allowed(&party, &topic) {
                    self.count_no_ack_refusal();
                    return Ok(());
                }
                self.stream.producer(&topic).send_bytes(Bytes::from(payload));
                // intentionally no send()
            }

            // Fire-and-forget batch — no response sent.
            ClientFrame::PublishBatchNoAck { topic, payloads } => {
                // As above: a refusal sends nothing back, and counts.
                if !self.publish_allowed(&party, &topic) {
                    self.count_no_ack_refusal();
                    return Ok(());
                }
                let producer = self.stream.producer(&topic);
                for payload in payloads {
                    producer.send_bytes(Bytes::from(payload));
                }
                // intentionally no send()
            }

            // Zero-copy single-topic batch — ack is just (first_offset, count).
            ClientFrame::PublishBatchTopic { topic, payloads } => {
                if !self.publish_allowed(&party, &topic) {
                    return self.refuse(ERR_FORBIDDEN, "publish refused: no grant for this topic").await;
                }
                let count = payloads.len() as u32;
                let producer = self.stream.producer(&topic);
                let mut first_offset = 0u64;
                for (i, payload) in payloads.into_iter().enumerate() {
                    let offset = producer.send_bytes(Bytes::from(payload));
                    if i == 0 {
                        first_offset = offset;
                    }
                }
                self.send(ServerFrame::BatchAckCompact { first_offset, count }).await?;
            }

            ClientFrame::Subscribe { topic, from_offset } => {
                // The grant comes first, a repeated `Subscribe` included: a
                // topic the party is no longer granted is refused, and a
                // subscription the connection still holds there ends now.
                if !party.may_subscribe(&topic) {
                    self.end_subscription(&topic);
                    self.log_refusal(&party, &topic, "subscribe: no grant for this topic");
                    return self.refuse(ERR_FORBIDDEN, "subscribe refused: no grant for this topic").await;
                }
                if self.subscriptions.contains_key(&topic) {
                    // Already subscribed — re-use existing subscription.
                    // It still gets its one reply.
                    let head = self.stream.head(&topic);
                    return self.send(ServerFrame::Subscribed { head }).await;
                }
                let Some(slot) = self.auth.quotas.try_acquire(&party) else {
                    self.log_refusal(&party, &topic, "subscribe: the party's subscription quota is used up");
                    return self.refuse(ERR_QUOTA, "subscribe refused: subscription quota reached").await;
                };

                let (tx, mut rx) = mpsc::channel::<OwnedMessage>(self.channel_capacity);
                let topic_name = topic.clone();

                let sub_id = self.stream
                    .subscribe_owned(&topic, {
                        let tx = tx.clone();
                        let topic_name = topic_name.clone();
                        move |msg: OwnedMessage| {
                            if tx.try_send(msg).is_err() {
                                // Channel full — drop oldest (ring semantics)
                            }
                            let _ = topic_name.len(); // silence unused warning
                        }
                    })
                    .map_err(|e| NodeError::SubscribeLimit(e.to_string()));
                // A topic with no subscriber slot left refuses the request
                // with `Error`; the connection stays open.
                let sub_id = match sub_id {
                    Ok(sub_id) => sub_id,
                    Err(e) => return self.refuse(ERR_TOPIC_FULL, &e.to_string()).await,
                };

                self.subscriptions.insert(topic.clone(), sub_id);

                // The success reply goes out now, before the forwarding task
                // that writes every Message for this subscription, replays
                // included, exists. So `Subscribed` always comes first.
                let head = self.stream.head(&topic);
                self.send(ServerFrame::Subscribed { head }).await?;

                // If from_offset is set, replay ring buffer first.
                // The forwarding task sends the replayed frames, in order,
                // before the live ones, so stopping the task stops both.
                let mut backlog = Vec::new();
                if let Some(offset) = from_offset {
                    let topic_ref = topic.clone();
                    self.stream.replay_ring(&topic_ref, offset, |view| {
                        let frame = ServerFrame::Message {
                            topic: view.topic.to_string(),
                            offset: view.offset,
                            timestamp: view.timestamp,
                            payload: view.data.to_vec(),
                        };
                        backlog.push(frame);
                    });
                }

                // Spawn a forwarding task: channel → TCP write.
                let writer = Arc::clone(&self.writer);
                let topic_clone = topic.clone();
                let close = Arc::clone(&self.close);
                // Set when the subscription ends on a live connection. The
                // task reads it under the writer lock, before each frame.
                let stop = Arc::new(AtomicBool::new(false));
                let task_stop = Arc::clone(&stop);
                #[cfg(feature = "test-hooks")]
                let gate = self.forwarder_gate.clone();
                let forwarder = tokio::spawn(async move {
                    for frame in backlog {
                        if !matches!(forward(&writer, &frame, &task_stop, &close).await, Forwarded::Written) {
                            return;
                        }
                    }
                    while let Some(msg) = rx.recv().await {
                        let frame = ServerFrame::Message {
                            topic: topic_clone.clone(),
                            offset: msg.offset,
                            timestamp: msg.timestamp,
                            payload: msg.data.to_vec(),
                        };
                        // Test hook, feature `test-hooks` only: the pause point
                        // between receiving the message and taking the lock.
                        #[cfg(feature = "test-hooks")]
                        {
                            if let Some(gate) = &gate {
                                gate.pause_point().await;
                            }
                        }
                        if !matches!(forward(&writer, &frame, &task_stop, &close).await, Forwarded::Written) {
                            return;
                        }
                    }
                })
                .abort_handle();
                self.held.insert(topic.clone(), Held { _slot: slot, forwarder, stop });
            }

            ClientFrame::Unsubscribe { topic } => {
                // The forwarding task stops once the frame it may be writing
                // has gone out, so nothing of this subscription follows a
                // later `Subscribed`. The quota slot goes back.
                self.end_subscription(&topic);
            }

            ClientFrame::ListTopics => {
                let stats: Vec<TopicStats> = self.stream.topics()
                    .into_iter()
                    // Only the topics this party holds a grant on.
                    .filter(|name| party.may_list(name))
                    .map(|name| {
                        let head = self.stream.head(&name);
                        let subscriber_count = self.stream.subscriber_count(&name);
                        TopicStats { name, head, subscriber_count }
                    })
                    .collect();
                self.send(ServerFrame::TopicList(stats)).await?;
            }

            ClientFrame::Ping => {
                self.send(ServerFrame::Pong).await?;
            }

            ClientFrame::Hello { .. } => {
                return self.refuse(ERR_BAD_REQUEST, "already logged in").await;
            }
        }
        Ok(())
    }

    /// Write a reply under the write deadline.
    async fn send(&self, frame: ServerFrame) -> Result<(), NodeError> {
        write_within_deadline(&self.writer, &frame).await
    }

    /// Answer a refused frame with `Error`. The connection stays open.
    async fn refuse(&self, code: u32, message: &str) -> Result<(), NodeError> {
        self.send(ServerFrame::Error { code, message: message.to_string() }).await
    }

    /// The publish grant. A refusal writes the rate-limited warn line.
    fn publish_allowed(&self, party: &Party, topic: &str) -> bool {
        if party.may_publish(topic) {
            return true;
        }
        self.log_refusal(party, topic, "publish: no grant for this topic");
        false
    }

    /// The warn line for a refusal, at most one per source IP per 10 seconds.
    fn log_refusal(&self, party: &Party, topic: &str, what: &str) {
        self.auth.log_refusal(self.peer.ip(), &party.id, topic, what);
    }

    /// Count a refused fire-and-forget publish in this connection's window.
    /// The window is fixed: it begins at the first refusal and lasts 60
    /// seconds, and the next refusal after that begins a new one. The 101st
    /// refusal inside one window closes the connection, with one warn line
    /// that names the party (it is authenticated). Acknowledged refusals are
    /// not counted: their sender reads the `Error`.
    fn count_no_ack_refusal(&mut self) {
        let now = Instant::now();
        let (began, refusals) = match self.no_ack_refusals {
            Some((began, refusals)) if now.duration_since(began) < auth::NO_ACK_REFUSAL_WINDOW => (began, refusals + 1),
            _ => (now, 1),
        };
        self.no_ack_refusals = Some((began, refusals));
        if refusals > auth::MAX_NO_ACK_REFUSALS {
            warn!(
                party = %self.party_id,
                source = %self.peer,
                refusals,
                "connection closed: more than 100 refused fire-and-forget publishes within 60 seconds"
            );
            self.closing = true;
        }
    }
}

/// Write `frame` to the peer within the write deadline, which covers waiting
/// for the writer as well as the write itself. A write that misses it fails
/// with a timeout, and the connection closes.
async fn write_within_deadline<W>(writer: &tokio::sync::Mutex<W>, frame: &ServerFrame) -> Result<(), NodeError>
where
    W: AsyncWrite + Unpin,
{
    match tokio::time::timeout(auth::WRITE_DEADLINE, async {
        let mut guard = writer.lock().await;
        write_server_frame(&mut *guard, frame).await
    })
    .await
    {
        Ok(written) => written,
        Err(_) => Err(write_deadline_missed()),
    }
}

/// What one of a forwarding task's writes came to.
enum Forwarded {
    /// The frame went out.
    Written,
    /// The subscription had ended: the frame was dropped, and the task ends.
    Stopped,
    /// The write failed or missed its deadline: the connection closes.
    Failed,
}

/// Write one of a subscription's frames. The task takes the writer lock,
/// then reads its stop flag, then writes: never the flag before the lock, or
/// an `Unsubscribe` and a new `Subscribe` could slip in between and this frame
/// would follow the new `Subscribed`. A stopped subscription's frame is
/// dropped. The write deadline covers the lock and the write; a write that
/// fails or misses it tells the connection to close, so a peer that stopped
/// reading holds nothing open.
async fn forward<W>(writer: &tokio::sync::Mutex<W>, frame: &ServerFrame, stop: &AtomicBool, close: &Notify) -> Forwarded
where
    W: AsyncWrite + Unpin,
{
    let written = tokio::time::timeout(auth::WRITE_DEADLINE, async {
        let mut guard = writer.lock().await;
        if stop.load(Ordering::SeqCst) {
            return Ok(false);
        }
        write_server_frame(&mut *guard, frame).await.map(|()| true)
    })
    .await;
    match written {
        Ok(Ok(true)) => Forwarded::Written,
        Ok(Ok(false)) => Forwarded::Stopped,
        Ok(Err(_)) | Err(_) => {
            close.notify_one();
            Forwarded::Failed
        }
    }
}

/// The error a connection closes with when a write to its peer did not
/// finish within the write deadline.
fn write_deadline_missed() -> NodeError {
    NodeError::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "a write to the peer did not finish within 10 seconds",
    ))
}
