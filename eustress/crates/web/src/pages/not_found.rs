// =============================================================================
// Eustress Web - 404 Not Found Page
// =============================================================================
// The router's fallback (app.rs) and the page behind `/economics-draft` in a
// build without the draft, so that address answers as any address the site does
// not have. The prerender writes this page to `404.html`, which the host serves
// with a 404 status for an address that is no file and no rewrite.
// =============================================================================

use leptos::prelude::*;

/// 404 Not Found page.
#[component]
pub fn NotFoundPage() -> impl IntoView {
    view! {
        <main class="page page-not-found" data-not-found="true">
            <div class="not-found-content">
                <span class="not-found-code">"404"</span>
                <h1>"Page Not Found"</h1>
                <p>"The page you're looking for doesn't exist or has been moved."</p>
                <a href="/" class="btn btn-primary">
                    "Go Home"
                </a>
                <p class="not-found-links">
                    <a href="/learn">"Read the docs"</a>
                    " or "
                    <a href="/gallery">"browse the Gallery"</a>
                    "."
                </p>
            </div>
        </main>
    }
}
