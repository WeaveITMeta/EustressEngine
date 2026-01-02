// =============================================================================
// Eustress Web - Careers Page
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[component]
pub fn CareersPage() -> impl IntoView {
    view! {
        <div class="page page-careers">
            <CentralNav active="".to_string() />
            
            <div class="careers-bg">
                <div class="careers-grid-overlay"></div>
                <div class="careers-glow glow-1"></div>
            </div>
            
            <div class="careers-container">
                <div class="careers-header">
                    <div class="hero-header">
                        <div class="header-line"></div>
                        <span class="header-tag">"CAREERS"</span>
                        <div class="header-line"></div>
                    </div>
                    <h1 class="careers-title">"Built by One, Powered by AI"</h1>
                    <p class="careers-subtitle">"The future of game development, crafted differently"</p>
                </div>
                
                <section class="about-team">
                    <div class="team-card">
                        <div class="founder-section">
                            <a href="https://x.com/simbuilder" target="_blank" rel="noopener" class="founder-avatar-link">
                                <img src="/assets/team/McKaleOlson.JPG" alt="Simbuilder" class="founder-avatar-img" />
                            </a>
                            <div class="founder-info">
                                <h2>"@Simbuilder"</h2>
                                <p class="founder-role">"Founder & Solo Developer"</p>
                                <a href="https://x.com/simbuilder" target="_blank" rel="noopener" class="social-link">
                                    <img src="/assets/icons/twitter-x.svg" alt="X" class="x-icon" />
                                </a>
                            </div>
                        </div>
                        
                        <div class="team-description">
                            <p>
                                "Eustress Engine is built and operated by "<strong>"@Simbuilder"</strong>
                                " in collaboration with AI assistants. This unique approach allows us to move fast, 
                                stay lean, and focus entirely on building the best game engine possible."
                            </p>
                            <p>
                                "By leveraging AI for development, documentation, and operations, we can deliver 
                                a professional-grade engine without the overhead of a traditional studio. 
                                Every feature, every line of code, every design decision is made with intention."
                            </p>
                        </div>
                    </div>
                </section>
                
                <section class="ai-section">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/sparkles.svg" alt="AI" class="section-icon" />
                        <h2>"AI-Assisted Development"</h2>
                    </div>
                    
                    <div class="ai-grid">
                        <div class="ai-card">
                            <h3>"Code Generation"</h3>
                            <p>"AI assists in writing, reviewing, and optimizing Rust code for maximum performance and safety."</p>
                        </div>
                        <div class="ai-card">
                            <h3>"Documentation"</h3>
                            <p>"Comprehensive docs, tutorials, and API references generated and maintained with AI assistance."</p>
                        </div>
                        <div class="ai-card">
                            <h3>"Design & UX"</h3>
                            <p>"UI/UX decisions informed by AI analysis of best practices from leading game engines."</p>
                        </div>
                        <div class="ai-card">
                            <h3>"Community Support"</h3>
                            <p>"AI helps manage community questions, bug reports, and feature requests efficiently."</p>
                        </div>
                    </div>
                </section>
                
                <section class="hiring-section">
                    <div class="hiring-card">
                        <div class="hiring-status">
                            <span class="status-indicator not-hiring"></span>
                            <span class="status-text">"Not Currently Hiring"</span>
                        </div>
                        <h2>"Join Us in the Future"</h2>
                        <p>
                            "We're not hiring at the moment, but that may change as Eustress grows. 
                            If you're passionate about Rust, game engines, or AI-assisted development, 
                            follow "<a href="https://x.com/simbuilder" target="_blank" rel="noopener">"@Simbuilder"</a>
                            " for updates."
                        </p>
                        <div class="hiring-cta">
                            <a href="https://discord.gg/DGP9my8DYN" class="btn-secondary-steel">
                                "Join Our Discord"
                            </a>
                        </div>
                    </div>
                </section>
                
                <section class="values-section">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/rocket.svg" alt="Values" class="section-icon" />
                        <h2>"Our Philosophy"</h2>
                    </div>
                    
                    <div class="values-grid">
                        <div class="value-item">
                            <span class="value-icon">"🦀"</span>
                            <h4>"Rust First"</h4>
                            <p>"Memory safety and performance without compromise"</p>
                        </div>
                        <div class="value-item">
                            <span class="value-icon">"🚀"</span>
                            <h4>"Ship Fast"</h4>
                            <p>"Iterate quickly, learn from users, improve constantly"</p>
                        </div>
                        <div class="value-item">
                            <span class="value-icon">"🤖"</span>
                            <h4>"AI-Augmented"</h4>
                            <p>"Embrace AI as a force multiplier, not a replacement"</p>
                        </div>
                        <div class="value-item">
                            <span class="value-icon">"💎"</span>
                            <h4>"Quality Over Quantity"</h4>
                            <p>"Every feature polished, every bug squashed"</p>
                        </div>
                    </div>
                </section>
            </div>
            
            <Footer />
        </div>
    }
}
