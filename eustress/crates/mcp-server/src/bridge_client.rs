//! Synchronous client for the engine's JSON-RPC "engine bridge".
//!
//! The engine binds a localhost TCP listener (`127.0.0.1:<port>`) and
//! writes the chosen port to `<universe>/.eustress/engine.port` — the
//! same sentinel convention IDEs use to find the LSP (`.eustress/lsp.port`).
//! On unix platforms it *also* binds a Unix domain socket at
//! `<universe>/.eustress/engine.sock`. Sibling processes (this MCP server,
//! future plugins) discover the bridge by reading those files, preferring
//! the socket when present (see [`connect_under`]).
//!
//! ## Why two transports
//!
//! Some MCP-connector runtimes sandbox their child processes with a
//! *network* policy that refuses `AF_INET`/`AF_INET6` sockets outright —
//! including loopback — while still permitting ordinary filesystem
//! operations under an explicitly writable root. In that environment the
//! TCP bridge is completely unreachable even though both processes are on
//! the same host and the engine is definitely listening. `AF_UNIX`
//! sockets are filesystem objects rather than network connections, so
//! they fall outside that policy — a sandbox that blocks loopback TCP
//! typically has no reason to also block `connect()` on a `.sock` file
//! under a directory it already lets the process write to.
//!
//! Wire format (mirrors `engine_bridge::protocol`): one newline-
//! terminated JSON line per direction.
//!
//! Request frame:
//! ```json
//! {"jsonrpc":"2.0","id":1,"method":"ecs.inspect","params":{"limit":10}}
//! ```
//! Response frame:
//! ```json
//! {"jsonrpc":"2.0","id":1,"result":{...}}        // success
//! {"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"..."}} // error
//! ```
//!
//! Kept deliberately synchronous + std-only (`std::net::TcpStream` /
//! `std::os::unix::net::UnixStream`): the MCP server's tool-dispatch path
//! is synchronous, and pulling tokio into a single short request/response
//! round-trip would buy nothing. Every failure path returns a friendly
//! `Err(String)` — this function never panics, so a tool can surface
//! "engine isn't running" to the AI.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

/// Either transport the bridge might be reachable over. `Read`/`Write` are
/// implemented by delegating to whichever variant is active — the framing
/// logic in [`call_engine`] doesn't need to know which one it has.
enum BridgeStream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}

impl BridgeStream {
    fn try_clone(&self) -> std::io::Result<Self> {
        match self {
            BridgeStream::Tcp(s) => Ok(BridgeStream::Tcp(s.try_clone()?)),
            #[cfg(unix)]
            BridgeStream::Unix(s) => Ok(BridgeStream::Unix(s.try_clone()?)),
        }
    }

    fn set_read_timeout(&self, dur: Option<Duration>) -> std::io::Result<()> {
        match self {
            BridgeStream::Tcp(s) => s.set_read_timeout(dur),
            #[cfg(unix)]
            BridgeStream::Unix(s) => s.set_read_timeout(dur),
        }
    }

    fn set_write_timeout(&self, dur: Option<Duration>) -> std::io::Result<()> {
        match self {
            BridgeStream::Tcp(s) => s.set_write_timeout(dur),
            #[cfg(unix)]
            BridgeStream::Unix(s) => s.set_write_timeout(dur),
        }
    }
}

impl Read for BridgeStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            BridgeStream::Tcp(s) => s.read(buf),
            #[cfg(unix)]
            BridgeStream::Unix(s) => s.read(buf),
        }
    }
}

impl Write for BridgeStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            BridgeStream::Tcp(s) => s.write(buf),
            #[cfg(unix)]
            BridgeStream::Unix(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            BridgeStream::Tcp(s) => s.flush(),
            #[cfg(unix)]
            BridgeStream::Unix(s) => s.flush(),
        }
    }
}

/// How long to wait on connect / read before giving up. The engine
/// services bridge requests on its main frame, so a live engine answers
/// in well under a frame; 2 s is generous headroom that still fails fast
/// when nothing is listening.
const TIMEOUT: Duration = Duration::from_secs(2);

/// Friendly, AI-readable message for the "no live engine" case. Returned
/// for every discovery/connection failure so the model is told to open
/// the engine rather than seeing a raw OS error.
fn not_running(detail: &str) -> String {
    format!(
        "Eustress engine is not running (no live bridge at .eustress/engine.port). \
         Open the engine and retry. [{detail}]"
    )
}

