//! # Desktop transport: WebTransport over wtransport
//!
//! The IO half of a session on desktop. [`start_host`] listens; [`start_join`]
//! connects. Each returns a [`NetLink`] for the session layer and runs its
//! sockets on a thread of its own with a single-threaded tokio runtime, so
//! nothing here ever blocks a Bevy system and nothing in the session layer
//! ever sees tokio.
//!
//! ## Why WebTransport
//!
//! A browser can open WebTransport but not a raw QUIC connection with a
//! custom protocol. Hosting over WebTransport means one listener, one port and
//! one certificate serve the desktop Player and a browser Player alike.
//!
//! ## Trust
//!
//! - **Certificate.** Each host mints a fresh self-signed ECDSA P-256
//!   certificate valid for 14 days. A player pins its SHA-256, carried in the
//!   join link. Without a pin, a player may only connect to this machine.
//! - **Join key.** A session is requested at `/join/<key>`. A wrong key is
//!   refused with HTTP 403 before any connection state exists. The key
//!   `open` is honoured only from a loopback address.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::sync::mpsc;
use wtransport::endpoint::IncomingSession;
use wtransport::tls::Sha256Digest;
use wtransport::{ClientConfig, Connection, Endpoint, Identity, RecvStream, SendStream, ServerConfig, VarInt};

use crate::join_link::{key_from_path, JoinLink, OPEN_KEY};
use crate::session::{link_pair, ConnId, LinkCommand, LinkEnds, LinkEvent, NetLink, HOST_CONN};
use crate::wire::MAX_FRAME_BYTES;

/// Default port a Studio host listens on.
pub const DEFAULT_PORT: u16 = 7777;

const KEEP_ALIVE: Duration = Duration::from_secs(3);
const IDLE_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a closing connection waits for its last frames to reach the peer.
const CLOSE_GRACE: Duration = Duration::from_millis(250);

/// How a host listens.
#[derive(Debug, Clone)]
pub struct HostOptions {
    /// 0 picks any free port.
    pub port: u16,
    /// Listen on every interface, so players on the local network can join.
    /// Off by default: a host is reachable from this machine only.
    pub lan: bool,
    /// The secret a join link carries. See [`new_join_key`].
    pub join_key: String,
}

/// A fresh join key: 32 random hex characters.
pub fn new_join_key() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// This machine's address on its local network, for a LAN join link.
///
/// Asks the OS which interface would route to a documentation address
/// (TEST-NET-1). A UDP `connect` sends nothing; it only selects a route.
pub fn local_network_ip() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// Where a running host on this machine leaves its join link, one file per
/// port: `<workspace>/.eustress/hosts/<port>.link`. A Player started with just
/// `--connect 127.0.0.1:<port>` reads it, so joining a host on the same
/// machine needs no copying. The file holds the join key; it sits under the
/// user's own workspace, where every process that could read it already runs
/// as that user.
pub fn host_file_path(workspace: &std::path::Path, port: u16) -> std::path::PathBuf {
    workspace.join(".eustress").join("hosts").join(format!("{port}.link"))
}

/// Record a running host's join link. Best effort.
pub fn write_host_file(workspace: &std::path::Path, link: &JoinLink) {
    let path = host_file_path(workspace, link.port);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, link.to_link());
}

/// Forget a host's join link. Best effort.
pub fn remove_host_file(workspace: &std::path::Path, port: u16) {
    let _ = std::fs::remove_file(host_file_path(workspace, port));
}

/// The join link a host on this machine left for `port`, if any.
pub fn read_host_file(workspace: &std::path::Path, port: u16) -> Option<JoinLink> {
    let text = std::fs::read_to_string(host_file_path(workspace, port)).ok()?;
    JoinLink::parse(&text).ok().filter(|l| l.port == port)
}

/// Start listening. Returns at once; the link reports
/// [`LinkEvent::Listening`] with the certificate pin, or
/// [`LinkEvent::Failed`] if the port could not be bound.
pub fn start_host(options: HostOptions) -> Result<NetLink, String> {
    // Names are informational: players pin the certificate hash, not a name.
    let identity = Identity::self_signed(["localhost", "127.0.0.1"])
        .map_err(|e| format!("could not create the host certificate: {e}"))?;
    let pin = certificate_pin(&identity);
    let (link, ends) = link_pair();
    std::thread::Builder::new()
        .name("eustress-net-host".into())
        .spawn(move || {
            let events = ends.events.clone();
            match runtime() {
                Ok(rt) => rt.block_on(host_main(options, identity, pin, ends)),
                Err(reason) => {
                    let _ = events.send(LinkEvent::Failed { reason });
                }
            }
        })
        .map_err(|e| format!("could not start the network thread: {e}"))?;
    Ok(link)
}

