//! Startup self-test for the Engine Bridge.
//!
//! After the listener binds, we connect to our OWN port over localhost
//! and do exactly one JSON-RPC `ping` round-trip. This catches silent
//! regressions the migration just bit us with: a listener that binds but
//! never accepts, a drain system that never runs, or a transport that
//! frames responses wrong. On success it logs loudly; on any failure it
//! logs an ERROR with the concrete cause so the MCP/AI loop never dies
//! quietly again.
//!
//! It is spawned as a tokio task (never blocks Startup): the `ping`
//! response is produced by `drain_bridge_requests` on a later `Update`
//! frame, so the self-test simply awaits it with a generous timeout while
//! the Bevy schedule comes up.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// When a slow answer earns a note in the log. The drain runs every
/// `Update`, and loading a Space can hold the first frames longer than this.
const SLOW_NOTICE: std::time::Duration = std::time::Duration::from_secs(10);

/// Total budget for the round-trip. A 10 s budget reported a working bridge
/// as dead whenever a Space took longer than that to reach its first frame.
const ROUND_TRIP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// Connect to `127.0.0.1:<port>`, send one `ping`, await the response.
pub(crate) async fn run(port: u16) {
    let mut roundtrip = std::pin::pin!(ping_roundtrip(port));
    let result = match tokio::time::timeout(SLOW_NOTICE, roundtrip.as_mut()).await {
        Ok(r) => Some(r),
        Err(_) => {
            tracing::info!(
                "Engine Bridge self-test: no ping answer after {}s on 127.0.0.1:{port}; the first frames are still loading, waiting up to {}s",
                SLOW_NOTICE.as_secs(),
                ROUND_TRIP_TIMEOUT.as_secs()
            );
            tokio::time::timeout(ROUND_TRIP_TIMEOUT - SLOW_NOTICE, roundtrip.as_mut()).await.ok()
        }
    };
    match result {
        Some(Ok(())) => {
            tracing::info!(
                "✅ Engine Bridge SELF-TEST PASSED — ping round-trip OK on 127.0.0.1:{port} (bridge is accepting + draining)"
            );
        }
        Some(Err(e)) => {
            tracing::error!(
                "❌ Engine Bridge SELF-TEST FAILED on 127.0.0.1:{port}: {e} — the MCP/AI bridge is NOT usable this run"
            );
        }
        None => {
            tracing::error!(
                "❌ Engine Bridge SELF-TEST TIMED OUT after {}s on 127.0.0.1:{port} — listener bound but no ping response (drain not running?)",
                ROUND_TRIP_TIMEOUT.as_secs()
            );
        }
    }
}

async fn ping_roundtrip(port: u16) -> std::io::Result<()> {
    let stream = TcpStream::connect(("127.0.0.1", port)).await?;
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    // Newline-delimited JSON-RPC, exactly as `handle_connection` expects.
    let req = br#"{"jsonrpc":"2.0","id":"__self_test__","method":"ping"}"#;
    writer.write_all(req).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;

    let line = lines.next_line().await?.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "connection closed before any response",
        )
    })?;

    // Validate it's a well-formed JSON-RPC reply to OUR id with a pong.
    let v: serde_json::Value = serde_json::from_str(line.trim()).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("response was not JSON: {e} (raw: {line:?})"),
        )
    })?;

    if v.get("id").and_then(|i| i.as_str()) != Some("__self_test__") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("response id mismatch (raw: {line:?})"),
        ));
    }
    if v.get("result")
        .and_then(|r| r.get("pong"))
        .and_then(|p| p.as_bool())
        != Some(true)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("response missing result.pong=true (raw: {line:?})"),
        ));
    }
    Ok(())
}
