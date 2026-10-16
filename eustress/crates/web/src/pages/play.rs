// =============================================================================
// Eustress Web - /play/:id
// =============================================================================
// The address people already have for "play this Space". The page itself is gone: a Space's page is /simulation/:id, and every Play
// button there opens the Play chooser in place (components/play_chooser.rs). This route sends the visitor to that page with `?play=1`,
// which opens the chooser once the Space has loaded, so an old link still lands on Play. An id that is not an id goes to the Gallery.
// =============================================================================

use leptos::prelude::*;
use leptos_router::components::Redirect;
use leptos_router::NavigateOptions;

/// Where `/play/<id>` goes: the Space's page with the chooser asked for, or the Gallery when `id` is not one the Worker issues
/// (letters, digits and hyphens only), so nothing typed in the address can reach the redirect target.
pub fn play_redirect(id: &str) -> String {
    let ok = !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if ok { format!("/simulation/{id}?play=1") } else { "/gallery".to_string() }
}

/// The address does not stay in the history: Back from the Space's page goes to where the visitor came from.
fn replace() -> NavigateOptions {
    NavigateOptions { replace: true, ..Default::default() }
}

#[component]
pub fn PlayPage() -> impl IntoView {
    let params = leptos_router::hooks::use_params_map();
    move || {
        let path = play_redirect(&params.read().get("id").unwrap_or_default());
        view! { <Redirect path=path options=replace() /> }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_play_link_lands_on_the_space_with_the_chooser_asked_for() {
        assert_eq!(play_redirect("07d084d8-9098-4192-bb48-84d2c428db36"), "/simulation/07d084d8-9098-4192-bb48-84d2c428db36?play=1");
    }

    #[test]
    fn an_id_that_is_not_an_id_never_reaches_the_redirect() {
        for bad in ["", "a/b", "a?x=1", "a b", "..", "a%2Fb", "<script>", "//evil.example"] {
            assert_eq!(play_redirect(bad), "/gallery", "{bad:?}");
        }
        assert_eq!(play_redirect(&"a".repeat(65)), "/gallery");
    }
}
