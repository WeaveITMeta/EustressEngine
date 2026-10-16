//! `StreamNodeClient` — connect to a remote EustressStream node over TCP.
//!
//! Every client here is native: it pins the node's certificate (SPKI
//! SHA-256) and logs in before its first frame.
//!
//! - [`StreamNodeClient`]: TCP, TLS 1.3, and the TCP login.
//! - [`RestClient`]: the native REST client. It logs in with the certificate
//!   binding and mints bearer tokens.
//! - [`request_token`]: what `eustress-stream-node party token` runs on a
//!   party's own machine.
//! - [`RetryPolicy`]: the retry rule for a login refused by a node limit.

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use bytes::Bytes;
use dashmap::DashMap;
use eustress_identity::keypair::UserKeypair;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::client::conn::http1::SendRequest;
use hyper::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HOST, RETRY_AFTER};
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use rand::RngCore;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::warn;

use crate::auth::{self, Binding, Channel, IssuedToken, TokenClass, BINDING_LEN, PRE_LOGIN_MAX_FRAME, PROTOCOL_VERSION};
use crate::error::NodeError;
use crate::protocol::{
    read_server_frame, write_client_frame, ClientFrame, ServerFrame, TopicStats,
};
use crate::transport::{self, SpkiPin};

const MAX_FRAME: usize = 16 * 1024 * 1024;

/// The most an identity file can plausibly hold.
const MAX_IDENTITY_FILE_BYTES: usize = 64 * 1024;

type BoxedRead = Box<dyn AsyncRead + Send + Unpin>;
type BoxedWrite = Box<dyn AsyncWrite + Send + Unpin>;

// ─────────────────────────────────────────────────────────────────────────────
// Credentials and the login
// ─────────────────────────────────────────────────────────────────────────────

/// What a client logs in with: its party id, the party's keypair, and the pin
/// of the node it expects.
#[derive(Clone)]
pub struct ClientAuth {
    /// The party id enrolled in the node's `parties.toml`.
    pub party_id: String,
    /// The party's Ed25519 keypair (from its identity file).
    pub keypair: Arc<UserKeypair>,
    /// SHA-256 of the node certificate's SPKI. Any other certificate is
    /// refused.
    pub pin: SpkiPin,
}

impl fmt::Debug for ClientAuth {
    // The keypair is left out on purpose.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientAuth").field("party_id", &self.party_id).field("pin", &self.pin).finish_non_exhaustive()
    }
}

impl ClientAuth {
    pub fn new(party_id: impl Into<String>, keypair: Arc<UserKeypair>, pin: SpkiPin) -> Self {
        ClientAuth { party_id: party_id.into(), keypair, pin }
    }

    /// The raw 64-byte Ed25519 signature over `message`.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, NodeError> {
        STANDARD
            .decode(self.keypair.sign_bytes(message))
            .map_err(|_| NodeError::Protocol("the keypair produced an unreadable signature".to_string()))
    }
}

/// The client half of a TCP or QUIC login: read `Challenge`, sign this
/// channel's transcript, send `Hello`, and read `Welcome` or `AuthFailed`.
/// Returns the node id the node announced.
///
/// A connection closed before `Challenge` arrives gives
/// [`NodeError::Refused`], which the retry rule retries. Once `Challenge` has
/// arrived nothing is retried: a close after it, and `AuthFailed`, are
/// returned as they are.
pub(crate) async fn client_login<R, W>(
    reader: &mut R,
    writer: &mut W,
    auth: &ClientAuth,
    channel: Channel,
    exporter: &[u8; BINDING_LEN],
) -> Result<String, NodeError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let first = match read_server_frame(reader, PRE_LOGIN_MAX_FRAME).await {
        Ok(frame) => frame,
        Err(e) => return Err(closed_before_challenge(e)),
    };
    let ServerFrame::Challenge { node_id, nonce } = first else {
        return Err(NodeError::Protocol("the node did not open with Challenge".to_string()));
    };
    match answer_challenge(reader, writer, auth, channel, exporter, &node_id, &nonce).await {
        Ok(()) => Ok(node_id),
        // `Challenge` has arrived, so this is never a refusal by a limit.
        Err(e) => Err(e),
    }
}

/// Sign the transcript, send `Hello`, and read `Welcome` or `AuthFailed`.
async fn answer_challenge<R, W>(
    reader: &mut R,
    writer: &mut W,
    auth: &ClientAuth,
    channel: Channel,
    exporter: &[u8; BINDING_LEN],
    node_id: &str,
    nonce: &[u8; 32],
) -> Result<(), NodeError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let message = auth::transcript(
        channel,
        node_id,
        nonce,
        Binding::Exporter(exporter),
        &auth.party_id,
        PROTOCOL_VERSION,
        None,
    )?;
    let signature = auth.sign(&message)?;
    let hello = ClientFrame::Hello { protocol_version: PROTOCOL_VERSION, party_id: auth.party_id.clone(), signature };
    write_client_frame(writer, &hello).await?;
    match read_server_frame(reader, PRE_LOGIN_MAX_FRAME).await? {
        ServerFrame::Welcome { .. } => Ok(()),
        ServerFrame::AuthFailed { reason } => Err(NodeError::AuthFailed(reason)),
        _ => Err(NodeError::Protocol("the node answered Hello with neither Welcome nor AuthFailed".to_string())),
    }
}

