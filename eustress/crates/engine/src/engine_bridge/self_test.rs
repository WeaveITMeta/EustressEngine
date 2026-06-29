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

/// Total budget for the round-trip. The drain runs every `Update`, but
/// the first frame can be slow on 0.19 startup, so we allow plenty.
const ROUND_TRIP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Connect to `127.0.0.1:<port>`, send one `ping`, await the response.
pub(crate) async fn run(port: u16) {
    match tokio::time::timeout(ROUND_TRIP_TIMEOUT, ping_roundtrip(port)).await {
        Ok(Ok(())) => {
            tracing::info!(
                "✅ Engine Bridge SELF-TEST PASSED — ping round-trip OK on 127.0.0.1:{port} (bridge is accepting + draining)"
            );
        }
        Ok(Err(e)) => {
            tracing::error!(
                "❌ Engine Bridge SELF-TEST FAILED on 127.0.0.1:{port}: {e} — the MCP/AI bridge is NOT usable this run"
            );
        }
        Err(_) => {
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
