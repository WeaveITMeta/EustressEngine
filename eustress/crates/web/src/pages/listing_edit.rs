// =============================================================================
// Eustress Web - The author edits a published listing from its page (/simulation/:id)
// =============================================================================
// The author of a listing sees "Edit listing" on its page. The dialog changes the title, the description, who can see it, and
// the tags (the tag editor from /projects, project_tags.rs). The routes, and what each answer means, are in the two functions
// marked CONTRACT, so a change to a route changes those and nothing else:
//
//   PATCH /api/simulations/{id}/listing   the title, description and visibility (docs/networking/LISTING_EDIT.md)
//   POST  /api/simulations/{id}/submit    asks for the review, the same request Studio makes after a publish
//   PATCH /api/simulations/{id}/tags      the tags (project_tags.rs)
//
// A new title or description is text the review reads: the listing leaves the Gallery until the review passes it, and the
// dialog asks for the review at once. Visibility is not reviewed. The Worker refuses an edit while a review is running or a
// hold is on the listing (409 in_review), and the dialog says so in the words the tag editor uses.
//
// Table of Contents:
// 1. The routes (CONTRACT) and their answers
// 2. Wording and the draft's rules (pure, tested)
// 3. The dialog
// 4. Tests
// =============================================================================

use leptos::prelude::*;
use serde::Deserialize;

use crate::api::catalog::Listing;
use crate::api::{self, ApiClient, ApiError};
use crate::state::AppState;
use crate::utils::after_event;
use super::project_tags::TagEditor;

// -----------------------------------------------------------------------------
// 1. The routes and their answers
// -----------------------------------------------------------------------------

/// The longest title and description the Worker takes (src/listing_edit.mjs LISTING_EDIT_LIMITS), counted as it counts them.
pub const TITLE_MAX: usize = 120;
pub const DESCRIPTION_MAX: usize = 4000;

/// A request to the Worker: the method, the path and the JSON body.
#[derive(Clone, Debug, PartialEq)]
pub struct EditRequest {
    pub method: &'static str,
    pub path: String,
    pub body: serde_json::Value,
}

// CONTRACT: listing edit route (src/listing_edit.mjs, docs/networking/LISTING_EDIT.md)
/// What saves the title, description and visibility: `PATCH /api/simulations/{id}/listing`, signed in as the author. Answers:
/// 200 with the listing as stored, `changed` (the fields that changed), `review` (true when the change sends it back to
/// review) and `status`; 400 with the Worker's sentence; 401 signed out; 404 not the author's; 409 `in_review`.
pub fn listing_edit_request(id: &str, name: &str, description: &str, is_public: bool) -> EditRequest {
    EditRequest {
        method: "PATCH",
        path: format!("/api/simulations/{}/listing", urlencoding::encode(id)),
        body: serde_json::json!({ "name": name, "description": description, "is_public": is_public }),
    }
}

// CONTRACT: review submit route (src/moderation.mjs, the route Studio calls after a publish)
/// What asks for the review: `POST /api/simulations/{id}/submit` with no body. Answers 200 with the decided `status` (the
/// review ran while the request waited), 202 `in_review` (it carries on and the author hears when it is decided), 200
/// without a run when there is nothing new to review, and 429 when the listing has been reviewed as often as it may be today
/// or a review was asked for a moment ago.
pub fn review_request(id: &str) -> EditRequest {
    EditRequest {
        method: "POST",
        path: format!("/api/simulations/{}/submit", urlencoding::encode(id)),
        body: serde_json::json!({}),
    }
}

/// The Worker's 200 to an edit.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct EditReply {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub changed: Vec<String>,
    #[serde(default)]
    pub review: bool,
    #[serde(default)]
    pub status: String,
}

/// The Worker's answer to a review request.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ReviewReply {
    #[serde(default)]
    pub status: String,
}

