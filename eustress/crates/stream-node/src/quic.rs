//! QUIC transport for EustressStream nodes.
//!
//! Uses Quinn (Rust QUIC / RFC 9000) with a self-signed TLS certificate for
//! loopback and LAN operation. Suitable for Forge cluster mesh.
//!
//! ## Why QUIC over TCP?
//!
//! - **0-RTT reconnection** — clients resume without a handshake round-trip
//! - **Stream multiplexing** — no head-of-line blocking: pub and sub streams are independent
//! - **UDP-native** — lower kernel overhead on loopback; avoids Nagle's algorithm
//! - **Built-in TLS 1.3** — Forge cluster mesh encryption out of the box
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
//! use eustress_stream_node::NodeConfig;
//!
//! #[tokio::main]
//! async fn main() {
//!     let node = QuicNode::start(NodeConfig::default()).await.unwrap();
//!     println!("QUIC node on {}", node.listen_addr());
//!
//!     let client = QuicNodeClient::connect(node.listen_addr(), node.cert_der()).await.unwrap();
//!     let offset = client.publish("scene_deltas", bytes::Bytes::from_static(b"hello")).await.unwrap();
//! }
//! ```

#![cfg(feature = "quic")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use quinn::{
    ClientConfig, Endpoint, RecvStream, SendStream, ServerConfig, TransportConfig,
};
use rcgen::generate_simple_self_signed;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, info, warn};

use eustress_stream::EustressStream;

use crate::config::NodeConfig;
use crate::error::NodeError;
use crate::protocol::{ClientFrame, ServerFrame, TopicStats};

// ─────────────────────────────────────────────────────────────────────────────
// TLS helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Install the rustls ring crypto provider once per process.
/// Idempotent — safe to call multiple times.
pub fn install_crypto_provider() {
    static INSTALLED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    INSTALLED.get_or_init(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok(); // ignore if already installed
    });
}

/// Generate a self-signed certificate for the given server name.
pub fn generate_self_signed(server_name: &str) -> (CertificateDer<'static>, PrivatePkcs8KeyDer<'static>) {
    install_crypto_provider();
    let cert = generate_simple_self_signed(vec![server_name.to_string()])
        .expect("self-signed cert generation");
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());
    (cert_der, key_der)
}

fn server_config(cert: CertificateDer<'static>, key: PrivatePkcs8KeyDer<'static>) -> ServerConfig {
    let mut tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key.into())
        .expect("TLS server config");
    tls.alpn_protocols = vec![b"eustress/1".to_vec()];

    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(5)));
    transport.max_idle_timeout(Some(Duration::from_secs(30).try_into().unwrap()));

    let mut cfg = ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(tls).expect("QUIC server config"),
    ));
    cfg.transport_config(Arc::new(transport));
    cfg
}

fn client_config_from_cert(cert: &CertificateDer<'static>) -> ClientConfig {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.clone()).expect("add server cert");

    let mut tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"eustress/1".to_vec()];

    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(5)));

    let mut cfg = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(tls).expect("QUIC client config"),
    ));
    cfg.transport_config(Arc::new(transport));
    cfg
}

// ─────────────────────────────────────────────────────────────────────────────
// QuicNode — server side
// ─────────────────────────────────────────────────────────────────────────────

/// A QUIC-transport EustressStream node.
///
/// Each QUIC connection opens two unidirectional streams:
/// - stream 0: client → server (ClientFrames)
/// - stream 1: server → client (ServerFrames)
pub struct QuicNode {
    pub stream: EustressStream,
    pub listen_addr: SocketAddr,
    cert_der: CertificateDer<'static>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
}

