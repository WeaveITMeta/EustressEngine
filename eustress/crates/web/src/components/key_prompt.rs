// =============================================================================
// Eustress Web - The one-time "replace your key" prompt
// =============================================================================
// Shown once to an account that has never replaced its identity key and has not
// seen this prompt. It exists only here, inside the signed-in site: no email
// carries a link to it, and the text says so. Design:
// docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md.
//
// A two-step checklist with live state. Step 1, confirming the email address, is
// recommended and not required: the replacement does not need it. The prompt
// watches for the confirmation, so the check mark appears when the link is
// opened in another tab. Step 2 is the primary button.
//
// A dialog: focus starts inside it and Tab stays inside it, Escape means "Not
// now", and every control is a native button.
// =============================================================================

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::api::{self, ApiClient};
use crate::state::{AppState, AuthState, KeyReplaceTerms};

/// How long "Send again" waits after a link was sent (the Worker allows three an hour).
const RESEND_SECONDS: u32 = 60;
/// How often the prompt asks whether the address was confirmed, while a link is out.
const WATCH_EVERY_MS: u32 = 5_000;
/// How many times it asks (fifteen minutes) before it stops and leaves "Send again".
const WATCH_TIMES: u32 = 180;

/// `o***@example.com`: enough to recognise the address, not to read it off a screen.
pub fn mask_address(address: &str) -> String {
    match address.split_once('@') {
        Some((local, domain)) if !local.is_empty() => format!("{}***@{}", local.chars().next().unwrap_or('*'), domain),
        _ => "your address".to_string(),
    }
}

fn days(n: u32) -> String {
    if n == 1 { "1 day".to_string() } else { format!("{n} days") }
}

/// What a replacement pauses, in the Worker's own numbers. With none (a deployment
/// that did not send them) the sentence says no number rather than a wrong one.
pub fn pause_sentence(terms: Option<&KeyReplaceTerms>) -> String {
    match terms {
        Some(t) if t.pause_days_with_value > t.pause_days => format!(
            "Payouts and transfers pause for {} ({} if your account holds value), and the payout bank account is locked for {}.",
            days(t.pause_days), days(t.pause_days_with_value), days(t.payout_pin_days)
        ),
        Some(t) => format!(
            "Payouts and transfers pause for {}, and the payout bank account is locked for {}.",
            days(t.pause_days), days(t.payout_pin_days)
        ),
        None => "Payouts and transfers pause for a few days, and the payout bank account is locked for a while.".to_string(),
    }
}

/// Where focus is inside the dialog: on its first control, its last, on the dialog itself
/// (where it starts), or on a control in between.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    First,
    Last,
    Dialog,
    Between,
}

