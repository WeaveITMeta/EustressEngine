// =============================================================================
// Eustress Web - Mobile Identity Verification
// =============================================================================
// The phone half of the desktop -> mobile KYC handoff.
//
// A desktop browser usually has a poor webcam or none at all, so registration
// shows a QR code and the applicant finishes document capture here. The session
// id arrives as `?s=<id>`; everything else (which document, whether a back side
// is needed, the applicant's name and birthday) is held server-side against
// that id, so nothing personal travels through the QR code or this URL.
//
// This page is also linkable directly on a phone, in which case it behaves the
// same way as long as the handoff has not expired.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use web_sys::MediaStream;

use crate::capture::{aspect_for, capture_document, overlay_rect, start_camera, stop_camera};
use crate::components::Footer;


#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    /// Fetching the handoff context for the session id in the URL.
    Loading,
    /// No session id, or the handoff expired. Terminal.
    Invalid,
    /// Explaining what is about to be photographed.
    Intro,
    /// Camera live, waiting for a capture.
    Capturing,
    /// Showing a capture for the applicant to accept or retake.
    Review,
    /// Sending the accepted image.
    Uploading,
    /// Documents in; waiting on the verification result.
    Submitting,
    /// Verification could not run (the Worker's 503 `kyc_unavailable`: xAI out of
    /// credits or down). Not a verdict: the uploads are kept, and "Try again"
    /// submits the same session again without photographing anything.
    Unavailable,
    /// Terminal, success or failure.
    Done,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct Handoff {
    #[serde(default)]
    id_type: String,
    #[serde(default)]
    needs_back: bool,
    #[serde(default)]
    minimum_age: u32,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct SubmitResult {
    #[serde(default)]
    status: String,
    #[serde(default)]
    reason: String,
}

/// Read the `s` query parameter from the current URL.
fn session_from_url() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    params.get("s").filter(|s| !s.trim().is_empty())
}

