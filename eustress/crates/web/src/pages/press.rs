// =============================================================================
// Eustress Web - Press Kit Page
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[component]
pub fn PressPage() -> impl IntoView {
    view! {
        <div class="page page-press">
            <CentralNav active="".to_string() />
            
            <div class="press-bg">
                <div class="press-grid-overlay"></div>
            </div>
            
            <div class="press-container">
                <div class="press-header">
                    <div class="hero-header">
                        <div class="header-line"></div>
                        <span class="header-tag">"PRESS KIT"</span>
                        <div class="header-line"></div>
                    </div>
                    <h1 class="press-title">"Press & Media Resources"</h1>
                    <p class="press-subtitle">"Official assets and information for press coverage"</p>
                </div>
                
                // Quick Facts
                <section class="press-section">
                    <h2>"Quick Facts"</h2>
                    <div class="facts-grid">
                        <div class="fact-item">
                            <span class="fact-label">"Product"</span>
                            <span class="fact-value">"Eustress Engine"</span>
                        </div>
                        <div class="fact-item">
                            <span class="fact-label">"Developer"</span>
                            <span class="fact-value">"@Simbuilder"</span>
                        </div>
                        <div class="fact-item">
                            <span class="fact-label">"Platform"</span>
                            <span class="fact-value">"Windows, macOS, Linux, Web, VR"</span>
                        </div>
                        <div class="fact-item">
                            <span class="fact-label">"Engine"</span>
                            <span class="fact-value">"100% Rust (Bevy-based)"</span>
                        </div>
                        <div class="fact-item">
                            <span class="fact-label">"Price"</span>
                            <span class="fact-value">"Free (Premium subscriptions available)"</span>
                        </div>
                        <div class="fact-item">
                            <span class="fact-label">"Website"</span>
                            <span class="fact-value">"eustress.dev"</span>
                        </div>
                    </div>
                </section>
                
                // Description
                <section class="press-section">
                    <h2>"About Eustress Engine"</h2>
                    <div class="description-card">
                        <h3>"Short Description"</h3>
                        <p class="description-text">
                            "Eustress Engine is a next-generation game engine built 100% in Rust, offering unprecedented performance, memory safety, and developer experience—all while remaining free forever."
                        </p>
                        
                        <h3>"Long Description"</h3>
                        <p class="description-text">
                            "Eustress is a complete ecosystem for creating, sharing, and playing interactive 3D experiences. Built from the ground up in Rust by @Simbuilder with AI assistance, Eustress delivers professional-grade tools without the overhead of traditional game engines. With zero garbage collection pauses, support for 10M+ entity instances, and built-in multiplayer networking, Eustress is designed for creators who demand performance without compromise. The platform includes Eustress Engine for game development and Eustress Player for discovering community-created experiences."
                        </p>
                    </div>
                </section>
                
                // Logo Assets
                <section class="press-section">
                    <h2>"Logo & Brand Assets"</h2>
                    <p class="section-note">"Please use these official assets when featuring Eustress Engine. Do not modify, distort, or recolor the logos."</p>
                    
                    <div class="assets-grid">
                        <div class="asset-card">
                            <div class="asset-preview dark">
                                <img src="/assets/logo.svg" alt="Eustress Logo" />
                            </div>
                            <div class="asset-info">
                                <h3>"Primary Logo"</h3>
                                <p>"Full logo with wordmark. Use on dark backgrounds."</p>
                                <div class="asset-downloads">
                                    <a href="/assets/press/eustress-logo.svg" download class="download-btn">"SVG"</a>
                                    <a href="/assets/press/eustress-logo.png" download class="download-btn">"PNG"</a>
                                </div>
                            </div>
                        </div>
                        
                        <div class="asset-card">
                            <div class="asset-preview dark">
                                <img src="/assets/icons/eustress-gear.svg" alt="Eustress Icon" />
                            </div>
                            <div class="asset-info">
                                <h3>"Icon / Mark"</h3>
                                <p>"Gear icon for app icons and small spaces."</p>
                                <div class="asset-downloads">
                                    <a href="/assets/press/eustress-icon.svg" download class="download-btn">"SVG"</a>
                                    <a href="/assets/press/eustress-icon.png" download class="download-btn">"PNG"</a>
                                </div>
                            </div>
                        </div>
                        
                        <div class="asset-card">
                            <div class="asset-preview light">
                                <img src="/assets/logo.svg" alt="Eustress Logo Light" />
                            </div>
                            <div class="asset-info">
                                <h3>"Logo (Light Background)"</h3>
                                <p>"Inverted version for light backgrounds."</p>
                                <div class="asset-downloads">
                                    <a href="/assets/press/eustress-logo-dark.svg" download class="download-btn">"SVG"</a>
                                    <a href="/assets/press/eustress-logo-dark.png" download class="download-btn">"PNG"</a>
                                </div>
                            </div>
                        </div>
                        
                        <div class="asset-card">
                            <div class="asset-preview dark">
                                <img src="/assets/icons/bliss.svg" alt="Bliss Currency" />
                            </div>
                            <div class="asset-info">
                                <h3>"Bliss Currency"</h3>
                                <p>"Official Bliss currency icon."</p>
                                <div class="asset-downloads">
                                    <a href="/assets/press/bliss-icon.svg" download class="download-btn">"SVG"</a>
                                    <a href="/assets/press/bliss-icon.png" download class="download-btn">"PNG"</a>
                                </div>
                            </div>
                        </div>
                    </div>
                    
                    <div class="download-all">
                        <a href="/assets/press/eustress-press-kit.zip" download class="btn-primary-steel">
                            "Download Complete Press Kit"
                            <span class="btn-icon">"↓"</span>
                        </a>
                    </div>
                </section>
                
                // Brand Colors
                <section class="press-section">
                    <h2>"Brand Colors"</h2>
                    <div class="colors-grid">
                        <div class="color-swatch">
                            <div class="swatch" style="background: #0a0a12;"></div>
                            <span class="color-name">"Deep Space"</span>
                            <span class="color-hex">"#0a0a12"</span>
                        </div>
                        <div class="color-swatch">
                            <div class="swatch" style="background: #8090a0;"></div>
                            <span class="color-name">"Steel"</span>
                            <span class="color-hex">"#8090a0"</span>
                        </div>
                        <div class="color-swatch">
                            <div class="swatch" style="background: #4a5260;"></div>
                            <span class="color-name">"Gunmetal"</span>
                            <span class="color-hex">"#4a5260"</span>
                        </div>
                        <div class="color-swatch">
                            <div class="swatch" style="background: #ffd700;"></div>
                            <span class="color-name">"Bliss Gold"</span>
                            <span class="color-hex">"#ffd700"</span>
                        </div>
                    </div>
                </section>
                
                // Screenshots
                <section class="press-section">
                    <h2>"Screenshots"</h2>
                    <p class="section-note">"High-resolution screenshots of Eustress Engine and games."</p>
                    <div class="screenshots-placeholder">
                        <p>"Screenshots coming soon"</p>
                    </div>
                </section>
                
                // Contact
                <section class="press-section">
                    <h2>"Press Contact"</h2>
                    <div class="press-contact-card">
                        <p>"For press inquiries, interviews, and review copies:"</p>
                        <a href="mailto:press@eustress.dev" class="contact-email">"press@eustress.dev"</a>
                        <p class="contact-social">
                            "Or reach out on X: "
                            <a href="https://x.com/simbuilder" target="_blank" rel="noopener">"@Simbuilder"</a>
                        </p>
                    </div>
                </section>
            </div>
            
            <Footer />
        </div>
    }
}