impl QuicNode {
    pub async fn start(config: NodeConfig) -> Result<Arc<Self>, NodeError> {
        let (cert_der, key_der) = generate_self_signed("eustress-node");
        let server_cfg = server_config(cert_der.clone(), key_der);

        let addr: SocketAddr = format!("0.0.0.0:{}", config.port).parse().unwrap();
        let endpoint = Endpoint::server(server_cfg, addr)
            .map_err(|e| NodeError::Io(std::io::Error::other(e.to_string())))?;
        let listen_addr = endpoint.local_addr()
            .map_err(NodeError::Io)?;

        info!("EustressStream QUIC node listening on {listen_addr}");

        let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);
        let stream = EustressStream::new(config.stream_config.clone());

        let node = Arc::new(QuicNode {
            stream: stream.clone(),
            listen_addr,
            cert_der,
            shutdown_tx: shutdown_tx.clone(),
        });

        let node_ref = Arc::clone(&node);
        let cap = config.connection_channel_capacity;
        let max_frame = config.frame_max_bytes;
        tokio::spawn(async move {
            node_ref.accept_loop(endpoint, cap, max_frame).await;
        });

        Ok(node)
    }

    /// DER-encoded server certificate — pass this to `QuicNodeClient::connect`.
    pub fn cert_der(&self) -> &CertificateDer<'static> {
        &self.cert_der
    }

    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    async fn accept_loop(&self, endpoint: Endpoint, cap: usize, max_frame: usize) {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        loop {
            tokio::select! {
                Some(conn) = endpoint.accept() => {
                    let stream = self.stream.clone();
                    tokio::spawn(async move {
                        match conn.await {
                            Ok(c) => handle_quic_connection(c, stream, cap, max_frame).await,
                            Err(e) => warn!("QUIC connection error: {e}"),
                        }
                    });
                }
                _ = shutdown_rx.recv() => {
                    endpoint.close(0u32.into(), b"shutdown");
                    break;
                }
            }
        }
    }
}

async fn handle_quic_connection(
    conn: quinn::Connection,
    stream: EustressStream,
    cap: usize,
    max_frame: usize,
) {
    // Each QUIC connection uses a single bidirectional stream for frames.
    match conn.accept_bi().await {
        Ok((send, recv)) => {
            quic_connection_loop(send, recv, stream, cap, max_frame).await;
        }
        Err(e) => {
            debug!("QUIC bi-stream accept error: {e}");
        }
    }
}

async fn quic_connection_loop(
    send: SendStream,
    mut recv: RecvStream,
    stream: EustressStream,
    cap: usize,
    max_frame: usize,
) {
    let send = Arc::new(Mutex::new(send));

    loop {
        let frame = match read_quic_frame(&mut recv, max_frame).await {
            Ok(f) => f,
            Err(NodeError::ConnectionClosed) => break,
            Err(e) => { warn!("QUIC recv error: {e}"); break; }
        };

        let reply = dispatch_quic_frame(frame, &stream, cap, Arc::clone(&send)).await;
        if let Some(reply) = reply {
            let mut guard = send.lock().await;
            if write_quic_frame(&mut *guard, &reply).await.is_err() {
                break;
            }
        }
    }
}

