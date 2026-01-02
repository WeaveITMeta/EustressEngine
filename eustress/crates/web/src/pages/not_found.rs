// =============================================================================
// Eustress Web - 404 Not Found Page
// =============================================================================

use leptos::prelude::*;

/// 404 Not Found page.
#[component]
pub fn NotFoundPage() -> impl IntoView {
    view! {
        <div class="page page-not-found">
            <div class="not-found-content">
                <span class="not-found-code">"404"</span>
                <h1>"Page Not Found"</h1>
                <p>"The page you're looking for doesn't exist or has been moved."</p>
                <a href="/" class="btn btn-primary">
                    "Go Home"
                </a>
            </div>
        </div>
    }
}