/// Read a port from `port_path`, parse it, and open a TCP connection to the
/// bridge (short timeout so a dead engine fails fast). Used for both the
/// per-universe port file and the global workspace-root fallback.
fn connect_via_port_file(port_path: &Path) -> Result<TcpStream, String> {
    let raw = std::fs::read_to_string(port_path)
        .map_err(|_| not_running(&format!("no port file at {}", port_path.display())))?;
    let port: u16 = raw
        .trim()
        .parse()
        .map_err(|_| not_running(&format!("invalid port file contents: {:?}", raw.trim())))?;
    let addr = format!("127.0.0.1:{port}");
    let sock_addr: SocketAddr = addr
        .parse()
        .map_err(|e| format!("internal: bad bridge address {addr}: {e}"))?;
    TcpStream::connect_timeout(&sock_addr, TIMEOUT)
        .map_err(|e| not_running(&format!("connect {addr} failed: {e}")))
}

/// Connect to the bridge advertised under `dir/.eustress/`. Tries the Unix
/// domain socket first when present (see module docs for why), falling
/// back to the TCP port file — either because this platform never binds a
/// socket (Windows), or because the pointer is stale (engine crashed
/// without cleaning it up, or points at a path with nothing listening)
/// and `connect()` fails.
///
/// `engine.sock` is a POINTER file, not the socket itself — its contents
/// are the real (short, temp-dir) bind path. See the engine's
/// `unix_socket_file` module docs: `AF_UNIX` addresses are capped at
/// ~104-108 bytes, and Universe paths routinely exceed that once nested a
/// few directories deep, so the socket can't live at `<dir>/.eustress/`
/// directly.
fn connect_under(dir: &Path) -> Result<BridgeStream, String> {
    #[cfg(unix)]
    {
        let pointer_path = dir.join(".eustress").join("engine.sock");
        if let Ok(raw) = std::fs::read_to_string(&pointer_path) {
            let bind_path = raw.trim();
            if !bind_path.is_empty() {
                match UnixStream::connect(bind_path) {
                    Ok(s) => return Ok(BridgeStream::Unix(s)),
                    Err(e) => {
                        tracing::debug!(
                            "bridge: unix socket connect to {} (from pointer {}) failed: {} — falling back to TCP",
                            bind_path,
                            pointer_path.display(),
                            e
                        );
                    }
                }
            }
        }
    }
    let port_path = dir.join(".eustress").join("engine.port");
    connect_via_port_file(&port_path).map(BridgeStream::Tcp)
}

