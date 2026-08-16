// =============================================================================
// Eustress Web - Document Capture
// =============================================================================
// Camera access and the image pipeline behind /verify.
//
// A phone photo of an ID is rarely usable as-is: it arrives at 4000px with the
// document occupying a third of the frame, tilted lighting, and often motion
// blur. This module turns a live video frame into something a verifier can
// actually read:
//
//   1. Crop to the on-screen guide, so only the document survives.
//   2. Downscale to a long edge of MAX_EDGE (OCR gains nothing above this and
//      the upload gets ~8x smaller).
//   3. Auto-level using percentile black/white points, which fixes the dim,
//      grey look of an indoor capture without touching colour balance.
//   4. Score sharpness, brightness, and glare, and refuse a capture that is
//      obviously unreadable rather than letting the applicant discover it
//      after the verification round-trip.
//
// Processing stays deliberately conservative. Heavy filtering (thresholding,
// aggressive sharpening) makes a genuine document look tampered with, and a
// document-authenticity check is exactly what these images feed into.
// =============================================================================

use wasm_bindgen::{Clamped, JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Blob, BlobPropertyBag, CanvasRenderingContext2d, HtmlCanvasElement, HtmlVideoElement,
    ImageData, MediaStream, MediaStreamConstraints,
};

/// Longest edge of the stored image, in pixels. 1600 keeps the smallest text on
/// a driver's licence legible while holding a JPEG under roughly 800 KB.
const MAX_EDGE: f64 = 1600.0;

/// JPEG quality. High enough to avoid ringing around small print, low enough
/// that a phone on cellular data still uploads quickly.
const JPEG_QUALITY: f64 = 0.92;

/// Aspect ratio of an ID-1 card (85.6mm x 53.98mm): driver's licences and most
/// national ID cards.
pub const ASPECT_CARD: f64 = 1.586;

/// Aspect ratio of a passport data page (125mm x 88mm).
pub const ASPECT_PASSPORT: f64 = 1.42;

/// Pick the guide shape from the document type chosen during registration.
pub fn aspect_for(id_type: &str) -> f64 {
    let t = id_type.to_lowercase();
    if t.contains("passport") {
        ASPECT_PASSPORT
    } else {
        ASPECT_CARD
    }
}

// ── Quality scoring ──────────────────────────────────────────────────────────

/// What the pipeline measured about a capture, and whether it is good enough.
#[derive(Clone, Debug)]
pub struct CaptureQuality {
    /// Variance of the Laplacian. Low means out of focus or motion blurred.
    pub sharpness: f64,
    /// Mean luminance, 0-255.
    pub brightness: f64,
    /// Percent of pixels blown to near-white, which is what flash glare and
    /// reflections off a laminated card look like.
    pub glare_pct: f64,
    /// False when the capture should be retaken.
    pub acceptable: bool,
    /// Plain-language reason shown to the applicant when `acceptable` is false.
    pub advice: String,
}

// Thresholds are intentionally permissive: a false reject costs the applicant a
// retake for no reason, while a marginal image still has a chance of passing
// verification. Only clearly unusable captures are refused.
const MIN_SHARPNESS: f64 = 55.0;
const MIN_BRIGHTNESS: f64 = 42.0;
const MAX_BRIGHTNESS: f64 = 232.0;
const MAX_GLARE_PCT: f64 = 8.0;

impl CaptureQuality {
    fn evaluate(sharpness: f64, brightness: f64, glare_pct: f64) -> Self {
        // Order matters: report the single most actionable problem rather than
        // a list the applicant has to triage.
        let advice = if brightness < MIN_BRIGHTNESS {
            "Too dark. Move somewhere brighter or turn on a light."
        } else if brightness > MAX_BRIGHTNESS {
            "Too bright. Move out of direct light."
        } else if glare_pct > MAX_GLARE_PCT {
            "Glare is covering part of the document. Tilt it away from the light."
        } else if sharpness < MIN_SHARPNESS {
            "Too blurry. Hold steady and let the camera focus."
        } else {
            ""
        };

        Self {
            sharpness,
            brightness,
            glare_pct,
            acceptable: advice.is_empty(),
            advice: advice.to_string(),
        }
    }
}

/// A processed capture, ready to preview and upload.
pub struct Captured {
    /// `data:image/jpeg;base64,...` for the preview <img>.
    pub data_url: String,
    /// The same bytes as a Blob, for the multipart upload.
    pub blob: Blob,
    pub quality: CaptureQuality,
}

// ── Camera lifecycle ─────────────────────────────────────────────────────────

