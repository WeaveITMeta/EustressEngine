//! QUIC transport for EustressStream nodes.
//!
//! Uses Quinn (Rust QUIC / RFC 9000) with the node's operator-supplied
//! certificate: TLS 1.3, and no early data in either direction. Suitable for
//! the Forge cluster mesh.
//!
//! Every connection logs in. The node opens one bidirectional stream, sends
//! `Challenge` on it, and the client answers `Hello`, signed over the QUIC
//! transcript (label `eustress-stream-login-quic-v1`, bound to this
//! connection's exporter). After `Welcome` the same stream carries frames
//! under the party's grants, through the dispatcher TCP uses. A client cannot
//! open streams of its own.
//!
//! ## Why QUIC over TCP?
//!
//! - **Stream multiplexing** — no head-of-line blocking: pub and sub streams are independent
//! - **UDP-native** — lower kernel overhead on loopback; avoids Nagle's algorithm
//! - **Built-in TLS 1.3** — Forge cluster mesh encryption out of the box
//!
//! 0-RTT is off: the node accepts no early data and clients send none.
//!
//! The node answers every address it has not validated with a Retry. Only a
//! validated address, one whose Initial carries the Retry token, takes a
//! pre-login slot, so a spoofed source address can hold none.
//!
//! ## Feature gate
//!
//! Enable with `features = ["quic"]` in Cargo.toml.
//!
//! ```toml
//! eustress-stream-node = { path = "...", features = ["quic"] }
//! ```
//!
//! ## Quick start
//!
//! ```rust,no_run,ignore
//! use eustress_stream_node::quic::{QuicNode, QuicNodeClient};
//! use eustress_stream_node::{ClientAuth, NodeConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = NodeConfig::default()
//!         .with_tls("node.crt", "private/node.key")
//!         .with_parties("private/parties.toml");
//!     let node = QuicNode::start(config).await.unwrap();
//!     println!("QUIC node on {} (pin {})", node.listen_addr(), node.spki_pin());
//!
//!     let auth: ClientAuth = todo!("party id, the party's keypair, and the node's pin");
//!     let client = QuicNodeClient::connect(node.listen_addr(), &auth).await.unwrap();
//!     let offset = client.publish("log/output", bytes::Bytes::from_static(b"hello")).await.unwrap();
//! }
//! ```

#![cfg(feature = "quic")]

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use quinn::{
    Endpoint, RecvStream, SendStream,
};
use rustls::pki_types::CertificateDer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tracing::{debug, info, warn};

use eustress_stream::EustressStream;

use crate::auth::{self, Channel, HelloOutcome, NodeAuth, Party, PreLoginPermit, SessionGuard};
use crate::client::{
    client_login, close_conn, send_frame, spawn_writer, with_retry, ClientAuth, Conn, Outgoing, Pending, RetryPolicy,
};
use crate::config::NodeConfig;
use crate::error::NodeError;
use crate::handler::{serve_party, ConnectionContext};
use crate::protocol::{read_client_frame, write_server_frame, ClientFrame, ServerFrame};
use crate::transport::{self, SpkiPin};

// ─────────────────────────────────────────────────────────────────────────────
// QuicNode — server side
// ─────────────────────────────────────────────────────────────────────────────

/// A QUIC-transport EustressStream node.
///
/// Each connection carries one bidirectional stream, opened by the node: the
/// login first, then client frames one way and server frames the other.
pub struct QuicNode {
    pub stream: EustressStream,
    pub listen_addr: SocketAddr,
    cert_der: CertificateDer<'static>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    spki_pin: SpkiPin,
    auth: Arc<NodeAuth>,
}

impl QuicNode {
    /// Run the startup checks (see [`NodeAuth::from_config`]), then listen.
    /// The node uses the configured certificate; it never generates one.
    pub async fn start(config: NodeConfig) -> Result<Arc<Self>, NodeError> {
        let auth = NodeAuth::from_config(&config)?;
        let stream = EustressStream::new(config.stream_config.clone());
        Self::start_with(stream, config, auth).await
    }

