// =============================================================================
// Eustress Web - Eustress on Discord (/discord)
// =============================================================================
// The showcase for the Eustress Discord app, and the Custom URL Discord sends
// people to from the app's profile and App Directory "Add App" button. Public,
// prerendered, and honest: what ships now is described plainly; what does not
// yet ship sits under "Coming next".
//
// Table of Contents:
// 1. Install addresses
// 2. The page
// 3. Tests
// =============================================================================

use leptos::prelude::*;

use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// 1. Install addresses
// -----------------------------------------------------------------------------

/// The Eustress app's id on Discord.
pub const DISCORD_CLIENT_ID: &str = "1571137497331662879";

/// The invite to the Eustress community server.
pub const COMMUNITY_INVITE: &str = "https://discord.gg/DGP9my8DYN";

/// Where each install kind goes. `Server` installs the app for everyone in a
/// server (the person needs Manage Server there); `User` puts the commands on
/// the person's own account, so they work in DMs and in servers without the app.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstallKind {
    Server,
    User,
}

/// The Discord authorize address for an install kind.
pub fn install_url(kind: InstallKind) -> String {
    match kind {
        // permissions=19456: View Channels (1024), Send Messages (2048), Embed Links (16384).
        InstallKind::Server => format!(
            "https://discord.com/oauth2/authorize?client_id={DISCORD_CLIENT_ID}&permissions=19456&integration_type=0&scope=bot+applications.commands"
        ),
        InstallKind::User => format!(
            "https://discord.com/oauth2/authorize?client_id={DISCORD_CLIENT_ID}&integration_type=1&scope=applications.commands"
        ),
    }
}

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

#[component]
fn Command(name: &'static str, what: &'static str, example: &'static str, note: &'static str) -> impl IntoView {
    view! {
        <article class="dc-card g-glass">
            <h3 class="dc-cmd"><code>{name}</code></h3>
            <p>{what}</p>
            <p class="dc-example"><code>{example}</code></p>
            <p class="dc-note">{note}</p>
        </article>
    }
}

