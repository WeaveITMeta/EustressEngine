// =============================================================================
// Eustress Web - Eustress Player (/downloads/player)
// =============================================================================
// The Player's download page: the shared page (components/download_ui.rs) drawn for the Player.
// =============================================================================

use leptos::prelude::*;

use crate::api::release_notify::Product;
use crate::components::download_ui::ProductDownload;

#[component]
pub fn DownloadPlayerPage() -> impl IntoView {
    view! { <ProductDownload product=Product::Player /> }
}
