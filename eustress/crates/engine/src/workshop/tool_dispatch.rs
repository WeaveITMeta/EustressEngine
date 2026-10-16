//! # Running Workshop tool calls off the main thread
//!
//! A tool the model calls without needing approval used to run inside the
//! system that received the model's answer, on the main thread. That is fine
//! for a file read and wrong for anything that waits: a tool that calls back
//! into this engine over the bridge (a camera capture waits for rendered
//! frames, `await_simulation` polls) asks the main thread for something only
//! the main thread can answer, so Studio stood still until the bridge gave up.
//! Approved tools already ran on a thread; this module is the one place both
//! kinds now go through.
//!
//! ## What it guarantees
//!
//! * **Order.** One worker thread runs one assistant turn's calls one after
//!   another, in the order the model emitted them. The system prompt tells the
//!   model to create a Model and then its child Parts, often in a single turn,
//!   so running them in parallel would run the child before its parent exists.
//! * **Stop.** Every call carries its own cancel flag, which is also handed to
//!   the tool as `ToolContext::cancelled`: a long-polling tool stops between
//!   polls, and a call that has not started yet never starts.
//! * **No wedged turn.** A tool that panics becomes a failed result instead of
//!   a card stuck on Running forever, and a thread that cannot be started
//!   fails every call it was meant to run.
//! * **Images are checked before the model sees them.** A tool can return a
//!   picture (a camera capture). It goes into the conversation only if it is
//!   a complete PNG, JPEG, GIF or WebP within the provider's size limits,
//!   because history is resent on every turn: one corrupt image would make
//!   every later request fail until the session ended.
//!
//! The pure parts (validation, the runner, the history shape) have no engine
//! dependency and are covered by the tests at the bottom.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use serde_json::{json, Value};

use eustress_tools::image_result::{IMAGE_B64_KEY, IMAGE_MIME_KEY};

use super::tools::ToolResult;
use super::{ChatMessage, McpCommandStatus};

/// Largest image sent to a model, in decoded bytes. Anthropic rejects an image
/// over 5 MB, and a rejected image in history fails every later request.
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

/// Largest width or height accepted, in pixels. Anthropic rejects an image
/// over 8000 pixels on a side.
pub const MAX_IMAGE_SIDE: u32 = 8000;

/// How many tool images stay attached to the conversation. A picture costs
/// thousands of tokens on every turn it rides along, and the model has almost
/// always moved on from a frame it looked at several captures ago.
pub const KEEP_RECENT_IMAGES: usize = 3;

// ============================================================================
// 1. Images a tool returns
// ============================================================================

/// An image a tool returned, held in memory for the model to see.
///
/// In memory only: session files do not store tool results either, so a
/// reloaded session loses nothing it had. That also keeps captures out of the
/// Space, which is a git repository.
#[derive(Clone)]
pub struct ToolImage {
    /// One of the four types the providers accept.
    pub media_type: &'static str,
    /// Base64 of the validated bytes, or `None` once evicted.
    data: Option<Arc<str>>,
    /// Decoded size, for logs and notes.
    pub bytes: usize,
}

impl ToolImage {
    pub fn new(media_type: &'static str, base64: &str, bytes: usize) -> Self {
        Self { media_type, data: Some(Arc::from(base64)), bytes }
    }

    /// The base64 payload, while still attached.
    pub fn data(&self) -> Option<&str> {
        self.data.as_deref()
    }

    pub fn is_attached(&self) -> bool {
        self.data.is_some()
    }

    /// Drop the pixels and keep the fact that there was one.
    pub fn evict(&mut self) {
        self.data = None;
    }
}

// Never prints the payload: a derived Debug would put megabytes in a log line.
impl std::fmt::Debug for ToolImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ToolImage({}, {} bytes, {})",
            self.media_type,
            self.bytes,
            if self.is_attached() { "attached" } else { "evicted" }
        )
    }
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// The last chunk of every complete PNG: zero length, "IEND", its CRC.
const PNG_IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82];

