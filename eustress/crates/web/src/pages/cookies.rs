// =============================================================================
// Eustress Web - Cookie Policy Page
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[component]
pub fn CookiesPage() -> impl IntoView {
    view! {
        <div class="page page-legal">
            <CentralNav active="".to_string() />
            
            <div class="legal-container">
                <div class="legal-header">
                    <h1>"Cookie Policy"</h1>
                    <p class="legal-updated">"Last updated: December 6, 2025"</p>
                </div>
                
                <div class="legal-content">
                    <section class="legal-section">
                        <h2>"What Are Cookies?"</h2>
                        <p>"Cookies are small text files stored on your device when you visit websites. They help us provide a better experience by remembering your preferences and login status."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"Cookies We Use"</h2>
                        
                        <h3>"Essential Cookies"</h3>
                        <p>"Required for the Service to function. These cannot be disabled."</p>
                        <ul>
                            <li>"Authentication tokens"</li>
                            <li>"Session management"</li>
                            <li>"Security features"</li>
                        </ul>
                        
                        <h3>"Functional Cookies"</h3>
                        <p>"Remember your preferences and settings."</p>
                        <ul>
                            <li>"Language preferences"</li>
                            <li>"Theme settings"</li>
                            <li>"UI customizations"</li>
                        </ul>
                        
                        <h3>"Analytics Cookies"</h3>
                        <p>"Help us understand how users interact with the Service."</p>
                        <ul>
                            <li>"Page views and navigation"</li>
                            <li>"Feature usage"</li>
                            <li>"Performance metrics"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"Third-Party Cookies"</h2>
                        <p>"We use cookies from:"</p>
                        <ul>
                            <li>"Discord - Authentication"</li>
                            <li>"Cloudflare - Security and performance"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"Managing Cookies"</h2>
                        <p>"You can control cookies through your browser settings. Note that disabling essential cookies may prevent the Service from functioning properly."</p>
                        <p>"Most browsers allow you to:"</p>
                        <ul>
                            <li>"View and delete cookies"</li>
                            <li>"Block cookies from specific sites"</li>
                            <li>"Block all third-party cookies"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"Local Storage"</h2>
                        <p>"We also use browser local storage for:"</p>
                        <ul>
                            <li>"Caching game assets for faster loading"</li>
                            <li>"Storing offline preferences"</li>
                            <li>"Draft content auto-save"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"Contact"</h2>
                        <p>"Questions about our cookie practices? Contact privacy@eustress.dev"</p>
                    </section>
                </div>
            </div>
            
            <Footer />
        </div>
    }
}
