// =============================================================================
// Eustress Web - Discord Linked Roles (/discord/roles)
// =============================================================================
// The Linked Roles Verification URL. A member who opens "Connect" from a
// server's Linked Roles menu lands here. Signed in, they link Discord (the same
// call as Settings) and Discord checks the server's role requirements against
// Eustress. A small sessionStorage flag tells /auth/discord/callback to say
// "go back to Discord" instead of pointing at Settings.
// =============================================================================

use gloo_storage::Storage;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{begin_discord_link, discord_failure_sentence};
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};

/// The sessionStorage key and value that mark "the link began from the Linked Roles page".
pub const RETURN_KEY: &str = "eustress.discord.return";
pub const RETURN_ROLES: &str = "roles";

/// Marks that the link about to begin came from /discord/roles.
pub fn mark_roles_return() {
    let _ = gloo_storage::SessionStorage::set(RETURN_KEY, RETURN_ROLES);
}

/// True if the link came from /discord/roles. The mark is cleared either way.
pub fn take_roles_return() -> bool {
    let value = gloo_storage::SessionStorage::get::<String>(RETURN_KEY).ok();
    gloo_storage::SessionStorage::delete(RETURN_KEY);
    is_roles_return(value.as_deref())
}

/// Whether a stored flag value means "came from the Linked Roles page".
pub fn is_roles_return(value: Option<&str>) -> bool {
    value == Some(RETURN_ROLES)
}

#[component]
pub fn DiscordRolesPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());

    let connect = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(String::new());
        mark_roles_return();
        spawn_local(async move {
            match begin_discord_link(&api_url.get_value()).await {
                Ok(url) => {
                    // Stays busy: the page is leaving for Discord.
                    let opened = web_sys::window().map(|w| w.location().assign(&url).is_ok()).unwrap_or(false);
                    if !opened {
                        busy.set(false);
                        let _ = take_roles_return();
                        error.set("Your browser did not open Discord. Try again.".to_string());
                    }
                }
                Err(e) => {
                    busy.set(false);
                    let _ = take_roles_return();
                    error.set(discord_failure_sentence(&e));
                }
            }
        });
    };

    view! {
        <div class="page g-page dc-page">
            <CentralNav active="".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main">
                <div class="dc-wrap dc-roles">
                    {move || match auth.get() {
                        AuthState::Unknown => view! {
                            <section class="dc-card g-glass"><h1 class="dc-card-title">"One moment"</h1></section>
                        }.into_any(),
                        AuthState::Unauthenticated => view! {
                            <section class="dc-card g-glass">
                                <h1 class="dc-card-title">"Sign in to connect your Eustress roles"</h1>
                                <p>"Discord checks your roles against your Eustress account. Sign in, then come back to this page from the server's Linked Roles menu."</p>
                                <div class="dc-actions"><a class="g-btn g-btn-primary g-btn-lg" href="/login">"Sign in"</a></div>
                            </section>
                        }.into_any(),
                        _ => view! {
                            <section class="dc-card g-glass">
                                <h1 class="dc-card-title">"Connect your Eustress roles on Discord"</h1>
                                <p>"This links your Discord account to your Eustress account, so the server can check roles such as Verified Eustress account or Creator. Eustress reads only your Discord name and ID."</p>
                                <div class="dc-actions">
                                    <button class="g-btn g-btn-primary g-btn-lg" disabled=move || busy.get() on:click=connect>
                                        {move || if busy.get() { "Opening Discord..." } else { "Connect with Discord" }}
                                    </button>
                                </div>
                                <Show when=move || !error.get().is_empty()>
                                    <p class="dc-error" role="alert">{move || error.get()}</p>
                                </Show>
                            </section>
                        }.into_any(),
                    }}
                </div>
            </main>
            <Footer />
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_roles_mark_means_the_link_came_from_the_roles_page() {
        assert!(is_roles_return(Some("roles")));
        assert!(!is_roles_return(Some("settings")));
        assert!(!is_roles_return(Some("")));
        assert!(!is_roles_return(None));
    }
}
