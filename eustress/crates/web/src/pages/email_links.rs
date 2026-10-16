// =============================================================================
// Eustress Web - Pages a link in an email opens
// =============================================================================
// /verify-email?token=...        confirm the address on file
// /cancel-email-change?token=... stop a change of address that was not asked for
// /undo-key-replacement?token=... put an earlier key back after support replaced it
//
// A link in an email is opened by mail scanners as well as by people, so none of
// these pages does anything when it loads: the token is read once, taken out of
// the address bar (it must not stay in the history, or travel in a Referer), and a
// person presses the button. Each state is its own compact card that says what
// happened in its title (components/result_card.rs); none of them is a hero.
// Design: docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md, "Verified email".
//
// Opening the link needs no sign-in: the token is the proof. A signed-out reader
// gets "Sign in" where a signed-in one gets "Back to Settings".
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{self, link_failure, ApiClient, LinkFailure};
use crate::components::key_prompt::mask_address;
use crate::components::result_card::{Glyph, ResultCard, ResultPage, Tone};
use crate::state::{AppState, AuthState};
use crate::utils::{current_query, hold_back_referrer, query_value, replace_path};

/// The token in the address bar, taken out of it. `None` when the link has no
/// token (a link that was cut off).
fn take_token(path: &str) -> Option<String> {
    let token = query_value(&current_query(), "token").filter(|t| !t.is_empty());
    replace_path(path);
    hold_back_referrer();
    token
}

/// What a reader who is not signed in can do next: sign in, or leave. Before the
/// site has found out whether they are signed in, they get the signed-in choice.
fn way_on(auth: RwSignal<AuthState>) -> (&'static str, &'static str) {
    if matches!(auth.get(), AuthState::Unauthenticated) { ("Sign in", "/login") } else { ("Back to Settings", "/settings") }
}

/// The two buttons under most cards: the way on, and the gallery.
fn actions(auth: RwSignal<AuthState>) -> AnyView {
    view! {
        <div class="result-actions">
            {move || { let (label, href) = way_on(auth); view! { <a href=href class="btn btn-primary">{label}</a> }}}
            <a href="/gallery" class="btn btn-secondary">"Go to the Gallery"</a>
        </div>
    }
    .into_any()
}

/// The card for a link with no token in it.
fn incomplete(auth: RwSignal<AuthState>, what: &'static str) -> AnyView {
    view! {
        <ResultCard tone=Tone::Plain glyph=Glyph::BrokenLink title="This link is incomplete">
            <p>{what}</p>
            {actions(auth)}
        </ResultCard>
    }
    .into_any()
}

/// The card for an answer that did not come. The link is kept, so "Try again" works.
fn try_again(retry: Callback<()>, what: &'static str) -> AnyView {
    view! {
        <ResultCard tone=Tone::Bad glyph=Glyph::Alert title="Could not reach Eustress">
            <p>{what}</p>
            <div class="result-actions">
                <button class="btn btn-primary" on:click=move |_| retry.run(())>"Try again"</button>
            </div>
        </ResultCard>
    }
    .into_any()
}

fn working(title: &'static str) -> AnyView {
    view! {
        <ResultCard tone=Tone::Plain glyph=Glyph::Busy title=title>
            <p>"One moment."</p>
        </ResultCard>
    }
    .into_any()
}

// -----------------------------------------------------------------------------
// Confirm the address
// -----------------------------------------------------------------------------

/// Where the confirm page stands.
#[derive(Clone, PartialEq)]
enum Verify {
    Ready,
    Working,
    /// Confirmed just now.
    Done { address: Option<String>, recovery: Recovery },
    /// Confirmed earlier: the link was used, or a new one was asked for after it was.
    Already { address: Option<String>, recovery: Recovery },
    Expired,
    Invalid,
    Incomplete,
    TryAgain,
    NewLinkSent { to: Option<String> },
}

/// What the page says about account recovery: from when the address counts.
#[derive(Clone, PartialEq)]
enum Recovery {
    From(String),
    Now,
    Unknown,
}

fn recovery_line(recovery: &Recovery) -> Option<String> {
    match recovery {
        Recovery::From(date) => Some(format!("Eustress can use it for account recovery from {date}.")),
        Recovery::Now => Some("Eustress can already use it for account recovery.".to_string()),
        Recovery::Unknown => None,
    }
}

