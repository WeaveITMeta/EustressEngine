//! A tool result that carries an image. The screenshot tools return the PNG
//! itself, not only its path, so a reader with no filesystem access (an MCP
//! client, the Workshop's agent) sees the frame.

use crate::{ToolContext, ToolResult};
use serde_json::Value;

/// Keys a tool sets in `structured_data` to return an image. A host lifts them
/// out into an image block: the MCP server into an MCP `image` content block,
/// the Workshop into an image block of the tool result. Underscore-prefixed so
/// they never collide with a real engine field.
pub const IMAGE_B64_KEY: &str = "_mcp_image_base64";
pub const IMAGE_MIME_KEY: &str = "_mcp_image_mime";

/// How long to wait for a capture PNG to finish landing on disk.
///
/// Sized for a slow scene, not a fast one. The AI camera holds a fixed frame
/// budget either side of its readback (warm up, queue, cool down), so the wall
/// time to produce a PNG is that frame count divided by the engine's current
/// framerate: on a heavy Space running at about 2 FPS that is several seconds.
/// A tight bound here turns "the engine is busy" into a spurious "the capture
/// failed", so the cap is generous and the timeout message says what to check.
pub const CAPTURE_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Largest PNG inlined as base64. Bigger frames stay path-only: a 4K
/// screenshot base64s to well past what a client accepts in one message, and
/// silently blowing the reader's context is worse than one extra read.
pub const MAX_INLINE_PNG_BYTES: u64 = 2_800_000;

/// Block until `path` holds a complete PNG, then return its bytes.
///
/// The engine answers a capture request before the render graph has written
/// the file, so the path it returns points at a file that does not exist yet
/// (or is half-written). Waiting for the IEND terminator is a definitive done
/// signal. A cancelled call ([`ToolContext::is_cancelled`]: the Workshop's
/// Stop, an MCP cancellation) stops waiting at once.
pub fn wait_for_png(path: &std::path::Path, ctx: &ToolContext) -> Result<Vec<u8>, String> {
    const IEND: &[u8] = b"\x00\x00\x00\x00IEND\xaeB\x60\x82";
    let start = std::time::Instant::now();
    let mut last_err = String::from("capture file never appeared");
    while start.elapsed() < CAPTURE_WAIT {
        if ctx.is_cancelled() {
            return Err("the capture was cancelled before its PNG finished".to_string());
        }
        match std::fs::read(path) {
            Ok(bytes) if bytes.len() > IEND.len() && bytes.ends_with(IEND) => return Ok(bytes),
            Ok(_) => last_err = "capture file is still being written".into(),
            Err(e) => last_err = e.to_string(),
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    Err(format!(
        "timed out after {}s waiting for {} ({last_err}); if the engine is running at a very \
         low framerate the capture may need longer than this",
        CAPTURE_WAIT.as_secs(),
        path.display()
    ))
}

/// Turn a capture's bridge response into a `ToolResult` carrying the pixels.
///
/// Returning only a path assumes the reader can open local files. Embedding
/// the PNG under [`IMAGE_B64_KEY`] makes the same tool work in every host, and
/// the path is still reported for a host that would rather open the file.
pub fn capture_result(tool_name: &str, mut result: Value, ctx: &ToolContext) -> ToolResult {
    use base64::Engine as _;

    let done = |success: bool, content: String, data: Option<Value>| ToolResult {
        tool_name: tool_name.to_string(),
        tool_use_id: String::new(),
        success,
        content,
        structured_data: data,
        stream_topic: None,
    };

    let path = match result.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
        _ => {
            return done(
                true,
                "Capture succeeded but the engine returned no file path.".to_string(),
                Some(result),
            )
        }
    };

    let bytes = match wait_for_png(&path, ctx) {
        Ok(b) => b,
        Err(e) => return done(false, format!("Capture was requested but the PNG never completed: {e}"), None),
    };

    let size = bytes.len() as u64;
    if size > MAX_INLINE_PNG_BYTES {
        return done(
            true,
            format!(
                "Screenshot saved to {} ({size} bytes, too large to inline; read the file path to view it).",
                path.display()
            ),
            Some(result),
        );
    }

    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    if let Some(map) = result.as_object_mut() {
        map.insert(IMAGE_B64_KEY.to_string(), Value::String(b64));
        map.insert(IMAGE_MIME_KEY.to_string(), Value::String("image/png".to_string()));
        map.insert("bytes".to_string(), Value::from(size));
    }
    done(
        true,
        format!("Screenshot captured ({size} bytes) and attached below. Saved to {}.", path.display()),
        Some(result),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(cancelled: bool) -> ToolContext {
        ToolContext {
            space_root: std::env::temp_dir(),
            universe_root: std::env::temp_dir(),
            user_id: None,
            username: None,
            luau_executor: None,
            display_unit: None,
            cancelled: Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(cancelled))),
            permissions: crate::Permissions::standard(),
        }
    }

    fn temp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eus_image_result_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A finished PNG comes back inline under the image keys, with its path.
    #[test]
    fn a_finished_png_is_attached() {
        let dir = temp("attached");
        let path = dir.join("shot.png");
        let mut png = b"\x89PNG\r\n\x1a\nbody".to_vec();
        png.extend_from_slice(b"\x00\x00\x00\x00IEND\xaeB\x60\x82");
        std::fs::write(&path, &png).unwrap();
        let r = capture_result("t", serde_json::json!({ "path": path.to_string_lossy() }), &ctx(false));
        assert!(r.success, "{}", r.content);
        let data = r.structured_data.unwrap();
        assert!(data[IMAGE_B64_KEY].as_str().is_some_and(|s| !s.is_empty()));
        assert_eq!(data[IMAGE_MIME_KEY], "image/png");
        assert_eq!(data["bytes"], png.len() as u64);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A response with no path is reported as such, not waited on.
    #[test]
    fn a_response_without_a_path_is_said_plainly() {
        let r = capture_result("t", serde_json::json!({}), &ctx(false));
        assert!(r.success);
        assert!(r.content.contains("no file path"), "{}", r.content);
    }

    /// A cancelled call stops waiting at once instead of holding its thread
    /// for the whole capture window. The folder's name says nothing about
    /// cancelling, so only the cancellation message can match; the bound is
    /// "well inside the window" so a busy machine cannot fail it, while a
    /// call that ignored the cancel would wait all of CAPTURE_WAIT.
    #[test]
    fn a_cancelled_capture_stops_waiting_at_once() {
        let dir = temp("stop");
        let path = dir.join("never.png");
        let started = std::time::Instant::now();
        let r = capture_result("t", serde_json::json!({ "path": path.to_string_lossy() }), &ctx(true));
        let elapsed = started.elapsed();
        assert!(!r.success);
        assert!(
            r.content.contains("the capture was cancelled before its PNG finished"),
            "not the cancellation (after {elapsed:?}): {}",
            r.content
        );
        assert!(elapsed < CAPTURE_WAIT / 3, "it waited {elapsed:?} of the {CAPTURE_WAIT:?} window: {}", r.content);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
