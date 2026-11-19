// =============================================================================
// Eustress Web - Footer Component (Industrial Design)
// =============================================================================
// Global footer shown on all pages
// =============================================================================

use leptos::prelude::*;

use crate::state::AppState;

// -----------------------------------------------------------------------------
// Footer Component
// -----------------------------------------------------------------------------

#[cfg(not(feature = "ssr"))]
thread_local! {
    /// Whether an offering is live, once the Worker has answered: asked once per visit, not on every page.
    static OFFERING_LIVE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether the footer shows Invest: only after the Worker says an offering is live (api/offering.rs). The prerendered page and
/// every visit before that answer have no Invest link, so nothing about investing is said in public until an offering is.
fn offering_live() -> RwSignal<bool> {
    let live = RwSignal::new(false);
    #[cfg(not(feature = "ssr"))]
    {
        if let Some(known) = OFFERING_LIVE.with(|c| c.get()) {
            live.set(known);
        } else if let Some(state) = use_context::<AppState>() {
            let api_url = state.api_url.clone();
            leptos::task::spawn_local(async move {
                let client = crate::api::ApiClient::new(&api_url);
                let yes = crate::api::offering::live_offering(&client).await.is_some();
                OFFERING_LIVE.with(|c| c.set(Some(yes)));
                live.try_set(yes);
            });
        }
    }
    #[cfg(feature = "ssr")]
    let _ = use_context::<AppState>();
    live
}

/// Industrial footer component.
#[component]
pub fn Footer() -> impl IntoView {
    let invest = offering_live();
    view! {
        <footer class="footer-industrial">
            <div class="footer-grid-bg"></div>
            
            <div class="footer-main">
                // Brand Column
                <div class="footer-brand-col">
                    <a href="/" class="footer-logo-link">
                        <img src="/assets/logo.svg" alt="Eustress Engine" class="footer-logo-img" />
                    </a>
                    <p class="footer-tagline">"Creation at the speed of thought."</p>
                    <div class="footer-social-row">
                        <a href="https://twitter.com/eustressengine" class="social-link" title="X">
                            <img src="/assets/icons/twitter-x.svg" alt="X" />
                        </a>
                        <a href="/discord" class="social-link" title="Discord">
                            <img src="/assets/icons/discord.svg" alt="Discord" />
                        </a>
                        <a href="https://github.com/WeaveITMeta/EustressEngine" class="social-link" title="GitHub">
                            <img src="/assets/icons/github.svg" alt="GitHub" />
                        </a>
                    </div>
                </div>
                
                // Links Columns
                <div class="footer-links-grid">
                    <div class="footer-link-col">
                        <h5 class="footer-col-title">"Product"</h5>
                        <a href="/gallery" class="footer-link">"Gallery"</a>
                        <a href="/learn" class="footer-link">"Learn"</a>
                        <a href="/currency" class="footer-link">"Currency"</a>
                        <a href="/premium" class="footer-link">"Premium"</a>
                        <a href="/sponsor" class="footer-link">"Sponsor"</a>
                        <a href="/license" class="footer-link">"License"</a>
                    </div>
                    <div class="footer-link-col">
                        <h5 class="footer-col-title">"Community"</h5>
                        <a href="/discord" class="footer-link">"Discord"</a>
                        <a href="/slack" class="footer-link">"Slack"</a>
                        <a href="https://x.com/search?q=%23EustressEngine" target="_blank" rel="noopener" class="footer-link">"X"</a>
                        <a href="https://x.com/simbuilder" target="_blank" rel="noopener" class="footer-link">"Forums"</a>
                        <a href="/kyc" class="footer-link">"KYC"</a>
                        <a href="/grants" class="footer-link">"Grants"</a>
                    </div>
                    <div class="footer-link-col">
                        <h5 class="footer-col-title">"Company"</h5>
                        <a href="/about" class="footer-link">"About"</a>
                        <a href="/blog" class="footer-link">"Blog"</a>
                        <a href="/careers" class="footer-link">"Careers"</a>
                        <a href="/contact" class="footer-link">"Contact"</a>
                        <a href="/press" class="footer-link">"Press Kit"</a>
                        {move || invest.get().then(|| view! { <a href="/invest" class="footer-link">"Invest"</a> })}
                    </div>
                    <div class="footer-link-col">
                        <h5 class="footer-col-title">"Legal"</h5>
                        <a href="/terms" class="footer-link">"Terms"</a>
                        <a href="/privacy" class="footer-link">"Privacy"</a>
                        <a href="/cookies" class="footer-link">"Cookies"</a>
                        <a href="/acts" class="footer-link">"Acts"</a>
                        <a href="/quality-gate" class="footer-link">"Quality Gate"</a>
                        <a href="/support" class="footer-link">"Support"</a>
                    </div>
                </div>
            </div>
            
            <div class="footer-divider"></div>
            
            <div class="footer-bottom-bar">
                <p class="footer-copyright">"© 2025 Eustress Engine. All rights reserved."</p>
                <div class="footer-status">
                    <span class="status-dot"></span>
                    "All systems operational"
                </div>
            </div>
        </footer>
    }
}