/// A failure to read `Challenge` because the connection closed. A refusal at
/// accept by a pre-login limit looks like this, so it is retried.
fn closed_before_challenge(error: NodeError) -> NodeError {
    match error {
        NodeError::ConnectionClosed => NodeError::Refused {
            reason: "the node closed the connection before Challenge".to_string(),
            retry_after: None,
        },
        NodeError::Io(e) if transport::closed_by_peer(&e) => NodeError::Refused {
            reason: format!("the node closed the connection before Challenge ({e})"),
            retry_after: None,
        },
        other => other,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The retry rule
// ─────────────────────────────────────────────────────────────────────────────

/// Where the retry jitter comes from. Tests inject a fixed source.
pub trait RandomSource: Send + Sync {
    /// A uniformly random `u64`.
    fn next_u64(&self) -> u64;
}

/// The operating system's generator.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsRandom;

impl RandomSource for OsRandom {
    fn next_u64(&self) -> u64 {
        rand::rngs::OsRng.next_u64()
    }
}

/// The retry rule for a login refused by a node limit: a 429 or 503 on REST,
/// or a connection closed before `Challenge` on TCP or QUIC. At most three
/// attempts in all. Before each retry the client waits the node's
/// `Retry-After`, or one second when it gives none, plus 0 to 500 ms of
/// jitter. Nothing else is retried: not a close after `Challenge`, not
/// `AuthFailed`, not a TLS failure.
#[derive(Clone)]
pub struct RetryPolicy {
    /// Attempts in all, the first included.
    pub max_attempts: u32,
    /// The jitter source.
    pub random: Arc<dyn RandomSource>,
}

impl RetryPolicy {
    /// Attempts in all under the rule.
    pub const MAX_ATTEMPTS: u32 = 3;
    /// The wait when the node gives no `Retry-After`.
    pub const DEFAULT_WAIT: Duration = Duration::from_secs(1);
    /// The most jitter added to a wait.
    pub const MAX_JITTER: Duration = Duration::from_millis(500);

    /// The rule's three attempts, with jitter from `random`.
    pub fn with_random(random: Arc<dyn RandomSource>) -> Self {
        RetryPolicy { max_attempts: Self::MAX_ATTEMPTS, random }
    }

    /// Jitter from 0 to 500 ms, both included.
    pub fn jitter(&self) -> Duration {
        let span = Self::MAX_JITTER.as_millis() as u64 + 1;
        Duration::from_millis(self.random.next_u64() % span)
    }

    /// The wait before the next attempt.
    pub fn wait_before_retry(&self, retry_after: Option<Duration>) -> Duration {
        retry_after.unwrap_or(Self::DEFAULT_WAIT) + self.jitter()
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::with_random(Arc::new(OsRandom))
    }
}

impl fmt::Debug for RetryPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetryPolicy").field("max_attempts", &self.max_attempts).finish_non_exhaustive()
    }
}

