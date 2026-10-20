// =============================================================================
// Eustress Web - Eustress Player (/downloads/player)
// =============================================================================
// The Player's download page: the shared page (components/download_ui.rs) drawn for the Player.
//
// Production reads https://downloads.eustress.dev/player/latest.json. The staging
// website — the host that talks to api-staging.eustress.dev, the same API a Player
// link names with ?api=staging — reads /staging/player/latest.json instead. The
// choice is in api::releases, from the same host list as state::api_base.
// =============================================================================

use leptos::prelude::*;

use crate::api::release_notify::Product;
use crate::components::download_ui::ProductDownload;

#[component]
pub fn DownloadPlayerPage() -> impl IntoView {
    view! { <ProductDownload product=Product::Player /> }
}