/// Connect to a host. Returns at once; the link reports
/// [`LinkEvent::Opened`] when the session is up, or [`LinkEvent::Failed`].
pub fn start_join(link: &JoinLink) -> Result<NetLink, String> {
    if link.pin.is_none() && !link.is_loopback() {
        return Err(format!(
            "{}:{} is not on this machine, so the join link must carry the host's certificate pin. \
             Copy the full link the host shows.",
            link.host, link.port
        ));
    }
    let target = link.clone();
    let (net, ends) = link_pair();
    std::thread::Builder::new()
        .name("eustress-net-join".into())
        .spawn(move || {
            let events = ends.events.clone();
            match runtime() {
                Ok(rt) => rt.block_on(join_main(target, ends)),
                Err(reason) => {
                    let _ = events.send(LinkEvent::Failed { reason });
                }
            }
        })
        .map_err(|e| format!("could not start the network thread: {e}"))?;
    Ok(net)
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("could not start the network runtime: {e}"))
}

fn certificate_pin(identity: &Identity) -> [u8; 32] {
    let digest = identity.certificate_chain().as_slice()[0].hash();
    *AsRef::<[u8; 32]>::as_ref(&digest)
}

/// Commands arrive on a blocking crossbeam channel (the session side must
/// not depend on tokio). A small thread forwards them into the runtime.
fn forward_commands(commands: crossbeam_channel::Receiver<LinkCommand>) -> mpsc::UnboundedReceiver<LinkCommand> {
    let (tx, rx) = mpsc::unbounded_channel();
    let _ = std::thread::Builder::new().name("eustress-net-commands".into()).spawn(move || {
        while let Ok(cmd) = commands.recv() {
            let stop = matches!(cmd, LinkCommand::Shutdown);
            if tx.send(cmd).is_err() || stop {
                break;
            }
        }
    });
    rx
}

/// What a connection's writer task sends, in order.
enum Outbound {
    Frame(Vec<u8>),
    /// Flush what is queued, then close.
    Close(String),
}