/// Run `attempt` under `policy`. Only [`NodeError::Refused`] is retried.
/// When the last attempt allowed is refused too, the error is
/// [`NodeError::RetriesExhausted`], which names that refusal.
pub async fn with_retry<T, F, Fut>(policy: &RetryPolicy, mut attempt: F) -> Result<T, NodeError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, NodeError>>,
{
    let max_attempts = policy.max_attempts.max(1);
    let mut made = 0;
    loop {
        made += 1;
        match attempt().await {
            Err(NodeError::Refused { reason, retry_after }) => {
                if made >= max_attempts {
                    return Err(NodeError::RetriesExhausted { attempts: made, reason });
                }
                let wait = policy.wait_before_retry(retry_after);
                tracing::debug!(attempt = made, ?wait, "refused by a node limit ({reason}); retrying");
                tokio::time::sleep(wait).await;
            }
            other => return other,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The reply queue
// ─────────────────────────────────────────────────────────────────────────────

/// A connection's subscriber channels, by topic. The reader delivers each
/// `Message` to its topic's channel, and drops it when the topic has none.
pub(crate) type Channels = DashMap<String, mpsc::Sender<(u64, u64, Vec<u8>)>>;

/// A request that waits for its one reply. Each carries the sender that
/// completes its caller.
pub(crate) enum Pending {
    Ack(oneshot::Sender<Result<u64, NodeError>>),
    BatchAck(oneshot::Sender<Result<Vec<u64>, NodeError>>),
    Compact(oneshot::Sender<Result<(u64, u32), NodeError>>),
    Topics(oneshot::Sender<Result<Vec<TopicStats>, NodeError>>),
    /// A `Subscribe`, with the channel to register for `topic` when its
    /// `Subscribed` arrives, and not before.
    Subscribed {
        topic: String,
        channel: mpsc::Sender<(u64, u64, Vec<u8>)>,
        done: oneshot::Sender<Result<u64, NodeError>>,
    },
    Pong(oneshot::Sender<Result<(), NodeError>>),
}

impl Pending {
    /// The reply this request expects, for an error message.
    fn expects(&self) -> &'static str {
        match self {
            Pending::Ack(_) => "Ack",
            Pending::BatchAck(_) => "BatchAck",
            Pending::Compact(_) => "BatchAckCompact",
            Pending::Topics(_) => "TopicList",
            Pending::Subscribed { .. } => "Subscribed",
            Pending::Pong(_) => "Pong",
        }
    }

    /// Complete the caller with an error.
    fn fail(self, error: NodeError) {
        match self {
            Pending::Ack(tx) => {
                let _ = tx.send(Err(error));
            }
            Pending::BatchAck(tx) => {
                let _ = tx.send(Err(error));
            }
            Pending::Compact(tx) => {
                let _ = tx.send(Err(error));
            }
            Pending::Topics(tx) => {
                let _ = tx.send(Err(error));
            }
            Pending::Subscribed { done, .. } => {
                let _ = done.send(Err(error));
            }
            Pending::Pong(tx) => {
                let _ = tx.send(Err(error));
            }
        }
    }
}

/// The name of a server frame, for an error message.
fn frame_name(frame: &ServerFrame) -> &'static str {
    match frame {
        ServerFrame::Message { .. } => "Message",
        ServerFrame::TopicList(_) => "TopicList",
        ServerFrame::Ack { .. } => "Ack",
        ServerFrame::BatchAck { .. } => "BatchAck",
        ServerFrame::BatchAckCompact { .. } => "BatchAckCompact",
        ServerFrame::Error { .. } => "Error",
        ServerFrame::Pong => "Pong",
        ServerFrame::Challenge { .. } => "Challenge",
        ServerFrame::Welcome { .. } => "Welcome",
        ServerFrame::AuthFailed { .. } => "AuthFailed",
        ServerFrame::Subscribed { .. } => "Subscribed",
    }
}

/// A client's requests waiting for their reply, in the order they went out.
///
/// The node answers every reply-expecting request exactly once, in arrival
/// order, and the transport is ordered, so one queue across all kinds matches
/// every reply to its request with no request ids. The connection's writer
/// task queues a request just before it writes the request's frame, so a
/// reply can never arrive for a request not yet queued, and a queued request
/// always has its whole frame on the wire.
#[derive(Default)]
pub(crate) struct PendingQueue {
    queue: VecDeque<Pending>,
    closed: bool,
}

impl PendingQueue {
    /// Queue a request. Refused once the connection has closed.
    pub(crate) fn push(&mut self, pending: Pending) -> Result<(), NodeError> {
        if self.closed {
            return Err(NodeError::ConnectionClosed);
        }
        self.queue.push_back(pending);
        Ok(())
    }

    /// Answer the front request with `reply`, which is any server frame but
    /// `Message`. An `Error` completes it with [`NodeError::RequestRefused`].
    /// A reply of another kind than the front request expects, or a reply with
    /// no request waiting, is a protocol error: the front request fails with
    /// it, and the caller must close the connection.
    ///
    /// `Subscribed` registers the subscription's channel in `channels` here,
    /// as the reader handles the reply and before it reads the next frame, so
    /// every Message after the reply reaches the channel and none before it
    /// does. A refused `Subscribe` leaves its topic no channel.
    pub(crate) fn answer(&mut self, reply: ServerFrame, channels: &Channels) -> Result<(), NodeError> {
        let Some(front) = self.queue.pop_front() else {
            return Err(NodeError::Protocol(format!(
                "the node sent {} with no request waiting",
                frame_name(&reply)
            )));
        };
        let expected = front.expects();
        match (front, reply) {
            // A refused `Subscribe`: its topic keeps no channel.
            (Pending::Subscribed { topic, done, .. }, ServerFrame::Error { code, message }) => {
                channels.remove(&topic);
                let _ = done.send(Err(NodeError::RequestRefused { code, message }));
            }
            (front, ServerFrame::Error { code, message }) => front.fail(NodeError::RequestRefused { code, message }),
            (Pending::Ack(tx), ServerFrame::Ack { offset }) => {
                let _ = tx.send(Ok(offset));
            }
            (Pending::BatchAck(tx), ServerFrame::BatchAck { offsets }) => {
                let _ = tx.send(Ok(offsets));
            }
            (Pending::Compact(tx), ServerFrame::BatchAckCompact { first_offset, count }) => {
                let _ = tx.send(Ok((first_offset, count)));
            }
            (Pending::Topics(tx), ServerFrame::TopicList(list)) => {
                let _ = tx.send(Ok(list));
            }
            // The channel is registered now, before the caller hears of it.
            // A caller that has gone (its receiver dropped) registers nothing.
            (Pending::Subscribed { topic, channel, done }, ServerFrame::Subscribed { head }) => {
                if !channel.is_closed() {
                    channels.insert(topic, channel);
                }
                let _ = done.send(Ok(head));
            }
            (Pending::Pong(tx), ServerFrame::Pong) => {
                let _ = tx.send(Ok(()));
            }
            (front, other) => {
                let error = format!("the node answered a request that expects {expected} with {}", frame_name(&other));
                front.fail(NodeError::Protocol(error.clone()));
                return Err(NodeError::Protocol(error));
            }
        }
        Ok(())
    }

    /// The connection is gone: every request still waiting fails, and no new
    /// one is queued.
    pub(crate) fn close(&mut self) {
        self.closed = true;
        for pending in self.queue.drain(..) {
            pending.fail(NodeError::ConnectionClosed);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The writer task
// ─────────────────────────────────────────────────────────────────────────────

/// Frames that may wait for a connection's writer task.
const WRITER_QUEUE: usize = 1024;

/// What a client connection's reader and writer tasks share: the write half,
/// the reply queue and the per-topic subscriber channels. Only the writer
/// task writes frames; the write half is shut down when the connection ends.
pub(crate) struct Conn<W> {
    pub(crate) writer: Mutex<W>,
    pub(crate) pending: Mutex<PendingQueue>,
    pub(crate) sub_channels: Channels,
}

impl<W> Conn<W> {
    pub(crate) fn new(writer: W) -> Arc<Self> {
        Arc::new(Conn {
            writer: Mutex::new(writer),
            pending: Mutex::new(PendingQueue::default()),
            sub_channels: DashMap::new(),
        })
    }
}

/// One frame for a connection's writer task, with the waiter for its reply
/// when it expects one.
pub(crate) struct Outgoing {
    frame: ClientFrame,
    pending: Option<Pending>,
}

/// Start a connection's one writer task and return its channel. For each
/// frame it takes, the task queues the reply's waiter, then writes the frame,
/// in that order and with nothing in between, so the reply queue always
/// matches the frames on the wire. A caller dropped before its frame reaches
/// the channel never sends it; one dropped after loses only its own reply.
/// Every write has the write deadline; a write that misses it, or fails,
/// closes the connection. When every handle is gone the connection closes.
pub(crate) fn spawn_writer<W>(conn: Arc<Conn<W>>) -> mpsc::Sender<Outgoing>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (tx, mut rx) = mpsc::channel::<Outgoing>(WRITER_QUEUE);
    tokio::spawn(async move {
        while let Some(Outgoing { frame, pending }) = rx.recv().await {
            if let Some(pending) = pending {
                // On a closed connection the waiter fails here, with
                // `ConnectionClosed`, and the frame is not written.
                if conn.pending.lock().await.push(pending).is_err() {
                    continue;
                }
            }
            let written = tokio::time::timeout(auth::WRITE_DEADLINE, async {
                let mut writer = conn.writer.lock().await;
                write_client_frame(&mut *writer, &frame).await
            })
            .await;
            match written {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::debug!("a write to the node failed: {e}; closing the connection");
                    break;
                }
                Err(_) => {
                    warn!("a write to the node did not finish within 10 seconds; closing the connection");
                    break;
                }
            }
        }
        close_conn(&conn).await;
    });
    tx
}

/// Hand `frame` to a connection's writer task, with the waiter for its reply
/// when it expects one. Returns once the frame is in the task's channel.
pub(crate) async fn send_frame(
    outgoing: &mpsc::Sender<Outgoing>,
    frame: ClientFrame,
    pending: Option<Pending>,
) -> Result<(), NodeError> {
    outgoing.send(Outgoing { frame, pending }).await.map_err(|_| NodeError::ConnectionClosed)
}

/// End a client connection: every request still waiting fails, none is
/// queued after this, the subscriptions end, and the write half closes,
/// waiting at most a second.
pub(crate) async fn close_conn<W>(conn: &Conn<W>)
where
    W: AsyncWrite + Unpin,
{
    conn.pending.lock().await.close();
    conn.sub_channels.clear();
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        let mut writer = conn.writer.lock().await;
        let _ = writer.shutdown().await;
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// StreamNodeClient (TCP)
// ─────────────────────────────────────────────────────────────────────────────

/// A client connected to a single EustressStream node.
///
/// Cheaply cloneable — all clones share the same TCP connection.
#[derive(Clone)]
pub struct StreamNodeClient {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    // The write half, the reply queue (one across all kinds, because replies
    // come back in request order) and the per-topic subscriber channels.
    conn: Arc<Conn<BoxedWrite>>,
    // The writer task's channel: every frame goes out through it.
    outgoing: mpsc::Sender<Outgoing>,
}

impl StreamNodeClient {
    /// Connect to an EustressStream node.
    ///
    /// Opens TLS 1.3 pinned to `auth.pin` and logs in as `auth.party_id`
    /// before the first frame. A connection the node closes before
    /// `Challenge` (a pre-login limit) is retried under
    /// [`RetryPolicy::default`]: at most three attempts in all.
    pub async fn connect(addr: SocketAddr, auth: &ClientAuth) -> Result<Self, NodeError> {
        Self::connect_with(|| TcpStream::connect(addr), auth, &RetryPolicy::default()).await
    }

    /// The same with a dialer and a retry policy of the caller's choosing.
    /// `dial` opens a fresh byte stream for each attempt.
    pub async fn connect_with<D, Fut, IO>(mut dial: D, auth: &ClientAuth, policy: &RetryPolicy) -> Result<Self, NodeError>
    where
        D: FnMut() -> Fut,
        Fut: Future<Output = std::io::Result<IO>>,
        IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        with_retry(policy, || {
            let opening = dial();
            async move { Self::connect_io(opening.await?, auth).await }
        })
        .await
    }

    /// One attempt over a byte stream that is already open: TLS 1.3 pinned
    /// to `auth.pin`, then the login. Not retried.
    pub async fn connect_io<IO>(stream: IO, auth: &ClientAuth) -> Result<Self, NodeError>
    where
        IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let tls = transport::tls_connect(stream, auth.pin, &[]).await?;
        let exporter = {
            let (_, connection) = tls.get_ref();
            transport::tcp_exporter(&**connection)?
        };
        let (mut read_half, mut write_half) = tokio::io::split(tls);
        client_login(&mut read_half, &mut write_half, auth, Channel::Tcp, &exporter).await?;
        let read_half: BoxedRead = Box::new(read_half);
        let write_half: BoxedWrite = Box::new(write_half);

        let conn = Conn::new(write_half);
        let outgoing = spawn_writer(Arc::clone(&conn));

        // Spawn reader task.
        let reader_conn = Arc::clone(&conn);
        tokio::spawn(async move {
            reader_task(read_half, reader_conn).await;
        });

        Ok(StreamNodeClient { inner: Arc::new(ClientInner { conn, outgoing }) })
    }

    /// Publish bytes to a topic. Returns the assigned offset.
    pub async fn publish(&self, topic: &str, payload: Bytes) -> Result<u64, NodeError> {
        let (tx, rx) = oneshot::channel::<Result<u64, NodeError>>();
        self.request(
            ClientFrame::Publish {
                topic: topic.to_string(),
                payload: payload.to_vec(),
            },
            Pending::Ack(tx),
        )
        .await?;
        rx.await.map_err(|_| NodeError::ConnectionClosed)?
    }

    /// Publish a batch of `(topic, payload)` pairs in one round trip.
    /// Returns the assigned offsets in order.
    pub async fn publish_batch(&self, messages: Vec<(String, Bytes)>) -> Result<Vec<u64>, NodeError> {
        let (tx, rx) = oneshot::channel::<Result<Vec<u64>, NodeError>>();
        self.request(
            ClientFrame::PublishBatch {
                messages: messages.into_iter().map(|(t, p)| (t, p.to_vec())).collect(),
            },
            Pending::BatchAck(tx),
        )
        .await?;
        rx.await.map_err(|_| NodeError::ConnectionClosed)?
    }

    /// **Fire-and-forget** — publishes and returns immediately without waiting
    /// for any ack. Server writes to the ring; no response is sent.
    /// A publish the party holds no grant for is not written, and nothing is
    /// sent back for it either.
    ///
    /// Use for best-effort streams (`scene_deltas`, `log/output`, etc.).
    pub async fn publish_no_ack(&self, topic: &str, payload: Bytes) -> Result<(), NodeError> {
        send_frame(
            &self.inner.outgoing,
            ClientFrame::PublishNoAck {
                topic: topic.to_string(),
                payload: payload.to_vec(),
            },
            None,
        )
        .await
    }

    /// **Fire-and-forget batch** — sends N payloads to one topic and returns
    /// immediately. No ack of any kind. This is the fastest TCP publish path.
    ///
    /// Throughput is bounded only by TCP write bandwidth (~9M msg/s @ 100B on
    /// loopback) rather than round-trip latency.
    pub async fn publish_batch_no_ack(
        &self,
        topic: &str,
        payloads: Vec<Bytes>,
    ) -> Result<(), NodeError> {
        send_frame(
            &self.inner.outgoing,
            ClientFrame::PublishBatchNoAck {
                topic: topic.to_string(),
                payloads: payloads.into_iter().map(|p| p.to_vec()).collect(),
            },
            None,
        )
        .await
    }

    /// **Zero-copy single-topic batch** — all payloads go to the same topic.
    ///
    /// Returns `(first_offset, count)`. Individual offsets are `first_offset + i`.
    /// Use this instead of `publish_batch` when all messages share one topic —
    /// the ack shrinks from `count × 8` bytes to 12 bytes fixed.
    pub async fn publish_batch_topic(
        &self,
        topic: &str,
        payloads: Vec<Bytes>,
    ) -> Result<(u64, u32), NodeError> {
        let (tx, rx) = oneshot::channel::<Result<(u64, u32), NodeError>>();
        self.request(
            ClientFrame::PublishBatchTopic {
                topic: topic.to_string(),
                payloads: payloads.into_iter().map(|p| p.to_vec()).collect(),
            },
            Pending::Compact(tx),
        )
        .await?;
        rx.await.map_err(|_| NodeError::ConnectionClosed)?
    }

    /// Subscribe to a topic. Returns a receiver for `(offset, timestamp, payload)` triples.
    ///
    /// Returns once the node answers `Subscribed`, which it writes before any
    /// `Message` for this subscription. A refusal returns the node's error and
    /// leaves no channel behind.
    pub async fn subscribe(
        &self,
        topic: &str,
        from_offset: Option<u64>,
    ) -> Result<mpsc::Receiver<(u64, u64, Vec<u8>)>, NodeError> {
        let (tx, rx) = mpsc::channel(4096);

        // The reader registers the channel when `Subscribed` arrives, before
        // it reads the next frame. A Message that comes before it, a stale one
        // from an earlier subscription to the topic included, finds no channel
        // and is dropped; a refused Subscribe registers nothing.
        let (done, subscribed) = oneshot::channel::<Result<u64, NodeError>>();
        self.request(
            ClientFrame::Subscribe {
                topic: topic.to_string(),
                from_offset,
            },
            Pending::Subscribed { topic: topic.to_string(), channel: tx, done },
        )
        .await?;
        subscribed.await.map_err(|_| NodeError::ConnectionClosed)??;

        Ok(rx)
    }

    /// Unsubscribe from a topic.
    pub async fn unsubscribe(&self, topic: &str) -> Result<(), NodeError> {
        self.inner.conn.sub_channels.remove(topic);
        send_frame(&self.inner.outgoing, ClientFrame::Unsubscribe { topic: topic.to_string() }, None).await
    }

    /// List topics on the connected node.
    /// Only the topics this party holds a grant on are listed.
    pub async fn list_topics(&self) -> Result<Vec<TopicStats>, NodeError> {
        let (tx, rx) = oneshot::channel::<Result<Vec<TopicStats>, NodeError>>();
        self.request(ClientFrame::ListTopics, Pending::Topics(tx)).await?;
        rx.await.map_err(|_| NodeError::ConnectionClosed)?
    }

    /// Ping the node.
    /// Returns when the node's `Pong` arrives.
    pub async fn ping(&self) -> Result<(), NodeError> {
        let (tx, rx) = oneshot::channel::<Result<(), NodeError>>();
        self.request(ClientFrame::Ping, Pending::Pong(tx)).await?;
        rx.await.map_err(|_| NodeError::ConnectionClosed)?
    }

    /// Whether this client holds a subscriber channel for `topic`.
    pub fn is_subscribed(&self, topic: &str) -> bool {
        self.inner.conn.sub_channels.contains_key(topic)
    }

    /// Hand a request to the writer task, which queues `pending` and then
    /// writes the frame, with nothing in between. A caller dropped before
    /// this returns never sends; one dropped after loses only its reply.
    async fn request(&self, frame: ClientFrame, pending: Pending) -> Result<(), NodeError> {
        send_frame(&self.inner.outgoing, frame, Some(pending)).await
    }
}

/// Background task: reads server frames and dispatches them.
async fn reader_task(
    mut reader: BoxedRead,
    conn: Arc<Conn<BoxedWrite>>,
) {
    loop {
        match read_server_frame(&mut reader, MAX_FRAME).await {
            Ok(ServerFrame::Message { topic, offset, timestamp, payload }) => {
                if let Some(entry) = conn.sub_channels.get(&topic) {
                    if entry.try_send((offset, timestamp, payload)).is_err() {
                        // Channel full — drop (ring semantics).
                    }
                }
            }
            // Every other frame is the reply to the front request.
            Ok(reply) => {
                if let Err(e) = conn.pending.lock().await.answer(reply, &conn.sub_channels) {
                    warn!("{e}; closing the connection");
                    break;
                }
            }
            Err(NodeError::ConnectionClosed) => break,
            Err(e) => {
                warn!("client reader error: {e}");
                break;
            }
        }
    }

    // The connection is gone, or the node broke the reply order: every
    // request still waiting fails, none is queued after this, the
    // subscriptions end, and the write side closes.
    close_conn(&conn).await;
}

// ─────────────────────────────────────────────────────────────────────────────
// RestClient (native REST)
// ─────────────────────────────────────────────────────────────────────────────

/// A REST response read to the end.
#[derive(Debug, Clone)]
pub struct RestResponse {
    pub status: u16,
    pub body: Bytes,
    /// The `Retry-After` header in seconds, when the node sent one.
    pub retry_after: Option<Duration>,
}

/// A challenge from `POST /v1/login/challenge`.
#[derive(Debug, Clone)]
pub struct RestChallenge {
    pub nonce: [u8; 32],
    pub node_id: String,
}

/// One HTTP/1.1 connection to a node's REST port, over TLS 1.3 pinned to the
/// node's certificate. This is a native client: it holds the pin and can log
/// in with the certificate binding.
pub struct RestClient {
    sender: SendRequest<Full<Bytes>>,
    cert_sha256: [u8; 32],
    /// The task driving the connection. Dropping the client stops it, which
    /// closes the connection at once.
    connection: tokio::task::AbortHandle,
}

impl Drop for RestClient {
    fn drop(&mut self) {
        self.connection.abort();
    }
}

impl RestClient {
    /// Connect to `node` (`host:port`) and open TLS 1.3 pinned to `pin`. One
    /// attempt; [`rest_login_with`] and [`request_token`] retry.
    pub async fn connect(node: &str, pin: SpkiPin) -> Result<Self, NodeError> {
        let stream = TcpStream::connect(node).await?;
        Self::handshake(stream, pin).await
    }

    /// The same over any byte stream.
    pub async fn handshake<IO>(io: IO, pin: SpkiPin) -> Result<Self, NodeError>
    where
        IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let tls = transport::tls_connect(io, pin, &[transport::REST_ALPN]).await?;
        // The certificate as this client saw it during the handshake. Its hash
        // is the REST login's channel binding.
        let cert_sha256 = {
            let (_, connection) = tls.get_ref();
            let leaf = connection
                .peer_certificates()
                .and_then(|certs| certs.first())
                .ok_or_else(|| NodeError::Tls("the node presented no certificate".to_string()))?;
            transport::sha256(leaf.as_ref())
        };
        // VERIFY: hyper 1.x client::conn::http1::handshake(io) -> (SendRequest<B>,
        // Connection<T, B>) with T: hyper::rt::Read + Write (TokioIo adapts tokio
        // IO). Needs hyper features "client" and "http1".
        let (sender, connection) = hyper::client::conn::http1::handshake::<_, Full<Bytes>>(TokioIo::new(tls))
            .await
            .map_err(|e| NodeError::Io(std::io::Error::other(format!("HTTP handshake: {e}"))))?;
        let connection = tokio::spawn(async move {
            let _ = connection.await;
        })
        .abort_handle();
        Ok(RestClient { sender, cert_sha256, connection })
    }

    /// SHA-256 of the node's DER certificate as this client saw it.
    pub fn cert_sha256(&self) -> [u8; 32] {
        self.cert_sha256
    }

    async fn send(
        &mut self,
        method: Method,
        path: &str,
        bearer: Option<&str>,
        json_body: Option<&serde_json::Value>,
        accept_events: bool,
    ) -> Result<Response<Incoming>, NodeError> {
        let mut builder = Request::builder().method(method).uri(path).header(HOST, transport::NODE_SERVER_NAME);
        if let Some(token) = bearer {
            builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        if accept_events {
            builder = builder.header(ACCEPT, "text/event-stream");
        }
        let body = match json_body {
            Some(value) => {
                builder = builder.header(CONTENT_TYPE, "application/json");
                let bytes = serde_json::to_vec(value)
                    .map_err(|e| NodeError::Protocol(format!("request body: {e}")))?;
                Full::new(Bytes::from(bytes))
            }
            None => Full::new(Bytes::new()),
        };
        let request = builder
            .body(body)
            .map_err(|e| NodeError::Protocol(format!("request: {e}")))?;
        self.sender
            .ready()
            .await
            .map_err(|e| NodeError::Io(std::io::Error::other(format!("HTTP connection: {e}"))))?;
        self.sender
            .send_request(request)
            .await
            .map_err(|e| NodeError::Io(std::io::Error::other(format!("HTTP request: {e}"))))
    }

    /// Send a request and read the whole response. `bearer` goes in the
    /// `Authorization` header, never in the URL.
    pub async fn request(
        &mut self,
        method: Method,
        path: &str,
        bearer: Option<&str>,
        json_body: Option<&serde_json::Value>,
    ) -> Result<RestResponse, NodeError> {
        let response = self.send(method, path, bearer, json_body, false).await?;
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let body = response
            .into_body()
            .collect()
            .await
            .map_err(|e| NodeError::Io(std::io::Error::other(format!("HTTP body: {e}"))))?
            .to_bytes();
        Ok(RestResponse { status, body, retry_after })
    }

    /// Open a server-sent event stream the way a browser `fetch` does: a GET
    /// with the bearer token in the `Authorization` header.
    pub async fn open_stream(&mut self, path: &str, bearer: &str) -> Result<EventStream, NodeError> {
        let response = self.send(Method::GET, path, Some(bearer), None, true).await?;
        Ok(EventStream { status: response.status().as_u16(), body: response.into_body(), pending: String::new() })
    }

    /// `POST /v1/login/challenge`. A 429 or 503 is [`NodeError::Refused`],
    /// with the node's `Retry-After`.
    pub async fn challenge(&mut self) -> Result<RestChallenge, NodeError> {
        #[derive(Deserialize)]
        struct ChallengeBody {
            nonce: String,
            node_id: String,
        }
        let response = self.request(Method::POST, "/v1/login/challenge", None, None).await?;
        if let Some(refused) = refused_by_limit("the challenge", &response) {
            return Err(refused);
        }
        if response.status != 200 {
            return Err(NodeError::Protocol(format!("the challenge was answered with HTTP {}", response.status)));
        }
        let body: ChallengeBody = serde_json::from_slice(&response.body)
            .map_err(|e| NodeError::Protocol(format!("challenge reply: {e}")))?;
        let nonce: [u8; 32] = STANDARD
            .decode(body.nonce.as_bytes())
            .ok()
            .and_then(|raw| raw.try_into().ok())
            .ok_or_else(|| NodeError::Protocol("challenge nonce is not 32 bytes of base64".to_string()))?;
        Ok(RestChallenge { nonce, node_id: body.node_id })
    }

    /// Fetch a challenge and log in as `party_id` for a token of `class`. The
    /// signature covers the REST transcript, with the hash of the certificate
    /// this client saw. One attempt; [`rest_login_with`] retries.
    pub async fn login(
        &mut self,
        party_id: &str,
        keypair: &UserKeypair,
        class: TokenClass,
    ) -> Result<IssuedToken, NodeError> {
        #[derive(Deserialize)]
        struct GrantBody {
            token: String,
            expires_in: u64,
        }
        let challenge = self.challenge().await?;
        let message = auth::rest_transcript(
            &challenge.node_id,
            &challenge.nonce,
            &self.cert_sha256,
            party_id,
            PROTOCOL_VERSION,
            class,
        )?;
        let body = serde_json::json!({
            "party_id": party_id,
            "nonce": STANDARD.encode(challenge.nonce),
            "token_class": class.as_str(),
            "signature": keypair.sign_bytes(&message),
        });
        let response = self.request(Method::POST, "/v1/login", None, Some(&body)).await?;
        if let Some(refused) = refused_by_limit("the login", &response) {
            return Err(refused);
        }
        if response.status != 200 {
            return Err(NodeError::AuthFailed(format!("login refused with HTTP {}", response.status)));
        }
        let grant: GrantBody = serde_json::from_slice(&response.body)
            .map_err(|e| NodeError::Protocol(format!("login reply: {e}")))?;
        Ok(IssuedToken { token: grant.token, class, expires_in: Duration::from_secs(grant.expires_in) })
    }
}

/// A 429 or 503: a refusal by a node limit, which the retry rule retries.
fn refused_by_limit(what: &str, response: &RestResponse) -> Option<NodeError> {
    match response.status {
        429 | 503 => Some(NodeError::Refused {
            reason: format!("{what} was refused with HTTP {}", response.status),
            retry_after: response.retry_after,
        }),
        _ => None,
    }
}

/// A server-sent event stream, read one `data:` line at a time.
pub struct EventStream {
    /// The response status.
    pub status: u16,
    body: Incoming,
    pending: String,
}

impl EventStream {
    /// The next `data:` line's text, or `None` when the stream ends.
    pub async fn next_data(&mut self) -> Result<Option<String>, NodeError> {
        loop {
            if let Some(end) = self.pending.find('\n') {
                let line: String = self.pending.drain(..=end).collect();
                let line = line.trim_end_matches(['\r', '\n']);
                if let Some(data) = line.strip_prefix("data:") {
                    return Ok(Some(data.trim_start().to_string()));
                }
                continue;
            }
            match self.body.frame().await {
                None => return Ok(None),
                Some(Err(e)) => return Err(NodeError::Io(std::io::Error::other(format!("event stream: {e}")))),
                Some(Ok(frame)) => {
                    if let Ok(data) = frame.into_data() {
                        self.pending.push_str(&String::from_utf8_lossy(&data));
                    }
                }
            }
        }
    }
}

/// Log in over REST under `policy`: each attempt opens a fresh connection
/// with `dial`, verifies the pin, fetches a fresh challenge, and signs it.
/// Only a refusal by a node limit is retried.
pub async fn rest_login_with<D, Fut, IO>(
    mut dial: D,
    pin: SpkiPin,
    party_id: &str,
    keypair: &UserKeypair,
    class: TokenClass,
    policy: &RetryPolicy,
) -> Result<IssuedToken, NodeError>
where
    D: FnMut() -> Fut,
    Fut: Future<Output = std::io::Result<IO>>,
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    with_retry(policy, || {
        let opening = dial();
        async move {
            let mut client = RestClient::handshake(opening.await?, pin).await?;
            client.login(party_id, keypair, class).await
        }
    })
    .await
}