/// Call one bridge method and return its `result` value.
///
/// `universe_dir` is the Universe root (`ToolContext::universe_root`);
/// the port file lives at `<universe_dir>/.eustress/engine.port`.
///
/// On success returns the JSON-RPC `result` object. On any failure —
/// missing port file, unparseable port, connection refused, timeout,
/// malformed response, or a JSON-RPC `error` from the engine — returns a
/// clear `Err(String)`.
pub fn call_engine(
    universe_dir: &Path,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    // ── Discover the transport + connect ──────────────────────────────
    // Try the configured universe first (Unix socket, then TCP port file —
    // see `connect_under`); if neither answers, fall back to the GLOBAL
    // discovery files at the shared Eustress workspace root (the engine
    // writes both there too). This lets the MCP find the live engine even
    // when it launched into a DIFFERENT universe than the one this server
    // is configured for.
    let global_dir = universe_dir.parent();

    let stream = match connect_under(universe_dir) {
        Ok(s) => s,
        Err(primary_err) => match global_dir {
            // On global failure, surface the PRIMARY (per-universe) error — it's
            // the more relevant "your configured universe has no live engine".
            Some(gd) => connect_under(gd).map_err(|_| primary_err)?,
            None => return Err(primary_err),
        },
    };
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|e| format!("internal: set_read_timeout failed: {e}"))?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|e| format!("internal: set_write_timeout failed: {e}"))?;

    // ── Send one newline-terminated request frame ────────────────────
    //
    // `id` is a constant 1: this is a single synchronous round-trip on a
    // fresh connection, so there are no concurrent requests to
    // disambiguate. The engine echoes it back; we don't bother checking.
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });
    let mut line = serde_json::to_string(&request)
        .map_err(|e| format!("internal: failed to encode request: {e}"))?;
    line.push('\n');

    let mut writer = stream
        .try_clone()
        .map_err(|e| not_running(&format!("socket clone failed: {e}")))?;
    writer
        .write_all(line.as_bytes())
        .map_err(|e| not_running(&format!("write to bridge failed: {e}")))?;
    writer
        .flush()
        .map_err(|e| not_running(&format!("flush to bridge failed: {e}")))?;

    // ── Read exactly one newline-terminated response frame ───────────
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    let n = reader
        .read_line(&mut resp_line)
        .map_err(|e| not_running(&format!("read from bridge failed: {e}")))?;
    if n == 0 {
        return Err(not_running("bridge closed the connection without responding"));
    }

    // ── Parse the BridgeResponse and unwrap result / error ───────────
    let resp: Value = serde_json::from_str(resp_line.trim())
        .map_err(|e| format!("bridge returned malformed JSON: {e} — raw: {}", resp_line.trim()))?;

    if let Some(err) = resp.get("error") {
        let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("(no message)");
        return Err(format!("engine bridge error {code}: {msg}"));
    }

    match resp.get("result") {
        Some(result) => Ok(result.clone()),
        None => Err(format!(
            "bridge response had neither result nor error: {}",
            resp_line.trim()
        )),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// Proves `call_engine` actually completes a round trip over the Unix
    /// socket transport — not just that it compiles. No `engine.port` file
    /// is ever created in this test's universe dir, so a pass is only
    /// possible if `connect_under` read the `engine.sock` pointer file and
    /// connected to the (short, temp-dir) path inside it.
    ///
    /// The pointer file's CONTENTS must stay short regardless of how long
    /// `dir` is (`sizeof(sockaddr_un.sun_path)` is ~104-108 bytes) — that's
    /// the whole reason `engine.sock` is a pointer rather than the socket
    /// itself. `dir` here is deliberately given a longish, descriptive name
    /// to make sure a regression back to "bind directly under `dir`" would
    /// fail loudly rather than happening to fit under the limit in CI.
    #[test]
    fn call_engine_round_trips_over_unix_socket() {
        let dir = std::env::temp_dir().join(format!(
            "eustress-bridge-uds-test-{}-call_engine_round_trips_over_unix_socket",
            std::process::id()
        ));
        let eustress_dir = dir.join(".eustress");
        std::fs::create_dir_all(&eustress_dir).expect("create test .eustress dir");

        // The REAL bind path: short, under the system temp dir directly —
        // exactly what `unix_socket_file::bind_path_for` produces in
        // production, just without depending on engine-crate code from
        // this mcp-server-crate test.
        let bind_path =
            std::env::temp_dir().join(format!("eustress-bridge-test-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&bind_path);

        // The POINTER file: lives under the (long) test dir, contains the
        // short bind path as text — mirrors what the engine writes.
        let pointer_path = eustress_dir.join("engine.sock");
        std::fs::write(&pointer_path, bind_path.to_string_lossy().as_bytes())
            .expect("write pointer file");

        let listener = UnixListener::bind(&bind_path).expect("bind test unix socket");
        let server = std::thread::spawn(move || {
            let (mut stream, _addr) = listener.accept().expect("accept test connection");
            let mut reader = BufReader::new(stream.try_clone().expect("clone test stream"));
            let mut line = String::new();
            reader.read_line(&mut line).expect("read test request");
            assert!(line.contains("\"method\":\"ping\""));
            stream
                .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"pong\":true}}\n")
                .expect("write test response");
        });

        let result = call_engine(&dir, "ping", serde_json::json!({}));

        server.join().expect("test server thread panicked");
        let _ = std::fs::remove_file(&bind_path);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            result.expect("call_engine should succeed over the unix socket"),
            serde_json::json!({"pong": true})
        );
    }

    /// When the pointer file exists but names a path nothing is listening
    /// on — e.g. the engine crashed without cleaning up — `connect_under`
    /// must fall through to the TCP path (or fail cleanly) rather than
    /// hanging or panicking.
    #[test]
    fn stale_unix_socket_pointer_falls_back_cleanly() {
        let dir = std::env::temp_dir().join(format!(
            "eustress-bridge-uds-test-{}-stale_pointer",
            std::process::id()
        ));
        let eustress_dir = dir.join(".eustress");
        std::fs::create_dir_all(&eustress_dir).expect("create test .eustress dir");
        let pointer_path = eustress_dir.join("engine.sock");
        // Points at a path nothing is bound to — connect() must fail.
        let dead_bind_path = std::env::temp_dir()
            .join(format!("eustress-bridge-test-dead-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&dead_bind_path);
        std::fs::write(&pointer_path, dead_bind_path.to_string_lossy().as_bytes())
            .expect("write stale pointer file");

        let result = call_engine(&dir, "ping", serde_json::json!({}));
        let _ = std::fs::remove_dir_all(&dir);

        // No engine.port either, so this must fail — the point is that it
        // fails with the normal "not running" error, not a hang or panic.
        assert!(result.is_err());
    }
}