#[component]
pub fn VerifyPage() -> impl IntoView {
    let stage = RwSignal::new(Stage::Loading);
    let error = RwSignal::new(String::new());
    let advice = RwSignal::new(String::new());

    let session = RwSignal::new(String::new());
    // The session's upload-only token, from the link's fragment. Memory only:
    // never storage, never a log. It cannot register or link an account.
    let upload_token = RwSignal::new(String::new());
    let id_type = RwSignal::new(String::new());
    let needs_back = RwSignal::new(true);
    let minimum_age = RwSignal::new(18u32);

    // Which side is being photographed right now.
    let side = RwSignal::new("front".to_string());
    let front_done = RwSignal::new(false);

    // Preview of the pending capture. Only the data URL is kept in a signal;
    // the Blob is rebuilt from it at upload time so no non-Send JS value has
    // to live in reactive storage.
    let preview = RwSignal::new(String::new());

    // Verification outcome.
    let verified = RwSignal::new(false);
    let result_msg = RwSignal::new(String::new());

    // Guide overlay geometry, recomputed from the live video box.
    let guide = RwSignal::new((0.0f64, 0.0f64, 0.0f64, 0.0f64));

    let video_ref = NodeRef::<leptos::html::Video>::new();
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    // MediaStream is not Send, so it lives in local (non-threaded) storage.
    let stream_store = StoredValue::new_local(None::<MediaStream>);

    // ── Load the handoff context ─────────────────────────────────────────────
    Effect::new(move |_| {
        let Some(s) = session_from_url() else {
            error.set("This verification link is missing its code. Open the link from your registration screen again.".into());
            stage.set(Stage::Invalid);
            return;
        };
        session.set(s.clone());
        // The link carries the upload token in its fragment (#t=), which no server
        // saw. Read it into memory, then take it out of the address bar.
        upload_token.set(upload_token_from_url().unwrap_or_default());
        strip_fragment_from_address_bar();
        // The link's context needs the token (in a header, never the URL).
        let tok = upload_token.get_untracked();

        spawn_local(async move {
            match gloo_net::http::Request::get(&format!("{}/api/kyc/handoff/{}", crate::state::api_base(), s))
                .header("X-KYC-Token", &tok)
                .send()
                .await
            {
                Ok(resp) if resp.status() == 200 => {
                    let h: Handoff = resp.json().await.unwrap_or_default();
                    id_type.set(h.id_type);
                    needs_back.set(h.needs_back);
                    if h.minimum_age > 0 {
                        minimum_age.set(h.minimum_age);
                    }
                    stage.set(Stage::Intro);
                }
                Ok(_) => {
                    error.set("This verification link has expired. Start again from your registration screen.".into());
                    stage.set(Stage::Invalid);
                }
                Err(_) => {
                    error.set("Could not reach the verification service. Check your connection and reload.".into());
                    stage.set(Stage::Invalid);
                }
            }
        });
    });

    // Release the camera if the applicant navigates away mid-flow.
    on_cleanup(move || {
        stream_store.update_value(|s| {
            if let Some(stream) = s.take() {
                stop_camera(&stream);
            }
        });
    });

    // ── Camera control ───────────────────────────────────────────────────────
    // Used both to open the camera the first time and to bring the viewfinder
    // back for the second side of a document. Reviewing a shot unmounts the
    // <video>, so returning here always mounts a fresh element that has to be
    // re-bound to the stream.
    let ensure_camera = move || {
        error.set(String::new());
        advice.set(String::new());
        stage.set(Stage::Capturing);

        spawn_local(async move {
            // Wait for the freshly rendered <video> to exist.
            let mut video = None;
            for _ in 0..40 {
                if let Some(v) = video_ref.get_untracked() {
                    video = Some(v);
                    break;
                }
                gloo_timers::future::TimeoutFuture::new(50).await;
            }
            let Some(video) = video else {
                error.set("Camera view failed to load. Reload the page.".into());
                stage.set(Stage::Intro);
                return;
            };

            // Reuse the open stream when there is one; only ask the OS for the
            // camera again if it went away.
            let existing = stream_store
                .get_value()
                .filter(crate::capture::stream_is_live);

            match existing {
                Some(stream) => crate::capture::attach_stream(&video, &stream).await,
                None => match start_camera(&video).await {
                    Ok(stream) => stream_store.set_value(Some(stream)),
                    Err(e) => {
                        error.set(e);
                        stage.set(Stage::Intro);
                        return;
                    }
                },
            }

            // Recompute the guide until the video reports its real dimensions;
            // metadata can land a beat after play resolves.
            for _ in 0..20 {
                gloo_timers::future::TimeoutFuture::new(120).await;
                if let Some(v) = video_ref.get_untracked() {
                    let (vw, vh) = (v.video_width() as f64, v.video_height() as f64);
                    let (ew, eh) = (v.client_width() as f64, v.client_height() as f64);
                    if vw > 0.0 && ew > 0.0 {
                        guide.set(overlay_rect(vw, vh, ew, eh, aspect_for(&id_type.get_untracked())));
                    }
                }
            }
        });
    };

    let shutdown_camera = move || {
        stream_store.update_value(|s| {
            if let Some(stream) = s.take() {
                stop_camera(&stream);
            }
        });
    };

    // ── Capture ──────────────────────────────────────────────────────────────
    let take_photo = move |_| {
        let (Some(video), Some(canvas)) = (video_ref.get_untracked(), canvas_ref.get_untracked())
        else {
            error.set("Camera view is not ready yet.".into());
            return;
        };

        match capture_document(&video, &canvas, aspect_for(&id_type.get_untracked())) {
            Ok(shot) => {
                if !shot.quality.acceptable {
                    // Keep the camera running so a retake is one tap away.
                    advice.set(shot.quality.advice);
                    return;
                }
                advice.set(String::new());
                preview.set(shot.data_url);
                stage.set(Stage::Review);
            }
            Err(e) => error.set(e),
        }
    };

    // ── Upload + submit ──────────────────────────────────────────────────────
    let accept_photo = move |_| {
        let data_url = preview.get_untracked();
        let sess = session.get_untracked();
        let token = upload_token.get_untracked();
        let this_side = side.get_untracked();
        let idt = id_type.get_untracked();
        let wants_back = needs_back.get_untracked();

        stage.set(Stage::Uploading);

        spawn_local(async move {
            match upload_capture(&data_url, &sess, &token, &this_side, &idt).await {
                Err(e) => {
                    error.set(e);
                    stage.set(Stage::Review);
                }
                Ok(()) => {
                    if this_side == "front" && wants_back {
                        // Straight back to the viewfinder for the second side.
                        front_done.set(true);
                        side.set("back".into());
                        preview.set(String::new());
                        ensure_camera();
                        return;
                    }

                    run_submit(sess, token, wants_back, stage, verified, result_msg).await;
                }
            }
        });
    };

    // "Try again" after verification could not run: the same session, whose
    // uploads the Worker kept, so nothing is photographed or uploaded again.
    let retry_submit = move |_| {
        let sess = session.get_untracked();
        let token = upload_token.get_untracked();
        let wants_back = needs_back.get_untracked();
        spawn_local(run_submit(sess, token, wants_back, stage, verified, result_msg));
    };

    let retake = move |_| {
        preview.set(String::new());
        ensure_camera();
    };

    // Stop the camera as soon as the flow leaves the capture stages.
    Effect::new(move |_| {
        if matches!(stage.get(), Stage::Done | Stage::Invalid | Stage::Unavailable) {
            shutdown_camera();
        }
    });

    view! {
        <div class="page page-verify">
            <div class="verify-shell">
                <header class="verify-header">
                    <span class="verify-brand">"EUSTRESS"</span>
                    <span class="verify-step">
                        {move || match stage.get() {
                            Stage::Capturing | Stage::Review | Stage::Uploading => {
                                if side.get() == "back" { "Back of document" } else { "Front of document" }
                            }
                            _ => "Identity verification",
                        }}
                    </span>
                </header>

                <Show when=move || !error.get().is_empty()>
                    <p class="verify-error" role="alert">{move || error.get()}</p>
                </Show>

                {move || match stage.get() {
                    Stage::Loading => view! {
                        <div class="verify-card verify-center">
                            <div class="verify-spinner" />
                            <p>"Opening your verification session..."</p>
                        </div>
                    }.into_any(),

                    Stage::Invalid => view! {
                        <div class="verify-card verify-center">
                            <div class="verify-icon verify-icon-warn">"!"</div>
                            <h1>"Link unavailable"</h1>
                            <p class="verify-muted">{move || error.get()}</p>
                        </div>
                    }.into_any(),

                    Stage::Intro => view! {
                        <div class="verify-card">
                            <h1>"Verify your identity"</h1>
                            <p class="verify-muted">
                                "You must be at least "
                                <strong>{move || minimum_age.get().to_string()}</strong>
                                " to earn on Eustress. Photograph your "
                                <strong>{move || pretty_id_type(&id_type.get())}</strong>
                                " so we can confirm who you are."
                            </p>

                            <ul class="verify-tips">
                                <li>"Lay the document flat on a dark surface"</li>
                                <li>"Fill the frame and keep all four corners visible"</li>
                                <li>"Avoid glare from overhead lights"</li>
                                <li>{move || if needs_back.get() {
                                    "You will photograph the front, then the back"
                                } else {
                                    "One photo of the photo page is enough"
                                }}</li>
                            </ul>

                            <button class="verify-btn verify-btn-primary" on:click=move |_| ensure_camera()>
                                "Open camera"
                            </button>
                            <p class="verify-privacy">
                                "Your images are encrypted in transit, stored in a restricted vault, and used only to verify your identity."
                            </p>
                        </div>
                    }.into_any(),

                    Stage::Capturing => view! {
                        <div class="verify-camera">
                            <video
                                node_ref=video_ref
                                class="verify-video"
                                muted=true
                                autoplay=true
                                playsinline=true
                            />
                            // Guide is positioned from the live video box so
                            // what is framed is exactly what gets cropped.
                            <div
                                class="verify-guide"
                                style=move || {
                                    let (l, t, w, h) = guide.get();
                                    format!("left:{}px;top:{}px;width:{}px;height:{}px;", l, t, w, h)
                                }
                            >
                                <span class="guide-corner tl" />
                                <span class="guide-corner tr" />
                                <span class="guide-corner bl" />
                                <span class="guide-corner br" />
                            </div>

                            <Show when=move || !advice.get().is_empty()>
                                <p class="verify-advice" role="status">{move || advice.get()}</p>
                            </Show>

                            <div class="verify-camera-bar">
                                <p class="verify-camera-hint">
                                    {move || if side.get() == "back" {
                                        "Now the back of the document"
                                    } else {
                                        "Line up the document inside the frame"
                                    }}
                                </p>
                                <button class="verify-shutter" on:click=take_photo aria-label="Take photo" />
                            </div>
                        </div>
                    }.into_any(),

                    Stage::Review => view! {
                        <div class="verify-card">
                            <h1>"Is this readable?"</h1>
                            <img class="verify-preview" src=move || preview.get() alt="Captured document" />
                            <p class="verify-muted">"Every letter should be sharp and the whole document visible."</p>
                            <div class="verify-actions">
                                <button class="verify-btn verify-btn-ghost" on:click=retake>"Retake"</button>
                                <button class="verify-btn verify-btn-primary" on:click=accept_photo>"Use this photo"</button>
                            </div>
                        </div>
                    }.into_any(),

                    Stage::Uploading => view! {
                        <div class="verify-card verify-center">
                            <div class="verify-spinner" />
                            <p>"Uploading securely..."</p>
                        </div>
                    }.into_any(),

                    Stage::Submitting => view! {
                        <div class="verify-card verify-center">
                            <div class="verify-spinner" />
                            <p>"Checking your document and confirming your age..."</p>
                            <p class="verify-muted">"This usually takes a few seconds."</p>
                        </div>
                    }.into_any(),

                    Stage::Unavailable => view! {
                        <div class="verify-card verify-center">
                            <div class="verify-icon verify-icon-warn">"!"</div>
                            <h1>"Verification is temporarily unavailable"</h1>
                            <p class="verify-muted">{move || result_msg.get()}</p>
                            <div class="verify-actions">
                                <button class="verify-btn verify-btn-primary" on:click=retry_submit>"Try again"</button>
                            </div>
                        </div>
                    }.into_any(),

                    Stage::Done => view! {
                        <div class="verify-card verify-center">
                            <div class=move || if verified.get() {
                                "verify-icon verify-icon-ok"
                            } else {
                                "verify-icon verify-icon-warn"
                            }>
                                {move || if verified.get() { "OK" } else { "!" }}
                            </div>
                            <h1>{move || if verified.get() { "You are verified" } else { "Not verified" }}</h1>
                            <p class="verify-muted">{move || result_msg.get()}</p>
                        </div>
                    }.into_any(),
                }}

                // Offscreen scratch surface for the crop and clean pass.
                <canvas node_ref=canvas_ref class="verify-canvas" />
            </div>

            <Show when=move || matches!(stage.get(), Stage::Invalid | Stage::Done)>
                <Footer />
            </Show>
        </div>
    }
}