    /// Listen with a given stream and authentication state that already
    /// passed the startup checks.
    pub async fn start_with(
        stream: EustressStream,
        config: NodeConfig,
        auth: Arc<NodeAuth>,
    ) -> Result<Arc<Self>, NodeError> {
        let server_cfg = auth.tls().quic_server_config()?;

        let addr = SocketAddr::new(config.bind_addr, config.port);
        let endpoint = Endpoint::server(server_cfg, addr)
            .map_err(|e| NodeError::Io(std::io::Error::other(e.to_string())))?;
        let listen_addr = endpoint.local_addr()
            .map_err(NodeError::Io)?;

        info!("EustressStream QUIC node listening on {listen_addr}");
        info!("TLS 1.3, no early data. Clients pin SPKI SHA-256 {}", auth.tls().spki_pin());

        let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);
        let ctx = ConnectionContext::new(
            stream.clone(),
            Arc::clone(&auth),
            config.frame_max_bytes,
            config.connection_channel_capacity,
        );

        let node = Arc::new(QuicNode {
            stream: stream.clone(),
            listen_addr,
            cert_der: auth.tls().certificate().clone(),
            shutdown_tx: shutdown_tx.clone(),
            spki_pin: auth.tls().spki_pin(),
            auth: Arc::clone(&auth),
        });

        auth.spawn_revocation_task(Some(shutdown_tx.subscribe()));

        let node_ref = Arc::clone(&node);
        tokio::spawn(async move {
            node_ref.accept_loop(endpoint, ctx).await;
        });

