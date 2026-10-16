// =============================================================================
// Eustress Web - Player Download
// =============================================================================
// The latest release of each product, read from its release manifest so the pages follow each release. The download pages
// (components/download_ui.rs) and the Play chooser read them through these hooks.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::releases::{fetch_player_release, fetch_studio_release, ReleaseState};

/// The Player's latest release, looked up once in the browser.
///
/// An effect rather than a fetch at construction: the download pages are
/// prerendered, and the static copy keeps `Checking`, which reads correctly
/// without scripts.
pub fn use_player_release() -> RwSignal<ReleaseState> {
    let state = RwSignal::new(ReleaseState::Checking);
    Effect::new(move |_| {
        spawn_local(async move {
            state.set(fetch_player_release().await);
        });
    });
    state
}

/// Studio's latest release, looked up once in the browser, for the pages that say whether there is one.
pub fn use_studio_release() -> RwSignal<ReleaseState> {
    let state = RwSignal::new(ReleaseState::Checking);
    Effect::new(move |_| {
        spawn_local(async move {
            state.set(fetch_studio_release().await);
        });
    });
    state
}