/// Turn a raw id_type slug into something readable in a sentence.
fn pretty_id_type(raw: &str) -> String {
    if raw.is_empty() || raw == "unknown" {
        return "government ID".to_string();
    }
    raw.replace('_', " ")
}

/// POST one captured side to the KYC upload endpoint.
async fn upload_capture(
    data_url: &str,
    session_id: &str,
    upload_token: &str,
    side: &str,
    id_type: &str,
) -> Result<(), String> {
    let blob = crate::capture::data_url_to_blob(data_url)?;

    let form = web_sys::FormData::new().map_err(|_| "Could not build the upload.".to_string())?;
    let filename = format!("id-{}.jpg", side);
    form.append_with_blob_and_filename("document", &blob, &filename)
        .map_err(|_| "Could not attach the image.".to_string())?;
    form.append_with_str("side", side).ok();
    form.append_with_str("id_type", id_type).ok();
    form.append_with_str("session_id", session_id).ok();
    form.append_with_str("upload_token", upload_token).ok();

    let resp = gloo_net::http::Request::post(&format!("{}/api/kyc/upload", crate::state::api_base()))
        .body(form)
        .map_err(|_| "Could not prepare the upload.".to_string())?
        .send()
        .await
        .map_err(|_| "Upload failed. Check your connection and try again.".to_string())?;

    if resp.status() == 200 || resp.status() == 201 {
        Ok(())
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(extract_error(&body)
            .unwrap_or_else(|| format!("Upload was rejected ({}).", resp.status())))
    }
}