const RECOVERY_WHY: &str = "An address only counts for recovery after it has been confirmed for a while, so someone who briefly gets into your account cannot swap in their own.";

/// Confirm the address on file.
#[component]
pub fn VerifyEmailPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;
    let token = RwSignal::new(take_token("/verify-email"));
    let stage = RwSignal::new(if token.get_untracked().is_some() { Verify::Ready } else { Verify::Incomplete });
    let sending = RwSignal::new(false);
    let problem = RwSignal::new(String::new());

    // A link that did not work, for a reader who is signed in and has already confirmed the
    // address: say so, instead of "this link can't be used".
    let after_invalid = move || {
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match api::email_status(&client).await {
                Ok(v) if v.verified => {
                    let recovery = match (v.aged, v.aged_at.as_deref()) {
                        (true, _) => Recovery::Now,
                        (false, Some(at)) => Recovery::From(api::iso_to_local_date(at)),
                        _ => Recovery::Unknown,
                    };
                    stage.set(Verify::Already { address: v.on_file.as_deref().map(mask_address), recovery });
                }
                _ => stage.set(Verify::Invalid),
            }
        });
    };

    let confirm = Callback::new(move |_: ()| {
        let Some(t) = token.get_untracked() else { return };
        stage.set(Verify::Working);
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match api::email_confirm(&client, &t).await {
                Ok(r) => {
                    token.set(None);
                    let recovery = r.aged_at.as_deref().map(api::iso_to_local_date).map(Recovery::From).unwrap_or(Recovery::Unknown);
                    stage.set(Verify::Done { address: r.address, recovery });
                }
                Err(e) => match link_failure(&e) {
                    LinkFailure::Expired => {
                        token.set(None);
                        stage.set(Verify::Expired);
                    }
                    LinkFailure::Invalid => {
                        token.set(None);
                        stage.set(Verify::Invalid);
                        after_invalid();
                    }
                    // Nothing was spent: the same link works when the answer comes.
                    LinkFailure::Unavailable => stage.set(Verify::TryAgain),
                },
            }
        });
    });

    let new_link = Callback::new(move |_: ()| {
        if sending.get_untracked() {
            return;
        }
        sending.set(true);
        problem.set(String::new());
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match api::email_start(&client).await {
                Ok(r) if r.already_verified => after_invalid(),
                Ok(r) => stage.set(Verify::NewLinkSent { to: r.sent_to }),
                Err(e) => problem.set(api::failure_sentence(&e)),
            }
            sending.set(false);
        });
    });

    view! {
        <ResultPage>
            {move || match stage.get() {
                Verify::Ready => view! {
                    <ResultCard tone=Tone::Info glyph=Glyph::Shield title="Confirm your email">
                        <p>"Press the button to confirm that this address is yours."</p>
                        <p class="result-why">"This only confirms the address. It cannot change your key or your account."</p>
                        <div class="result-actions">
                            <button class="btn btn-primary" on:click=move |_| confirm.run(())>"Confirm this address"</button>
                        </div>
                    </ResultCard>
                }.into_any(),
                Verify::Working => working("Confirming..."),
                Verify::Done { address, recovery } => view! {
                    <ResultCard tone=Tone::Ok glyph=Glyph::Check title="Email confirmed">
                        {address.map(|a| view! { <p class="result-sub">{a}</p> })}
                        {recovery_line(&recovery).map(|line| view! { <p>{line}</p><p class="result-why">{RECOVERY_WHY}</p> })}
                        {actions(auth)}
                    </ResultCard>
                }.into_any(),
                Verify::Already { address, recovery } => view! {
                    <ResultCard tone=Tone::Ok glyph=Glyph::Check title="Email already confirmed">
                        {address.map(|a| view! { <p class="result-sub">{a}</p> })}
                        <p>"There is nothing more to do."</p>
                        {recovery_line(&recovery).map(|line| view! { <p class="result-why">{line}</p> })}
                        {actions(auth)}
                    </ResultCard>
                }.into_any(),
                Verify::Expired => view! {
                    <ResultCard tone=Tone::Plain glyph=Glyph::Clock title="This link has expired">
                        <p>"Confirmation links only work for a limited time, and a new link replaces the last one."</p>
                        {move || (!problem.get().is_empty()).then(|| view! { <p class="result-problem" role="alert">{problem.get()}</p> })}
                        <div class="result-actions">
                            {move || if matches!(auth.get(), AuthState::Unauthenticated) {
                                view! { <a href="/login" class="btn btn-primary">"Sign in to send a new link"</a> }.into_any()
                            } else {
                                view! {
                                    <button class="btn btn-primary" disabled=move || sending.get() on:click=move |_| new_link.run(())>
                                        {move || if sending.get() { "Sending..." } else { "Send a new link" }}
                                    </button>
                                }.into_any()
                            }}
                            <a href="/gallery" class="btn btn-secondary">"Go to the Gallery"</a>
                        </div>
                    </ResultCard>
                }.into_any(),
                Verify::NewLinkSent { to } => view! {
                    <ResultCard tone=Tone::Info glyph=Glyph::Mail title="Check your inbox">
                        {to.map(|a| view! { <p class="result-sub">{a}</p> })}
                        <p>"A new link is on its way. It replaces the one that expired."</p>
                        {actions(auth)}
                    </ResultCard>
                }.into_any(),
                Verify::Invalid => view! {
                    <ResultCard tone=Tone::Plain glyph=Glyph::BrokenLink title="This link can't be used">
                        <p>"It was already used, was replaced by a newer link, or is not valid."</p>
                        <p class="result-why">"If your address is already confirmed, there is nothing more to do. Otherwise ask for a new link in Settings, Security."</p>
                        {actions(auth)}
                    </ResultCard>
                }.into_any(),
                Verify::Incomplete => incomplete(auth, "Open the whole link from the email, or ask for a new one in Settings, Security."),
                Verify::TryAgain => try_again(confirm, "Nothing was changed. Check your connection and try again."),
            }}
        </ResultPage>
    }
}