// ─────────────────────────────────────────────────────────────────────────────
// Identity files and `party token`
// ─────────────────────────────────────────────────────────────────────────────

/// Read a party's keypair from its identity file. The file is opened with the
/// owner-only check (`eustress_identity::private_file::verify_open`) and read
/// through the handle that check returned, so the file read is the file
/// checked; a file that fails the check is not read.
pub fn read_identity_keypair(path: &Path) -> Result<UserKeypair, NodeError> {
    use std::io::Read as _;
    let checked = eustress_identity::private_file::verify_open(path).map_err(|e| {
        NodeError::Identity(format!("refusing to read the identity file: {}", auth::owner_only_refusal(path, &e)))
    })?;
    let mut bytes = Vec::new();
    checked
        .take(MAX_IDENTITY_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| NodeError::Identity(format!("cannot read {}: {e}", path.display())))?;
    if bytes.len() > MAX_IDENTITY_FILE_BYTES {
        return Err(NodeError::Identity(format!("{} is too large to be an identity file", path.display())));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| NodeError::Identity(format!("{} is not UTF-8", path.display())))?;
    parse_identity_keypair(&text)
}

/// The keypair in an identity file's text: `public_key` and `private_key`
/// from its `[identity]` table (or the top level, an older file), each 64 hex
/// digits or base64 of 32 bytes, checked against each other.
pub fn parse_identity_keypair(text: &str) -> Result<UserKeypair, NodeError> {
    let text = text.trim_start_matches('\u{feff}');
    let doc: toml::Table = toml::from_str(text)
        .map_err(|_| NodeError::Identity("this is not an identity file (it does not parse as TOML)".to_string()))?;
    if doc.contains_key("lock") {
        return Err(NodeError::Identity(
            "the identity file is locked with a passphrase, which this command cannot read".to_string(),
        ));
    }
    let field = |name: &str| -> Option<String> {
        doc.get("identity")
            .and_then(|table| table.get(name))
            .or_else(|| doc.get(name))
            .and_then(|value| value.as_str())
            .map(|value| value.trim().to_string())
    };
    let public = field("public_key")
        .ok_or_else(|| NodeError::Identity("the identity file has no public key".to_string()))?;
    let private = field("private_key")
        .ok_or_else(|| NodeError::Identity("the identity file has no private key, so it cannot sign".to_string()))?;
    let public = decode_key32(&public)
        .ok_or_else(|| NodeError::Identity("the identity file's public key is not valid".to_string()))?;
    let private = decode_key32(&private)
        .ok_or_else(|| NodeError::Identity("the identity file's private key is not valid".to_string()))?;
    let keypair = UserKeypair::from_private_b64(&STANDARD.encode(private))
        .map_err(|_| NodeError::Identity("the identity file's private key is not valid".to_string()))?;
    if keypair.public_key_b64() != STANDARD.encode(public) {
        return Err(NodeError::Identity("the identity file's keys do not match".to_string()));
    }
    Ok(keypair)
}