#[component]
pub fn DiscordPage() -> impl IntoView {
    view! {
        <div class="page g-page dc-page">
            <CentralNav active="".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main">
                <div class="dc-wrap">
                    <header class="dc-hero g-glass">
                        <h1 class="dc-title">"Eustress on Discord"</h1>
                        <p class="dc-lede">
                            "Eustress is a simulation and data platform; it also builds games and interactive worlds. The Eustress app for Discord finds Spaces, shows who is playing right now, and lets you jump in from a chat."
                        </p>
                        <div class="dc-actions">
                            <a class="g-btn g-btn-primary g-btn-lg" href=install_url(InstallKind::Server) rel="noopener">"Add to server"</a>
                            <a class="g-btn g-btn-primary g-btn-lg" href=install_url(InstallKind::User) rel="noopener">"Add to my apps"</a>
                            <a class="g-btn g-btn-lg" href="/settings">"Link your Eustress account"</a>
                            <a class="g-btn g-btn-lg dc-join" href=COMMUNITY_INVITE target="_blank" rel="noopener">
                                <img src="/assets/icons/discord.svg" alt="" aria-hidden="true" />
                                "Join the Eustress Discord"
                            </a>
                        </div>
                        <ul class="dc-install-notes">
                            <li><strong>"Add to server"</strong>" installs the app for everyone in a server. You need the Manage Server permission there."</li>
                            <li><strong>"Add to my apps"</strong>" lets you use the commands anywhere, including DMs and servers that do not have the app."</li>
                        </ul>
                    </header>

                    <section class="dc-section">
                        <h2>"What it does, wherever you are"</h2>
                        <div class="dc-grid">
                            <Command
                                name="/space"
                                what="Find a Space and post its card, with Play in browser and Open in Player buttons."
                                example="/space query: city builder"
                                note="Anyone can use it." />
                            <Command
                                name="/live"
                                what="See the Spaces with people in them right now, and join one."
                                example="/live"
                                note="Anyone can use it." />
                            <Command
                                name="/profile"
                                what="Show a linked person's Eustress profile."
                                example="/profile user: @ada"
                                note="Works for people who have linked their account." />
                            <Command
                                name="/me"
                                what="Your Bliss, Tickets and notifications, visible only to you."
                                example="/me"
                                note="Needs a linked Eustress account." />
                            <Command
                                name="/follow"
                                what="Follow a Space or a creator into a channel. The app posts there when it is published, updated, or goes live."
                                example="/follow space: Pong"
                                note="Server admins only." />
                        </div>
                    </section>

                    <section class="dc-section">
                        <h2>"It knows where it is"</h2>
                        <div class="dc-table-wrap g-glass">
                            <table class="dc-table">
                                <thead>
                                    <tr><th scope="col">"Who you are"</th><th scope="col">"What you get"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><th scope="row">"Anyone, not linked"</th><td>"Public Spaces rated G to PG."</td></tr>
                                    <tr><th scope="row">"A linked account, 18 or older"</th><td>"Your own balances and notifications. Spaces above G to PG, only in age-restricted channels or in your DM with the app."</td></tr>
                                    <tr><th scope="row">"Server admins"</th><td>"Follows, and the posts the app makes in a channel."</td></tr>
                                </tbody>
                            </table>
                        </div>
                        <p class="dc-plain">"Content rated above PG never appears in regular channels."</p>
                    </section>

                    <section class="dc-section">
                        <h2>"Linked Roles"</h2>
                        <p class="dc-plain">
                            "Server admins can require roles such as Verified Eustress account, Creator, or a number of published Spaces. Discord checks them against Eustress. Members connect from the server's Linked Roles menu."
                        </p>
                    </section>

                    <section class="dc-section">
                        <h2>"Privacy"</h2>
                        <p class="dc-plain">
                            "The app reads only your Discord name and ID. Your Eustress profile shows in Discord once you link. You can disconnect at any time from "
                            <a href="/settings">"Settings"</a>
                            ", which removes it."
                        </p>
                    </section>

                    <section class="dc-section">
                        <h2>"Coming next"</h2>
                        <p class="dc-plain">"These are not available yet."</p>
                        <ul class="dc-list">
                            <li>"Play a Space inside a Discord voice channel, as an Activity."</li>
                            <li>"Discord Rich Presence and invites in the Eustress Player."</li>
                        </ul>
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
    fn the_two_install_addresses_carry_the_app_and_the_right_install_kind() {
        let server = install_url(InstallKind::Server);
        let user = install_url(InstallKind::User);
        assert_eq!(
            server,
            "https://discord.com/oauth2/authorize?client_id=1571137497331662879&permissions=19456&integration_type=0&scope=bot+applications.commands"
        );
        assert_eq!(
            user,
            "https://discord.com/oauth2/authorize?client_id=1571137497331662879&integration_type=1&scope=applications.commands"
        );
        // A user install asks for no bot and no permissions.
        assert!(!user.contains("permissions") && !user.contains("scope=bot"));
    }

    #[test]
    fn the_footer_points_at_this_page_and_the_community_invite_lives_here() {
        let footer = include_str!("../components/footer.rs");
        assert!(!footer.contains("discord.gg"), "the footer sends people to /discord, not straight to the server");
        assert_eq!(footer.matches("href=\"/discord\"").count(), 2, "the icon and the Community link");
        let page = include_str!("discord.rs");
        assert!(page.contains("dc-join\" href=COMMUNITY_INVITE"), "the purple join button carries the invite");
        assert!(COMMUNITY_INVITE.starts_with("https://discord.gg/"));
    }

    #[test]
    fn the_page_copy_has_no_long_dashes_and_never_says_game_engine() {
        let source = include_str!("discord.rs");
        // The long dashes are spelled by code point so this file does not hold them itself.
        for dash in ['\u{2014}', '\u{2013}'] {
            assert!(!source.contains(dash), "discord.rs holds a long dash");
        }
        let phrase = format!("{} {}", "game", "engine");
        assert!(!source.to_lowercase().contains(&phrase));
    }
}