/// Run verification for the session and return the outcome.
async fn submit_for_verification(session_id: &str, upload_token: &str, needs_back: bool) -> Result<SubmitResult, String> {
    let payload = serde_json::json!({
        "session_id": session_id,
        "upload_token": upload_token,
        "needs_back": needs_back,
    });

    let resp = gloo_net::http::Request::post(&format!("{}/api/kyc/submit", crate::state::api_base()))
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .map_err(|_| "Could not prepare the request.".to_string())?
        .send()
        .await
        .map_err(|_| "Could not reach the verification service.".to_string())?;

    let body = resp.text().await.unwrap_or_default();
    // Verification could not run: the uploads are kept, so this is an outcome the
    // page offers to retry, not an error and not a rejection.
    if let Some(message) = kyc_unavailable_message(resp.status(), &body) {
        return Ok(SubmitResult { status: "unavailable".to_string(), reason: message });
    }
    if resp.status() != 200 {
        return Err(extract_error(&body)
            .unwrap_or_else(|| "Verification could not be completed.".to_string()));
    }

    serde_json::from_str::<SubmitResult>(&body)
        .map_err(|_| "Unexpected response from the verification service.".to_string())
}

/// The upload token in a link's fragment (`#t=<64 lower-case hex>`), or None.
pub(crate) fn token_from_fragment(hash: &str) -> Option<String> {
    hash.trim_start_matches('#')
        .split('&')
        .find_map(|kv| kv.strip_prefix("t="))
        .filter(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .map(str::to_string)
}

/// The upload token of the link this page was opened from.
fn upload_token_from_url() -> Option<String> {
    let hash = web_sys::window()?.location().hash().ok()?;
    token_from_fragment(&hash)
}

/// Takes the fragment (the upload token) out of the address bar, so it is not
/// left in a screenshot, a shared link or the history.
fn strip_fragment_from_address_bar() {
    let Some(window) = web_sys::window() else { return };
    let location = window.location();
    let (Ok(path), Ok(search)) = (location.pathname(), location.search()) else { return };
    if let Ok(history) = window.history() {
        let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&format!("{path}{search}")));
    }
}

