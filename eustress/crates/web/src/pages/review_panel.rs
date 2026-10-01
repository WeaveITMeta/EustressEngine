// =============================================================================
// Eustress Web - Review Panel
// =============================================================================
// What review decided about one of the signed-in author's projects, and the
// appeal. Every string in the answer is already written for authors by the
// Worker (moderation.mjs authorView), so the panel shows them as they come and
// adds no wording of its own about why a decision was made.
//
// Table of Contents:
// 1. Types
// 2. Helpers
// 3. Panel Component
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use crate::api::{ApiClient, ApiError};

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// `GET /api/simulations/{id}/moderation`, the author's view.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ReviewView {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub headline: String,
    #[serde(default)]
    pub reasons: Vec<ReviewReason>,
    #[serde(default)]
    pub suggested_edit: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub rating: Option<String>,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub can_appeal: bool,
    #[serde(default)]
    pub appeal: Option<ReviewAppeal>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ReviewReason {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub what_to_change: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ReviewAppeal {
    /// `in_review`, `accepted` or `upheld`.
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Load {
    Loading,
    Failed(String),
    Ready(ReviewView),
}

#[derive(Clone, Debug, PartialEq)]
enum Sending {
    Idle,
    Sending,
    Sent,
    Failed(String),
}

// -----------------------------------------------------------------------------
// 2. Helpers
// -----------------------------------------------------------------------------

/// The shortest and longest appeal the Worker accepts, in characters.
const APPEAL_MIN: usize = 10;
const APPEAL_MAX: usize = 2000;

fn valid_appeal(text: &str) -> bool {
    let n = text.trim().chars().count();
    (APPEAL_MIN..=APPEAL_MAX).contains(&n)
}

/// The label for an author status. Held and quarantined cases both arrive as
/// `in_review` and read the same.
fn status_label(status: &str) -> &'static str {
    match status {
        "listed" => "Listed",
        "in_review" => "In review",
        "approved_private" => "Private",
        "not_listed" => "Not listed",
        "changes_requested" => "Changes requested",
        "appeal_in_review" => "Appeal in review",
        "unreviewed" => "Not reviewed yet",
        _ => "Status unknown",
    }
}

fn rating_label(rating: &str) -> &'static str {
    match rating {
        "all_ages" => "Everyone",
        "teen_13" => "Teen (13+)",
        "mature_17" => "Mature (17+)",
        "adult_18" => "Adults (18+)",
        _ => "Not rated",
    }
}

fn appeal_label(status: &str) -> &'static str {
    match status {
        "in_review" => "Your appeal is with a reviewer.",
        "accepted" => "Your appeal was accepted.",
        "upheld" => "The decision was upheld after your appeal.",
        _ => "Appeal status unknown.",
    }
}

/// A sentence for a failed call. A Worker error body is `{"error": "..."}`.
fn error_message(error: &ApiError) -> String {
    match error {
        ApiError::Unauthorized => "Sign in again to see this.".to_string(),
        ApiError::NotFound => "This project could not be found.".to_string(),
        ApiError::Server { status, message } => {
            let from_body = serde_json::from_str::<serde_json::Value>(message)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string));
            match (from_body, status) {
                (Some(text), _) => text,
                (None, 429) => "Too many requests. Try again in a minute.".to_string(),
                (None, _) => format!("The server answered {status}. Try again."),
            }
        }
        ApiError::Network(_) => "Could not reach the server. Check your connection.".to_string(),
        ApiError::Deserialize(_) => "The server's answer was not readable.".to_string(),
    }
}

// -----------------------------------------------------------------------------
// 3. Panel Component
// -----------------------------------------------------------------------------