/// Open the rear camera and attach it to `video`.
///
/// Resolution is requested, not demanded: `ideal` lets a phone that cannot do
/// 1920x1080 fall back to its best mode instead of failing outright, which an
/// `exact` constraint would do.
pub async fn start_camera(video: &HtmlVideoElement) -> Result<MediaStream, String> {
    let window = web_sys::window().ok_or("No window")?;
    let devices = window
        .navigator()
        .media_devices()
        .map_err(|_| "This browser does not expose camera access.".to_string())?;

    let video_constraints = js_sys::Object::new();
    // "environment" is the rear camera. Documents are photographed with the
    // rear camera; the selfie camera is both lower resolution and mirrored.
    js_sys::Reflect::set(
        &video_constraints,
        &JsValue::from_str("facingMode"),
        &JsValue::from_str("environment"),
    )
    .ok();
    for (key, value) in [("width", 1920.0), ("height", 1080.0)] {
        let ideal = js_sys::Object::new();
        js_sys::Reflect::set(&ideal, &JsValue::from_str("ideal"), &JsValue::from_f64(value)).ok();
        js_sys::Reflect::set(&video_constraints, &JsValue::from_str(key), &ideal).ok();
    }

    let constraints = MediaStreamConstraints::new();
    constraints.set_video(&video_constraints);
    constraints.set_audio(&JsValue::FALSE);

    let promise = devices
        .get_user_media_with_constraints(&constraints)
        .map_err(|_| "Could not request the camera.".to_string())?;

    let stream: MediaStream = JsFuture::from(promise)
        .await
        .map_err(|e| describe_camera_error(&e))?
        .dyn_into()
        .map_err(|_| "Camera returned an unexpected stream.".to_string())?;

    video.set_src_object(Some(&stream));
    // iOS Safari refuses to play an inline video without these, and a video
    // that will not play produces a black frame rather than an error.
    video.set_muted(true);
    video.set_autoplay(true);
    let _ = video.set_attribute("playsinline", "true");
    if let Ok(p) = video.play() {
        let _ = JsFuture::from(p).await;
    }

    Ok(stream)
}

/// Bind an already-open stream to a `<video>` and start playback.
///
/// The capture flow unmounts the viewfinder while a shot is being reviewed, so
/// returning for the second side of a document mounts a *new* `<video>` with no
/// source. Re-attaching the live stream avoids tearing the camera down and
/// asking the operating system for it again between the two sides.
pub async fn attach_stream(video: &HtmlVideoElement, stream: &MediaStream) {
    video.set_src_object(Some(stream));
    video.set_muted(true);
    video.set_autoplay(true);
    let _ = video.set_attribute("playsinline", "true");
    if let Ok(p) = video.play() {
        let _ = JsFuture::from(p).await;
    }
}

/// Whether a stream still has live tracks and can be re-attached.
///
/// A stream whose tracks ended (the OS revoked the camera, another app took
/// it) reports inactive, and re-attaching it would leave a black viewfinder.
pub fn stream_is_live(stream: &MediaStream) -> bool {
    stream.active()
}

/// Turn the camera off and drop the hardware indicator.
///
/// Clearing srcObject alone leaves the track live, so the phone keeps showing a
/// recording indicator after the applicant has finished.
pub fn stop_camera(stream: &MediaStream) {
    let tracks = stream.get_tracks();
    for i in 0..tracks.length() {
        if let Ok(track) = tracks.get(i).dyn_into::<web_sys::MediaStreamTrack>() {
            track.stop();
        }
    }
}

/// Map a getUserMedia rejection onto something an applicant can act on.
fn describe_camera_error(err: &JsValue) -> String {
    let name = err
        .clone()
        .dyn_into::<web_sys::DomException>()
        .map(|e| e.name())
        .unwrap_or_default();

    match name.as_str() {
        "NotAllowedError" | "SecurityError" =>
            "Camera access was blocked. Allow camera access for this site, then tap Retry.".into(),
        "NotFoundError" | "OverconstrainedError" =>
            "No camera was found on this device.".into(),
        "NotReadableError" =>
            "The camera is already in use by another app. Close it and tap Retry.".into(),
        _ => "Could not open the camera. You can upload a photo instead.".into(),
    }
}

// ── Geometry ─────────────────────────────────────────────────────────────────

/// The document guide, in the video's own pixel coordinates.
///
/// Returned as `(x, y, w, h)`. The guide fills 92% of whichever axis binds
/// first, so it stays fully visible whether the phone is held portrait or
/// landscape.
pub fn guide_rect(video_w: f64, video_h: f64, aspect: f64) -> (f64, f64, f64, f64) {
    let w = (video_w * 0.92).min(video_h * 0.92 * aspect);
    let h = w / aspect;
    (((video_w - w) / 2.0), ((video_h - h) / 2.0), w, h)
}