async fn send<T: serde::de::DeserializeOwned>(client: &ApiClient, request: &EditRequest) -> Result<T, ApiError> {
    match request.method {
        "POST" => client.post::<T, _>(&request.path, &request.body).await,
        _ => client.patch::<T, _>(&request.path, &request.body).await,
    }
}

// -----------------------------------------------------------------------------
// 2. Wording and the draft's rules
// -----------------------------------------------------------------------------

/// Length as the Worker measures it: the trimmed text, in UTF-16 code units (JavaScript's `length`).
pub fn worker_length(text: &str) -> usize {
    text.trim().encode_utf16().count()
}

/// Why the draft cannot be saved, or nothing. The Worker applies the same rules; this only says so before the request.
pub fn draft_problem(name: &str, description: &str) -> Option<String> {
    let title = worker_length(name);
    let about = worker_length(description);
    if title == 0 {
        Some("A listing needs a title.".to_string())
    } else if title > TITLE_MAX {
        Some(format!("The title is {title} characters; the most is {TITLE_MAX}."))
    } else if about > DESCRIPTION_MAX {
        Some(format!("The description is {about} characters; the most is {DESCRIPTION_MAX}."))
    } else {
        None
    }
}

/// The counter under a field: "12 of 120".
pub fn count_words(text: &str, max: usize) -> String {
    format!("{} of {max}", worker_length(text))
}

/// Shown while a review is running or a hold is on the listing (the Worker answers 409 `in_review`).
pub const IN_REVIEW_WORDS: &str = "This listing is being reviewed. You can edit it once the review is done.";

/// The line beside Save: what a new title or description does.
pub const REVIEW_NOTE: &str = "A new title or description goes back to review: your listing leaves the Gallery until the review passes the new text. Who can see it is not reviewed.";

/// The line about Studio, which keeps its own copy of the title and description.
pub const STUDIO_NOTE: &str = "Studio keeps its own copy of the title and description. A publish from Studio that still has the old ones keeps these; new text you write in Studio replaces them.";

/// Shown while an appeal waits for a person: a new title or description is reviewed instead, and the appeal is withdrawn.
pub const APPEAL_NOTE: &str = "Your appeal is with a reviewer. Saving a new title or description withdraws it, and the new version is reviewed instead. Changing only who can see it keeps the appeal.";

/// The appeal line for this listing, when an appeal is pending.
pub fn appeal_note(listing: &Listing) -> Option<&'static str> {
    (crate::components::play_chooser::status_of(listing) == crate::components::play_chooser::Status::AppealInReview).then_some(APPEAL_NOTE)
}

/// Why a save failed, in words.
pub fn save_error_words(e: &ApiError) -> String {
    match e {
        ApiError::Server { status: 400, message } => serde_json::from_str::<serde_json::Value>(message)
            .ok()
            .and_then(|v| v.get("error").and_then(|m| m.as_str()).map(str::to_string))
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| format!("Nothing was saved: {}", api::failure_sentence(e))),
        ApiError::Server { status: 409, .. } => IN_REVIEW_WORDS.to_string(),
        ApiError::Unauthorized => "Your sign-in has ended. Sign in again to edit this listing.".to_string(),
        ApiError::NotFound => "This Space was not found, or it is not yours.".to_string(),
        other => format!("Nothing was saved: {}", api::failure_sentence(other)),
    }
}

