// =============================================================================
// Eustress Web - Download Page (Industrial Design)
// =============================================================================
// OS-aware download buttons for Eustress Engine
// Showcases the engine with links to learn more
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Download page - platform-aware download buttons.
#[component]
pub fn DownloadPage() -> impl IntoView {
    // Detect OS from user agent (simplified - in production use wasm-bindgen)
    let detected_os = RwSignal::new("windows".to_string());
    
    view! {
        <div class="page page-download-industrial">
            <CentralNav active="".to_string() />
            
            // Background
            <div class="download-bg">
                <div class="download-grid-overlay"></div>
                <div class="download-glow glow-1"></div>
                <div class="download-glow glow-2"></div>
            </div>
            
            // Hero Section
            <section class="download-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"DOWNLOAD"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="download-title">"Eustress Engine"</h1>
                <p class="download-tagline">"The complete creation suite for building next-generation experiences"</p>
                
                // Version Info
                <div class="version-info">
                    <span class="version-badge">"v0.16.1"</span>
                    <span class="version-label">"Public Beta"</span>
                </div>
            </section>
            
            // Primary Download Section
            <section class="primary-download">
                <div class="download-card-main">
                    <div class="download-icon-large">
                        <img src="/assets/icons/eustress-gear.svg" alt="Eustress" />
                    </div>
                    
                    <h2>"Download for Your Platform"</h2>
                    <p class="download-desc">"Eustress Engine is available for Windows, macOS, and Linux"</p>
                    
                    // Platform Buttons
                    <div class="platform-buttons">
                        <a href="https://downloads.eustress.dev/windows/EustressEngine-Setup.exe" class="platform-btn windows">
                            <img src="/assets/icons/windows.svg" alt="Windows" />
                            <div class="btn-text">
                                <span class="btn-label">"Download for"</span>
                                <span class="btn-platform">"Windows"</span>
                            </div>
                            <span class="btn-size">"~150 MB"</span>
                        </a>
                        
                        <a href="https://downloads.eustress.dev/mac/EustressEngine.dmg" class="platform-btn macos">
                            <img src="/assets/icons/macos.svg" alt="macOS" />
                            <div class="btn-text">
                                <span class="btn-label">"Download for"</span>
                                <span class="btn-platform">"macOS"</span>
                            </div>
                            <span class="btn-size">"~160 MB"</span>
                        </a>
                        
                        <a href="https://downloads.eustress.dev/linux/EustressEngine.AppImage" class="platform-btn linux">
                            <img src="/assets/icons/linux.svg" alt="Linux" />
                            <div class="btn-text">
                                <span class="btn-label">"Download for"</span>
                                <span class="btn-platform">"Linux"</span>
                            </div>
                            <span class="btn-size">"~145 MB"</span>
                        </a>
                    </div>
                    
                </div>
            </section>
            
            // System Requirements
            <section class="requirements-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/settings.svg" alt="Requirements" class="section-icon" />
                    <h2>"System Requirements"</h2>
                </div>
                
                <div class="requirements-row">
                    <div class="req-card">
                        <h3>"Minimum"</h3>
                        <ul>
                            <li><strong>"OS:"</strong>" Windows 10 / macOS 12 / Ubuntu 22.04"</li>
                            <li><strong>"CPU:"</strong>" Intel i5 / AMD Ryzen 5"</li>
                            <li><strong>"RAM:"</strong>" 8 GB"</li>
                            <li><strong>"GPU:"</strong>" GTX 1060 / RX 580"</li>
                            <li><strong>"Storage:"</strong>" 10 GB SSD"</li>
                        </ul>
                    </div>
                    
                    <div class="req-card recommended">
                        <h3>"Recommended"</h3>
                        <ul>
                            <li><strong>"OS:"</strong>" Windows 11 / macOS 14 / Ubuntu 24.04"</li>
                            <li><strong>"CPU:"</strong>" Intel i7 / AMD Ryzen 7"</li>
                            <li><strong>"RAM:"</strong>" 16 GB"</li>
                            <li><strong>"GPU:"</strong>" RTX 3070 / RX 6800"</li>
                            <li><strong>"Storage:"</strong>" 20 GB NVMe SSD"</li>
                        </ul>
                    </div>
                </div>
            </section>
            
            // What is Eustress Section
            <section class="about-eustress">
                <div class="section-header-industrial">
                    <img src="/assets/icons/sparkles.svg" alt="About" class="section-icon" />
                    <h2>"What is Eustress Engine?"</h2>
                </div>
                
                <div class="about-content">
                    <p class="about-intro">
                        "Eustress Engine is a next-generation game engine built entirely in Rust, 
                        designed to make creation accessible to everyone while delivering 
                        professional-grade performance and features."
                    </p>
                    
                    <div class="features-highlight">
                        <div class="feature-item">
                            <img src="/assets/icons/rocket.svg" alt="Fast" />
                            <div>
                                <h4>"10x Faster Development"</h4>
                                <p>"Hot reload, Soul scripting, and instant preview let you iterate at the speed of thought"</p>
                            </div>
                        </div>
                        
                        <div class="feature-item">
                            <img src="/assets/icons/code.svg" alt="Rust" />
                            <div>
                                <h4>"100% Rust Powered"</h4>
                                <p>"Memory-safe, blazing fast, with zero garbage collection pauses"</p>
                            </div>
                        </div>
                        
                        <div class="feature-item">
                            <img src="/assets/icons/users.svg" alt="Multiplayer" />
                            <div>
                                <h4>"Built-in Multiplayer"</h4>
                                <p>"Networking is a first-class citizen with seamless multiplayer"</p>
                            </div>
                        </div>
                        
                        <div class="feature-item">
                            <img src="/assets/icons/cube.svg" alt="3D" />
                            <div>
                                <h4>"Professional 3D Tools"</h4>
                                <p>"Support for meshes, point clouds, CAD files, and 10M+ instances"</p>
                            </div>
                        </div>
                    </div>
                </div>
                
                <a href="/learn" class="btn-learn-more">
                    "Learn More About Eustress"
                    <img src="/assets/icons/arrow-right.svg" alt="Arrow" />
                </a>
            </section>
            
            // Quick Links
            <section class="quick-links">
                <div class="links-grid">
                    <a href="/learn" class="quick-link-card">
                        <img src="/assets/icons/book.svg" alt="Learn" />
                        <h3>"Documentation"</h3>
                        <p>"Tutorials, guides, and API reference"</p>
                    </a>
                    
                    <a href="/community" class="quick-link-card">
                        <img src="/assets/icons/users.svg" alt="Community" />
                        <h3>"Community"</h3>
                        <p>"Join creators and players worldwide"</p>
                    </a>
                    
                    <a href="/gallery" class="quick-link-card">
                        <img src="/assets/icons/gamepad.svg" alt="Gallery" />
                        <h3>"Gallery"</h3>
                        <p>"Explore spaces built with Eustress"</p>
                    </a>
                    
                    <a href="https://discord.gg/DGP9my8DYN" class="quick-link-card">
                        <img src="/assets/icons/discord.svg" alt="Discord" />
                        <h3>"Discord"</h3>
                        <p>"Get help and chat with the team"</p>
                    </a>
                </div>
            </section>
            
            <Footer />
        </div>
    }
}