/// What the page says when the Worker sent no sentence of its own.
pub(crate) const KYC_UNAVAILABLE_FALLBACK: &str =
    "ID verification is temporarily unavailable. Your upload is saved; try again in a few minutes.";

/// The Worker's sentence when verification could not run, or None for any other
/// answer. Only a 503 with code `kyc_unavailable` is this: the uploads are kept
/// and the try was not counted, so the page offers "Try again" with the same
/// session. A rejection, a rate limit or any other error is not.
pub(crate) fn kyc_unavailable_message(status: u16, body: &str) -> Option<String> {
    if status != 503 {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    if v.get("code").and_then(|c| c.as_str()) != Some("kyc_unavailable") {
        return None;
    }
    let said = v.get("error").and_then(|e| e.as_str()).map(str::trim).unwrap_or("");
    Some(if said.is_empty() { KYC_UNAVAILABLE_FALLBACK } else { said }.to_string())
}

/// Submits the session and moves the page to the outcome: Done (verified or not),
/// or Unavailable when verification could not run. "Try again" calls this again
/// with the same session; it never uploads.
async fn run_submit(
    sess: String,
    token: String,
    wants_back: bool,
    stage: RwSignal<Stage>,
    verified: RwSignal<bool>,
    result_msg: RwSignal<String>,
) {
    stage.set(Stage::Submitting);
    match submit_for_verification(&sess, &token, wants_back).await {
        Ok(res) if res.status == "unavailable" => {
            verified.set(false);
            result_msg.set(res.reason);
            stage.set(Stage::Unavailable);
            return;
        }
        Ok(res) => {
            verified.set(res.status == "verified");
            result_msg.set(if res.status == "verified" {
                "Your identity has been verified. You can close this page and continue on your computer.".to_string()
            } else if res.reason.is_empty() {
                "We could not verify your documents. Please start again.".to_string()
            } else {
                res.reason
            });
        }
        Err(e) => {
            verified.set(false);
            result_msg.set(e);
        }
    }
    stage.set(Stage::Done);
}

/// Pull a human-readable message out of an API error body.
fn extract_error(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("reason")
        .or_else(|| v.get("error"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod kyc_unavailable_tests {
    use super::*;

    const WORKER_503: &str = r#"{"status":"unavailable","code":"kyc_unavailable","error":"ID verification is temporarily unavailable. Your upload is saved; try again in a few minutes."}"#;

    #[test]
    fn the_workers_503_is_an_outage_and_its_own_sentence_is_shown() {
        assert_eq!(
            kyc_unavailable_message(503, WORKER_503).as_deref(),
            Some("ID verification is temporarily unavailable. Your upload is saved; try again in a few minutes."),
        );
        assert_eq!(
            kyc_unavailable_message(503, r#"{"code":"kyc_unavailable","error":"Back in a minute."}"#).as_deref(),
            Some("Back in a minute."),
            "the Worker's words, not the page's",
        );
    }

    #[test]
    fn a_503_with_no_sentence_still_offers_a_retry() {
        assert_eq!(kyc_unavailable_message(503, r#"{"code":"kyc_unavailable","error":"  "}"#).as_deref(), Some(KYC_UNAVAILABLE_FALLBACK));
        assert_eq!(kyc_unavailable_message(503, r#"{"code":"kyc_unavailable"}"#).as_deref(), Some(KYC_UNAVAILABLE_FALLBACK));
    }

    #[test]
    fn a_verdict_or_any_other_error_is_not_an_outage() {
        for (status, body) in [
            (200, r#"{"status":"rejected","reason":"This is a photo of a screen"}"#),
            (200, r#"{"status":"verified"}"#),
            (429, r#"{"error":"Too many verification attempts.","code":"rate_limited"}"#),
            (429, r#"{"code":"kyc_unavailable","error":"x"}"#),
            (503, r#"{"error":"Sign-in is temporarily unavailable.","code":"account_unavailable"}"#),
            (500, r#"{"error":"Submit failed: boom"}"#),
            (503, "Service Unavailable"),
        ] {
            assert_eq!(kyc_unavailable_message(status, body), None, "{status} {body}");
        }
    }

    #[test]
    fn try_again_submits_the_same_session_and_uploads_nothing() {
        let src = include_str!("verify.rs").replace("\r\n", "\n");
        let run = src.split("async fn run_submit(").nth(1).unwrap().split("\n}\n").next().unwrap();
        assert!(run.contains("submit_for_verification(&sess, &token, wants_back)"));
        assert!(run.contains("Stage::Unavailable"));
        assert!(!run.contains("upload_capture"));
        let retry = src.split("let retry_submit = move |_| {").nth(1).unwrap().split("\n    };\n").next().unwrap();
        assert!(retry.contains("session.get_untracked()") && retry.contains("run_submit("));
        assert!(!retry.contains("upload_capture") && !retry.contains("ensure_camera"));
    }

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn the_upload_token_is_read_from_the_fragment_only_in_its_own_shape() {
        assert_eq!(token_from_fragment(&format!("#t={TOKEN}")).as_deref(), Some(TOKEN));
        assert_eq!(token_from_fragment(&format!("t={TOKEN}")).as_deref(), Some(TOKEN));
        assert_eq!(token_from_fragment(&format!("#x=1&t={TOKEN}")).as_deref(), Some(TOKEN));
        for bad in ["", "#", "#t=", "#t=abc", &format!("#t={}", TOKEN.to_uppercase()), &format!("#t={TOKEN}0"), &format!("#s={TOKEN}"), "#t=zz23456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"] {
            assert_eq!(token_from_fragment(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_phone_page_never_holds_the_claim_secret_and_keeps_its_token_in_memory() {
        let src = include_str!("verify.rs").replace("\r\n", "\n");
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(!code.contains("claim_secret") && !code.contains("kyc_claim"), "the phone page has no claim secret");
        assert!(!code.contains("Storage"), "nothing goes to storage");
        let handoff = code.split("/api/kyc/handoff/").nth(1).unwrap().split(".send()").next().unwrap();
        assert!(handoff.contains(".header(\"X-KYC-Token\", &tok)"), "the hand-off context is read with the token in a header");
        for line in code.lines().filter(|l| l.contains("token")) {
            assert!(!line.contains("log::") && !line.contains("console"), "a token is logged: {line}");
        }
    }
}