/// What the dialog says once the Worker stored the edit, and, when the edit went back to review, what the review said.
pub fn saved_words(reply: &EditReply, review: Option<&Result<ReviewReply, ApiError>>) -> String {
    if reply.changed.is_empty() {
        return "Nothing changed. Your listing stays as it was.".to_string();
    }
    if !reply.review {
        return if reply.archived {
            "Saved. It stays private while it is archived; Unarchive uses this choice.".to_string()
        } else {
            match reply.status.as_str() {
                "listed" => "Saved. It is public and listed in the Gallery.".to_string(),
                "approved_private" => "Saved. It is private: only you can open it.".to_string(),
                _ if reply.is_public => "Saved. It will be public once it passes review.".to_string(),
                _ => "Saved. It is private: only you can open it.".to_string(),
            }
        };
    }
    match review {
        Some(Ok(r)) => match r.status.as_str() {
            "listed" => "Saved and reviewed. Your listing is in the Gallery with the new text.".to_string(),
            "approved_private" => "Saved and reviewed. It passed, and stays private.".to_string(),
            "changes_requested" => "Saved. The review asks for changes; open the review on your Projects page to see them.".to_string(),
            "not_listed" => "Saved. The review did not list it; open the review on your Projects page to see why.".to_string(),
            _ => "Saved. Your listing is in review, and you will hear from us when it is decided.".to_string(),
        },
        Some(Err(ApiError::Server { status: 429, message })) if message.contains("review_limit") => {
            "Saved. This listing has been reviewed as many times as it can be today; it is reviewed again tomorrow.".to_string()
        }
        _ => "Saved. Your listing is waiting for review, which starts on its own shortly.".to_string(),
    }
}

// -----------------------------------------------------------------------------
// 3. The dialog
// -----------------------------------------------------------------------------

/// Reload the page, so it shows the listing as the Worker now holds it.
fn reload_page() {
    if let Some(window) = web_sys::window() {
        let _ = window.location().reload();
    }
}

/// Ask for the review after a change it reads. Never fails the save: a review that does not start now starts on its own.
async fn ask_for_review(client: &ApiClient, id: &str) -> Result<ReviewReply, ApiError> {
    send::<ReviewReply>(client, &review_request(id)).await
}