struct ConnHandle {
    connection: Connection,
    outbound: mpsc::UnboundedSender<Outbound>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Host
// ─────────────────────────────────────────────────────────────────────────────

async fn host_main(options: HostOptions, identity: Identity, pin: [u8; 32], ends: LinkEnds) {
    let LinkEnds { events, commands } = ends;
    let ip: IpAddr = if options.lan { Ipv4Addr::UNSPECIFIED.into() } else { Ipv4Addr::LOCALHOST.into() };
    let bind = SocketAddr::new(ip, options.port);

    let config = match ServerConfig::builder()
        .with_bind_address(bind)
        .with_identity(identity)
        .keep_alive_interval(Some(KEEP_ALIVE))
        .max_idle_timeout(Some(IDLE_TIMEOUT))
    {
        Ok(builder) => builder.build(),
        Err(_) => {
            let _ = events.send(LinkEvent::Failed { reason: "invalid idle timeout".into() });
            return;
        }
    };
    let endpoint = match Endpoint::server(config) {
        Ok(e) => e,
        Err(e) => {
            let _ = events.send(LinkEvent::Failed {
                reason: format!("could not listen on {bind}: {e}. Is another host already using the port?"),
            });
            return;
        }
    };
    let port = endpoint.local_addr().map(|a| a.port()).unwrap_or(options.port);
    tracing::info!("net: hosting on {ip}:{port}{}", if options.lan { " (local network)" } else { " (this machine only)" });
    let _ = events.send(LinkEvent::Listening { port, cert_sha256: pin });

    let mut commands = forward_commands(commands);
    let (opened_tx, mut opened_rx) = mpsc::unbounded_channel::<(ConnId, ConnHandle)>();
    let (gone_tx, mut gone_rx) = mpsc::unbounded_channel::<ConnId>();
    let mut conns: HashMap<ConnId, ConnHandle> = HashMap::new();
    let mut next_conn: ConnId = 1;

    loop {
        tokio::select! {
            incoming = endpoint.accept() => {
                let conn = next_conn;
                next_conn += 1;
                tokio::spawn(serve_player(
                    incoming,
                    conn,
                    options.join_key.clone(),
                    events.clone(),
                    opened_tx.clone(),
                    gone_tx.clone(),
                ));
            }
            Some((conn, handle)) = opened_rx.recv() => {
                conns.insert(conn, handle);
            }
            Some(conn) = gone_rx.recv() => {
                conns.remove(&conn);
            }
            cmd = commands.recv() => match cmd {
                Some(LinkCommand::Frame { conn, frame }) => {
                    if let Some(h) = conns.get(&conn) {
                        let _ = h.outbound.send(Outbound::Frame(frame));
                    }
                }
                Some(LinkCommand::Datagram { conn, bytes }) => {
                    if let Some(h) = conns.get(&conn) {
                        // A datagram that does not fit is simply dropped:
                        // the next avatar sample replaces it anyway.
                        let _ = h.connection.send_datagram(&bytes);
                    }
                }
                Some(LinkCommand::Close { conn, reason }) => {
                    if let Some(h) = conns.remove(&conn) {
                        let _ = h.outbound.send(Outbound::Close(reason));
                    }
                }
                Some(LinkCommand::Shutdown) | None => break,
            },
        }
    }

    for (_, h) in conns.drain() {
        let _ = h.outbound.send(Outbound::Close("the host stopped".into()));
    }
    tokio::time::sleep(CLOSE_GRACE).await;
    endpoint.close(VarInt::from_u32(0), b"the host stopped");
    let _ = tokio::time::timeout(Duration::from_secs(1), endpoint.wait_idle()).await;
    tracing::info!("net: host stopped");
}

async fn serve_player(
    incoming: IncomingSession,
    conn: ConnId,
    join_key: String,
    events: crossbeam_channel::Sender<LinkEvent>,
    opened: mpsc::UnboundedSender<(ConnId, ConnHandle)>,
    gone: mpsc::UnboundedSender<ConnId>,
) {
    let Ok(request) = incoming.await else { return };
    let remote = request.remote_address();
    let allowed = match key_from_path(request.path()) {
        Some(k) if constant_time_eq(k.as_bytes(), join_key.as_bytes()) => true,
        Some(OPEN_KEY) => remote.ip().is_loopback(),
        _ => false,
    };
    if !allowed {
        tracing::warn!("net: refused a session from {remote}: wrong join key");
        request.forbidden().await;
        return;
    }
    let Ok(connection) = request.accept().await else { return };
    let (send, recv) = match tokio::time::timeout(CONNECT_TIMEOUT, connection.accept_bi()).await {
        Ok(Ok(streams)) => streams,
        _ => {
            connection.close(VarInt::from_u32(1), b"no stream opened");
            return;
        }
    };
    let datagrams = connection.max_datagram_size().is_some();
    let (outbound, outbox) = mpsc::unbounded_channel();
    if opened.send((conn, ConnHandle { connection: connection.clone(), outbound })).is_err() {
        return;
    }
    let _ = events.send(LinkEvent::Opened { conn, remote: remote.to_string(), datagrams });

    let reason = pump_connection(&connection, conn, send, recv, outbox, &events).await;
    let _ = events.send(LinkEvent::Closed { conn, reason });
    let _ = gone.send(conn);
}

// ─────────────────────────────────────────────────────────────────────────────
// Player
// ─────────────────────────────────────────────────────────────────────────────

async fn join_main(link: JoinLink, ends: LinkEnds) {
    let LinkEnds { events, commands } = ends;
    let fail = |reason: String| {
        let _ = events.send(LinkEvent::Failed { reason });
    };

    let builder = ClientConfig::builder().with_bind_default();
    let builder = match link.pin {
        Some(pin) => builder.with_server_certificate_hashes([Sha256Digest::new(pin)]),
        // Loopback only; `start_join` refuses anything else without a pin.
        None => builder.with_no_cert_validation(),
    };
    let config = match builder.keep_alive_interval(Some(KEEP_ALIVE)).max_idle_timeout(Some(IDLE_TIMEOUT)) {
        Ok(b) => b.build(),
        Err(_) => return fail("invalid idle timeout".into()),
    };
    let endpoint = match Endpoint::client(config) {
        Ok(e) => e,
        Err(e) => return fail(format!("could not open a network socket: {e}")),
    };

    let url = link.webtransport_url();
    let target = format!("{}:{}", link.host, link.port);
    let connection = match tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(url)).await {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            return fail(format!(
                "could not reach the host at {target}: {e}. Check the address, that the host is running, \
                 and that the link is current (each hosting session has a new key and pin)."
            ))
        }
        Err(_) => return fail(format!("the host at {target} did not answer within {} seconds", CONNECT_TIMEOUT.as_secs())),
    };
    let (send, recv) = match connection.open_bi().await {
        Ok(opening) => match opening.await {
            Ok(streams) => streams,
            Err(e) => return fail(format!("could not open the session stream: {e}")),
        },
        Err(e) => return fail(format!("could not open the session stream: {e}")),
    };
    let datagrams = connection.max_datagram_size().is_some();
    let _ = events.send(LinkEvent::Opened { conn: HOST_CONN, remote: target, datagrams });

    // One connection, so commands drive it directly. The driver runs as its
    // own task and hands a close to the writer rather than ending anything
    // itself: the connection ends when the writer has flushed, so a Goodbye
    // queued just before the close always reaches the host.
    let (outbound, outbox) = mpsc::unbounded_channel();
    let mut commands = forward_commands(commands);
    let conn_for_commands = connection.clone();
    tokio::spawn(async move {
        while let Some(cmd) = commands.recv().await {
            match cmd {
                LinkCommand::Frame { frame, .. } => {
                    let _ = outbound.send(Outbound::Frame(frame));
                }
                LinkCommand::Datagram { bytes, .. } => {
                    let _ = conn_for_commands.send_datagram(&bytes);
                }
                LinkCommand::Close { reason, .. } => {
                    let _ = outbound.send(Outbound::Close(reason));
                }
                LinkCommand::Shutdown => {
                    let _ = outbound.send(Outbound::Close("the player left".into()));
                    break;
                }
            }
        }
    });
    let reason = pump_connection(&connection, HOST_CONN, send, recv, outbox, &events).await;
    let _ = events.send(LinkEvent::Closed { conn: HOST_CONN, reason });
    endpoint.close(VarInt::from_u32(0), b"done");
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared: one connection's reader, datagram reader and writer
// ─────────────────────────────────────────────────────────────────────────────

