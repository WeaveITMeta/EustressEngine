//! TCP listener + per-connection JSON-RPC frame loop.
//!
//! The async task stays on the tokio runtime and never touches Bevy
//! directly. All it does is:
//!
//! 1. accept a connection,
//! 2. read newline-delimited JSON frames,
//! 3. enqueue the request with a oneshot channel for the response,
//! 4. wait for the response and write it back.
//!
//! Concurrency: each accepted connection gets its own tokio task, so
//! multiple siblings can talk to the bridge in parallel. Ordering is
//! per-connection — requests from the same client are delivered to the
//! Bevy thread in order, but requests from different clients may be
//! reordered by the single-threaded drain on the main frame.
//!
//! ## Second transport: Unix domain socket
//!
//! Some MCP-connector runtimes sandbox their child processes with a
//! *network* policy that categorically refuses `AF_INET`/`AF_INET6`
//! sockets — including loopback — while still permitting ordinary
//! filesystem operations under an explicitly writable root. A TCP
//! listener on `127.0.0.1` is invisible to a sibling running under such
//! a sandbox even though both processes are on the same host.
//!
//! `AF_UNIX` sockets are filesystem objects, not network connections, so
//! they fall outside that policy. We bind one alongside the TCP listener
//! at `<universe>/.eustress/engine.sock` (unix platforms only — Windows
//! has no equivalent and keeps TCP-only) and accept on both with the same
//! [`handle_connection`], generalized over the transport type.

use super::protocol::{BridgeRequest, BridgeResponse};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::sync::oneshot;

// ---------------------------------------------------------------------------
// Pending-request queue shared with the Bevy main thread
// ---------------------------------------------------------------------------

/// A request that's been parsed off the wire and is waiting for the
/// Bevy-thread drain to execute it. `responder` ships the response
/// back to the tokio task that's holding the TCP connection open.
pub(crate) struct Pending {
    pub request: BridgeRequest,
    pub responder: oneshot::Sender<BridgeResponse>,
}

#[derive(Default, Clone)]
pub(crate) struct PendingRequests {
    inner: Arc<Mutex<Vec<Pending>>>,
}

impl PendingRequests {
    pub(crate) fn push(&self, pending: Pending) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.push(pending);
        }
    }

    /// Drain up to `max` items. Returns them in insertion order.
    pub(crate) fn drain(&self, max: usize) -> Vec<Pending> {
        let Ok(mut guard) = self.inner.lock() else {
            return Vec::new();
        };
        let take = max.min(guard.len());
        guard.drain(..take).collect()
    }
}

// ---------------------------------------------------------------------------
// Listener entry point
// ---------------------------------------------------------------------------

/// Bind `127.0.0.1:0` and return both the bound listener and its
/// OS-assigned port. Bind is a near-instant syscall, so the caller can run
/// this via `Handle::block_on` and learn the real port deterministically —
/// no dependence on a freshly spawned worker thread getting scheduled in
/// time (the 0.19 startup race that left the bridge unbound).
pub(crate) async fn bind_listener() -> std::io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Bind a Unix domain socket at `path`. Removes any stale socket file left
/// behind by a previous run that didn't exit cleanly through `AppExit`
/// (crash, `kill -9`) — a leftover file at the bind path would otherwise
/// make the bind fail with `AddrInUse` even though nothing is listening.
#[cfg(unix)]
pub(crate) async fn bind_unix_listener(path: &std::path::Path) -> std::io::Result<UnixListener> {
    let _ = std::fs::remove_file(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    UnixListener::bind(path)
}

/// Run the accept loop on an already-bound listener. Intended to be
/// `handle.spawn`-ed as a long-lived task; each accepted connection gets
/// its own task so siblings can talk to the bridge in parallel.
pub(crate) async fn run_accept_loop(listener: TcpListener, queue: PendingRequests) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let queue = queue.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, queue).await {
                        tracing::debug!("EngineBridge: connection closed: {}", e);
                    }
                });
            }
            Err(e) => {
                tracing::warn!("EngineBridge: accept failed: {}", e);
                // Brief backoff so a thrashed error doesn't spin.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
}

/// Same as [`run_accept_loop`] but for the Unix domain socket transport.
#[cfg(unix)]
pub(crate) async fn run_unix_accept_loop(listener: UnixListener, queue: PendingRequests) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let queue = queue.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, queue).await {
                        tracing::debug!("EngineBridge: unix connection closed: {}", e);
                    }
                });
            }
            Err(e) => {
                tracing::warn!("EngineBridge: unix accept failed: {}", e);
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-connection frame loop
// ---------------------------------------------------------------------------

/// Read newline-delimited JSON frames, enqueue each, and wait for the
/// Bevy-thread response. Closes the connection on malformed input or
/// EOF.
///
/// Generic over the transport (`TcpStream` or, on unix, `UnixStream`) —
/// both implement `AsyncRead + AsyncWrite`, and the JSON-RPC framing above
/// this layer doesn't care which one carried the bytes. `tokio::io::split`
/// (rather than the transport-specific `into_split`) is what makes the
/// generic split work for either type.
async fn handle_connection<S>(
    stream: S,
    queue: PendingRequests,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Parse the frame. Parse errors produce a JSON-RPC error rather
        // than killing the connection — the client may recover by
        // sending a valid frame next.
        let request: BridgeRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let err = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("parse error: {}", e) }
                });
                writer.write_all(err.to_string().as_bytes()).await?;
                writer.write_all(b"\n").await?;
                continue;
            }
        };

        // Hand the request to the Bevy drain and await the response.
        let (tx, rx) = oneshot::channel();
        queue.push(Pending { request, responder: tx });

        match rx.await {
            Ok(response) => {
                let body = serde_json::to_string(&response)
                    .unwrap_or_else(|_| "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32603,\"message\":\"serialize failed\"}}".to_string());
                writer.write_all(body.as_bytes()).await?;
                writer.write_all(b"\n").await?;
            }
            Err(_) => {
                // The Bevy thread dropped the responder — happens if
                // the engine is shutting down. Give up gracefully.
                return Ok(());
            }
        }
    }

    Ok(())
}
