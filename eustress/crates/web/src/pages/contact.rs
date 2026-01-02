// =============================================================================
// Eustress Web - Contact Page
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[component]
pub fn ContactPage() -> impl IntoView {
    view! {
        <div class="page page-contact">
            <CentralNav active="".to_string() />
            
            <div class="contact-bg">
                <div class="contact-grid-overlay"></div>
            </div>
            
            <div class="contact-container">
                <div class="contact-header">
                    <div class="hero-header">
                        <div class="header-line"></div>
                        <span class="header-tag">"CONTACT"</span>
                        <div class="header-line"></div>
                    </div>
                    <h1 class="contact-title">"Get in Touch"</h1>
                    <p class="contact-subtitle">"Questions, feedback, or just want to say hi?"</p>
                </div>
                
                <div class="contact-main">
                    <div class="contact-card primary">
                        <div class="contact-icon">
                            <img src="/assets/icons/twitter-x.svg" alt="X" />
                        </div>
                        <h2>"@Simbuilder on X"</h2>
                        <p>"The fastest way to reach us. DMs are open for questions, feedback, bug reports, and feature requests."</p>
                        <a href="https://x.com/simbuilder" target="_blank" rel="noopener" class="btn-primary-steel">
                            "Message on X"
                            <span class="btn-icon">"→"</span>
                        </a>
                    </div>
                </div>
                
                <div class="contact-grid">
                    <div class="contact-card">
                        <div class="contact-icon small">
                            <img src="/assets/icons/discord.svg" alt="Discord" />
                        </div>
                        <h3>"Discord Community"</h3>
                        <p>"Join our community for help, discussions, and to connect with other creators."</p>
                        <a href="https://discord.gg/DGP9my8DYN" target="_blank" rel="noopener" class="contact-link">
                            "Join Discord"
                            <span>"→"</span>
                        </a>
                    </div>
                    
                    <div class="contact-card">
                        <div class="contact-icon small">
                            <img src="/assets/icons/github.svg" alt="GitHub" />
                        </div>
                        <h3>"GitHub Discussions"</h3>
                        <p>"Post in our GitHub discussions for technical support and development questions."</p>
                        <a href="https://github.com/eustressengine/eustress/discussions" target="_blank" rel="noopener" class="contact-link">
                            "Visit GitHub"
                            <span>"→"</span>
                        </a>
                    </div>
                    
                    <div class="contact-card">
                        <div class="contact-icon small">
                            <img src="/assets/icons/mail.svg" alt="Email" />
                        </div>
                        <h3>"Email"</h3>
                        <p>"For business inquiries, legal matters, or press requests only."</p>
                        <div class="email-list">
                            <a href="mailto:business@eustress.dev" class="contact-link">"business@eustress.dev"</a>
                            <a href="mailto:legal@eustress.dev" class="contact-link">"legal@eustress.dev"</a>
                            <a href="mailto:press@eustress.dev" class="contact-link">"press@eustress.dev"</a>
                            <a href="mailto:privacy@eustress.dev" class="contact-link">"privacy@eustress.dev"</a>
                        </div>
                    </div>
                    
                    <div class="contact-card">
                        <div class="contact-icon small">
                            <img src="/assets/icons/shield.svg" alt="DMCA" />
                        </div>
                        <h3>"DMCA & Copyright"</h3>
                        <p>"For copyright infringement notices and takedown requests."</p>
                        <a href="mailto:dmca@eustress.dev" class="contact-link">"dmca@eustress.dev"</a>
                    </div>
                </div>
                
                <div class="response-note">
                    <p>"We typically respond within 24-48 hours. For urgent issues, X DMs are fastest."</p>
                </div>
            </div>
            
            <Footer />
        </div>
    }
}
