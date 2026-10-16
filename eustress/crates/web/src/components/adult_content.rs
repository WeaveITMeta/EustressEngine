// =============================================================================
// Eustress Web - Adult content: the setting, and what a listing says when it is off
// =============================================================================
// Listings rated 18+ (adult_18) are filtered out unless the viewer turns on Adult content in their Profile (McKale, 2026-10-08:
// "Adult content needs to be NSFW filtered off by default and be accessible only through a setting in profile"). The Worker
// decides what each read returns; this file is the switch and the words:
//
//   GET  /api/account/adult-content            { enabled, can_enable, code?, reason? }   signed in
//   POST /api/account/adult-content { enabled } turning it on needs identity and age verification; a refusal is 403 with a code
//                                               (kyc_required, kyc_incomplete or the age check's own) and a reason; turning it
//                                               off always succeeds
//   a listing read for a viewer with it off      403 { code: "adult_content_off", signed_in }
//
// Every Gallery read already goes through ApiClient, which sends the viewer's session, so a viewer who turned it on sees adult
// listings; nothing here caches a viewer's answer.
//
// Table of Contents:
// 1. The words and the Worker's answers
// 2. The setting
// 3. The notice on an adult listing
// 4. Tests
// =============================================================================

use leptos::prelude::*;
use serde::Deserialize;

use crate::api::ApiError;

// -----------------------------------------------------------------------------
// 1. The words and the Worker's answers
// -----------------------------------------------------------------------------

pub const LABEL: &str = "Adult content";
pub const HELP: &str = "Shows listings rated Adults only. 18+ listings have sexual content. 21+ listings show alcohol, tobacco or psychedelic use, and need a verified age of 21 or over. Off by default. You can turn it on after identity and age verification.";
/// Shown under the switch when it is on and the account's verified age is under 21 (or unknown).
pub const UNDER_21_NOTE: &str = "21+ listings stay hidden: your verified age is under 21, or we do not have a verified date of birth. They open on your 21st birthday. Contact support@eustress.dev if you are 21 or over.";
/// A 21+ listing refused to a viewer whose setting is on but who is under 21, when the Worker sends no words of its own.
const AGE_21_FALLBACK: &str = "This listing is for adults 21 and over. It opens once your verified age is 21.";
pub const OFF_NOTICE: &str = "This listing is adult content. Turn on Adult content in your Profile to see it.";
/// Where the setting is: the Privacy tab of the settings page, scrolled to the switch.
pub const SETTING_HREF: &str = "/settings?tab=privacy#adult-content";
const NEEDS_VERIFY: &str = "Turning on adult content needs identity and age verification.";

/// `GET /api/account/adult-content`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct AdultSetting {
    pub enabled: bool,
    pub can_enable: bool,
    /// Whether the account's verified age is 21 or over. `None` from a Worker that predates the 21+ band.
    pub age_21_eligible: Option<bool>,
    pub code: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Refusal {
    code: Option<String>,
    reason: Option<String>,
    error: Option<String>,
    signed_in: Option<bool>,
}

/// A 21+ listing refused to a viewer who has the setting on but is under 21 (403 `age_21_required`): the words to show. There is
/// no link to the setting, since turning it on again changes nothing.
pub fn age_21_required(e: &ApiError) -> Option<String> {
    let ApiError::Server { status: 403, message } = e else { return None };
    let r: Refusal = serde_json::from_str(message).ok()?;
    (r.code.as_deref() == Some("age_21_required"))
        .then(|| r.error.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| AGE_21_FALLBACK.to_string()))
}

/// Why the Worker would not turn the setting on: its code and the words to show. Only a 403 is a refusal.
pub fn refusal(e: &ApiError) -> Option<(String, String)> {
    let ApiError::Server { status: 403, message } = e else { return None };
    let r: Refusal = serde_json::from_str(message).unwrap_or_default();
    let reason = r.reason.or(r.error).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| NEEDS_VERIFY.to_string());
    Some((r.code.unwrap_or_default(), reason))
}

/// A listing the Worker held back because the viewer's setting is off: whether the viewer is signed in.
pub fn adult_off(e: &ApiError) -> Option<bool> {
    let ApiError::Server { status: 403, message } = e else { return None };
    let r: Refusal = serde_json::from_str(message).ok()?;
    (r.code.as_deref() == Some("adult_content_off")).then(|| r.signed_in.unwrap_or(false))
}

// -----------------------------------------------------------------------------
// 2. The setting
// -----------------------------------------------------------------------------

/// What the switch knows: still asking, signed out, or the Worker's answer.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "ssr", allow(dead_code))]
enum State {
    Loading,
    SignedOut,
    Ready(AdultSetting),
    Failed,
}