/// Run a connection until it ends. Returns why it ended.
async fn pump_connection(
    connection: &Connection,
    conn: ConnId,
    mut send: SendStream,
    mut recv: RecvStream,
    mut outbox: mpsc::UnboundedReceiver<Outbound>,
    events: &crossbeam_channel::Sender<LinkEvent>,
) -> String {
    let reader = async {
        loop {
            match read_frame(&mut recv).await {
                Ok(body) => {
                    if events.send(LinkEvent::Frame { conn, body }).is_err() {
                        return "the session ended".to_string();
                    }
                }
                Err(reason) => return reason,
            }
        }
    };
    let datagrams = async {
        loop {
            match connection.receive_datagram().await {
                Ok(d) => {
                    if events.send(LinkEvent::Datagram { conn, bytes: d.payload().to_vec() }).is_err() {
                        return "the session ended".to_string();
                    }
                }
                Err(e) => return e.to_string(),
            }
        }
    };
    let writer = async {
        while let Some(out) = outbox.recv().await {
            match out {
                Outbound::Frame(frame) => {
                    if let Err(e) = send.write_all(&frame).await {
                        return e.to_string();
                    }
                }
                Outbound::Close(reason) => {
                    let _ = send.finish().await;
                    tokio::time::sleep(CLOSE_GRACE).await;
                    connection.close(VarInt::from_u32(0), reason.as_bytes());
                    return reason;
                }
            }
        }
        "the session ended".to_string()
    };
    tokio::select! {
        r = reader => r,
        r = datagrams => r,
        r = writer => r,
    }
}

async fn read_frame(recv: &mut RecvStream) -> Result<Vec<u8>, String> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await.map_err(|e| e.to_string())?;
    let n = u32::from_le_bytes(len) as usize;
    if n > MAX_FRAME_BYTES {
        return Err(format!("a frame of {n} bytes exceeds the {MAX_FRAME_BYTES}-byte limit"));
    }
    let mut body = vec![0u8; n];
    recv.read_exact(&mut body).await.map_err(|e| e.to_string())?;
    Ok(body)
}

/// Compare secrets without an early exit, so timing does not reveal how many
/// leading characters of a guessed key were right.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_distinct_and_valid_in_links() {
        let a = new_join_key();
        let b = new_join_key();
        assert_ne!(a, b);
        let link = JoinLink { host: "127.0.0.1".into(), port: 7777, key: Some(a.clone()), pin: None };
        assert_eq!(JoinLink::parse(&link.to_link()).unwrap().key.as_deref(), Some(a.as_str()));
    }

    #[test]
    fn remote_joins_need_a_pin() {
        let link = JoinLink { host: "192.168.1.9".into(), port: 7777, key: Some("abcdef".into()), pin: None };
        let err = start_join(&link).err().expect("should refuse");
        assert!(err.contains("certificate pin"));
    }

    #[test]
    fn secret_compare() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
