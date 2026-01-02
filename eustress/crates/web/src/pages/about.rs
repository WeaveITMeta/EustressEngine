// =============================================================================
// Eustress Web - About Page (Industrial Design)
// =============================================================================
// Company and product overview - What is Eustress, Engine vs Player
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// About page - What is Eustress.
#[component]
pub fn AboutPage() -> impl IntoView {
    view! {
        <div class="page page-about-industrial">
            <CentralNav active="".to_string() />
            
            // Background
            <div class="about-bg">
                <div class="about-grid-overlay"></div>
                <div class="about-glow glow-1"></div>
                <div class="about-glow glow-2"></div>
            </div>
            
            // Hero Section
            <section class="about-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"ABOUT"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="about-title">"What is Eustress?"</h1>
                <p class="about-tagline">"The next generation of game creation and play"</p>
            </section>
            
            // Mission Statement
            <section class="mission-section">
                <div class="mission-card">
                    <p class="mission-text">
                        "Eustress is a complete ecosystem for creating, sharing, and playing interactive 3D experiences. 
                        Built from the ground up in "<strong>"100% Rust"</strong>", we deliver unprecedented performance, 
                        memory safety, and developer experience—all while remaining "<strong>"free forever"</strong>"."
                    </p>
                </div>
            </section>
            
            // Two Products Section
            <section class="products-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/cube.svg" alt="Products" class="section-icon" />
                    <h2>"Two Products, One Vision"</h2>
                </div>
                
                <div class="products-grid">
                    // Eustress Engine
                    <div class="product-card studio">
                        <div class="product-icon">
                            <img src="/assets/icons/eustress-gear.svg" alt="Eustress Engine" />
                        </div>
                        <h3>"Eustress Engine"</h3>
                        <p class="product-subtitle">"The Engine"</p>
                        <p class="product-desc">
                            "A professional-grade game engine and creation suite. Build stunning 3D worlds, 
                            script gameplay logic, design UI, and publish to multiple platforms—all from one tool."
                        </p>
                        <ul class="product-features">
                            <li>"Visual editor with hot reload"</li>
                            <li>"Rust & Soul scripting"</li>
                            <li>"Built-in multiplayer networking"</li>
                            <li>"One-click publishing"</li>
                        </ul>
                        <a href="/download" class="product-cta">"Download Studio"</a>
                    </div>
                    
                    // Eustress Player
                    <div class="product-card player">
                        <div class="product-icon">
                            <img src="/assets/icons/gamepad.svg" alt="Eustress Player" />
                        </div>
                        <h3>"Eustress Player"</h3>
                        <p class="product-subtitle">"The Platform"</p>
                        <p class="product-desc">
                            "Discover and play thousands of community-created experiences. Jump into games instantly, 
                            connect with friends, and explore endless creativity."
                        </p>
                        <ul class="product-features">
                            <li>"Instant play"</li>
                            <li>"Cross-platform multiplayer"</li>
                            <li>"Social features & friends"</li>
                            <li>"Curated game discovery"</li>
                        </ul>
                        <a href="/gallery" class="product-cta">"Explore"</a>
                    </div>
                </div>
            </section>
            
            // Why Eustress Section
            <section class="why-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/rocket.svg" alt="Why" class="section-icon" />
                    <h2>"Why Eustress?"</h2>
                </div>
                <p class="section-subtitle">"Other engines fail where we excel"</p>
                
                <div class="why-grid">
                    <div class="why-card">
                        <div class="why-stat">"10x"</div>
                        <h4>"Faster Development"</h4>
                        <p>"Hot reload, instant preview, and Soul scripting eliminate the compile-wait-test cycle that plagues traditional engines."</p>
                    </div>
                    
                    <div class="why-card">
                        <div class="why-stat">"0"</div>
                        <h4>"GC Pauses"</h4>
                        <p>"Unlike Unity's C# or other managed languages, Rust has zero garbage collection. Your games run smooth, always."</p>
                    </div>
                    
                    <div class="why-card">
                        <div class="why-stat">"10M+"</div>
                        <h4>"Instances"</h4>
                        <p>"Handle massive worlds with millions of entities. Point clouds, voxels, CAD data—Eustress scales where others choke."</p>
                    </div>
                    
                    <div class="why-card">
                        <div class="why-stat">"100%"</div>
                        <h4>"Memory Safe"</h4>
                        <p>"Rust's ownership model eliminates crashes, memory leaks, and security vulnerabilities at compile time."</p>
                    </div>
                </div>
            </section>
            
            // Performance Comparison
            <section class="performance-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/trending.svg" alt="Performance" class="section-icon" />
                    <h2>"Performance That Matters"</h2>
                </div>
                
                <div class="performance-card">
                    <div class="perf-row">
                        <div class="perf-item">
                            <span class="perf-label">"Startup Time"</span>
                            <div class="perf-bars">
                                <div class="perf-bar eustress" style="width: 15%"><span>"0.3s"</span></div>
                                <div class="perf-bar unity" style="width: 60%"><span>"Unity: 2.1s"</span></div>
                                <div class="perf-bar unreal" style="width: 100%"><span>"Unreal: 4.5s"</span></div>
                            </div>
                        </div>
                        <div class="perf-item">
                            <span class="perf-label">"Memory Usage"</span>
                            <div class="perf-bars">
                                <div class="perf-bar eustress" style="width: 20%"><span>"180MB"</span></div>
                                <div class="perf-bar unity" style="width: 55%"><span>"Unity: 520MB"</span></div>
                                <div class="perf-bar unreal" style="width: 100%"><span>"Unreal: 1.2GB"</span></div>
                            </div>
                        </div>
                        <div class="perf-item">
                            <span class="perf-label">"Build Size (Web)"</span>
                            <div class="perf-bars">
                                <div class="perf-bar eustress" style="width: 10%"><span>"8MB"</span></div>
                                <div class="perf-bar unity" style="width: 45%"><span>"Unity: 35MB"</span></div>
                                <div class="perf-bar unreal" style="width: 100%"><span>"Unreal: N/A"</span></div>
                            </div>
                        </div>
                    </div>
                    <p class="perf-note">"* Benchmarks from empty project, release builds, 2024 hardware"</p>
                </div>
            </section>
            
            // What You Can Build
            <section class="build-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/sparkles.svg" alt="Build" class="section-icon" />
                    <h2>"What Can You Build?"</h2>
                </div>
                
                <div class="build-grid">
                    <div class="build-item">
                        <img src="/assets/icons/gamepad.svg" alt="Games" />
                        <span>"Games"</span>
                    </div>
                    <div class="build-item">
                        <img src="/assets/icons/users.svg" alt="Social" />
                        <span>"Social Spaces"</span>
                    </div>
                    <div class="build-item">
                        <img src="/assets/icons/cube.svg" alt="Simulations" />
                        <span>"Simulations"</span>
                    </div>
                    <div class="build-item">
                        <img src="/assets/icons/book.svg" alt="Education" />
                        <span>"Education"</span>
                    </div>
                    <div class="build-item">
                        <img src="/assets/icons/settings.svg" alt="CAD" />
                        <span>"CAD Viewers"</span>
                    </div>
                    <div class="build-item">
                        <img src="/assets/icons/network.svg" alt="Digital Twins" />
                        <span>"Digital Twins"</span>
                    </div>
                </div>
            </section>
            
            // Get Started CTA
            <section class="cta-section">
                <div class="cta-card-about">
                    <h2>"Ready to Create?"</h2>
                    <p>"Download Eustress Engine and start building your first experience in minutes"</p>
                    <div class="cta-buttons">
                        <a href="/download" class="btn-cta primary">
                            <img src="/assets/icons/download.svg" alt="Download" />
                            "Download Studio"
                        </a>
                        <a href="/learn" class="btn-cta secondary">
                            "View Tutorials"
                        </a>
                    </div>
                </div>
            </section>
            
            <Footer />
        </div>
    }
}