/// The "Edit listing" dialog. `on_close` runs when it closes without a change; once something was saved, closing reloads the
/// page instead, so it shows what the Worker stored.
#[component]
pub fn ListingEditDialog(listing: Listing, on_close: Callback<()>) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let id = StoredValue::new(listing.id.clone());
    let place_name = if listing.name.trim().is_empty() { "this Space".to_string() } else { listing.name.clone() };

    let name = RwSignal::new(listing.name.clone());
    let description = RwSignal::new(listing.description.clone());
    let is_public = RwSignal::new(listing.is_public_now());
    let saved = StoredValue::new((listing.name.trim().to_string(), listing.description.trim().to_string(), listing.is_public_now()));
    let appeal = appeal_note(&listing);
    let saving = RwSignal::new(false);
    let problem = RwSignal::new(None::<String>);
    let message = RwSignal::new(None::<String>);
    // Something was stored: closing reloads the page.
    let stored = RwSignal::new(false);

    let changed = move || {
        let (n, d, p) = saved.get_value();
        name.get().trim() != n || description.get().trim() != d || is_public.get() != p
    };
    let close = move || {
        if stored.get_untracked() {
            reload_page();
        } else {
            on_close.run(());
        }
    };

    let save = move |_| {
        if saving.get_untracked() {
            return;
        }
        let (n, d, p) = (name.get_untracked(), description.get_untracked(), is_public.get_untracked());
        if let Some(why) = draft_problem(&n, &d) {
            problem.set(Some(why));
            return;
        }
        saving.set(true);
        problem.set(None);
        message.set(None);
        let url = api_url.get_value();
        let id = id.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&url);
            match send::<EditReply>(&client, &listing_edit_request(&id, &n, &d, p)).await {
                Ok(reply) => {
                    if !reply.changed.is_empty() {
                        stored.set(true);
                        saved.set_value((reply.name.trim().to_string(), reply.description.trim().to_string(), reply.is_public));
                    }
                    let review = if reply.review { Some(ask_for_review(&client, &id).await) } else { None };
                    message.set(Some(saved_words(&reply, review.as_ref())));
                    saving.set(false);
                }
                Err(e) => {
                    problem.set(Some(save_error_words(&e)));
                    saving.set(false);
                }
            }
        });
    };

    // The tag editor saves on its own route; a changed set goes back to review, so the review is asked for here too. A request
    // with nothing new to review starts nothing.
    let after_tags = Callback::new(move |line: String| {
        stored.set(true);
        message.set(Some(line));
        let url = api_url.get_value();
        let id = id.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&url);
            let _ = ask_for_review(&client, &id).await;
        });
    });

    view! {
        <div class="modal-overlay" on:click=move |_| after_event(close)>
            <div class="modal-content settings-dialog listing-edit" role="dialog" aria-modal="true" aria-labelledby="listing-edit-title" on:click=|e| e.stop_propagation()>
                <h3 id="listing-edit-title">"Edit listing"</h3>

                <label class="listing-edit-label" for="listing-edit-name">"Title"</label>
                <input
                    id="listing-edit-name"
                    class="listing-edit-input"
                    type="text"
                    maxlength=TITLE_MAX.to_string()
                    prop:value=move || name.get()
                    on:input=move |e| { name.set(event_target_value(&e)); problem.set(None); }
                />
                <p class="listing-edit-count">{move || count_words(&name.get(), TITLE_MAX)}</p>

                <label class="listing-edit-label" for="listing-edit-description">"Description"</label>
                <textarea
                    id="listing-edit-description"
                    class="listing-edit-textarea"
                    rows="10"
                    prop:value=move || description.get()
                    on:input=move |e| { description.set(event_target_value(&e)); problem.set(None); }
                ></textarea>
                <p class="listing-edit-count">{move || count_words(&description.get(), DESCRIPTION_MAX)}</p>

                <fieldset class="listing-edit-visibility">
                    <legend class="listing-edit-label">"Who can see it"</legend>
                    <label>
                        <input type="radio" name="listing-edit-visibility" prop:checked=move || is_public.get() on:change=move |_| is_public.set(true) />
                        " Public: listed in the Gallery once it passes review"
                    </label>
                    <label>
                        <input type="radio" name="listing-edit-visibility" prop:checked=move || !is_public.get() on:change=move |_| is_public.set(false) />
                        " Private: only you can open it"
                    </label>
                </fieldset>

                {appeal.map(|line| view! { <p class="listing-edit-problem" role="note">{line}</p> })}
                <p class="listing-edit-note">{REVIEW_NOTE}</p>
                <p class="listing-edit-note">{STUDIO_NOTE}</p>
                {move || problem.get().map(|why| view! { <p class="listing-edit-problem" role="alert">{why}</p> })}
                {move || message.get().map(|words| view! { <p class="listing-edit-saved" role="status">{words}</p> })}

                <div class="modal-actions">
                    <button type="button" class="btn btn-secondary" on:click=move |_| after_event(close)>
                        {move || if stored.get() { "Done" } else { "Cancel" }}
                    </button>
                    <button type="button" class="btn btn-primary" disabled=move || saving.get() || !changed() on:click=save>
                        {move || if saving.get() { "Saving and reviewing..." } else { "Save" }}
                    </button>
                </div>

                <h4>"Tags"</h4>
                <TagEditor
                    place_id=listing.id.clone()
                    place_name=place_name
                    tags=listing.tags.clone()
                    tags_source=listing.tags_source.clone()
                    locked=false
                    on_saved=after_tags
                />
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 4. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_routes_are_the_workers_and_carry_every_field() {
        let r = listing_edit_request("07d0", "The Justice Super Station", "A courtroom.", false);
        assert_eq!((r.method, r.path.as_str()), ("PATCH", "/api/simulations/07d0/listing"));
        assert_eq!(r.body, serde_json::json!({ "name": "The Justice Super Station", "description": "A courtroom.", "is_public": false }));
        let s = review_request("07d0");
        assert_eq!((s.method, s.path.as_str()), ("POST", "/api/simulations/07d0/submit"));
        assert_eq!(listing_edit_request("a/b", "x", "", true).path, "/api/simulations/a%2Fb/listing", "an id is never a path");
    }

    #[test]
    fn the_draft_follows_the_workers_limits_counted_the_way_it_counts() {
        assert_eq!(draft_problem("  ", ""), Some("A listing needs a title.".to_string()));
        assert_eq!(draft_problem(&"x".repeat(TITLE_MAX), &"y".repeat(DESCRIPTION_MAX)), None);
        assert!(draft_problem(&"x".repeat(TITLE_MAX + 1), "").unwrap().contains("121 characters; the most is 120"));
        assert!(draft_problem("ok", &"y".repeat(DESCRIPTION_MAX + 1)).unwrap().contains("4001 characters"));
        // An emoji is two UTF-16 units, as JavaScript counts it.
        assert_eq!(worker_length(" \u{1F680} "), 2);
        assert_eq!(count_words("Justice ", TITLE_MAX), "7 of 120");
    }

    #[test]
    fn the_saved_line_says_what_happened_and_what_the_review_said() {
        let none = EditReply::default();
        assert_eq!(saved_words(&none, None), "Nothing changed. Your listing stays as it was.");
        let hidden = EditReply { changed: vec!["is_public".into()], status: "approved_private".into(), ..Default::default() };
        assert!(saved_words(&hidden, None).contains("private"));
        let shown = EditReply { changed: vec!["is_public".into()], status: "listed".into(), is_public: true, ..Default::default() };
        assert!(saved_words(&shown, None).contains("listed in the Gallery"));
        let archived = EditReply { changed: vec!["is_public".into()], archived: true, ..Default::default() };
        assert!(saved_words(&archived, None).contains("archived"));
        let text = EditReply { changed: vec!["description".into()], review: true, ..Default::default() };
        let listed = Ok(ReviewReply { status: "listed".into() });
        assert!(saved_words(&text, Some(&listed)).contains("in the Gallery with the new text"));
        let asks = Ok(ReviewReply { status: "changes_requested".into() });
        assert!(saved_words(&text, Some(&asks)).contains("asks for changes"));
        let running = Ok(ReviewReply { status: "in_review".into() });
        assert!(saved_words(&text, Some(&running)).contains("you will hear from us"));
        let limit = Err(ApiError::Server { status: 429, message: r#"{"code":"review_limit"}"#.into() });
        assert!(saved_words(&text, Some(&limit)).contains("tomorrow"));
        let offline = Err(ApiError::Network("down".into()));
        assert!(saved_words(&text, Some(&offline)).contains("starts on its own"));
    }

    #[test]
    fn a_pending_appeal_is_named_before_saving_and_no_other_listing_shows_it() {
        use crate::pages::review_panel::ReviewView;
        let with = |status: &str| Listing { listable: Some(false), moderation: Some(ReviewView { status: status.to_string(), ..Default::default() }), ..Default::default() };
        assert_eq!(appeal_note(&with("appeal_in_review")), Some(APPEAL_NOTE));
        for other in ["in_review", "not_listed", "changes_requested", "approved_private"] {
            assert_eq!(appeal_note(&with(other)), None, "{other}");
        }
        assert_eq!(appeal_note(&Listing::default()), None, "a listed listing has no appeal");
        assert!(APPEAL_NOTE.contains("withdraws"));
    }

    #[test]
    fn a_refusal_is_said_in_words_and_a_review_in_progress_reads_like_a_hold() {
        let bad = ApiError::Server { status: 400, message: r#"{"error":"The title is 130 characters; the most is 120.","code":"too_long"}"#.into() };
        assert_eq!(save_error_words(&bad), "The title is 130 characters; the most is 120.");
        assert_eq!(save_error_words(&ApiError::Server { status: 409, message: "{}".into() }), IN_REVIEW_WORDS);
        assert!(save_error_words(&ApiError::Unauthorized).contains("Sign in again"));
        assert!(save_error_words(&ApiError::NotFound).contains("not yours"));
    }
}