/// The review of project `sim_id`, as a dialog. Opening it asks the Worker, so
/// the panel is always as current as the last decision.
#[component]
pub fn ReviewPanel(
    sim_id: String,
    api_url: String,
    on_close: impl Fn() + 'static + Clone,
) -> impl IntoView {
    let load = RwSignal::new(Load::Loading);
    let appeal_text = RwSignal::new(String::new());
    let sending = RwSignal::new(Sending::Idle);

    let fetch = {
        let api_url = api_url.clone();
        let sim_id = sim_id.clone();
        move || {
            let api_url = api_url.clone();
            let sim_id = sim_id.clone();
            spawn_local(async move {
                let client = ApiClient::new(&api_url);
                match client.get::<ReviewView>(&format!("/api/simulations/{}/moderation", sim_id)).await {
                    Ok(view) => load.set(Load::Ready(view)),
                    Err(e) => load.set(Load::Failed(error_message(&e))),
                }
            });
        }
    };
    let first_fetch = fetch.clone();
    Effect::new(move |_| first_fetch());

    let submit = {
        let api_url = api_url.clone();
        let sim_id = sim_id.clone();
        let refetch = fetch.clone();
        move |_| {
            let text = appeal_text.get_untracked().trim().to_string();
            if !valid_appeal(&text) || sending.get_untracked() == Sending::Sending {
                return;
            }
            sending.set(Sending::Sending);
            let api_url = api_url.clone();
            let sim_id = sim_id.clone();
            let refetch = refetch.clone();
            spawn_local(async move {
                let client = ApiClient::new(&api_url);
                let body = serde_json::json!({ "text": text });
                let sent = client
                    .post::<serde_json::Value, _>(&format!("/api/simulations/{}/appeal", sim_id), &body)
                    .await;
                match sent {
                    // A 2xx whose body is not JSON still reached the Worker;
                    // the refetch below shows what it recorded.
                    Ok(_) | Err(ApiError::Deserialize(_)) => {
                        sending.set(Sending::Sent);
                        appeal_text.set(String::new());
                        refetch();
                    }
                    Err(e) => sending.set(Sending::Failed(error_message(&e))),
                }
            });
        }
    };

    let on_close_bg = on_close.clone();
    let on_close_btn = on_close.clone();

    view! {
        <div class="modal-overlay review-overlay" on:click=move |_| on_close_bg()>
            <div
                class="review-panel"
                role="dialog"
                aria-modal="true"
                aria-label="Review details"
                on:click=|e| e.stop_propagation()
            >
                <button class="modal-close" aria-label="Close" on:click=move |_| on_close_btn()>
                    "×"
                </button>
                {move || match load.get() {
                    Load::Loading => view! {
                        <div class="loading-state" aria-busy="true">
                            <div class="spinner"></div>
                            <p class="loading-message">"Loading review..."</p>
                        </div>
                    }
                    .into_any(),
                    Load::Failed(message) => view! {
                        <div class="review-body">
                            <h2 class="review-headline">"This review could not be loaded"</h2>
                            <p class="review-note">{message}</p>
                        </div>
                    }
                    .into_any(),
                    Load::Ready(v) => {
                        let submit = submit.clone();
                        let status_class = format!("review-status {}", v.status);
                        let can_appeal = v.can_appeal;
                        view! {
                            <div class="review-body">
                                <span class=status_class>{status_label(&v.status)}</span>
                                <h2 class="review-headline">{v.headline.clone()}</h2>
                                {v.rating.clone().map(|r| view! {
                                    <p class="review-rating">"Rated " <strong>{rating_label(&r)}</strong></p>
                                })}
                                {v.featured.then(|| view! {
                                    <p class="review-rating">"Featured in the Gallery"</p>
                                })}
                                {(!v.reasons.is_empty()).then(|| view! {
                                    <div class="review-reasons">
                                        {v.reasons.iter().map(|r| view! {
                                            <div class="review-reason">
                                                <h3>{r.title.clone()}</h3>
                                                {(!r.why.is_empty()).then(|| view! {
                                                    <p><span class="review-label">"Why"</span>{r.why.clone()}</p>
                                                })}
                                                {(!r.what_to_change.is_empty()).then(|| view! {
                                                    <p><span class="review-label">"What to change"</span>{r.what_to_change.clone()}</p>
                                                })}
                                            </div>
                                        }).collect::<Vec<_>>()}
                                    </div>
                                })}
                                {v.suggested_edit.clone().map(|text| view! {
                                    <div class="review-suggestion">
                                        <span class="review-label">"Suggested edit"</span>
                                        <p>{text}</p>
                                    </div>
                                })}
                                {v.note.clone().map(|text| view! {
                                    <div class="review-suggestion">
                                        <span class="review-label">"A note from the reviewer"</span>
                                        <p>{text}</p>
                                    </div>
                                })}
                                {v.appeal.clone().map(|a| view! {
                                    <div class="review-appeal-status">
                                        <p>{appeal_label(&a.status)}</p>
                                        {a.note.clone().map(|n| view! { <p class="review-note">{n}</p> })}
                                    </div>
                                })}
                                {can_appeal.then(|| view! {
                                    <div class="review-appeal">
                                        <h3>"Appeal this decision"</h3>
                                        <p class="review-hint">"Tell the reviewer what we got wrong, or what you changed."</p>
                                        <textarea
                                            class="review-textarea"
                                            rows="5"
                                            maxlength="2000"
                                            placeholder="Your appeal, 10 to 2000 characters"
                                            prop:value=move || appeal_text.get()
                                            on:input=move |e| appeal_text.set(event_target_value(&e))
                                        ></textarea>
                                        <div class="review-appeal-foot">
                                            <span class="review-count">
                                                {move || format!("{} / {}", appeal_text.get().chars().count(), APPEAL_MAX)}
                                            </span>
                                            <button
                                                class="review-send"
                                                disabled=move || !valid_appeal(&appeal_text.get()) || sending.get() == Sending::Sending
                                                on:click=submit
                                            >
                                                "Send appeal"
                                            </button>
                                        </div>
                                        {move || match sending.get() {
                                            Sending::Failed(message) => view! { <p class="review-error">{message}</p> }.into_any(),
                                            Sending::Sent => view! {
                                                <p class="review-sent">"Appeal sent. This page shows its status."</p>
                                            }.into_any(),
                                            _ => view! { <span></span> }.into_any(),
                                        }}
                                    </div>
                                })}
                            </div>
                        }
                        .into_any()
                    }
                }}
            </div>
        </div>
    }
}
