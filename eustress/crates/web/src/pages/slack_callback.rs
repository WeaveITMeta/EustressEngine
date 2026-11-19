// =============================================================================
// Eustress Web - Slack link callback (/auth/slack/callback)
// =============================================================================
// Slack sends the browser here after the person allows (or cancels) linking
// their Slack account from Settings. The address carries Slack's one-time `code`
// and the `state` the Worker made when the link began; this page takes both out
// of the address bar at once and hands them to the Worker, which finishes the
// link only for the account that began it (src/slack_link.mjs).
//
// Like the Discord callback, this page acts on load: nothing but Slack's own
// redirect lands here, the code is useless without this account's session, and
// it expires in minutes.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{finish_slack_link, slack_failure_sentence};
use crate::components::result_card::{Glyph, ResultCard, ResultPage, Tone};
use crate::state::{AppState, AuthState};
use crate::utils::{current_query, hold_back_referrer, query_value, replace_path};

const PATH: &str = "/auth/slack/callback";

/// What Slack handed back, read from a query string.
#[derive(Debug, Clone, PartialEq)]
enum Handed {
    /// The person allowed it: finish with these.
    Code { code: String, state: String },
    /// The person cancelled on Slack's page.
    Cancelled,
    /// No code, or Slack reported another error.
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
    if decoded(query, "error").is_some() {
        return Handed::Incomplete;
    }
    match (decoded(query, "code"), decoded(query, "state")) {
        (Some(code), Some(state)) => Handed::Code { code, state },
        _ => Handed::Incomplete,
    }
}

/// The sentence a finished link shows: who, and in which workspace, as far as Slack said.
fn linked_sentence(name: Option<&str>, team: Option<&str>) -> String {
    match (name, team) {
        (Some(n), Some(t)) => format!("Your Eustress account is now linked to the Slack account {n} in {t}."),
        (Some(n), None) => format!("Your Eustress account is now linked to the Slack account {n}."),
        (None, Some(t)) => format!("Your Eustress account is now linked to your Slack account in {t}."),
        (None, None) => "Your Eustress account is now linked to your Slack account.".to_string(),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Stage {
    Working,
    Linked(String),
    Cancelled,
    Incomplete,
    SignedOut,
    Failed(String),
}

#[component]
pub fn SlackCallbackPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;

    // Read once, then out of the address bar and out of any Referer.
    let handed = StoredValue::new(read_handed(&current_query()));
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
                    match finish_slack_link(&api_url.get_value(), &code, &state).await {
                        Ok(user) => {
                            let sentence = linked_sentence(user.slack_name.as_deref(), user.slack_team_name.as_deref());
                            app_state.login(user);
                            stage.set(Stage::Linked(sentence));
                        }
                        Err(e) => stage.set(Stage::Failed(slack_failure_sentence(&e))),
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
                    <ResultCard tone=Tone::Plain glyph=Glyph::Busy title="Connecting Slack">
                        <p>"One moment."</p>
                    </ResultCard>
                }.into_any(),
                Stage::Linked(sentence) => view! {
                    <ResultCard tone=Tone::Ok glyph=Glyph::Check title="Slack is connected">
                        <p>{sentence}</p>
                        {to_settings()}
                    </ResultCard>
                }.into_any(),
                Stage::Cancelled => view! {
                    <ResultCard tone=Tone::Plain glyph=Glyph::Shield title="Slack was not connected">
                        <p>"You cancelled on Slack's page, so nothing was linked. You can connect it from Settings at any time."</p>
                        {to_settings()}
                    </ResultCard>
                }.into_any(),
                Stage::Incomplete => view! {
                    <ResultCard tone=Tone::Plain glyph=Glyph::BrokenLink title="This link is incomplete">
                        <p>"Slack did not send back everything needed. Start again from Settings."</p>
                        {to_settings()}
                    </ResultCard>
                }.into_any(),
                Stage::SignedOut => view! {
                    <ResultCard tone=Tone::Warn glyph=Glyph::Key title="Sign in first">
                        <p>"Sign in to Eustress, then connect Slack from Settings."</p>
                        <div class="result-actions">
                            <a href="/login" class="btn btn-primary">"Sign in"</a>
                        </div>
                    </ResultCard>
                }.into_any(),
                Stage::Failed(sentence) => view! {
                    <ResultCard tone=Tone::Bad glyph=Glyph::Alert title="Slack was not connected">
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
    fn slack_callback_reads_what_slack_handed_back() {
        let state = "a".repeat(64);
        assert_eq!(
            read_handed(&format!("?code=1234567890.1234567890.abcdef0123&state={state}")),
            Handed::Code { code: "1234567890.1234567890.abcdef0123".into(), state: state.clone() }
        );
        assert_eq!(read_handed("?error=access_denied&state=x"), Handed::Cancelled);
        assert_eq!(read_handed("?error=invalid_scope&state=x"), Handed::Incomplete);
        // An error beside a code is still an error: nothing is posted.
        assert_eq!(read_handed(&format!("?error=server_error&code=1234.5678&state={state}")), Handed::Incomplete);
        assert_eq!(read_handed("?code=1234.5678"), Handed::Incomplete);
        assert_eq!(read_handed(""), Handed::Incomplete);
        assert_eq!(read_handed("?code=&state="), Handed::Incomplete);
    }

    #[test]
    fn slack_callback_says_who_was_linked() {
        assert_eq!(linked_sentence(Some("Ada L"), Some("Acme")), "Your Eustress account is now linked to the Slack account Ada L in Acme.");
        assert_eq!(linked_sentence(Some("Ada L"), None), "Your Eustress account is now linked to the Slack account Ada L.");
        assert_eq!(linked_sentence(None, Some("Acme")), "Your Eustress account is now linked to your Slack account in Acme.");
        assert_eq!(linked_sentence(None, None), "Your Eustress account is now linked to your Slack account.");
    }
}