/// Decide whether `bytes` are a complete image of a type the providers accept,
/// and say which. The type comes from the bytes, never from what the tool
/// claimed.
///
/// This is a structural check, not a decode: it catches the failures that
/// actually happen (a cut-off file, a wrong type, an absurd size). JPEG
/// dimensions are not read; the byte cap bounds those.
pub fn sniff_image(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.is_empty() {
        return Err("the image is empty".to_string());
    }
    if bytes.starts_with(&PNG_SIGNATURE) {
        sniff_png(bytes)?;
        return Ok("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        sniff_jpeg(bytes)?;
        return Ok("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        sniff_gif(bytes)?;
        return Ok("image/gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        sniff_webp(bytes)?;
        return Ok("image/webp");
    }
    Err("the data is not a PNG, JPEG, GIF or WebP image".to_string())
}

fn check_side(kind: &str, width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err(format!("the {kind} has no pixels"));
    }
    if width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
        return Err(format!(
            "the {kind} is {width}x{height}, over the {MAX_IMAGE_SIDE} pixel limit"
        ));
    }
    Ok(())
}

fn sniff_png(bytes: &[u8]) -> Result<(), String> {
    // Signature (8) + the 25-byte IHDR chunk + the 12-byte IEND chunk.
    if bytes.len() < 45 || &bytes[12..16] != b"IHDR" {
        return Err("the PNG is too short or has no header chunk".to_string());
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    check_side("PNG", width, height)?;
    if bytes[bytes.len() - 12..] != PNG_IEND {
        return Err("the PNG is cut off before its end".to_string());
    }
    Ok(())
}

fn sniff_jpeg(bytes: &[u8]) -> Result<(), String> {
    // The end-of-image marker, allowing a little trailing padding.
    let tail = &bytes[bytes.len().saturating_sub(32)..];
    if bytes.len() < 4 || !tail.windows(2).any(|pair| pair == [0xFF, 0xD9]) {
        return Err("the JPEG is cut off before its end".to_string());
    }
    Ok(())
}

fn sniff_gif(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 14 {
        return Err("the GIF is too short".to_string());
    }
    let width = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
    let height = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
    check_side("GIF", width, height)?;
    if bytes[bytes.len() - 1] != 0x3B {
        return Err("the GIF is cut off before its end".to_string());
    }
    Ok(())
}

fn sniff_webp(bytes: &[u8]) -> Result<(), String> {
    let riff_size = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if riff_size.checked_add(8).is_none_or(|total| bytes.len() < total) {
        return Err("the WebP is cut off before its end".to_string());
    }
    Ok(())
}

/// What `lift_image` found in a tool's structured data.
#[derive(Debug)]
pub enum ImageLift {
    /// The tool returned no image.
    Absent,
    Attached(ToolImage),
    /// The tool returned an image that cannot be sent; the reason is shown to
    /// the model so it knows the picture is missing instead of assuming it saw it.
    Rejected(String),
}

/// Take the image a tool put in `structured_data` (under the keys the MCP
/// server uses for the same purpose) and validate it.
///
/// The keys are removed whether or not the image is accepted, so the payload
/// does not linger in the result.
pub fn lift_image(structured: &mut Option<Value>) -> ImageLift {
    let Some(Value::Object(map)) = structured.as_mut() else {
        return ImageLift::Absent;
    };
    let Some(data) = map.remove(IMAGE_B64_KEY) else {
        return ImageLift::Absent;
    };
    // The claimed type is not trusted; the bytes decide.
    map.remove(IMAGE_MIME_KEY);

    let Some(b64) = data.as_str() else {
        return ImageLift::Rejected("the image data was not text".to_string());
    };
    // Refuse before decoding, so a huge string costs nothing.
    if b64.len() / 4 * 3 > MAX_IMAGE_BYTES + 3 {
        return ImageLift::Rejected(format!(
            "the image is over the {} MB limit",
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else {
        return ImageLift::Rejected("the image data was not valid base64".to_string());
    };
    if bytes.len() > MAX_IMAGE_BYTES {
        return ImageLift::Rejected(format!(
            "the image is over the {} MB limit",
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    match sniff_image(&bytes) {
        Ok(media_type) => ImageLift::Attached(ToolImage::new(media_type, b64, bytes.len())),
        Err(why) => ImageLift::Rejected(why),
    }
}

// ============================================================================
// 2. A finished call
// ============================================================================

/// A tool call's result plus the image it returned, if any.
pub struct ToolOutcome {
    pub result: ToolResult,
    pub image: Option<ToolImage>,
    /// Set when the tool returned an image that could not be attached. Added to
    /// the text the model reads, so a missing picture is stated, not silent.
    pub image_note: Option<String>,
}

impl ToolOutcome {
    pub fn from_result(mut result: ToolResult) -> Self {
        let (image, image_note) = match lift_image(&mut result.structured_data) {
            ImageLift::Absent => (None, None),
            ImageLift::Attached(image) => (Some(image), None),
            ImageLift::Rejected(why) => {
                (None, Some(format!("[The image from this result was not attached: {why}.]")))
            }
        };
        Self { result, image, image_note }
    }

    pub fn failed(
        tool_name: impl Into<String>,
        tool_use_id: impl Into<String>,
        why: impl Into<String>,
    ) -> Self {
        Self {
            result: ToolResult {
                tool_name: tool_name.into(),
                tool_use_id: tool_use_id.into(),
                success: false,
                content: why.into(),
                structured_data: None,
                stream_topic: None,
            },
            image: None,
            image_note: None,
        }
    }
}

/// Record a finished call on its card.
///
/// Returns `false`, changing nothing, if the card already has a result. That
/// happens when Stop or Skip closed it while the call was still running, and a
/// late answer must not rewrite what the conversation has already said about it.
pub fn apply_outcome(msg: &mut ChatMessage, outcome: ToolOutcome) -> bool {
    if msg.tool_result.is_some() {
        return false;
    }
    let mut text = outcome.result.content;
    if let Some(note) = outcome.image_note {
        text.push_str("\n\n");
        text.push_str(&note);
    }
    msg.mcp_status = Some(if outcome.result.success {
        McpCommandStatus::Done
    } else {
        McpCommandStatus::Error
    });
    msg.tool_result = Some(text);
    msg.tool_result_image = outcome.image;
    true
}

/// Keep only the newest `keep` attached images in the conversation; older ones
/// are evicted (their cards keep the text and a note that the picture is gone).
pub fn evict_old_images(messages: &mut [ChatMessage], keep: usize) {
    let mut seen = 0usize;
    for msg in messages.iter_mut().rev() {
        let Some(image) = msg.tool_result_image.as_mut() else { continue };
        if !image.is_attached() {
            continue;
        }
        seen += 1;
        if seen > keep {
            image.evict();
        }
    }
}

/// The `content` of a `tool_result` block in an Anthropic-shaped request.
///
/// A plain string whenever there is no image, which is the shape every
/// text-only result has always had. With an image: a text block and an image
/// block, in that order. A model that cannot see images, or an image that has
/// been evicted, gets the text and a note saying so.
pub fn tool_result_content(result: &str, image: Option<&ToolImage>, vision: bool) -> Value {
    let Some(image) = image else {
        return Value::String(result.to_string());
    };
    match (vision, image.data()) {
        (true, Some(data)) => {
            let mut blocks = Vec::with_capacity(2);
            // An empty text block is rejected by the API, so leave it out.
            if !result.trim().is_empty() {
                blocks.push(json!({ "type": "text", "text": result }));
            }
            blocks.push(json!({
                "type": "image",
                "source": { "type": "base64", "media_type": image.media_type, "data": data },
            }));
            Value::Array(blocks)
        }
        (false, _) => Value::String(format!(
            "{result}\n\n[An image came with this result, but this model cannot see images.]"
        )),
        (true, None) => Value::String(format!(
            "{result}\n\n[The image from this result is no longer attached. Capture again to see it.]"
        )),
    }
}

// ============================================================================
// 3. Running calls in order on a worker thread
// ============================================================================

/// Where a worker leaves a finished call for the main thread to pick up.
pub type ToolSlot = Arc<Mutex<Option<ToolOutcome>>>;

/// One tool call waiting to run on the worker.
pub struct PendingCall {
    pub message_id: u32,
    pub tool_name: String,
    pub tool_use_id: String,
    pub input: Value,
    pub cancel: Arc<AtomicBool>,
    pub slot: ToolSlot,
}

/// The main thread's handle on a call that is running or about to.
pub struct InFlight {
    pub message_id: u32,
    pub tool_name: String,
    pub slot: ToolSlot,
    pub cancel: Arc<AtomicBool>,
}

impl InFlight {
    /// The finished call, if the worker has delivered it. Takes it, so each
    /// result is applied once.
    pub fn take_outcome(&self) -> Option<ToolOutcome> {
        self.slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take()
    }
}

impl PendingCall {
    /// A call for the worker and the matching handle for the main thread,
    /// sharing one result slot and one cancel flag.
    pub fn new(
        message_id: u32,
        tool_name: String,
        tool_use_id: String,
        input: Value,
    ) -> (PendingCall, InFlight) {
        let slot: ToolSlot = Arc::new(Mutex::new(None));
        let cancel = Arc::new(AtomicBool::new(false));
        let in_flight = InFlight {
            message_id,
            tool_name: tool_name.clone(),
            slot: slot.clone(),
            cancel: cancel.clone(),
        };
        (PendingCall { message_id, tool_name, tool_use_id, input, cancel, slot }, in_flight)
    }
}

fn fill(slot: &ToolSlot, outcome: ToolOutcome) {
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(outcome);
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".to_string())
}

/// Run every call in `calls`, in order, on one background thread.
///
/// `make_dispatch` runs on that thread and returns the function that executes
/// one call (the caller builds the tool registry there, off the main thread).
/// It is a factory rather than the function itself because the registry need
/// not be `Send`, only creatable on the worker.
pub fn spawn_batch<F, D>(calls: Vec<PendingCall>, make_dispatch: F)
where
    F: FnOnce() -> D + Send + 'static,
    D: Fn(&PendingCall) -> ToolResult,
{
    if calls.is_empty() {
        return;
    }
    // Kept outside the thread, so a thread that cannot start can still answer
    // every call instead of leaving each card on Running.
    let waiting: Vec<(ToolSlot, String, String)> = calls
        .iter()
        .map(|c| (c.slot.clone(), c.tool_name.clone(), c.tool_use_id.clone()))
        .collect();

    let spawned = std::thread::Builder::new().name("workshop-tools".into()).spawn(move || {
        match catch_unwind(AssertUnwindSafe(make_dispatch)) {
            Ok(dispatch) => run_batch(&calls, &dispatch),
            Err(payload) => {
                let why = format!("The tools could not be set up: {}", panic_message(&*payload));
                for call in &calls {
                    fill(&call.slot, ToolOutcome::failed(&call.tool_name, &call.tool_use_id, why.clone()));
                }
            }
        }
    });

    if let Err(error) = spawned {
        for (slot, tool_name, tool_use_id) in waiting {
            fill(
                &slot,
                ToolOutcome::failed(
                    tool_name,
                    tool_use_id,
                    format!("Could not start a worker thread for this tool: {error}"),
                ),
            );
        }
    }
}

/// The ordered loop behind [`spawn_batch`], separate so it can be tested
/// without a thread.
pub fn run_batch<D>(calls: &[PendingCall], dispatch: &D)
where
    D: Fn(&PendingCall) -> ToolResult,
{
    for call in calls {
        if call.cancel.load(Ordering::Relaxed) {
            // Stopped before this call began: it never runs.
            fill(
                &call.slot,
                ToolOutcome::failed(&call.tool_name, &call.tool_use_id, "Stopped before this ran."),
            );
            continue;
        }
        let outcome = match catch_unwind(AssertUnwindSafe(|| dispatch(call))) {
            Ok(result) => ToolOutcome::from_result(result),
            Err(payload) => ToolOutcome::failed(
                &call.tool_name,
                &call.tool_use_id,
                format!("The tool crashed: {}", panic_message(&*payload)),
            ),
        };
        fill(&call.slot, outcome);
    }
}

// ============================================================================
// 4. Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    // ---- fixtures: complete (if tiny) images of each type -------------------

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 2, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 0]); // IHDR CRC, not checked here
        // A data chunk, so the image has a body like a real one: a cut-off
        // file is then longer than the header and reaches the end check.
        bytes.extend_from_slice(&4u32.to_be_bytes());
        bytes.extend_from_slice(b"IDAT");
        bytes.extend_from_slice(&[1, 2, 3, 4]);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(&PNG_IEND);
        bytes
    }

    fn jpeg() -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0];
        bytes.extend_from_slice(&[0xFF, 0xD9]);
        bytes
    }

    fn gif(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0]);
        bytes.extend_from_slice(&[0x2C, 1, 2, 3, 4, 5, 6, 7]); // image data
        bytes.push(0x3B);
        bytes
    }

    fn webp() -> Vec<u8> {
        let payload = b"VP8 ....";
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&((payload.len() + 4) as u32).to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(payload);
        bytes
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn structured(bytes: &[u8], claimed: &str) -> Option<Value> {
        Some(json!({ IMAGE_B64_KEY: b64(bytes), IMAGE_MIME_KEY: claimed, "bytes": bytes.len() }))
    }

    fn result(content: &str, data: Option<Value>) -> ToolResult {
        ToolResult {
            tool_name: "ai_camera_capture".into(),
            tool_use_id: "toolu_1".into(),
            success: true,
            content: content.into(),
            structured_data: data,
            stream_topic: None,
        }
    }

    // ---- sniffing -----------------------------------------------------------

    #[test]
    fn each_supported_type_is_recognised_by_its_bytes() {
        assert_eq!(sniff_image(&png(1920, 1080)), Ok("image/png"));
        assert_eq!(sniff_image(&jpeg()), Ok("image/jpeg"));
        assert_eq!(sniff_image(&gif(64, 64)), Ok("image/gif"));
        assert_eq!(sniff_image(&webp()), Ok("image/webp"));
    }

    #[test]
    fn a_cut_off_image_is_refused() {
        // The failure that actually happens: a file read before it was done.
        let mut cut_png = png(100, 100);
        cut_png.truncate(cut_png.len() - 5);
        assert!(sniff_image(&cut_png).unwrap_err().contains("cut off"));

        let mut cut_jpeg = jpeg();
        cut_jpeg.truncate(cut_jpeg.len() - 2);
        cut_jpeg.extend_from_slice(&[0u8; 40]);
        assert!(sniff_image(&cut_jpeg).unwrap_err().contains("cut off"));

        let mut cut_gif = gif(8, 8);
        cut_gif.pop();
        assert!(sniff_image(&cut_gif).unwrap_err().contains("cut off"));

        let mut cut_webp = webp();
        cut_webp.truncate(cut_webp.len() - 3);
        assert!(sniff_image(&cut_webp).unwrap_err().contains("cut off"));
    }

    #[test]
    fn absurd_dimensions_are_refused() {
        assert!(sniff_image(&png(MAX_IMAGE_SIDE + 1, 10)).unwrap_err().contains("pixel limit"));
        assert!(sniff_image(&png(10, 0)).unwrap_err().contains("no pixels"));
        assert!(sniff_image(&gif(9000, 10)).unwrap_err().contains("pixel limit"));
        // The limit itself is allowed.
        assert_eq!(sniff_image(&png(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE)), Ok("image/png"));
    }

    #[test]
    fn data_that_is_not_an_image_is_refused() {
        assert!(sniff_image(b"").is_err());
        assert!(sniff_image(b"<html>not an image</html>").unwrap_err().contains("not a PNG"));
        assert!(sniff_image(&[0x89, b'P', b'N', b'G']).is_err());
    }

    // ---- lifting an image out of a result -----------------------------------

    #[test]
    fn a_result_with_no_image_is_left_alone() {
        let mut data = Some(json!({ "entities": 3 }));
        assert!(matches!(lift_image(&mut data), ImageLift::Absent));
        assert_eq!(data, Some(json!({ "entities": 3 })));
        assert!(matches!(lift_image(&mut None), ImageLift::Absent));
        assert!(matches!(lift_image(&mut Some(json!("text"))), ImageLift::Absent));
    }

    #[test]
    fn the_bytes_decide_the_type_not_the_claim() {
        let mut data = structured(&png(64, 64), "image/jpeg");
        let ImageLift::Attached(image) = lift_image(&mut data) else { panic!("should attach") };
        assert_eq!(image.media_type, "image/png");
        assert!(image.is_attached());
    }

    #[test]
    fn lifting_removes_the_payload_even_when_the_image_is_refused() {
        let mut ok = structured(&png(8, 8), "image/png");
        lift_image(&mut ok);
        let mut bad = structured(b"not an image", "image/png");
        assert!(matches!(lift_image(&mut bad), ImageLift::Rejected(_)));
        for data in [ok.unwrap(), bad.unwrap()] {
            assert!(data.get(IMAGE_B64_KEY).is_none() && data.get(IMAGE_MIME_KEY).is_none());
            assert!(data.get("bytes").is_some(), "unrelated fields stay");
        }
    }

    #[test]
    fn broken_or_oversized_payloads_are_rejected_with_a_reason() {
        let mut not_base64 = Some(json!({ IMAGE_B64_KEY: "***not base64***" }));
        let ImageLift::Rejected(why) = lift_image(&mut not_base64) else { panic!() };
        assert!(why.contains("base64"), "{why}");

        let mut not_text = Some(json!({ IMAGE_B64_KEY: 12 }));
        assert!(matches!(lift_image(&mut not_text), ImageLift::Rejected(_)));

        // Over the cap: refused from the length alone, without decoding it.
        let huge = "A".repeat(MAX_IMAGE_BYTES / 3 * 4 + 400);
        let mut too_big = Some(json!({ IMAGE_B64_KEY: huge }));
        let ImageLift::Rejected(why) = lift_image(&mut too_big) else { panic!() };
        assert!(why.contains("limit"), "{why}");
    }

    #[test]
    fn a_refused_image_is_stated_in_the_result_text() {
        let outcome = ToolOutcome::from_result(result(
            "Screenshot captured and attached below.",
            structured(b"garbage", "image/png"),
        ));
        assert!(outcome.image.is_none());
        let note = outcome.image_note.expect("a refusal must be explained");
        assert!(note.contains("not attached"), "{note}");

        let ok = ToolOutcome::from_result(result("Captured.", structured(&png(8, 8), "image/png")));
        assert!(ok.image.is_some() && ok.image_note.is_none());
        let none = ToolOutcome::from_result(result("3 entities", Some(json!({ "n": 3 }))));
        assert!(none.image.is_none() && none.image_note.is_none());
    }

    // ---- recording and retention --------------------------------------------

    fn card() -> ChatMessage {
        ChatMessage { mcp_status: Some(McpCommandStatus::Running), ..Default::default() }
    }

    #[test]
    fn a_finished_call_is_recorded_on_its_card() {
        let mut msg = card();
        let outcome = ToolOutcome::from_result(result("Captured.", structured(&png(8, 8), "image/png")));
        assert!(apply_outcome(&mut msg, outcome));
        assert_eq!(msg.tool_result.as_deref(), Some("Captured."));
        assert_eq!(msg.mcp_status, Some(McpCommandStatus::Done));
        assert!(msg.tool_result_image.is_some());

        let mut failed = card();
        apply_outcome(&mut failed, ToolOutcome::failed("t", "id", "boom"));
        assert_eq!(failed.mcp_status, Some(McpCommandStatus::Error));
    }

    #[test]
    fn a_refusal_note_reaches_the_text_the_model_reads() {
        let mut msg = card();
        let outcome = ToolOutcome::from_result(result("Captured.", structured(b"garbage", "image/png")));
        apply_outcome(&mut msg, outcome);
        let text = msg.tool_result.unwrap();
        assert!(text.starts_with("Captured.") && text.contains("not attached"), "{text}");
        assert!(msg.tool_result_image.is_none());
    }

    #[test]
    fn a_late_answer_never_rewrites_a_closed_card() {
        // Stop closed this card with a note while the tool was still running.
        let mut msg = card();
        msg.tool_result = Some("Stopped by the user before this finished.".into());
        msg.mcp_status = Some(McpCommandStatus::Skipped);
        let late = ToolOutcome::from_result(result("Captured.", structured(&png(8, 8), "image/png")));
        assert!(!apply_outcome(&mut msg, late));
        assert_eq!(msg.tool_result.as_deref(), Some("Stopped by the user before this finished."));
        assert_eq!(msg.mcp_status, Some(McpCommandStatus::Skipped));
        assert!(msg.tool_result_image.is_none());
    }

    fn with_image(attached: bool) -> ChatMessage {
        let mut image = ToolImage::new("image/png", "AAAA", 3);
        if !attached {
            image.evict();
        }
        ChatMessage { tool_result_image: Some(image), ..Default::default() }
    }

    #[test]
    fn only_the_newest_images_stay_attached() {
        let mut messages: Vec<ChatMessage> = (0..6).map(|_| with_image(true)).collect();
        messages.insert(2, ChatMessage::default()); // a message with no image
        evict_old_images(&mut messages, 3);
        let attached: Vec<bool> = messages
            .iter()
            .filter_map(|m| m.tool_result_image.as_ref())
            .map(|i| i.is_attached())
            .collect();
        assert_eq!(attached, vec![false, false, false, true, true, true]);
    }

    #[test]
    fn eviction_counts_only_images_still_attached() {
        // An already-evicted image must not use up one of the kept slots.
        let mut messages = vec![with_image(true), with_image(false), with_image(true), with_image(true)];
        evict_old_images(&mut messages, 2);
        let attached: Vec<bool> = messages
            .iter()
            .filter_map(|m| m.tool_result_image.as_ref())
            .map(|i| i.is_attached())
            .collect();
        assert_eq!(attached, vec![false, false, true, true]);
    }

    // ---- the shape the provider receives --------------------------------------

    #[test]
    fn a_text_only_result_keeps_its_plain_string_shape() {
        assert_eq!(tool_result_content("3 entities", None, true), json!("3 entities"));
        assert_eq!(tool_result_content("3 entities", None, false), json!("3 entities"));
    }

    #[test]
    fn an_image_result_is_text_then_image() {
        let image = ToolImage::new("image/png", "QUJD", 3);
        let content = tool_result_content("Captured.", Some(&image), true);
        assert_eq!(
            content,
            json!([
                { "type": "text", "text": "Captured." },
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "QUJD" } },
            ])
        );
        // No empty text block: the API rejects one.
        let bare = tool_result_content("  ", Some(&image), true);
        assert_eq!(bare.as_array().map(|a| a.len()), Some(1));
    }

    #[test]
    fn a_model_that_cannot_see_gets_the_text_and_a_note() {
        let image = ToolImage::new("image/png", "QUJD", 3);
        let content = tool_result_content("Captured.", Some(&image), false);
        let text = content.as_str().expect("a plain string");
        assert!(text.starts_with("Captured.") && text.contains("cannot see images"), "{text}");
    }

    #[test]
    fn an_evicted_image_is_replaced_by_a_note() {
        let mut image = ToolImage::new("image/png", "QUJD", 3);
        image.evict();
        let content = tool_result_content("Captured.", Some(&image), true);
        let text = content.as_str().expect("a plain string");
        assert!(text.contains("no longer attached"), "{text}");
    }

    #[test]
    fn a_tool_image_never_prints_its_payload() {
        let image = ToolImage::new("image/png", &"Z".repeat(10_000), 7_500);
        let shown = format!("{image:?}");
        assert!(shown.len() < 80, "{shown}");
    }

    // ---- the ordered runner ---------------------------------------------------

    fn call(n: u32) -> (PendingCall, InFlight) {
        PendingCall::new(n, format!("tool_{n}"), format!("toolu_{n}"), json!({ "n": n }))
    }

    fn ok_result(call: &PendingCall) -> ToolResult {
        result(&format!("ran {}", call.tool_name), None)
    }

    fn text_of(handle: &InFlight) -> String {
        handle.take_outcome().expect("the call should have finished").result.content
    }

    #[test]
    fn calls_run_in_the_order_they_were_given() {
        // A Model before its child Part: the child must never run first.
        let order = Mutex::new(Vec::new());
        let (calls, handles): (Vec<_>, Vec<_>) = (1..=4).map(call).unzip();
        run_batch(&calls, &|c: &PendingCall| {
            order.lock().unwrap().push(c.message_id);
            ok_result(c)
        });
        assert_eq!(*order.lock().unwrap(), vec![1, 2, 3, 4]);
        assert_eq!(text_of(&handles[2]), "ran tool_3");
    }

    #[test]
    fn a_call_cancelled_before_it_starts_never_runs() {
        let ran = Mutex::new(Vec::new());
        let (calls, handles): (Vec<_>, Vec<_>) = (1..=3).map(call).unzip();
        handles[1].cancel.store(true, Ordering::Relaxed);
        run_batch(&calls, &|c: &PendingCall| {
            ran.lock().unwrap().push(c.message_id);
            ok_result(c)
        });
        assert_eq!(*ran.lock().unwrap(), vec![1, 3]);
        assert!(text_of(&handles[1]).contains("Stopped"));
    }

    #[test]
    fn stopping_mid_call_prevents_the_rest_of_the_turn() {
        let (calls, handles): (Vec<_>, Vec<_>) = (1..=3).map(call).unzip();
        let flags: Vec<_> = handles.iter().map(|h| h.cancel.clone()).collect();
        let ran = Mutex::new(Vec::new());
        run_batch(&calls, &|c: &PendingCall| {
            ran.lock().unwrap().push(c.message_id);
            // The user presses Stop while the first call is running.
            for flag in &flags {
                flag.store(true, Ordering::Relaxed);
            }
            ok_result(c)
        });
        assert_eq!(*ran.lock().unwrap(), vec![1], "calls 2 and 3 must not start");
    }

    #[test]
    fn a_crashing_tool_fails_alone() {
        let (calls, handles): (Vec<_>, Vec<_>) = (1..=3).map(call).unzip();
        run_batch(&calls, &|c: &PendingCall| {
            if c.message_id == 2 {
                panic!("handler blew up");
            }
            ok_result(c)
        });
        assert_eq!(text_of(&handles[0]), "ran tool_1");
        let crashed = handles[1].take_outcome().unwrap();
        assert!(!crashed.result.success);
        assert!(crashed.result.content.contains("handler blew up"), "{}", crashed.result.content);
        assert_eq!(text_of(&handles[2]), "ran tool_3", "the turn carries on past a crash");
    }

    fn wait_for(handle: &InFlight) -> ToolOutcome {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(outcome) = handle.take_outcome() {
                return outcome;
            }
            assert!(Instant::now() < deadline, "the worker never delivered a result");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_real_worker_thread_delivers_every_result() {
        let (calls, handles): (Vec<_>, Vec<_>) = (1..=3).map(call).unzip();
        let main_thread = std::thread::current().id();
        spawn_batch(calls, move || {
            move |c: &PendingCall| {
                assert_ne!(std::thread::current().id(), main_thread, "tools must not run on the caller's thread");
                ok_result(c)
            }
        });
        for (i, handle) in handles.iter().enumerate() {
            assert_eq!(wait_for(handle).result.content, format!("ran tool_{}", i + 1));
        }
    }

    #[test]
    fn a_registry_that_cannot_be_built_fails_every_call_instead_of_hanging() {
        let (calls, handles): (Vec<_>, Vec<_>) = (1..=2).map(call).unzip();
        spawn_batch(calls, || -> fn(&PendingCall) -> ToolResult { panic!("no registry today") });
        for handle in &handles {
            let outcome = wait_for(handle);
            assert!(!outcome.result.success);
            assert!(outcome.result.content.contains("no registry today"), "{}", outcome.result.content);
        }
    }

    #[test]
    fn an_image_returned_on_the_worker_arrives_validated() {
        let (calls, handles): (Vec<_>, Vec<_>) = (1..=1).map(call).unzip();
        spawn_batch(calls, || {
            |_: &PendingCall| result("Captured.", structured(&png(1280, 720), "image/png"))
        });
        let outcome = wait_for(&handles[0]);
        let image = outcome.image.expect("the capture should arrive attached");
        assert_eq!(image.media_type, "image/png");
        assert!(outcome.result.structured_data.is_none_or(|d| d.get(IMAGE_B64_KEY).is_none()));
    }
}
