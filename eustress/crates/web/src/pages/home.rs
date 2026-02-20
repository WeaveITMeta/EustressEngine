// =============================================================================
// Eustress Web - Home Page (EPIC Landing)
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

/// Public landing page - EPIC VERSION.
#[component]
pub fn HomePage() -> impl IntoView {
    view! {
        <div class="page page-home epic-landing">
            // ═══════════════════════════════════════════════════════════════
            // HERO SECTION - The Big Bang
            // ═══════════════════════════════════════════════════════════════
            <CentralNav active="home".to_string() />
            
            <section class="hero-industrial">
                // Background Effects
                <div class="hero-bg">
                    <div class="grid-overlay"></div>
                    <div class="glow-orb glow-1"></div>
                    <div class="glow-orb glow-2"></div>
                </div>
                
                // Main Content
                <div class="hero-main">
                    // Left Column - Text Content
                    <div class="hero-text">
                        <div class="beta-tag">
                            <span class="tag-dot"></span>
                            "PUBLIC BETA"
                        </div>
                        
                        <h1 class="hero-headline">
                            "The Future of"<br/>
                            <span class="headline-accent">"Creation"</span>
                        </h1>
                        
                        <p class="hero-description">
                            "Build anything at the speed of thought. "
                            <strong>"100% Rust."</strong>
                            " Zero compromises."
                        </p>
                        
                        // Stats Bar
                        <div class="stats-bar">
                            <div class="stat">
                                <span class="stat-value">"10x"</span>
                                <span class="stat-label">"Faster"</span>
                            </div>
                            <div class="stat-sep"></div>
                            <div class="stat">
                                <span class="stat-value">"60+"</span>
                                <span class="stat-label">"FPS"</span>
                            </div>
                            <div class="stat-sep"></div>
                            <div class="stat">
                                <span class="stat-value">"0"</span>
                                <span class="stat-label">"Crashes"</span>
                            </div>
                        </div>
                        
                        // CTA Buttons
                        <div class="hero-buttons">
                            <a href="/login" class="btn-primary-steel">
                                "Start Creating"
                                <span class="btn-icon">"→"</span>
                            </a>
                            <a href="/gallery" class="btn-secondary-steel">
                                "Explore"
                            </a>
                            <a href="/about" class="btn-secondary-steel">
                                "About"
                            </a>
                        </div>
                    </div>
                    
                    // Right Column - Visual
                    <div class="hero-visual-new">
                        <div class="studio-preview">
                            <div class="preview-header">
                                <div class="header-dots">
                                    <span class="dot red"></span>
                                    <span class="dot yellow"></span>
                                    <span class="dot green"></span>
                                </div>
                                <span class="header-title">"Eustress Engine"</span>
                            </div>
                            <div class="preview-body">
                                <div class="preview-scene">
                                    <div class="scene-floor"></div>
                                    <img src="/assets/icons/eustress-gear.svg" alt="Eustress Gear" class="scene-gear" />
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
                
                // Platform Bar - Bottom
                <div class="platform-bar">
                    <div class="platform-bar-inner">
                        <span class="bar-label">"DEPLOY TO"</span>
                        <div class="bar-divider"></div>
                        <div class="platform-list">
                            <div class="plat"><img src="/assets/icons/windows.svg" alt="Windows" />"Windows"</div>
                            <div class="plat"><img src="/assets/icons/macos.svg" alt="macOS" />"macOS"</div>
                            <div class="plat"><img src="/assets/icons/linux.svg" alt="Linux" />"Linux"</div>
                            <div class="plat"><img src="/assets/icons/ios.svg" alt="iOS" />"iOS"</div>
                            <div class="plat"><img src="/assets/icons/android.svg" alt="Android" />"Android"</div>
                            <div class="plat"><img src="/assets/icons/web.svg" alt="Web" />"Web"</div>
                            <div class="plat"><img src="/assets/icons/meta-quest.svg" alt="Quest" />"Quest"</div>
                            <div class="plat"><img src="/assets/icons/openxr.svg" alt="OpenXR" />"OpenXR"</div>
                            <div class="plat"><img src="/assets/icons/psvr.svg" alt="PSVR2" />"PSVR2"</div>
                        </div>
                    </div>
                </div>
            </section>
            
            // ═══════════════════════════════════════════════════════════════
            // POWER FEATURES - What Makes Us Different
            // ═══════════════════════════════════════════════════════════════
            <section class="power-section">
                <div class="section-header">
                    <span class="section-tag">"SUPERPOWERS"</span>
                    <h2 class="section-title-epic">"Built Different. Built Better."</h2>
                    <p class="section-desc">"Every feature designed for maximum developer happiness"</p>
                </div>
                
                <div class="power-grid">
                    <div class="power-card">
                        <div class="power-icon">"🦀"</div>
                        <h3>"100% Rust"</h3>
                        <p>"Memory-safe, blazingly fast, fearless concurrency. No garbage collection pauses. No null pointer exceptions."</p>
                    </div>
                    
                    <div class="power-card">
                        <div class="power-icon">"✨"</div>
                        <h3>"Soul Language"</h3>
                        <p>"English to Rust. Write logic in Markdown format. Soul compiles your intent into blazing-fast native code."</p>
                    </div>
                    
                    <div class="power-card">
                        <div class="power-icon">"🌍"</div>
                        <h3>"Massive Worlds"</h3>
                        <p>"Procedural terrain, infinite voxels, seamless streaming. Build worlds bigger than your imagination."</p>
                    </div>
                    
                    <div class="power-card">
                        <div class="power-icon">"🎯"</div>
                        <h3>"Avian Physics"</h3>
                        <p>"ECS-native physics with Avian. Deterministic simulation, soft bodies, ragdolls, and destruction out of the box."</p>
                    </div>
                    
                    <div class="power-card">
                        <div class="power-icon">"🔒"</div>
                        <h3>"QUIC/TLS Networking"</h3>
                        <p>"Next-gen networking with QUIC protocol. Encrypted, low-latency, multiplexed connections for real-time multiplayer."</p>
                    </div>
                    
                    <div class="power-card">
                        <div class="power-icon">"🎨"</div>
                        <h3>"Bevy Rendering"</h3>
                        <p>"Powered by Bevy's modern renderer. PBR materials, GPU-driven, clustered lighting, and WebGPU ready."</p>
                    </div>
                    
                    <div class="power-card">
                        <div class="power-icon">"🖥️"</div>
                        <h3>"120 FPS Servers"</h3>
                        <p>"Dedicated servers running at 120 tick rate. Ultra-responsive gameplay with server-authoritative physics."</p>
                    </div>
                    
                    <div class="power-card">
                        <div class="power-icon">"📦"</div>
                        <h3>"Asset Pipeline"</h3>
                        <p>"Import anything. GLTF, FBX, Blender, Photoshop. Automatic optimization and compression."</p>
                    </div>
                </div>
            </section>
            
            // ═══════════════════════════════════════════════════════════════
            // COMPARISON SECTION - The Truth
            // ═══════════════════════════════════════════════════════════════
            <section class="comparison-section">
                <div class="section-header">
                    <span class="section-tag">"THE TRUTH"</span>
                    <h2 class="section-title-epic">"How We Stack Up"</h2>
                    <p class="section-desc">"Honest comparison with the industry giants"</p>
                </div>
                
                <div class="comparison-table-wrapper">
                    <table class="comparison-table">
                        <thead>
                            <tr>
                                <th class="feature-col">"Feature"</th>
                                <th class="engine-col eustress">
                                    <img src="/assets/icons/eustress-gear.svg" alt="Eustress" class="engine-logo" />
                                    "Eustress"
                                </th>
                                <th class="engine-col roblox">
                                    <img src="/assets/icons/roblox.svg" alt="Roblox" class="engine-logo" />
                                    "Roblox"
                                </th>
                                <th class="engine-col unity">
                                    <img src="/assets/icons/unity.svg" alt="Unity" class="engine-logo" />
                                    "Unity"
                                </th>
                                <th class="engine-col unreal">
                                    <img src="/assets/icons/unreal.svg" alt="Unreal" class="engine-logo" />
                                    "Unreal"
                                </th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr>
                                <td class="feature-name">"Learning Curve"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Super Easy"</td>
                                <td class="roblox"><span class="check">"✓"</span>" Easy"</td>
                                <td class="unity"><span class="warn">"~"</span>" Medium"</td>
                                <td class="unreal"><span class="cross">"✗"</span>" Hard"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Performance"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Native Rust"</td>
                                <td class="roblox"><span class="warn">"~"</span>" Lua VM"</td>
                                <td class="unity"><span class="warn">"~"</span>" C# + Mono"</td>
                                <td class="unreal"><span class="check">"✓"</span>" C++"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Memory Safety"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Guaranteed"</td>
                                <td class="roblox"><span class="check">"✓"</span>" Sandboxed"</td>
                                <td class="unity"><span class="warn">"~"</span>" GC Pauses"</td>
                                <td class="unreal"><span class="cross">"✗"</span>" Manual"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Web Export"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Native WASM"</td>
                                <td class="roblox"><span class="cross">"✗"</span>" None"</td>
                                <td class="unity"><span class="warn">"~"</span>" WebGL"</td>
                                <td class="unreal"><span class="cross">"✗"</span>" Limited"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Multiplayer"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Built-in"</td>
                                <td class="roblox"><span class="check">"✓"</span>" Built-in"</td>
                                <td class="unity"><span class="warn">"~"</span>" Paid Add-on"</td>
                                <td class="unreal"><span class="check">"✓"</span>" Built-in"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Hot Reload"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Instant"</td>
                                <td class="roblox"><span class="check">"✓"</span>" Fast"</td>
                                <td class="unity"><span class="warn">"~"</span>" Slow"</td>
                                <td class="unreal"><span class="warn">"~"</span>" C++ Rebuild"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Pricing"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Revenue Share"</td>
                                <td class="roblox"><span class="warn">"~"</span>" Revenue Share"</td>
                                <td class="unity"><span class="cross">"✗"</span>" Per Seat"</td>
                                <td class="unreal"><span class="warn">"~"</span>" 5% Royalty"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Max Instances"</td>
                                <td class="eustress"><span class="check">"✓"</span>" 10M+"</td>
                                <td class="roblox"><span class="warn">"~"</span>" 100K"</td>
                                <td class="unity"><span class="warn">"~"</span>" 500K"</td>
                                <td class="unreal"><span class="check">"✓"</span>" 1M+"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Studio Editor"</td>
                                <td class="eustress"><span class="warn">"~"</span>" Maturing"</td>
                                <td class="roblox"><span class="check">"✓"</span>" Polished"</td>
                                <td class="unity"><span class="check">"✓"</span>" Polished"</td>
                                <td class="unreal"><span class="check">"✓"</span>" AAA"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Script Editor"</td>
                                <td class="eustress"><span class="warn">"~"</span>" Soul + Rune"</td>
                                <td class="roblox"><span class="check">"✓"</span>" Full IDE"</td>
                                <td class="unity"><span class="check">"✓"</span>" Full IDE"</td>
                                <td class="unreal"><span class="check">"✓"</span>" Full IDE"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"XR / VR"</td>
                                <td class="eustress"><span class="check">"✓"</span>" OpenXR Native"</td>
                                <td class="roblox"><span class="cross">"✗"</span>" None"</td>
                                <td class="unity"><span class="warn">"~"</span>" Plugin"</td>
                                <td class="unreal"><span class="check">"✓"</span>" Built-in"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"AI Integration"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Soul Language"</td>
                                <td class="roblox"><span class="cross">"✗"</span>" None"</td>
                                <td class="unity"><span class="warn">"~"</span>" Third-party"</td>
                                <td class="unreal"><span class="warn">"~"</span>" Third-party"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Pro Workflows"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Full Suite"</td>
                                <td class="roblox"><span class="cross">"✗"</span>" Basic"</td>
                                <td class="unity"><span class="warn">"~"</span>" Plugins"</td>
                                <td class="unreal"><span class="check">"✓"</span>" Built-in"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Data Formats"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Mesh, PCD, CAD"</td>
                                <td class="roblox"><span class="cross">"✗"</span>" Mesh Only"</td>
                                <td class="unity"><span class="warn">"~"</span>" Mesh, PCD"</td>
                                <td class="unreal"><span class="check">"✓"</span>" Mesh, PCD"</td>
                            </tr>
                            <tr class="highlight-row">
                                <td class="feature-name">"Overall Score"</td>
                                <td class="eustress score"><span class="score-badge best">"8.5/10"</span></td>
                                <td class="roblox score"><span class="score-badge okay">"7.0/10"</span></td>
                                <td class="unity score"><span class="score-badge okay">"7.0/10"</span></td>
                                <td class="unreal score"><span class="score-badge good">"8.5/10"</span></td>
                            </tr>
                        </tbody>
                    </table>
                </div>
                
                <div class="comparison-verdict">
                    <div class="verdict-card">
                        <h3>"🎯 Best For Beginners"</h3>
                        <p>"If you're new to game dev, Eustress gives you Roblox-level ease with better than Unreal Engine level power."</p>
                    </div>
                    <div class="verdict-card">
                        <h3>"🚀 Best For Performance"</h3>
                        <p>"Rust's zero-cost abstractions mean you get C++ speed with memory safety guarantees."</p>
                    </div>
                    <div class="verdict-card">
                        <h3>"💰 Best Value"</h3>
                        <p>"Eustress Engineers make 40% of Experiencial Revenue and a wide audience."</p>
                    </div>
                </div>
            </section>
            
            // ═══════════════════════════════════════════════════════════════
            // SHOWCASE - What People Are Building (Industrial)
            // ═══════════════════════════════════════════════════════════════
            <section class="showcase-industrial">
                <div class="showcase-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"SHOWCASE"</span>
                    <div class="header-line"></div>
                </div>
                <h2 class="showcase-title">"Built With Eustress"</h2>
                <p class="showcase-subtitle">"See what our community is creating"</p>
                
                <div class="showcase-grid-industrial">
                    <div class="showcase-card featured">
                        <div class="card-visual">
                            <div class="visual-icon">"🏔️"</div>
                            <div class="visual-scanline"></div>
                        </div>
                        <div class="card-info">
                            <span class="info-tag">"FEATURED"</span>
                            <h4>"Epic Adventure RPG"</h4>
                            <p>"Open world, 100+ hours"</p>
                            <div class="info-stats">
                                <span class="stat-plays">"▶ 2.5M"</span>
                            </div>
                        </div>
                    </div>
                    <div class="showcase-card">
                        <div class="card-visual">
                            <div class="visual-icon">"🏎️"</div>
                        </div>
                        <div class="card-info">
                            <h4>"Neon Racers"</h4>
                            <span class="stat-plays">"▶ 890K"</span>
                        </div>
                    </div>
                    <div class="showcase-card">
                        <div class="card-visual">
                            <div class="visual-icon">"🏰"</div>
                        </div>
                        <div class="card-info">
                            <h4>"Castle Defense"</h4>
                            <span class="stat-plays">"▶ 1.2M"</span>
                        </div>
                    </div>
                    <div class="showcase-card">
                        <div class="card-visual">
                            <div class="visual-icon">"🚀"</div>
                        </div>
                        <div class="card-info">
                            <h4>"Space Colony"</h4>
                            <span class="stat-plays">"▶ 650K"</span>
                        </div>
                    </div>
                    <div class="showcase-card">
                        <div class="card-visual">
                            <div class="visual-icon">"⚔️"</div>
                        </div>
                        <div class="card-info">
                            <h4>"Battle Arena"</h4>
                            <span class="stat-plays">"▶ 3.1M"</span>
                        </div>
                    </div>
                </div>
                
                <a href="/gallery" class="btn-secondary-steel showcase-btn">
                    "Explore All Games"
                    <span class="btn-icon">"→"</span>
                </a>
            </section>
            
            // ═══════════════════════════════════════════════════════════════
            // TESTIMONIALS - Social Proof (Industrial)
            // ═══════════════════════════════════════════════════════════════
            <section class="testimonials-industrial">
                <div class="testimonials-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"TESTIMONIALS"</span>
                    <div class="header-line"></div>
                </div>
                <h2 class="testimonials-title">"Developers Love Us"</h2>
                
                <div class="testimonials-track">
                    <div class="testimonial-panel">
                        <div class="panel-quote">
                            <span class="quote-mark">"❝"</span>
                            <p>"Switched from Roblox and never looked back. Soul scripting is so much easier and more powerful than Lua can ever hope to be."</p>
                        </div>
                        <div class="panel-author">
                            <div class="author-indicator"></div>
                            <div class="author-details">
                                <span class="author-name">"Alex Chen"</span>
                                <span class="author-role">"Indie Developer"</span>
                            </div>
                        </div>
                    </div>
                    
                    <div class="testimonial-panel highlight">
                        <div class="panel-quote">
                            <span class="quote-mark">"❝"</span>
                            <p>"Finally, an engine that doesn't fight you. Rust + ECS is the future, and Eustress nails it."</p>
                        </div>
                        <div class="panel-author">
                            <div class="author-indicator"></div>
                            <div class="author-details">
                                <span class="author-name">"Sarah Miller"</span>
                                <span class="author-role">"Lead Engineer @ GameStudio"</span>
                            </div>
                        </div>
                    </div>
                    
                    <div class="testimonial-panel">
                        <div class="panel-quote">
                            <span class="quote-mark">"❝"</span>
                            <p>"My 12-year-old built their first multiplayer game in a weekend. That's the power of good tools."</p>
                        </div>
                        <div class="panel-author">
                            <div class="author-indicator"></div>
                            <div class="author-details">
                                <span class="author-name">"Mike Johnson"</span>
                                <span class="author-role">"Parent & Developer"</span>
                            </div>
                        </div>
                    </div>
                </div>
            </section>
            
            // ═══════════════════════════════════════════════════════════════
            // CTA SECTION - The Big Ask (Industrial)
            // ═══════════════════════════════════════════════════════════════
            <section class="cta-industrial">
                <div class="cta-bg">
                    <div class="cta-grid-overlay"></div>
                    <div class="cta-glow-orb"></div>
                </div>
                <div class="cta-container">
                    <h2 class="cta-headline">"Ready to Build Something "<span class="cta-accent">"Amazing"</span>"?"</h2>
                    <p class="cta-subtext">"Join thousands of developers creating the next generation of games."</p>
                    
                    <a href="/login" class="btn-primary-steel cta-btn">
                        "Start Free Today"
                        <span class="btn-icon">"→"</span>
                    </a>
                    
                    <div class="cta-features">
                        <div class="cta-feature">
                            <span class="feature-check">"✓"</span>
                            "No credit card required"
                        </div>
                        <div class="cta-feature">
                            <span class="feature-check">"✓"</span>
                            "Free forever tier"
                        </div>
                        <div class="cta-feature">
                            <span class="feature-check">"✓"</span>
                            "Full feature access"
                        </div>
                    </div>
                </div>
            </section>
            
            // Footer
            <Footer />
        </div>
    }
}
