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

use super::protocol::{BridgeRequest, BridgeResponse};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
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

/// Bind `127.0.0.1:0`, return the OS-assigned port, and spawn the
/// accept loop. The caller hands off to `handle.spawn` — this function
/// itself returns immediately after the bind so the port can be
/// written to the sentinel file.
pub(crate) async fn start_listener(queue: PendingRequests) -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    tokio::spawn(async move {
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
    });

    Ok(port)
}

// ---------------------------------------------------------------------------
// Per-connection frame loop
// ---------------------------------------------------------------------------

/// Read newline-delimited JSON frames, enqueue each, and wait for the
/// Bevy-thread response. Closes the connection on malformed input or
/// EOF.
async fn handle_connection(
    stream: TcpStream,
    queue: PendingRequests,
) -> std::io::Result<()> {
    let (reader, mut writer) = stream.into_split();
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