// -----------------------------------------------------------------------------
// Stop a change of address
// -----------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Plain {
    Ready,
    Working,
    Done,
    Invalid,
    Incomplete,
    TryAgain,
}

/// Stop a change of address that was not asked for.
#[component]
pub fn CancelEmailChangePage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;
    let token = RwSignal::new(take_token("/cancel-email-change"));
    let stage = RwSignal::new(if token.get_untracked().is_some() { Plain::Ready } else { Plain::Incomplete });
    let state = app_state.clone();

    let cancel = Callback::new(move |_: ()| {
        let Some(t) = token.get_untracked() else { return };
        stage.set(Plain::Working);
        let state = state.clone();
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match api::email_cancel_link(&client, &t).await {
                Ok(()) => {
                    token.set(None);
                    // Every sign-in on the account ended, this browser's too.
                    state.logout();
                    stage.set(Plain::Done);
                }
                Err(e) => match link_failure(&e) {
                    LinkFailure::Unavailable => stage.set(Plain::TryAgain),
                    _ => {
                        token.set(None);
                        stage.set(Plain::Invalid);
                    }
                },
            }
        });
    });

    view! {
        <ResultPage>
            {move || match stage.get() {
                Plain::Ready => view! {
                    <ResultCard tone=Tone::Warn glyph=Glyph::Alert title="Cancel the change of address?">
                        <p>"Someone asked to change the email address on your account. If that was not you, cancel it now."</p>
                        <p class="result-warning">"Cancelling also signs out every device on your account."</p>
                        <p class="result-why">"If it was you, you do not need to do anything."</p>
                        <div class="result-actions">
                            <button class="btn btn-primary" on:click=move |_| cancel.run(())>"Cancel the change"</button>
                            <a href="/gallery" class="btn btn-secondary">"It was me, go to the Gallery"</a>
                        </div>
                    </ResultCard>
                }.into_any(),
                Plain::Working => working("Cancelling..."),
                Plain::Done => view! {
                    <ResultCard tone=Tone::Ok glyph=Glyph::Check title="Change cancelled">
                        <p>"The address on your account stays as it was. Every sign-in on your account ended, this browser's too."</p>
                        <p class="result-why">
                            "Sign in again with your identity file. If you did not ask for the change, replace your identity key in Settings, Security: "
                            "the request was signed with your current key."
                        </p>
                        <div class="result-actions">
                            <a href="/login" class="btn btn-primary">"Sign in"</a>
                            <a href="/gallery" class="btn btn-secondary">"Go to the Gallery"</a>
                        </div>
                    </ResultCard>
                }.into_any(),
                Plain::Invalid => view! {
                    <ResultCard tone=Tone::Plain glyph=Glyph::BrokenLink title="This link can't be used">
                        <p>"It has expired, was already used, or the change was already cancelled or made."</p>
                        <p class="result-why">"If you did not ask for a change of address, check Settings, Security."</p>
                        {actions(auth)}
                    </ResultCard>
                }.into_any(),
                Plain::Incomplete => incomplete(auth, "Open the whole link from the email."),
                Plain::TryAgain => try_again(cancel, "Nothing was changed. Check your connection and try again."),
            }}
        </ResultPage>
    }
}