/// Where to draw the guide overlay inside the <video> element's box.
///
/// The element is styled `object-fit: contain`, so the frame is letterboxed
/// rather than cropped. Deriving the overlay from the same contain transform
/// used for the crop keeps what the applicant frames identical to what is
/// actually saved. Returned as `(left, top, width, height)` in CSS pixels.
pub fn overlay_rect(
    video_w: f64,
    video_h: f64,
    element_w: f64,
    element_h: f64,
    aspect: f64,
) -> (f64, f64, f64, f64) {
    if video_w <= 0.0 || video_h <= 0.0 || element_w <= 0.0 || element_h <= 0.0 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    // contain: one scale for both axes, chosen so the whole frame fits.
    let scale = (element_w / video_w).min(element_h / video_h);
    let shown_w = video_w * scale;
    let shown_h = video_h * scale;
    let pad_x = (element_w - shown_w) / 2.0;
    let pad_y = (element_h - shown_h) / 2.0;

    let (gx, gy, gw, gh) = guide_rect(video_w, video_h, aspect);
    (
        pad_x + gx * scale,
        pad_y + gy * scale,
        gw * scale,
        gh * scale,
    )
}

// ── Capture pipeline ─────────────────────────────────────────────────────────

/// Grab the current video frame, crop it to the guide, clean it, and score it.
pub fn capture_document(
    video: &HtmlVideoElement,
    canvas: &HtmlCanvasElement,
    aspect: f64,
) -> Result<Captured, String> {
    let vw = video.video_width() as f64;
    let vh = video.video_height() as f64;
    if vw <= 0.0 || vh <= 0.0 {
        return Err("The camera is still starting. Try again in a moment.".into());
    }

    let (sx, sy, sw, sh) = guide_rect(vw, vh, aspect);

    // Never upscale: if the crop is already smaller than MAX_EDGE, keep it.
    let scale = (MAX_EDGE / sw.max(sh)).min(1.0);
    let dw = (sw * scale).round();
    let dh = (sh * scale).round();

    canvas.set_width(dw as u32);
    canvas.set_height(dh as u32);

    let ctx: CanvasRenderingContext2d = canvas
        .get_context("2d")
        .map_err(|_| "Canvas unavailable".to_string())?
        .ok_or("Canvas 2D context unavailable")?
        .dyn_into()
        .map_err(|_| "Canvas 2D context unavailable".to_string())?;

    ctx.set_image_smoothing_enabled(true);
    ctx.draw_image_with_html_video_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
        video, sx, sy, sw, sh, 0.0, 0.0, dw, dh,
    )
    .map_err(|_| "Could not read a frame from the camera.".to_string())?;

    // Process pixels, then write them back before encoding.
    let image = ctx
        .get_image_data(0.0, 0.0, dw, dh)
        .map_err(|_| "Could not read the captured frame.".to_string())?;
    let mut pixels = image.data().0;

    let quality = auto_level_and_score(&mut pixels, dw as usize, dh as usize);

    let cleaned = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&pixels), dw as u32, dh as u32)
        .map_err(|_| "Could not rebuild the cleaned image.".to_string())?;
    ctx.put_image_data(&cleaned, 0.0, 0.0)
        .map_err(|_| "Could not write the cleaned image.".to_string())?;

    let data_url = canvas
        .to_data_url_with_type_and_encoder_options("image/jpeg", &JsValue::from_f64(JPEG_QUALITY))
        .map_err(|_| "Could not encode the image.".to_string())?;

    let blob = data_url_to_blob(&data_url)?;

    Ok(Captured {
        data_url,
        blob,
        quality,
    })
}