/// Where Tab must send focus so that it stays inside the dialog. `Some(place)` is a wrap the
/// dialog does itself; `None` leaves the browser's own order alone (it stays inside there).
pub fn tab_wraps_to(at: Place, backwards: bool) -> Option<Place> {
    match (at, backwards) {
        (Place::Last, false) => Some(Place::First),
        (Place::First | Place::Dialog, true) => Some(Place::Last),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    /// The account has no address on file.
    NoAddress,
    /// An address that is not confirmed and no link was sent from here.
    Unconfirmed,
    /// A link is out; the prompt is watching for the confirmation.
    LinkSent,
    Confirmed,
}

/// A modal that asks the member to replace their key, once.
#[component]
pub fn KeyPrompt() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;
    let hidden = RwSignal::new(false);
    let navigate = StoredValue::new_local(use_navigate());
    let dialog = NodeRef::<leptos::html::Div>::new();

    // Only an account the Worker says has replaced its key zero times and was
    // never shown this. An account the Worker told nothing about (an older
    // session, a deployment without account objects) is not asked.
    let show = move || {
        !hidden.get()
            && match auth.get() {
                AuthState::Authenticated(user) => user.key_epoch == Some(0) && user.key_prompt_seen_at.is_none(),
                _ => false,
            }
    };
    let user = move || match auth.get() {
        AuthState::Authenticated(u) => Some(u),
        _ => None,
    };
    let address = move || user().and_then(|u| u.email.filter(|e| !e.is_empty()));
    let terms = move || user().and_then(|u| u.key_replace_terms);

    // Step 1: the address.
    let link_sent = RwSignal::new(false);
    let confirmed_here = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let problem = RwSignal::new(String::new());
    let cooldown = RwSignal::new(0u32);
    let watching = RwSignal::new(false);
    let step = move || {
        if confirmed_here.get() || user().map(|u| u.email_verified_at.is_some()).unwrap_or(false) {
            Step::Confirmed
        } else if address().is_none() {
            Step::NoAddress
        } else if link_sent.get() {
            Step::LinkSent
        } else {
            Step::Unconfirmed
        }
    };

    // While a link is out: ask every few seconds whether it was opened.
    let watch = {
        let state = app_state.clone();
        move || {
            if watching.get_untracked() {
                return;
            }
            watching.set(true);
            let state = state.clone();
            spawn_local(async move {
                let client = ApiClient::new(&api_url.get_value());
                for _ in 0..WATCH_TIMES {
                    TimeoutFuture::new(WATCH_EVERY_MS).await;
                    if hidden.get_untracked() || confirmed_here.get_untracked() {
                        break;
                    }
                    if matches!(api::email_status(&client).await, Ok(v) if v.verified) {
                        confirmed_here.set(true);
                        if let Ok(fresh) = api::get_current_user(&client).await {
                            state.login(fresh);
                        }
                        break;
                    }
                }
                watching.set(false);
            });
        }
    };
    // "Send again" counts down.
    let tick = move || {
        cooldown.set(RESEND_SECONDS);
        spawn_local(async move {
            while cooldown.get_untracked() > 0 && !hidden.get_untracked() {
                TimeoutFuture::new(1000).await;
                cooldown.update(|c| *c = c.saturating_sub(1));
            }
        });
    };
    let resend_blocked = move || busy.get() || cooldown.get() > 0;
    let send_link = Callback::new(move |_: ()| {
        if busy.get_untracked() || cooldown.get_untracked() > 0 {
            return;
        }
        busy.set(true);
        problem.set(String::new());
        let watch = watch.clone();
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match api::email_start(&client).await {
                Ok(r) if r.already_verified => confirmed_here.set(true),
                Ok(_) => {
                    link_sent.set(true);
                    tick();
                    watch();
                }
                Err(e) => problem.set(api::failure_sentence(&e)),
            }
            busy.set(false);
        });
    });

    // Either answer counts as having seen it.
    let mark_seen = move || {
        hidden.set(true);
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            let _ = api::mark_key_prompt_seen(&client).await;
        });
    };
    let not_now = Callback::new(move |_: ()| mark_seen());
    let replace_now = Callback::new(move |_: ()| {
        mark_seen();
        navigate.with_value(|go| go("/settings?replace_key=1", Default::default()));
    });
    let add_address = Callback::new(move |_: ()| {
        mark_seen();
        navigate.with_value(|go| go("/settings", Default::default()));
    });

    // Focus starts on the dialog, so a screen reader reads it and Tab goes to its first control.
    Effect::new(move |_| {
        if show() {
            if let Some(el) = dialog.get() {
                let _ = el.focus();
            }
        }
    });
    // Escape is "Not now". Tab stays inside: the last control is always "Replace my key now"
    // and the first is the first button or link in the dialog.
    let replace_button = NodeRef::<leptos::html::Button>::new();
    let keys = move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
        "Escape" => {
            ev.prevent_default();
            not_now.run(());
        }
        "Tab" => {
            let (Some(root), Some(last)) = (dialog.get_untracked(), replace_button.get_untracked()) else { return };
            let first = root.query_selector("button:not([disabled]), a[href]").ok().flatten();
            let active = web_sys::window().and_then(|w| w.document()).and_then(|d| d.active_element());
            let last_el: &web_sys::Element = last.as_ref();
            let root_el: &web_sys::Element = root.as_ref();
            let at = match &active {
                Some(el) if el == last_el => Place::Last,
                Some(el) if first.as_ref() == Some(el) => Place::First,
                Some(el) if el == root_el => Place::Dialog,
                _ => Place::Between,
            };
            match tab_wraps_to(at, ev.shift_key()) {
                Some(Place::Last) => {
                    ev.prevent_default();
                    let _ = last.focus();
                }
                Some(Place::First) => {
                    ev.prevent_default();
                    if let Some(first) = first.and_then(|f| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(f).ok()) {
                        let _ = first.focus();
                    }
                }
                _ => {}
            }
        }
        _ => {}
    };

    view! {
        <Show when=show>
            <div class="key-prompt-overlay">
                <div class="key-prompt" role="dialog" aria-modal="true" aria-labelledby="key-prompt-title" aria-describedby="key-prompt-lead"
                    tabindex="-1" node_ref=dialog on:keydown=keys>

                    <div class="key-prompt-callout">
                        <svg class="key-prompt-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3l7 3v5c0 4.5-3 8.2-7 10-4-1.8-7-5.5-7-10V6l7-3z"/><path d="M9.5 12l2 2 3.5-4"/></svg>
                        <p>"You only see this inside your signed-in account. Eustress never emails you asking to replace your key."</p>
                    </div>

                    <h2 id="key-prompt-title">"Replace your identity key"</h2>
                    <p id="key-prompt-lead" class="key-prompt-lead">
                        "Your identity file holds the private key to your account. Earlier files were sent by email, so a copy of yours may be sitting in an inbox. "
                        "Replacing the key makes every old copy useless."
                    </p>

                    <ul class="key-prompt-facts">
                        <li>
                            <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="8" cy="12" r="3.5"/><path d="M11.5 12H21M18 12v3M15 12v2"/></svg>
                            <span>"You need your current identity file. You get a new one."</span>
                        </li>
                        <li>
                            <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="5" width="14" height="10" rx="1.5"/><path d="M7 19h6M10 15v4"/><rect x="19" y="9" width="2.5" height="8" rx="1"/></svg>
                            <span>"The old file stops working, and you are signed out everywhere else."</span>
                        </li>
                        <li>
                            <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M10 9v6M14 9v6"/></svg>
                            <span>{move || pause_sentence(terms().as_ref())}</span>
                        </li>
                    </ul>

                    <ol class="key-prompt-steps">
                        <li class="key-prompt-step" class:done=move || step() == Step::Confirmed>
                            <span class="key-prompt-badge" aria-hidden="true">
                                {move || if step() == Step::Confirmed {
                                    view! { <svg viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg> }.into_any()
                                } else {
                                    "1".into_any()
                                }}
                            </span>
                            <div class="key-prompt-step-body">
                                <div class="key-prompt-step-title">
                                    "Confirm your email"
                                    <span class="key-prompt-tag">"Recommended"</span>
                                </div>
                                <div class="key-prompt-step-text" aria-live="polite">
                                    {move || {
                                        let masked = address().map(|a| mask_address(&a)).unwrap_or_default();
                                        match step() {
                                            Step::Confirmed => format!("{masked} is confirmed. Eustress can reach you if your account is ever at risk."),
                                            Step::LinkSent => format!("Link sent to {masked}. Open it and press the button on the page; this updates here when you do."),
                                            Step::Unconfirmed => format!("{masked}: confirm it so Eustress can reach you if your account is ever at risk. You can replace your key without it."),
                                            Step::NoAddress => "Your account has no email address. Add one in Settings, Security, so Eustress can warn you about changes.".to_string(),
                                        }
                                    }}
                                </div>
                                {move || (!problem.get().is_empty()).then(|| view! { <div class="key-prompt-problem" role="alert">{problem.get()}</div> })}
                                <div class="key-prompt-step-action">
                                    {move || match step() {
                                        Step::Unconfirmed => view! {
                                            <button class="btn btn-secondary btn-sm" disabled=move || busy.get() on:click=move |_| send_link.run(())>
                                                {move || if busy.get() { "Sending..." } else { "Send link" }}
                                            </button>
                                        }.into_any(),
                                        Step::LinkSent => view! {
                                            <button class="btn btn-secondary btn-sm" disabled=resend_blocked on:click=move |_| send_link.run(())>
                                                {move || if cooldown.get() > 0 { format!("Send again in {} s", cooldown.get()) } else { "Send again".to_string() }}
                                            </button>
                                        }.into_any(),
                                        Step::NoAddress => view! {
                                            <button class="btn btn-secondary btn-sm" on:click=move |_| add_address.run(())>"Open Settings"</button>
                                        }.into_any(),
                                        Step::Confirmed => ().into_any(),
                                    }}
                                </div>
                            </div>
                        </li>
                        <li class="key-prompt-step">
                            <span class="key-prompt-badge" aria-hidden="true">"2"</span>
                            <div class="key-prompt-step-body">
                                <div class="key-prompt-step-title">"Replace your key"</div>
                                <div class="key-prompt-step-text">"Pick your current identity file, and your browser makes the new one."</div>
                            </div>
                        </li>
                    </ol>

                    <div class="key-prompt-footer">
                        <p class="key-prompt-hint">"You can do this any time in Settings > Security. This message is shown once."</p>
                        <div class="key-prompt-actions">
                            <button class="btn btn-ghost" on:click=move |_| not_now.run(())>"Not now"</button>
                            <button class="btn btn-primary" node_ref=replace_button on:click=move |_| replace_now.run(())>"Replace my key now"</button>
                        </div>
                    </div>
                </div>
            </div>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_masked_to_its_first_letter_and_its_domain() {
        assert_eq!(mask_address("owner@example.com"), "o***@example.com");
        assert_eq!(mask_address("Ünal@example.com"), "Ü***@example.com");
        assert_eq!(mask_address("no-at-sign"), "your address");
        assert_eq!(mask_address("@example.com"), "your address");
        assert_eq!(mask_address(""), "your address");
    }

    #[test]
    fn the_pause_is_said_in_the_workers_numbers() {
        let t = KeyReplaceTerms { pause_days: 3, pause_days_with_value: 7, payout_pin_days: 30 };
        assert_eq!(
            pause_sentence(Some(&t)),
            "Payouts and transfers pause for 3 days (7 days if your account holds value), and the payout bank account is locked for 30 days."
        );
        let same = KeyReplaceTerms { pause_days: 1, pause_days_with_value: 1, payout_pin_days: 1 };
        assert_eq!(pause_sentence(Some(&same)), "Payouts and transfers pause for 1 day, and the payout bank account is locked for 1 day.");
    }

    #[test]
    fn with_no_numbers_from_the_worker_the_prompt_says_no_number() {
        let text = pause_sentence(None);
        assert!(!text.chars().any(|c| c.is_ascii_digit()), "{text}");
        assert!(text.contains("pause"));
    }

    #[test]
    fn tab_wraps_at_both_ends_and_otherwise_leaves_the_browser_alone() {
        assert_eq!(tab_wraps_to(Place::Last, false), Some(Place::First), "forward from the last goes to the first");
        assert_eq!(tab_wraps_to(Place::First, true), Some(Place::Last), "backward from the first goes to the last");
        assert_eq!(tab_wraps_to(Place::Dialog, true), Some(Place::Last), "backward from the dialog itself would leave it, so it wraps");
        assert_eq!(tab_wraps_to(Place::Dialog, false), None, "forward from the dialog itself goes to the first control by itself");
        assert_eq!(tab_wraps_to(Place::Between, false), None);
        assert_eq!(tab_wraps_to(Place::Between, true), None);
        assert_eq!(tab_wraps_to(Place::First, false), None);
        assert_eq!(tab_wraps_to(Place::Last, true), None);
    }
}