/// The Adult content switch for the settings page. Off until the Worker says otherwise; turning it on asks the Worker, which
/// refuses an account without identity and age verification and says why.
#[component]
pub fn AdultContentSetting() -> impl IntoView {
    let state = RwSignal::new(State::Loading);
    let saving = RwSignal::new(false);
    let refused = RwSignal::new(None::<String>);
    let failed = RwSignal::new(None::<String>);
    #[cfg(not(feature = "ssr"))]
    let api_url = || use_context::<crate::state::AppState>().map(|s| s.api_url.clone()).unwrap_or_default();

    #[cfg(not(feature = "ssr"))]
    let base = StoredValue::new(api_url());
    #[cfg(not(feature = "ssr"))]
    leptos::task::spawn_local(async move {
        let client = crate::api::ApiClient::new(&base.get_value());
        state.set(match client.get::<AdultSetting>("/api/account/adult-content").await {
            Ok(s) => State::Ready(s),
            Err(ApiError::Unauthorized) => State::SignedOut,
            Err(_) => State::Failed,
        });
    });

    let toggle = move |_| {
        let State::Ready(current) = state.get_untracked() else { return };
        if saving.get_untracked() {
            return;
        }
        let want = !current.enabled;
        refused.set(None);
        failed.set(None);
        saving.set(true);
        #[cfg(not(feature = "ssr"))]
        leptos::task::spawn_local(async move {
            let client = crate::api::ApiClient::new(&base.get_value());
            match client.post::<serde_json::Value, _>("/api/account/adult-content", &serde_json::json!({ "enabled": want })).await {
                Ok(_) => state.update(|s| {
                    if let State::Ready(x) = s {
                        x.enabled = want;
                    }
                }),
                Err(e) => match refusal(&e) {
                    Some((_, reason)) => refused.set(Some(reason)),
                    None => failed.set(Some("The setting could not be saved. Try again.".to_string())),
                },
            }
            saving.set(false);
        });
        #[cfg(feature = "ssr")]
        let _ = want;
    };

    let on = move || matches!(state.get(), State::Ready(AdultSetting { enabled: true, .. }));
    view! {
        <div class="settings-group" id="adult-content">
            <h2 class="group-title">{LABEL}</h2>
            <div class="toggle-row">
                <div class="toggle-info">
                    <span class="toggle-label" id="adult-content-label">{LABEL}</span>
                    <span class="toggle-description">{HELP}</span>
                </div>
                <button
                    type="button"
                    class="toggle-switch"
                    role="switch"
                    aria-labelledby="adult-content-label"
                    aria-checked=move || on().to_string()
                    class:active=on
                    disabled=move || !matches!(state.get(), State::Ready(_)) || saving.get()
                    on:click=toggle
                >
                    <span class="toggle-knob"></span>
                </button>
            </div>
            {move || match state.get() {
                State::SignedOut => Some(view! { <p class="toggle-description"><a href="/login">"Sign in"</a>" to change this setting."</p> }.into_any()),
                State::Failed => Some(view! { <p class="toggle-description" role="alert">"The setting could not be read. Reload the page to try again."</p> }.into_any()),
                _ => None,
            }}
            {move || matches!(state.get(), State::Ready(AdultSetting { enabled: true, age_21_eligible: Some(false), .. }))
                .then(|| view! { <p class="toggle-description" role="note">{UNDER_21_NOTE}</p> })}
            {move || refused.get().map(|reason| view! {
                <p class="toggle-description" role="alert">{reason}" "<a href="/verify">"Verify your identity and age"</a></p>
            })}
            {move || failed.get().map(|why| view! { <p class="toggle-description" role="alert">{why}</p> })}
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. The notice on an adult listing
// -----------------------------------------------------------------------------

/// What a direct link to a 21+ listing shows a viewer under 21: the Worker's words, and the way back to the Gallery.
#[component]
pub fn Age21ListingNotice(message: String) -> impl IntoView {
    view! {
        <div class="g-wrap">
            <div class="g-glass g-none" role="status">
                <h1 class="g-none-title">"Adults only (21+)"</h1>
                <p>{message}</p>
                <div class="g-actions">
                    <a href="/gallery" class="g-btn g-btn-primary">"Go to the Gallery"</a>
                </div>
            </div>
        </div>
    }
}

/// What a direct link to an adult listing shows a viewer whose setting is off: the plain notice, and the way to the setting (or to
/// sign in first).
#[component]
pub fn AdultListingNotice(signed_in: bool) -> impl IntoView {
    view! {
        <div class="g-wrap">
            <div class="g-glass g-none" role="status">
                <h1 class="g-none-title">"Adult content"</h1>
                <p>{OFF_NOTICE}</p>
                <div class="g-actions">
                    {if signed_in {
                        view! { <a href=SETTING_HREF class="g-btn g-btn-primary">"Open the setting"</a> }.into_any()
                    } else {
                        view! { <a href="/login" class="g-btn g-btn-primary">"Sign in"</a> }.into_any()
                    }}
                    <a href="/gallery" class="g-btn">"Go to the Gallery"</a>
                </div>
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
    use leptos::tachys::view::RenderHtml;

    fn forbidden(body: &str) -> ApiError {
        ApiError::Server { status: 403, message: body.to_string() }
    }

    #[test]
    fn a_refusal_to_turn_it_on_says_the_workers_reason() {
        let e = forbidden(r#"{"error":"Verification required","code":"kyc_required","reason":"Verify your identity and age first."}"#);
        assert_eq!(refusal(&e), Some(("kyc_required".to_string(), "Verify your identity and age first.".to_string())));
        // no reason: the error, then a sentence of our own
        assert_eq!(refusal(&forbidden(r#"{"error":"Not old enough","code":"underage"}"#)).unwrap().1, "Not old enough");
        assert_eq!(refusal(&forbidden("not json")).unwrap().1, NEEDS_VERIFY);
        // only a 403 is a refusal
        assert_eq!(refusal(&ApiError::Server { status: 500, message: "{}".into() }), None);
        assert_eq!(refusal(&ApiError::Unauthorized), None);
    }

    #[test]
    fn a_listing_held_back_by_the_setting_is_told_apart_from_any_other_403() {
        assert_eq!(adult_off(&forbidden(r#"{"code":"adult_content_off","signed_in":true}"#)), Some(true));
        assert_eq!(adult_off(&forbidden(r#"{"code":"adult_content_off","signed_in":false}"#)), Some(false));
        assert_eq!(adult_off(&forbidden(r#"{"code":"adult_content_off"}"#)), Some(false));
        assert_eq!(adult_off(&forbidden(r#"{"code":"kyc_required"}"#)), None);
        assert_eq!(adult_off(&forbidden("Forbidden")), None);
        assert_eq!(adult_off(&ApiError::NotFound), None);
    }

    #[test]
    fn the_switch_starts_off_and_says_what_it_shows() {
        let owner = Owner::new();
        let html = owner.with(|| AdultContentSetting().to_html());
        assert!(html.contains("id=\"adult-content\"") && html.contains("role=\"switch\""));
        assert!(html.contains("aria-checked=\"false\""), "off until the Worker says otherwise: {html}");
        assert!(html.contains(HELP));
        assert_eq!(LABEL, "Adult content");
        for t in [HELP, OFF_NOTICE, UNDER_21_NOTE, AGE_21_FALLBACK] {
            assert!(!t.contains('\u{2014}') && !t.contains('\u{2013}'), "no dashes: {t}");
        }
        assert!(HELP.contains("Off by default.") && HELP.contains("21+") && !HELP.contains("drug use"));
    }

    #[test]
    fn a_21_plus_refusal_is_told_apart_and_says_the_workers_words() {
        let msg = "This listing is for adults 21 and over. It opens once your verified age is 21.";
        assert_eq!(age_21_required(&forbidden(&format!(r#"{{"error":"{msg}","code":"age_21_required","signed_in":true}}"#))).as_deref(), Some(msg));
        assert_eq!(age_21_required(&forbidden(r#"{"code":"age_21_required"}"#)).as_deref(), Some(AGE_21_FALLBACK));
        assert_eq!(age_21_required(&forbidden(r#"{"code":"adult_content_off","signed_in":true}"#)), None);
        assert_eq!(adult_off(&forbidden(r#"{"code":"age_21_required","signed_in":true}"#)), None);
        assert_eq!(age_21_required(&ApiError::NotFound), None);
        // the field: absent from an older Worker, present from a newer one
        let old: AdultSetting = serde_json::from_str(r#"{"enabled":true,"can_enable":true}"#).unwrap();
        assert_eq!(old.age_21_eligible, None);
        let young: AdultSetting = serde_json::from_str(r#"{"enabled":true,"can_enable":true,"age_21_eligible":false}"#).unwrap();
        assert_eq!(young.age_21_eligible, Some(false));
        // the note is shown only for an enabled setting known to be under 21
        let src = include_str!("adult_content.rs");
        assert!(src.contains("State::Ready(AdultSetting { enabled: true, age_21_eligible: Some(false), .. })"));
        // the notice links to the Gallery and never to the setting
        let owner = Owner::new();
        let html = owner.with(|| Age21ListingNotice(Age21ListingNoticeProps { message: msg.to_string() }).to_html());
        assert!(html.contains(msg) && html.contains("href=\"/gallery\"") && !html.contains("/settings"));
    }

    #[test]
    fn the_notice_sends_a_signed_in_viewer_to_the_setting_and_anyone_else_to_sign_in() {
        let owner = Owner::new();
        let signed_in = owner.with(|| AdultListingNotice(AdultListingNoticeProps { signed_in: true }).to_html());
        assert!(signed_in.contains(OFF_NOTICE) && signed_in.contains("href=\"/settings?tab=privacy#adult-content\"") && !signed_in.contains("href=\"/login\""));
        let signed_out = owner.with(|| AdultListingNotice(AdultListingNoticeProps { signed_in: false }).to_html());
        assert!(signed_out.contains("href=\"/login\"") && !signed_out.contains("/settings"));
        // and the settings page opens on the tab the link names, with the switch on it
        let settings = include_str!("../pages/settings.rs");
        assert!(settings.contains(r#"query.contains("tab=privacy") { "privacy" }"#) && settings.contains("<AdultContentSetting />"));
    }
}
