// =============================================================================
// Eustress Web - Eustress Studio (/download)
// =============================================================================
// Studio's download page: the shared page (components/download_ui.rs) drawn for Studio. Its release state comes from
// downloads.eustress.dev/latest.json; with none, the page says the first release is coming soon and offers the email sign-up
// instead of a download link that goes nowhere. Downloads need a signed-in account, as they always have.
// =============================================================================

use leptos::prelude::*;

use crate::api::release_notify::Product;
use crate::components::download_ui::ProductDownload;

#[component]
pub fn DownloadPage() -> impl IntoView {
    view! { <ProductDownload product=Product::Studio /> }
}
