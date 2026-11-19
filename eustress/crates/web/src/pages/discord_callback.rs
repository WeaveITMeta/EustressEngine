// =============================================================================
// Eustress Web - Discord link callback (/auth/discord/callback)
// =============================================================================
// Discord sends the browser here after the person allows (or cancels) linking
// their Discord account from Settings. The address carries Discord's one-time
// `code` and the `state` the Worker made when the link began; this page takes
// both out of the address bar at once and hands them to the Worker, which
// finishes the link only for the account that began it (src/discord_link.mjs).
//
// Unlike a link in an email, this page acts on load: nothing but Discord's own
// redirect lands here, the code is useless without this account's session, and
// it expires in minutes.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{discord_failure_sentence, finish_discord_link};
use crate::pages::discord_roles::take_roles_return;
use crate::components::result_card::{Glyph, ResultCard, ResultPage, Tone};
use crate::state::{AppState, AuthState};
use crate::utils::{current_query, hold_back_referrer, query_value, replace_path};

const PATH: &str = "/auth/discord/callback";

/// What Discord handed back, read from a query string.
#[derive(Debug, Clone, PartialEq)]
enum Handed {
    /// The person allowed it: finish with these.
    Code { code: String, state: String },
    /// The person pressed Cancel on Discord's page.
    Cancelled,
    /// No code, or Discord reported another error.
    Incomplete,
}

fn decoded(query: &str, name: &str) -> Option<String> {
    let raw = query_value(query, name)?;
    let text = urlencoding::decode(&raw).map(|c| c.into_owned()).unwrap_or(raw);
    (!text.is_empty()).then_some(text)
}

fn read_handed(query: &str) -> Handed {
    if decoded(query, "error").as_deref() == Some("access_denied") {
        return Handed::Cancelled;
    }
    match (decoded(query, "code"), decoded(query, "state")) {
        (Some(code), Some(state)) => Handed::Code { code, state },
        _ => Handed::Incomplete,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Stage {
    Working,
    Linked(Option<String>),
    Cancelled,
    Incomplete,
    SignedOut,
    Failed(String),
}

#[component]
pub fn DiscordCallbackPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;

    // Read once, then out of the address bar and out of any Referer.
    let handed = StoredValue::new(read_handed(&current_query()));
    // Set by /discord/roles before it sent the browser to Discord. Taken (and cleared) once, whatever the outcome.
    let from_roles = StoredValue::new(take_roles_return());
    replace_path(PATH);
    hold_back_referrer();

    let stage = RwSignal::new(Stage::Working);
    let started = StoredValue::new(false);

    // The session is restored from storage at start; wait until it is known.
    Effect::new(move |_| {
        let state_now = auth.get();
        if started.get_value() || matches!(state_now, AuthState::Unknown) {
            return;
        }
        started.set_value(true);
        match handed.get_value() {
            Handed::Cancelled => stage.set(Stage::Cancelled),
            Handed::Incomplete => stage.set(Stage::Incomplete),
            Handed::Code { .. } if matches!(state_now, AuthState::Unauthenticated) => stage.set(Stage::SignedOut),
            Handed::Code { code, state } => {
                let app_state = app_state.clone();
                spawn_local(async move {
                    match finish_discord_link(&api_url.get_value(), &code, &state).await {
                        Ok(user) => {
                            let handle = user.discord_username.clone();
                            app_state.login(user);
                            stage.set(Stage::Linked(handle));
                        }
                        Err(e) => stage.set(Stage::Failed(discord_failure_sentence(&e))),
                    }
                });
            }
        }
    });

    let to_settings = || view! {
        <div class="result-actions">
            <a href="/settings" class="btn btn-primary">"Back to Settings"</a>
        </div>
    };

    view! {
        <ResultPage>
            {move || match stage.get() {
                Stage::Working => view! {
                    <ResultCard tone=Tone::Plain glyph=Glyph::Busy title="Connecting Discord">
                        <p>"One moment."</p>
                    </ResultCard>
                }.into_any(),
                Stage::Linked(_) if from_roles.get_value() => view! {
                    <ResultCard tone=Tone::Ok glyph=Glyph::Check title="Discord is connected">
                        <p>"Your Discord roles are updated. You can go back to Discord."</p>
                    </ResultCard>
                }.into_any(),
                Stage::Linked(handle) => view! {
                    <ResultCard tone=Tone::Ok glyph=Glyph::Check title="Discord is connected">
                        <p>{match handle {
                            Some(h) => format!("Your Eustress account is now linked to the Discord account {h}."),
                            None => "Your Eustress account is now linked to your Discord account.".to_string(),
                        }}</p>
                        {to_settings()}
                    </ResultCard>
                }.into_any(),
                Stage::Cancelled => view! {
                    <ResultCard tone=Tone::Plain glyph=Glyph::Shield title="Discord was not connected">
                        <p>"You cancelled on Discord's page, so nothing was linked. You can connect it from Settings at any time."</p>
                        {to_settings()}
                    </ResultCard>
                }.into_any(),
                Stage::Incomplete => view! {
                    <ResultCard tone=Tone::Plain glyph=Glyph::BrokenLink title="This link is incomplete">
                        <p>"Discord did not send back everything needed. Start again from Settings."</p>
                        {to_settings()}
                    </ResultCard>
                }.into_any(),
                Stage::SignedOut => view! {
                    <ResultCard tone=Tone::Warn glyph=Glyph::Key title="Sign in first">
                        <p>"Sign in to Eustress, then connect Discord from Settings."</p>
                        <div class="result-actions">
                            <a href="/login" class="btn btn-primary">"Sign in"</a>
                        </div>
                    </ResultCard>
                }.into_any(),
                Stage::Failed(sentence) => view! {
                    <ResultCard tone=Tone::Bad glyph=Glyph::Alert title="Discord was not connected">
                        <p>{sentence}</p>
                        {to_settings()}
                    </ResultCard>
                }.into_any(),
            }}
        </ResultPage>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_discord_handed_back() {
        let state = "a".repeat(64);
        assert_eq!(
            read_handed(&format!("?code=Abc123xyz&state={state}")),
            Handed::Code { code: "Abc123xyz".into(), state: state.clone() }
        );
        assert_eq!(read_handed("?error=access_denied&error_description=The+user+denied&state=x"), Handed::Cancelled);
        assert_eq!(read_handed("?error=server_error&state=x"), Handed::Incomplete);
        assert_eq!(read_handed("?code=Abc123xyz"), Handed::Incomplete);
        assert_eq!(read_handed(""), Handed::Incomplete);
        assert_eq!(read_handed("?code=&state="), Handed::Incomplete);
    }
}
