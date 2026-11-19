// =============================================================================
// Eustress Web - Eustress on Slack (/slack)
// =============================================================================
// The showcase for the Eustress Slack app. Public, prerendered, and honest:
// what ships now is described plainly. The Worker starts the OAuth install at
// api.eustress.dev and sends the browser back here with `?installed=1` or
// `?install_error=<code>`; those are read only in the browser, after the
// prerendered page has loaded.
//
// Table of Contents:
// 1. Install address and the result of an install
// 2. The page
// 3. Tests
// =============================================================================

use leptos::prelude::*;

use crate::components::{CentralNav, Footer};
use crate::utils::{current_query, query_value};

// -----------------------------------------------------------------------------
// 1. Install address and the result of an install
// -----------------------------------------------------------------------------

/// Where "Add to Slack" goes: the Worker starts the Slack OAuth install here.
pub const INSTALL_URL: &str = "https://api.eustress.dev/api/slack/install";

/// What the Worker reported when it sent the browser back to /slack.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum InstallResult {
    Installed,
    Failed(String),
}

/// Read the outcome from a query string ("?installed=1" or "?install_error=code").
pub fn install_result(query: &str) -> Option<InstallResult> {
    if query_value(query, "installed").as_deref() == Some("1") {
        return Some(InstallResult::Installed);
    }
    query_value(query, "install_error").map(InstallResult::Failed)
}

/// A plain sentence for an install error code. An unknown code gets the general one.
pub fn error_sentence(code: &str) -> &'static str {
    match code {
        "access_denied" => "The install was cancelled in Slack. Try Add to Slack again when you are ready.",
        "invalid_state" | "state_expired" => "The install took too long or was started twice. Try Add to Slack again.",
        _ => "The install did not finish. Try Add to Slack again.",
    }
}

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

#[component]
fn Command(name: &'static str, what: &'static str, example: &'static str, note: &'static str) -> impl IntoView {
    view! {
        <article class="sl-card g-glass">
            <h3 class="sl-cmd"><code>{name}</code></h3>
            <p>{what}</p>
            <p class="sl-example"><code>{example}</code></p>
            <p class="sl-note">{note}</p>
        </article>
    }
}

#[component]
pub fn SlackPage() -> impl IntoView {
    // Empty on the server and on first paint; filled in the browser, so the prerendered page never differs from what hydrates.
    let result = RwSignal::new(None::<InstallResult>);
    Effect::new(move |_| {
        result.set(install_result(&current_query()));
    });

    view! {
        <div class="page g-page sl-page">
            <CentralNav active="".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main">
                <div class="sl-wrap">
                    {move || match result.get() {
                        Some(InstallResult::Installed) => view! {
                            <div class="sl-result sl-result-ok g-glass" role="status">
                                <strong>"Eustress is installed in your workspace."</strong>
                                " Type "<code>"/eustress help"</code>" in any channel."
                            </div>
                        }.into_any(),
                        Some(InstallResult::Failed(code)) => view! {
                            <div class="sl-result sl-result-err g-glass" role="alert">{error_sentence(&code)}</div>
                        }.into_any(),
                        None => ().into_any(),
                    }}
                    <header class="sl-hero g-glass">
                        <h1 class="sl-title">"Eustress on Slack"</h1>
                        <p class="sl-lede">
                            "Eustress is a simulation and data platform; it also builds games and interactive worlds. The Eustress app for Slack finds Spaces, shows who is playing right now, and shares a Space's card in a channel."
                        </p>
                        <div class="sl-actions">
                            <a class="sl-add" href=INSTALL_URL rel="noopener">
                                <img alt="Add to Slack" height="40" width="139" src="https://platform.slack-edge.com/img/add_to_slack.png" srcset="https://platform.slack-edge.com/img/add_to_slack.png 1x, https://platform.slack-edge.com/img/add_to_slack@2x.png 2x" />
                            </a>
                            <a class="g-btn g-btn-lg" href="/settings">"Link your Eustress account"</a>
                        </div>
                        <p class="sl-install-note">"Installing needs permission to add apps to the workspace."</p>
                    </header>

                    <section class="sl-section">
                        <h2>"What it does"</h2>
                        <div class="sl-grid">
                            <Command
                                name="/eustress space <name>"
                                what="Find a Space and share its card in the channel."
                                example="/eustress space city builder"
                                note="Anyone can use it." />
                            <Command
                                name="/eustress live"
                                what="See the Spaces with people in them right now."
                                example="/eustress live"
                                note="Anyone can use it." />
                            <Command
                                name="/eustress profile @someone"
                                what="Show a linked person's Eustress profile."
                                example="/eustress profile @ada"
                                note="Works for people who have linked their account." />
                            <Command
                                name="/eustress me"
                                what="Your Bliss, Tickets and notifications. Only you see it."
                                example="/eustress me"
                                note="Needs Connect Slack, below." />
                            <Command
                                name="/eustress follow"
                                what="Follow a Space or a creator into a channel. The app posts there when it is published, updated, or goes live."
                                example="/eustress follow"
                                note="The person who added the app, or a workspace admin. Invite the app to the channel first with /invite @Eustress Engine." />
                        </div>
                    </section>

                    <section class="sl-section">
                        <h2>"What it can see"</h2>
                        <p class="sl-plain">
                            "The app uses three permissions: run its slash command, post messages, and view the member list, only to check who is a workspace admin. It cannot read your messages, channels or files."
                        </p>
                        <p class="sl-plain">
                            "Content rated above PG never appears in channels. A linked account that is 18 or older sees more only in its own DM with the app."
                        </p>
                    </section>

                    <section class="sl-section">
                        <h2>"Connect Slack"</h2>
                        <p class="sl-plain">
                            "Link your Slack account in "
                            <a href="/settings">"Settings"</a>
                            " so that /eustress me and /eustress profile know who you are. Eustress reads only your Slack name and workspace, and keeps no Slack sign-in token."
                        </p>
                    </section>
                </div>
            </main>
            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_install_link_is_exactly_the_workers_install_start() {
        assert_eq!(INSTALL_URL, "https://api.eustress.dev/api/slack/install");
        let source = include_str!("slack.rs");
        assert!(source.contains("href=INSTALL_URL"), "the Add to Slack button carries the install address");
        assert!(source.contains("src=\"https://platform.slack-edge.com/img/add_to_slack.png\""));
    }

    #[test]
    fn the_query_picks_the_card_and_unknown_codes_get_the_general_sentence() {
        assert_eq!(install_result("?installed=1"), Some(InstallResult::Installed));
        assert_eq!(install_result("?install_error=access_denied"), Some(InstallResult::Failed("access_denied".into())));
        assert_eq!(install_result(""), None);
        assert_eq!(install_result("?installed=0"), None);
        assert_eq!(error_sentence("zzz"), "The install did not finish. Try Add to Slack again.");
    }

    #[test]
    fn the_page_copy_has_no_long_dashes_and_never_says_game_engine() {
        let source = include_str!("slack.rs");
        // The long dashes are spelled by code point so this file does not hold them itself.
        for dash in ['\u{2014}', '\u{2013}'] {
            assert!(!source.contains(dash), "slack.rs holds a long dash");
        }
        let phrase = format!("{} {}", "game", "engine");
        assert!(!source.to_lowercase().contains(&phrase));
    }

    #[test]
    fn the_shell_carries_the_slack_app_id() {
        let index = include_str!("../../index.html");
        assert!(index.contains("<meta name=\"slack-app-id\" content=\"A0CHNKPQCKX\">"));
        assert!(index.contains("href=\"style/slack.css\""));
    }
}
