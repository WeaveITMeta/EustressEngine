// =============================================================================
// Eustress Web - Footer Component (Industrial Design)
// =============================================================================
// Global footer shown on all pages
// =============================================================================

use leptos::prelude::*;

// -----------------------------------------------------------------------------
// Footer Component
// -----------------------------------------------------------------------------

/// Industrial footer component.
#[component]
pub fn Footer() -> impl IntoView {
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
                        <a href="https://discord.gg/DGP9my8DYN" class="social-link" title="Discord">
                            <img src="/assets/icons/discord.svg" alt="Discord" />
                        </a>
                    </div>
                </div>
                
                // Links Columns
                <div class="footer-links-grid">
                    <div class="footer-link-col">
                        <h5 class="footer-col-title">"Product"</h5>
                        <a href="/gallery" class="footer-link">"Gallery"</a>
                        <a href="/learn" class="footer-link">"Learn"</a>
                        <a href="/bliss" class="footer-link">"Bliss"</a>
                        <a href="/premium" class="footer-link">"Premium"</a>
                    </div>
                    <div class="footer-link-col">
                        <h5 class="footer-col-title">"Community"</h5>
                        <a href="/groups" class="footer-link">"Groups"</a>
                        <a href="https://discord.gg/DGP9my8DYN" class="footer-link">"Discord"</a>
                        <a href="https://x.com/search?q=%23EustressEngine" target="_blank" rel="noopener" class="footer-link">"X"</a>
                        <a href="https://x.com/simbuilder" target="_blank" rel="noopener" class="footer-link">"Forums"</a>
                    </div>
                    <div class="footer-link-col">
                        <h5 class="footer-col-title">"Company"</h5>
                        <a href="/about" class="footer-link">"About"</a>
                        <a href="/careers" class="footer-link">"Careers"</a>
                        <a href="/contact" class="footer-link">"Contact"</a>
                        <a href="/press" class="footer-link">"Press Kit"</a>
                    </div>
                    <div class="footer-link-col">
                        <h5 class="footer-col-title">"Legal"</h5>
                        <a href="/terms" class="footer-link">"Terms"</a>
                        <a href="/privacy" class="footer-link">"Privacy"</a>
                        <a href="/cookies" class="footer-link">"Cookies"</a>
                        <a href="/acts" class="footer-link">"Acts"</a>
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