/// 32 bytes from 64 hex digits or from base64.
fn decode_key32(text: &str) -> Option<[u8; 32]> {
    if let Some(bytes) = transport::decode_hex32(text) {
        return Some(bytes);
    }
    STANDARD.decode(text.as_bytes()).ok()?.try_into().ok()
}

/// What `eustress-stream-node party token` asks for.
#[derive(Debug, Clone)]
pub struct TokenRequest {
    /// The party to log in as.
    pub party_id: String,
    /// The node's REST address, `host:port`.
    pub node: String,
    /// SHA-256 of the node certificate's SPKI.
    pub pin: SpkiPin,
    /// The party's identity file.
    pub identity: PathBuf,
    /// The token class: `browser` (15 minutes) or `session` (one hour).
    pub class: TokenClass,
}

/// Run `party token`: check and read the identity file, then log in to the
/// node's REST port over TLS 1.3 pinned to `pin`, with the certificate
/// binding, under the retry rule, and return the token.
pub async fn request_token(request: &TokenRequest) -> Result<IssuedToken, NodeError> {
    if !auth::is_valid_party_id(&request.party_id) {
        return Err(NodeError::Identity(
            "a party id is 1 to 64 characters of A-Z, a-z, 0-9, _ and -".to_string(),
        ));
    }
    let keypair = read_identity_keypair(&request.identity)?;
    rest_login_with(
        || TcpStream::connect(request.node.as_str()),
        request.pin,
        &request.party_id,
        &keypair,
        request.class,
        &RetryPolicy::default(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(u64);

    impl RandomSource for Fixed {
        fn next_u64(&self) -> u64 {
            self.0
        }
    }

    #[test]
    fn the_wait_is_retry_after_or_one_second_plus_the_jitter() {
        let policy = RetryPolicy::with_random(Arc::new(Fixed(250)));
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(policy.wait_before_retry(None), Duration::from_millis(1_250));
        assert_eq!(policy.wait_before_retry(Some(Duration::from_secs(1))), Duration::from_millis(1_250));
        assert_eq!(policy.wait_before_retry(Some(Duration::from_secs(4))), Duration::from_millis(4_250));
    }

    #[test]
    fn only_a_close_before_challenge_is_a_refusal() {
        assert!(matches!(closed_before_challenge(NodeError::ConnectionClosed), NodeError::Refused { .. }));
        let reset = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
        assert!(matches!(closed_before_challenge(NodeError::Io(reset)), NodeError::Refused { .. }));
        assert!(matches!(closed_before_challenge(NodeError::Tls("pin".into())), NodeError::Tls(_)));
        assert!(matches!(closed_before_challenge(NodeError::Decode("x".into())), NodeError::Decode(_)));
    }
}