// -----------------------------------------------------------------------------
// Put the earlier key back
// -----------------------------------------------------------------------------

/// Put the earlier key back after support replaced it, from the link in the notice.
#[component]
pub fn UndoKeyReplacementPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;
    let token = RwSignal::new(take_token("/undo-key-replacement"));
    let stage = RwSignal::new(if token.get_untracked().is_some() { Plain::Ready } else { Plain::Incomplete });
    let state = app_state.clone();

    let undo = Callback::new(move |_: ()| {
        let Some(t) = token.get_untracked() else { return };
        stage.set(Plain::Working);
        let state = state.clone();
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match api::key_restore(&client, &t).await {
                Ok(()) => {
                    token.set(None);
                    // Every sign-in on the account ended, this browser's too.
                    state.logout();
                    stage.set(Plain::Done);
                }
                Err(e) => match link_failure(&e) {
                    LinkFailure::Unavailable => stage.set(Plain::TryAgain),
                    _ => {
                        token.set(None);
                        stage.set(Plain::Invalid);
                    }
                },
            }
        });
    });

    view! {
        <ResultPage>
            {move || match stage.get() {
                Plain::Ready => view! {
                    <ResultCard tone=Tone::Warn glyph=Glyph::Key title="Put your earlier key back?">
                        <p>"Support replaced the identity key on your account."</p>
                        <p>
                            "If that was not you, or you did not ask support for it, put your earlier key back now. It ends every sign-in, "
                            "keeps payouts and transfers paused, and marks the case for a person to review."
                        </p>
                        <p class="result-warning"><strong>"If you did ask support to replace your key, do not press the button: it puts your OLD key back."</strong></p>
                        <div class="result-actions">
                            <button class="btn btn-primary" on:click=move |_| undo.run(())>"Put my earlier key back"</button>
                            <a href="/gallery" class="btn btn-secondary">"I asked for it, go to the Gallery"</a>
                        </div>
                    </ResultCard>
                }.into_any(),
                Plain::Working => working("Putting your key back..."),
                Plain::Done => view! {
                    <ResultCard tone=Tone::Ok glyph=Glyph::Check title="Earlier key restored">
                        <p>"Every sign-in on your account ended. Sign in with the identity file you had before support replaced your key."</p>
                        <p class="result-why">"Payouts and transfers stay paused for a few days, and a person at Eustress will review the case."</p>
                        <div class="result-actions">
                            <a href="/login" class="btn btn-primary">"Sign in"</a>
                            <a href="/gallery" class="btn btn-secondary">"Go to the Gallery"</a>
                        </div>
                    </ResultCard>
                }.into_any(),
                Plain::Invalid => view! {
                    <ResultCard tone=Tone::Plain glyph=Glyph::BrokenLink title="This link can't be used">
                        <p>"It has expired or was already used."</p>
                        <p class="result-why">"If you still need help, write to support@eustress.dev and say your username."</p>
                        {actions(auth)}
                    </ResultCard>
                }.into_any(),
                Plain::Incomplete => incomplete(auth, "Open the whole link from the email."),
                Plain::TryAgain => try_again(undo, "Nothing was changed. Check your connection and try again."),
            }}
        </ResultPage>
    }
}