async fn dispatch_quic_frame(
    frame: ClientFrame,
    stream: &EustressStream,
    cap: usize,
    send: Arc<Mutex<SendStream>>,
) -> Option<ServerFrame> {
    match frame {
        ClientFrame::Publish { topic, payload } => {
            let offset = stream.producer(&topic).send_bytes(Bytes::from(payload));
            Some(ServerFrame::Ack { offset })
        }
        ClientFrame::PublishBatch { messages } => {
            let offsets: Vec<u64> = messages.into_iter()
                .map(|(t, p)| stream.producer(&t).send_bytes(Bytes::from(p)))
                .collect();
            Some(ServerFrame::BatchAck { offsets })
        }
        ClientFrame::Subscribe { topic, from_offset } => {
            let (tx, mut rx) = mpsc::channel(cap);
            let _ = stream.subscribe_owned(&topic, move |msg| {
                let _ = tx.try_send(msg);
            });

            if let Some(off) = from_offset {
                let send_ref = Arc::clone(&send);
                stream.replay_ring(&topic, off, |view| {
                    let f = ServerFrame::Message {
                        topic: view.topic.to_string(),
                        offset: view.offset,
                        timestamp: view.timestamp,
                        payload: view.data.to_vec(),
                    };
                    let s = Arc::clone(&send_ref);
                    tokio::spawn(async move {
                        let mut g = s.lock().await;
                        let _ = write_quic_frame(&mut *g, &f).await;
                    });
                });
            }

            tokio::spawn(async move {
                while let Some(msg) = rx.recv().await {
                    let f = ServerFrame::Message {
                        topic: msg.topic.clone(),
                        offset: msg.offset,
                        timestamp: msg.timestamp,
                        payload: msg.data.to_vec(),
                    };
                    let mut g = send.lock().await;
                    if write_quic_frame(&mut *g, &f).await.is_err() {
                        break;
                    }
                }
            });
            None
        }
        ClientFrame::ListTopics => {
            let stats: Vec<TopicStats> = stream.topics().into_iter().map(|name| {
                let head = stream.head(&name);
                let subscriber_count = stream.subscriber_count(&name);
                TopicStats { name, head, subscriber_count }
            }).collect();
            Some(ServerFrame::TopicList(stats))
        }
        ClientFrame::PublishBatchTopic { topic, payloads } => {
            let count = payloads.len() as u32;
            let producer = stream.producer(&topic);
            let mut first_offset = 0u64;
            for (i, payload) in payloads.into_iter().enumerate() {
                let offset = producer.send_bytes(Bytes::from(payload));
                if i == 0 { first_offset = offset; }
            }
            Some(ServerFrame::BatchAckCompact { first_offset, count })
        }
        ClientFrame::PublishNoAck { topic, payload } => {
            stream.producer(&topic).send_bytes(Bytes::from(payload));
            None
        }
        ClientFrame::PublishBatchNoAck { topic, payloads } => {
            let producer = stream.producer(&topic);
            for payload in payloads {
                producer.send_bytes(Bytes::from(payload));
            }
            None
        }
        ClientFrame::Ping => Some(ServerFrame::Pong),
        ClientFrame::Unsubscribe { .. } => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// QuicNodeClient
// ─────────────────────────────────────────────────────────────────────────────

/// QUIC client — connects to a `QuicNode`.
#[derive(Clone)]
pub struct QuicNodeClient {
    inner: Arc<QuicClientInner>,
}

struct QuicClientInner {
    send: Mutex<SendStream>,
    ack_queue: Mutex<std::collections::VecDeque<oneshot::Sender<u64>>>,
    batch_ack_queue: Mutex<std::collections::VecDeque<oneshot::Sender<Vec<u64>>>>,
    topic_list_waiter: Mutex<Option<oneshot::Sender<Vec<TopicStats>>>>,
    sub_channels: dashmap::DashMap<String, mpsc::Sender<(u64, u64, Vec<u8>)>>,
}

impl QuicNodeClient {
    pub async fn connect(
        addr: SocketAddr,
        server_cert: &CertificateDer<'static>,
    ) -> Result<Self, NodeError> {
        let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let mut endpoint = Endpoint::client(bind)
            .map_err(|e| NodeError::Io(std::io::Error::other(e.to_string())))?;
        endpoint.set_default_client_config(client_config_from_cert(server_cert));

        let conn = endpoint.connect(addr, "eustress-node")
            .map_err(|e| NodeError::Io(std::io::Error::other(e.to_string())))?
            .await
            .map_err(|e| NodeError::Io(std::io::Error::other(e.to_string())))?;

        let (send, recv) = conn.open_bi().await
            .map_err(|e| NodeError::Io(std::io::Error::other(e.to_string())))?;

        let inner = Arc::new(QuicClientInner {
            send: Mutex::new(send),
            ack_queue: Mutex::new(std::collections::VecDeque::new()),
            batch_ack_queue: Mutex::new(std::collections::VecDeque::new()),
            topic_list_waiter: Mutex::new(None),
            sub_channels: dashmap::DashMap::new(),
        });

        let inner_ref = Arc::clone(&inner);
        tokio::spawn(async move {
            quic_client_reader(recv, inner_ref).await;
        });

        Ok(QuicNodeClient { inner })
    }

    pub async fn publish(&self, topic: &str, payload: Bytes) -> Result<u64, NodeError> {
        let (tx, rx) = oneshot::channel::<u64>();
        self.inner.ack_queue.lock().await.push_back(tx);
        let mut send = self.inner.send.lock().await;
        write_quic_frame(&mut *send, &ClientFrame::Publish {
            topic: topic.to_string(),
            payload: payload.to_vec(),
        }).await?;
        drop(send);
        rx.await.map_err(|_| NodeError::ConnectionClosed)
    }

    pub async fn publish_batch(&self, messages: Vec<(String, Bytes)>) -> Result<Vec<u64>, NodeError> {
        let (tx, rx) = oneshot::channel::<Vec<u64>>();
        self.inner.batch_ack_queue.lock().await.push_back(tx);
        let mut send = self.inner.send.lock().await;
        write_quic_frame(&mut *send, &ClientFrame::PublishBatch {
            messages: messages.into_iter().map(|(t, p)| (t, p.to_vec())).collect(),
        }).await?;
        drop(send);
        rx.await.map_err(|_| NodeError::ConnectionClosed)
    }

    pub async fn ping(&self) -> Result<(), NodeError> {
        let mut send = self.inner.send.lock().await;
        write_quic_frame(&mut *send, &ClientFrame::Ping).await
    }
}

async fn quic_client_reader(mut recv: RecvStream, inner: Arc<QuicClientInner>) {
    loop {
        match read_quic_frame(&mut recv, 16 * 1024 * 1024).await {
            Ok(ServerFrame::Ack { offset }) => {
                if let Some(tx) = inner.ack_queue.lock().await.pop_front() {
                    let _ = tx.send(offset);
                }
            }
            Ok(ServerFrame::BatchAck { offsets }) => {
                if let Some(tx) = inner.batch_ack_queue.lock().await.pop_front() {
                    let _ = tx.send(offsets);
                }
            }
            Ok(ServerFrame::TopicList(list)) => {
                if let Some(tx) = inner.topic_list_waiter.lock().await.take() {
                    let _ = tx.send(list);
                }
            }
            Ok(ServerFrame::Message { topic, offset, timestamp, payload }) => {
                if let Some(ch) = inner.sub_channels.get(&topic) {
                    let _ = ch.try_send((offset, timestamp, payload));
                }
            }
            Ok(ServerFrame::BatchAckCompact { .. }) => {
                // QUIC QuicNodeClient doesn't yet expose publish_batch_topic —
                // ignore compact acks until the QUIC client is extended.
            }
            Ok(ServerFrame::Pong) => {}
            Ok(ServerFrame::Error { code, message }) => {
                warn!("QUIC server error {code}: {message}");
            }
            Err(NodeError::ConnectionClosed) => break,
            Err(e) => { warn!("QUIC client read error: {e}"); break; }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Frame codec over QUIC streams
// ─────────────────────────────────────────────────────────────────────────────

async fn write_quic_frame(send: &mut SendStream, frame: &impl serde::Serialize) -> Result<(), NodeError> {
    let payload = bincode::serialize(frame)
        .map_err(|e| NodeError::Encode(e.to_string()))?;
    let len = payload.len() as u64;
    send.write_all(&len.to_le_bytes()).await
        .map_err(|e| NodeError::Io(std::io::Error::other(e.to_string())))?;
    send.write_all(&payload).await
        .map_err(|e| NodeError::Io(std::io::Error::other(e.to_string())))?;
    Ok(())
}

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