/// Stretch contrast to percentile black/white points and measure the result.
///
/// Operates on `pixels` in place (RGBA). Returns the quality scores, which are
/// computed from the ORIGINAL luminance so that auto-levelling cannot flatter a
/// blurry or glare-covered capture into passing.
fn auto_level_and_score(pixels: &mut [u8], width: usize, height: usize) -> CaptureQuality {
    let count = width * height;
    if count == 0 {
        return CaptureQuality::evaluate(0.0, 0.0, 100.0);
    }

    // Luminance plane, reused for the histogram and the sharpness pass.
    let mut luma = vec![0u8; count];
    let mut histogram = [0u32; 256];
    let mut sum = 0f64;
    let mut blown = 0u32;

    for i in 0..count {
        let r = pixels[i * 4] as f64;
        let g = pixels[i * 4 + 1] as f64;
        let b = pixels[i * 4 + 2] as f64;
        // Rec. 601 luma; matches how the eye weights the channels.
        let y = 0.299 * r + 0.587 * g + 0.114 * b;
        let yb = y as u8;
        luma[i] = yb;
        histogram[yb as usize] += 1;
        sum += y;
        if y >= 250.0 {
            blown += 1;
        }
    }

    let brightness = sum / count as f64;
    let glare_pct = blown as f64 * 100.0 / count as f64;
    let sharpness = laplacian_variance(&luma, width, height);

    // Percentile endpoints. Clipping 0.5% at each end ignores specular dots and
    // dark corners that would otherwise pin the range wide open and do nothing.
    let clip = (count as f64 * 0.005) as u32;
    let low = percentile_bound(&histogram, clip, false) as f64;
    let high = percentile_bound(&histogram, clip, true) as f64;

    // Only stretch when there is a real range to work with. Below this the
    // image is nearly flat and stretching it mostly amplifies sensor noise.
    let span = high - low;
    if span > 40.0 {
        let gain = 255.0 / span;
        // A lookup table beats recomputing per subpixel: 256 entries versus
        // several million multiply-and-clamp operations.
        let mut lut = [0u8; 256];
        for (v, entry) in lut.iter_mut().enumerate() {
            *entry = (((v as f64 - low) * gain).clamp(0.0, 255.0)) as u8;
        }
        for i in 0..count {
            pixels[i * 4] = lut[pixels[i * 4] as usize];
            pixels[i * 4 + 1] = lut[pixels[i * 4 + 1] as usize];
            pixels[i * 4 + 2] = lut[pixels[i * 4 + 2] as usize];
            // Alpha (i * 4 + 3) is left alone; canvas frames are already opaque.
        }
    }

    CaptureQuality::evaluate(sharpness, brightness, glare_pct)
}

/// Lowest (or highest) luminance remaining after discarding `clip` pixels from
/// that end of the histogram.
fn percentile_bound(histogram: &[u32; 256], clip: u32, from_top: bool) -> u8 {
    let mut seen = 0u32;
    if from_top {
        for v in (0..256).rev() {
            seen += histogram[v];
            if seen > clip {
                return v as u8;
            }
        }
        255
    } else {
        for (v, bucket) in histogram.iter().enumerate() {
            seen += bucket;
            if seen > clip {
                return v as u8;
            }
        }
        0
    }
}

/// Variance of the Laplacian: the standard focus measure.
///
/// A sharp document has strong second derivatives at every glyph edge, so the
/// response spreads out. A blurred one has a response clustered near zero.
/// Sampled on a stride to keep this well under a frame budget on a phone.
fn laplacian_variance(luma: &[u8], width: usize, height: usize) -> f64 {
    if width < 3 || height < 3 {
        return 0.0;
    }
    // Roughly 40k samples regardless of resolution: enough for a stable
    // variance, cheap enough to run on every capture.
    let stride = ((width * height) as f64 / 40_000.0).sqrt().max(1.0) as usize;

    let mut sum = 0f64;
    let mut sum_sq = 0f64;
    let mut n = 0f64;

    let mut y = 1;
    while y < height - 1 {
        let mut x = 1;
        while x < width - 1 {
            let c = luma[y * width + x] as f64;
            // 4-neighbour Laplacian kernel.
            let response = luma[(y - 1) * width + x] as f64
                + luma[(y + 1) * width + x] as f64
                + luma[y * width + x - 1] as f64
                + luma[y * width + x + 1] as f64
                - 4.0 * c;
            sum += response;
            sum_sq += response * response;
            n += 1.0;
            x += stride;
        }
        y += stride;
    }

    if n < 2.0 {
        return 0.0;
    }
    let mean = sum / n;
    (sum_sq / n - mean * mean).max(0.0)
}

// ── Encoding ─────────────────────────────────────────────────────────────────

/// Convert a `data:...;base64,...` URL into a Blob for multipart upload.
pub fn data_url_to_blob(data_url: &str) -> Result<Blob, String> {
    let comma = data_url
        .find(',')
        .ok_or("Malformed image data".to_string())?;
    let header = &data_url[..comma];
    let payload = &data_url[comma + 1..];

    let mime = header
        .strip_prefix("data:")
        .and_then(|h| h.split(';').next())
        .unwrap_or("image/jpeg")
        .to_string();

    let window = web_sys::window().ok_or("No window")?;
    // atob yields a binary string: one char per byte, each in 0..=255.
    let binary = window
        .atob(payload)
        .map_err(|_| "Could not decode the image data.".to_string())?;
    let bytes: Vec<u8> = binary.chars().map(|c| c as u8).collect();

    let array = js_sys::Uint8Array::from(&bytes[..]);
    let parts = js_sys::Array::new();
    parts.push(&array);

    let options = BlobPropertyBag::new();
    options.set_type(&mime);

    Blob::new_with_u8_array_sequence_and_options(&parts, &options)
        .map_err(|_| "Could not package the image for upload.".to_string())
}