        Ok(node)
    }

    /// The DER certificate this node presents.
    pub fn cert_der(&self) -> &CertificateDer<'static> {
        &self.cert_der
    }

    /// The pin a client passes to [`QuicNodeClient::connect`] through
    /// [`ClientAuth`]: SHA-256 of this node's certificate SPKI.
    pub fn spki_pin(&self) -> SpkiPin {
        self.spki_pin
    }

    /// The node's authentication and authorization state.
    pub fn auth(&self) -> &Arc<NodeAuth> {
        &self.auth
    }

    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    async fn accept_loop(&self, endpoint: Endpoint, ctx: Arc<ConnectionContext>) {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        loop {
            tokio::select! {
                Some(incoming) = endpoint.accept() => {
                    let peer = incoming.remote_address();
                    // VERIFY: quinn 0.11 Incoming::remote_address_validated(&self) -> bool,
                    // Incoming::retry(self) -> Result<(), RetryError> and
                    // RetryError::into_incoming(self) -> Incoming, copied from
                    // quinn-0.11.11/src/incoming.rs.
                    if !incoming.remote_address_validated() {
                        // An address the node has not validated gets a Retry and
                        // takes no pre-login slot. Its next Initial carries the
                        // token, which proves the address; only that one takes a
                        // slot. An unvalidated Initial can always be retried, so
                        // the refusal below is a guard, not a path.
                        if let Err(refused) = incoming.retry() {
                            refused.into_incoming().refuse();
                        }
                    } else {
                        // Take a pre-login slot, or refuse the connection at accept.
                        let accepted_at = Instant::now();
                        match ctx.auth.pre_login.try_acquire(peer.ip()) {
                            Some(permit) => {
                                let ctx = Arc::clone(&ctx);
                                tokio::spawn(async move {
                                    handle_quic_connection(incoming, peer, permit, ctx, accepted_at).await;
                                });
                            }
                            None => {
                                ctx.auth.log_refused_at_accept(peer.ip(), Channel::Quic);
                                incoming.refuse();
                            }
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    endpoint.close(0u32.into(), b"shutdown");
                    break;
                }
            }
        }
    }
}

/// Log in within 5 seconds of accept (the QUIC handshake counts), then serve
/// frames under the party's grants until the peer closes or the party is
/// revoked.
async fn handle_quic_connection(
    incoming: quinn::Incoming,
    peer: SocketAddr,
    permit: PreLoginPermit,
    ctx: Arc<ConnectionContext>,
    accepted_at: Instant,
) {
    let deadline = accepted_at + auth::LOGIN_DEADLINE;
    let login = tokio::time::timeout_at(deadline, login_quic(incoming, peer, &ctx, deadline)).await;
    drop(permit);
    let (conn, send, recv, party, session) = match login {
        Ok(Some(done)) => done,
        Ok(None) => return,
        Err(_) => {
            ctx.auth.log_login_failure(
                peer.ip(),
                Channel::Quic,
                None,
                auth::REASON_AUTH_FAILED,
                "no Welcome within 5 seconds of accept",
            );
            return;
        }
    };

    info!(party = %party.id, source = %peer, channel = "quic", "login succeeded");
    serve_party(recv, send, peer, party, session, &ctx).await;
    conn.close(0u32.into(), b"closed");
}

/// The QUIC handshake, then `Challenge` on a stream the node opens, `Hello`,
/// and `Welcome` or `AuthFailed`. The party's session is registered before
/// `Welcome` goes out.
async fn login_quic(
    incoming: quinn::Incoming,
    peer: SocketAddr,
    ctx: &ConnectionContext,
    deadline: Instant,
) -> Option<(quinn::Connection, SendStream, RecvStream, Arc<Party>, SessionGuard)> {
    let auth = &ctx.auth;
    let conn = match incoming.await {
        Ok(conn) => conn,
        Err(e) => {
            debug!(source = %peer, "QUIC handshake failed: {e}");
            auth.log_login_failure(peer.ip(), Channel::Quic, None, auth::REASON_AUTH_FAILED, "QUIC handshake failed");
            return None;
        }
    };
    let exporter = match transport::quic_exporter(&conn) {
        Ok(exporter) => exporter,
        Err(e) => {
            warn!(source = %peer, "cannot derive the QUIC login binding: {e}");
            return None;
        }
    };
    let (mut send, mut recv) = match conn.open_bi().await {
        Ok(streams) => streams,
        Err(e) => {
            debug!(source = %peer, "cannot open the login stream: {e}");
            return None;
        }
    };

    let nonce = auth::fresh_nonce();
    let challenge = ServerFrame::Challenge { node_id: auth.node_id().to_string(), nonce };
    if write_server_frame(&mut send, &challenge).await.is_err() {
        return None;
    }

    let first = match read_client_frame(&mut recv, auth::PRE_LOGIN_MAX_FRAME).await {
        Ok(frame) => frame,
        Err(e) => {
            let cause = match e {
                NodeError::FrameTooLarge(_) => "pre-login frame over 64 KB",
                NodeError::Decode(_) => "first frame does not decode",
                _ => "closed before Hello",
            };
            auth.log_login_failure(peer.ip(), Channel::Quic, None, auth::REASON_AUTH_FAILED, cause);
            return None;
        }
    };
    let ClientFrame::Hello { protocol_version, party_id, signature } = first else {
        auth.log_login_failure(peer.ip(), Channel::Quic, None, auth::REASON_AUTH_FAILED, "first frame was not Hello");
        conn.close(0u32.into(), b"protocol");
        return None;
    };

    match auth.check_hello(Channel::Quic, &nonce, &exporter, protocol_version, &party_id, &signature) {
        HelloOutcome::Welcome(party) => {
            // The session is registered before `Welcome` goes out, and the
            // party is checked again right after, so a revocation that lands
            // during the login is never missed.
            let Some(session) = auth.register_session(&party) else {
                auth.log_login_failure(
                    peer.ip(),
                    Channel::Quic,
                    Some(&party.id),
                    auth::REASON_AUTH_FAILED,
                    "party revoked during the login",
                );
                let reason = auth::REASON_AUTH_FAILED.to_string();
                let _ = write_server_frame(&mut send, &ServerFrame::AuthFailed { reason }).await;
                let _ = send.finish();
                let _ = tokio::time::timeout_at(deadline, conn.closed()).await;
                conn.close(0u32.into(), b"auth_failed");
                return None;
            };
            let welcome = ServerFrame::Welcome { party_id: party.id.clone() };
            if write_server_frame(&mut send, &welcome).await.is_err() {
                return None;
            }
            Some((conn, send, recv, party, session))
        }
        HelloOutcome::Refused(refusal) => {
            auth.log_login_failure(
                peer.ip(),
                Channel::Quic,
                refusal.enrolled_party.as_deref(),
                refusal.reason,
                refusal.cause,
            );
            let _ = write_server_frame(&mut send, &ServerFrame::AuthFailed { reason: refusal.reason.to_string() }).await;
            let _ = send.finish();
            // The client closes once it reads the refusal; the deadline bounds
            // the wait for a client that does not.
            let _ = tokio::time::timeout_at(deadline, conn.closed()).await;
            conn.close(0u32.into(), b"auth_failed");
            None
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// QuicNodeClient
// ─────────────────────────────────────────────────────────────────────────────

/// QUIC client — connects to a `QuicNode`.
///
/// It pins the node's certificate (SPKI SHA-256) and logs in before its
/// first frame.
#[derive(Clone)]
pub struct QuicNodeClient {
    inner: Arc<QuicClientInner>,
}

struct QuicClientInner {
    // The writer task's channel: every frame goes out through it, and the
    // task queues each request's waiter before it writes the frame.
    outgoing: mpsc::Sender<Outgoing>,
}

impl QuicNodeClient {
    /// Connect over QUIC pinned to `auth.pin`, then log in as `auth.party_id`
    /// before the first frame. A connection the node refuses or closes before
    /// `Challenge` (a pre-login limit) is retried under
    /// [`RetryPolicy::default`]: at most three attempts in all.
    pub async fn connect(
        addr: SocketAddr,
        auth: &ClientAuth,
    ) -> Result<Self, NodeError> {
        Self::connect_with_policy(addr, auth, &RetryPolicy::default()).await
    }

    /// The same with a retry policy of the caller's choosing.
    pub async fn connect_with_policy(addr: SocketAddr, auth: &ClientAuth, policy: &RetryPolicy) -> Result<Self, NodeError> {
        with_retry(policy, || Self::connect_once(addr, auth)).await
    }

    async fn connect_once(addr: SocketAddr, auth: &ClientAuth) -> Result<Self, NodeError> {
        let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let mut endpoint = Endpoint::client(bind)
            .map_err(|e| NodeError::Io(std::io::Error::other(e.to_string())))?;
        endpoint.set_default_client_config(transport::quic_client_config(auth.pin)?);

        let conn = endpoint.connect(addr, "eustress-node")
            .map_err(|e| NodeError::Io(std::io::Error::other(e.to_string())))?
            .await
            .map_err(handshake_error)?;

        // The node opens the one stream and sends `Challenge` on it.
        let exporter = transport::quic_exporter(&conn)?;
        let (mut send, mut recv) = conn.accept_bi().await
            .map_err(closed_before_challenge)?;
        client_login(&mut recv, &mut send, auth, Channel::Quic, &exporter).await?;

        let conn = Conn::new(send);
        let outgoing = spawn_writer(Arc::clone(&conn));
        tokio::spawn(async move {
            quic_client_reader(recv, conn).await;
        });

        Ok(QuicNodeClient { inner: Arc::new(QuicClientInner { outgoing }) })
    }

    pub async fn publish(&self, topic: &str, payload: Bytes) -> Result<u64, NodeError> {
        let (tx, rx) = oneshot::channel::<Result<u64, NodeError>>();
        self.request(ClientFrame::Publish {
            topic: topic.to_string(),
            payload: payload.to_vec(),
        }, Pending::Ack(tx)).await?;
        rx.await.map_err(|_| NodeError::ConnectionClosed)?
    }

    pub async fn publish_batch(&self, messages: Vec<(String, Bytes)>) -> Result<Vec<u64>, NodeError> {
        let (tx, rx) = oneshot::channel::<Result<Vec<u64>, NodeError>>();
        self.request(ClientFrame::PublishBatch {
            messages: messages.into_iter().map(|(t, p)| (t, p.to_vec())).collect(),
        }, Pending::BatchAck(tx)).await?;
        rx.await.map_err(|_| NodeError::ConnectionClosed)?
    }

    /// Ping the node and wait for its `Pong`.
    pub async fn ping(&self) -> Result<(), NodeError> {
        let (tx, rx) = oneshot::channel::<Result<(), NodeError>>();
        self.request(ClientFrame::Ping, Pending::Pong(tx)).await?;
        rx.await.map_err(|_| NodeError::ConnectionClosed)?
    }

    /// Hand a request to the writer task, which queues `pending` and then
    /// writes the frame, with nothing in between.
    async fn request(&self, frame: ClientFrame, pending: Pending) -> Result<(), NodeError> {
        send_frame(&self.inner.outgoing, frame, Some(pending)).await
    }
}

async fn quic_client_reader(mut recv: RecvStream, conn: Arc<Conn<SendStream>>) {
    loop {
        match read_quic_frame(&mut recv, 16 * 1024 * 1024).await {
            Ok(ServerFrame::Message { topic, offset, timestamp, payload }) => {
                if let Some(ch) = conn.sub_channels.get(&topic) {
                    let _ = ch.try_send((offset, timestamp, payload));
                }
            }
            // Every other frame is the reply to the front request.
            Ok(reply) => {
                if let Err(e) = conn.pending.lock().await.answer(reply, &conn.sub_channels) {
                    warn!("QUIC: {e}; closing the connection");
                    break;
                }
            }
            Err(NodeError::ConnectionClosed) => break,
            Err(e) => { warn!("QUIC client read error: {e}"); break; }
        }
    }

    // The connection is gone, or the node broke the reply order: every
    // request still waiting fails, none is queued after this, and the send
    // side finishes.
    close_conn(&conn).await;
}

/// A failed QUIC handshake. A node that refuses the connection at accept (a
/// pre-login limit) closes it with CONNECTION_REFUSED; that, a reset, and a
/// close by the node are refusals the retry rule covers. Anything else, a
/// TLS failure such as a pin mismatch among them, is not retried.
fn handshake_error(error: quinn::ConnectionError) -> NodeError {
    match &error {
        // VERIFY: quinn 0.11 reports Incoming::refuse() to the client as
        // ConnectionError::ConnectionClosed with error_code
        // TransportErrorCode::CONNECTION_REFUSED (quinn-proto-0.11.15
        // src/endpoint.rs, Endpoint::refuse).
        quinn::ConnectionError::ConnectionClosed(close)
            if close.error_code == quinn::TransportErrorCode::CONNECTION_REFUSED =>
        {
            NodeError::Refused { reason: format!("the node refused the QUIC connection ({error})"), retry_after: None }
        }
        quinn::ConnectionError::ApplicationClosed(_) | quinn::ConnectionError::Reset => {
            NodeError::Refused { reason: format!("the node closed the QUIC connection ({error})"), retry_after: None }
        }
        _ => NodeError::Tls(format!("QUIC handshake with the node failed: {error}")),
    }
}

/// The node's login stream never arrived: the connection ended before
/// `Challenge`, which the retry rule covers. A timeout is not retried.
fn closed_before_challenge(error: quinn::ConnectionError) -> NodeError {
    match &error {
        quinn::ConnectionError::ConnectionClosed(_)
        | quinn::ConnectionError::ApplicationClosed(_)
        | quinn::ConnectionError::Reset => NodeError::Refused {
            reason: format!("the node closed the QUIC connection before Challenge ({error})"),
            retry_after: None,
        },
        _ => NodeError::Io(std::io::Error::other(error.to_string())),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Frame codec over QUIC streams
// ─────────────────────────────────────────────────────────────────────────────

async fn read_quic_frame<T: serde::de::DeserializeOwned>(
    recv: &mut RecvStream,
    max_bytes: usize,
) -> Result<T, NodeError> {
    let mut len_buf = [0u8; 8];
    recv.read_exact(&mut len_buf).await
        .map_err(|e| match e {
            quinn::ReadExactError::FinishedEarly(_) => NodeError::ConnectionClosed,
            quinn::ReadExactError::ReadError(e) => NodeError::Io(std::io::Error::other(e.to_string())),
        })?;
    let len = u64::from_le_bytes(len_buf) as usize;
    if len > max_bytes {
        return Err(NodeError::FrameTooLarge(len));
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await
        .map_err(|e| match e {
            quinn::ReadExactError::FinishedEarly(_) => NodeError::ConnectionClosed,
            quinn::ReadExactError::ReadError(e) => NodeError::Io(std::io::Error::other(e.to_string())),
        })?;
    bincode::deserialize(&buf).map_err(|e| NodeError::Decode(e.to_string()))
}
