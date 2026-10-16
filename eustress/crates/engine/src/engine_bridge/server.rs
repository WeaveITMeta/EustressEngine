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

use super::protocol::{BridgeAuth, BridgeRequest, BridgeResponse};
use super::token_file;
use eustress_common::wire_guard::{looks_like_http, MAX_FRAME_BYTES, MAX_UNAUTHENTICATED_FRAME_BYTES};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::sync::oneshot;

/// The next connection's number, for the write log (`write_log`).
static NEXT_CONNECTION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

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
pub(crate) async fn run_accept_loop(listener: TcpListener, queue: PendingRequests, token: Arc<str>) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let queue = queue.clone();
                let token = token.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, queue, token).await {
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
pub(crate) async fn run_unix_accept_loop(listener: UnixListener, queue: PendingRequests, token: Arc<str>) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let queue = queue.clone();
                let token = token.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, queue, token).await {
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

/// What a request without this launch's token is told.
const UNAUTHORIZED: &str = "unauthorized: every request needs this engine's bridge token in \"auth\". \
    eustress-bridge-client reads it from ~/.eustress_engine/bridge/<port>.token; a client without it, \
    or one older than the token, cannot drive this engine.";

/// What a frame that is not a JSON-RPC request is told.
const PARSE_ERROR: &str = "parse error: the frame is not a JSON-RPC request";

/// Read newline-delimited JSON frames, enqueue each, and wait for the
/// Bevy-thread response. Closes the connection on EOF, on an HTTP request,
/// and after answering a frame that is too large, is not JSON-RPC, or does
/// not carry this launch's token (`token_file`).
///
/// A web page can reach this port: a browser's POST (text/plain needs no
/// preflight) arrives as a request line and headers, then the body. When a
/// bad frame left the connection open, each header line drew a parse error
/// and the body then ran as a bridge call. The token is the second wall: a
/// page cannot read it. Every real client sends well-formed frames with the
/// token, one per connection, so none of these rules costs it anything.
///
/// Generic over the transport (`TcpStream` or, on unix, `UnixStream`) —
/// both implement `AsyncRead + AsyncWrite`, and the JSON-RPC framing above
/// this layer doesn't care which one carried the bytes. `tokio::io::split`
/// (rather than the transport-specific `into_split`) is what makes the
/// generic split work for either type.
async fn handle_connection<S>(
    stream: S,
    queue: PendingRequests,
    token: Arc<str>,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let conn = NEXT_CONNECTION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut frame = Vec::new();
    let mut first = true;
    // Until a frame on this connection has carried the token, a peer is
    // read no further than a small frame.
    let mut authenticated = false;

    loop {
        // One frame, capped: a peer that never sends a newline stops here
        // instead of growing the buffer until the process dies.
        let cap = if authenticated { MAX_FRAME_BYTES } else { MAX_UNAUTHENTICATED_FRAME_BYTES };
        frame.clear();
        let n = (&mut reader).take(cap as u64 + 1).read_until(b'\n', &mut frame).await?;
        if n == 0 {
            return Ok(());
        }
        if frame.len() > cap {
            return refuse(&mut writer, Value::Null, -32600, &format!("frame larger than {cap} bytes")).await;
        }
        let line = String::from_utf8_lossy(&frame);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // An HTTP request (a web page, or a client pointed at the wrong
        // port): close before reading a byte more, so no body runs.
        if first && looks_like_http(trimmed) {
            tracing::warn!("EngineBridge: refused an HTTP request on the bridge port and closed the connection");
            return Ok(());
        }
        first = false;

        // A frame that is not JSON-RPC is answered, and the connection
        // closes. The answer is fixed text: a parser's message can quote
        // the frame, and the peer may not have authenticated.
        let Ok(mut request) = serde_json::from_str::<BridgeRequest>(trimmed) else {
            return refuse(&mut writer, Value::Null, -32700, PARSE_ERROR).await;
        };
        // The token goes no further than this check: the queued request,
        // and anything that logs it, never holds it.
        let auth = request.auth.take();
        if !token_file::matches(auth.as_ref().map(BridgeAuth::as_str), &token) {
            tracing::warn!("EngineBridge: refused a request without this launch's token and closed the connection");
            return refuse(&mut writer, request.id.clone(), -32001, UNAUTHORIZED).await;
        }
        authenticated = true;

        // Hand the request to the Bevy drain and await the response.
        let (tx, rx) = oneshot::channel();
        request.conn = conn;
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
}

/// Answer with a JSON-RPC error; the caller then closes the connection.
async fn refuse<W: AsyncWrite + Unpin>(writer: &mut W, id: Value, code: i64, message: &str) -> std::io::Result<()> {
    let err = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    });
    writer.write_all(err.to_string().as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// Send `payload` on a fresh connection, answer every request the
    /// connection queues, and return what came back with how many requests
    /// reached the queue. Each queued request is checked to hold no token.
    async fn exchange(payload: &[u8]) -> (String, usize) {
        let (client, server) = tokio::io::duplex(1 << 16);
        let (mut client_read, mut client_write) = tokio::io::split(client);
        let payload = payload.to_vec();
        // Written alongside the server, so a payload larger than the pipe
        // still goes through; the server closing early ends the write.
        let sender = tokio::spawn(async move {
            let _ = client_write.write_all(&payload).await;
            let _ = client_write.shutdown().await;
        });
        let queue = PendingRequests::default();
        let connection = tokio::spawn(handle_connection(server, queue.clone(), Arc::from(TOKEN)));
        let mut executed = 0;
        while !connection.is_finished() {
            for p in queue.drain(16) {
                assert!(p.request.auth.is_none(), "the token must not reach the queue");
                executed += 1;
                let _ = p.responder.send(BridgeResponse::ok(p.request.id.clone(), serde_json::json!({ "pong": true })));
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        connection.await.unwrap().unwrap();
        executed += queue.drain(16).len();
        let _ = sender.await;
        let mut out = String::new();
        client_read.read_to_string(&mut out).await.unwrap();
        (out, executed)
    }

    fn ping(id: u32) -> String {
        format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ping","auth":"{TOKEN}"}}"#)
    }

    /// What a web page's fetch sends: the request line, headers, a blank
    /// line, then a JSON-RPC body (here even with the right token, which a
    /// page cannot have). Nothing runs, nothing is answered.
    #[tokio::test]
    async fn an_http_post_runs_nothing_and_closes() {
        let body = ping(1);
        let payload = format!(
            "POST / HTTP/1.1\r\nHost: 127.0.0.1:50123\r\nOrigin: https://evil.example\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{body}\n",
            body.len() + 1
        );
        let (out, executed) = exchange(payload.as_bytes()).await;
        assert_eq!(executed, 0, "the body must never reach the engine");
        assert_eq!(out, "", "an HTTP request is closed without an answer");
    }

    /// A header-style line that does not look like HTTP closes at the parse
    /// rule, before the frame after it.
    #[tokio::test]
    async fn a_header_line_closes_at_the_parse_rule() {
        let (out, executed) = exchange(format!("X-Foo: bar\n{}\n", ping(1)).as_bytes()).await;
        assert_eq!(executed, 0);
        assert_eq!(out.lines().count(), 1);
        assert!(out.contains("-32700"), "{out}");
    }

    /// A first line that is not JSON-RPC is answered with a fixed error that
    /// quotes nothing of it, and the connection closes, so a valid frame
    /// after it never runs.
    #[tokio::test]
    async fn a_bad_first_line_closes() {
        let (out, executed) = exchange(format!("hello-quoted-back\n{}\n", ping(1)).as_bytes()).await;
        assert_eq!(executed, 0);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("-32700"), "{out}");
        assert!(!out.contains("hello"), "the answer must not quote the frame: {out}");
    }

    /// A bad frame later in a session closes it after the frames before it
    /// were answered.
    #[tokio::test]
    async fn a_bad_later_line_closes_after_its_error() {
        let (out, executed) = exchange(format!("{}\nnot json\n{}\n", ping(1), ping(2)).as_bytes()).await;
        assert_eq!(executed, 1);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[0].contains("pong"));
        assert!(lines[1].contains("-32700"));
    }

    /// Well-formed frames with the token, blank lines between them
    /// included, are answered in order.
    #[tokio::test]
    async fn a_json_rpc_session_with_the_token_runs() {
        let (out, executed) = exchange(format!("{}\n\n{}\n", ping(1), ping(2)).as_bytes()).await;
        assert_eq!(executed, 2);
        let ids: Vec<serde_json::Value> = out
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["id"].clone())
            .collect();
        assert_eq!(ids, vec![serde_json::json!(1), serde_json::json!(2)]);
    }

    /// No token, or the wrong one: nothing runs, the answer says why, and
    /// the connection closes before the next frame.
    #[tokio::test]
    async fn a_request_without_this_launchs_token_runs_nothing() {
        let bare = r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#;
        let wrong = format!(r#"{{"jsonrpc":"2.0","id":8,"method":"ping","auth":"{}"}}"#, "f".repeat(64));
        for first in [bare.to_string(), wrong] {
            let (out, executed) = exchange(format!("{first}\n{}\n", ping(2)).as_bytes()).await;
            assert_eq!(executed, 0, "{first}");
            let lines: Vec<&str> = out.lines().collect();
            assert_eq!(lines.len(), 1, "{out}");
            assert!(lines[0].contains("-32001") && lines[0].contains("unauthorized"), "{out}");
        }
    }

    /// A request printed with Debug, before the check or after, never shows
    /// the token.
    #[test]
    fn a_request_never_prints_its_token() {
        let request: BridgeRequest = serde_json::from_str(&ping(1)).unwrap();
        let printed = format!("{request:?}");
        assert!(!printed.contains(TOKEN) && printed.contains("<redacted>"), "{printed}");
        assert_eq!(request.auth.as_ref().map(BridgeAuth::as_str), Some(TOKEN));
    }

    /// Before any frame has carried the token, a frame past the small cap
    /// is refused unread past it, and the connection closes.
    #[tokio::test]
    async fn an_unauthenticated_frame_past_the_small_cap_closes() {
        let mut payload = vec![b'x'; MAX_UNAUTHENTICATED_FRAME_BYTES + 10];
        payload.extend_from_slice(format!("\n{}\n", ping(1)).as_bytes());
        let (out, executed) = exchange(&payload).await;
        assert_eq!(executed, 0);
        assert_eq!(out.lines().count(), 1);
        assert!(out.contains("-32600") && out.contains(&MAX_UNAUTHENTICATED_FRAME_BYTES.to_string()), "{out}");
    }

    /// After a frame has carried the token, a larger one is read; one past
    /// the full cap still closes.
    #[tokio::test]
    async fn an_authenticated_connection_takes_large_frames_up_to_the_cap() {
        let big = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"ping","auth":"{TOKEN}","params":{{"pad":"{}"}}}}"#,
            "p".repeat(MAX_UNAUTHENTICATED_FRAME_BYTES * 2)
        );
        let (out, executed) = exchange(format!("{}\n{big}\n", ping(1)).as_bytes()).await;
        assert_eq!(executed, 2, "{}", &out[..out.len().min(300)]);

        let mut payload = format!("{}\n", ping(1)).into_bytes();
        payload.extend(std::iter::repeat(b'x').take(MAX_FRAME_BYTES + 10));
        let (out, executed) = exchange(&payload).await;
        assert_eq!(executed, 1);
        assert!(out.lines().nth(1).is_some_and(|l| l.contains("-32600")), "{}", &out[..out.len().min(300)]);
    }
}
