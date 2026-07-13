// =============================================================================
// Eustress Web - Home Page (Simulation & Data Platform positioning)
// Voice: McKale Olson (beats, no em dashes, stakes first).
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

/// Public landing page - simulation & data platform.
#[component]
pub fn HomePage() -> impl IntoView {
    view! {
        <div class="page page-home epic-landing">
            // ═══════════════════════════════════════════════════════════════
            // HERO
            // ═══════════════════════════════════════════════════════════════
            <CentralNav active="home".to_string() />

            <section class="hero-industrial">
                <div class="hero-bg">
                    <div class="grid-overlay"></div>
                    <div class="glow-orb glow-1"></div>
                    <div class="glow-orb glow-2"></div>
                </div>

                <div class="hero-main">
                    <div class="hero-text">
                        <div class="beta-tag">
                            <span class="tag-dot"></span>
                            "SIMULATION & DATA PLATFORM · PUBLIC ALPHA"
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

                        // Stats - real platform metrics (FPS stays)
                        <div class="stats-bar">
                            <div class="stat">
                                <span class="stat-value">"10M+"</span>
                                <span class="stat-label">"Entities"</span>
                            </div>
                            <div class="stat-sep"></div>
                            <div class="stat">
                                <span class="stat-value">"1 yr/s"</span>
                                <span class="stat-label">"Sim Speed"</span>
                            </div>
                            <div class="stat-sep"></div>
                            <div class="stat">
                                <span class="stat-value">"60+"</span>
                                <span class="stat-label">"FPS"</span>
                            </div>
                        </div>

                        <div class="hero-buttons">
                            <a href="/login" class="btn-primary-steel">
                                "Start Building"
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

                    <div class="hero-visual-new">
                        <div class="studio-preview">
                            <div class="preview-header">
                                <div class="header-dots">
                                    <span class="dot red"></span>
                                    <span class="dot yellow"></span>
                                    <span class="dot green"></span>
                                </div>
                                <span class="header-title">"Eustress"</span>
                            </div>
                            <div class="preview-body">
                                <div class="preview-scene">
                                    <div class="scene-floor"></div>
                                    <img src="/assets/icons/eustress-gear.svg" alt="Eustress" class="scene-gear" />
                                </div>
                            </div>
                        </div>
                    </div>
                </div>

                <div class="platform-bar">
                    <div class="platform-bar-inner">
                        <span class="bar-label">"RUNS ON"</span>
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
            // WHAT'S INSIDE - consolidated tabbed panel (Worlds / Systems / Data / Superpowers)
            // Auto-carousels every 30s; a click jumps immediately and the timer
            // keeps advancing from wherever the user leaves it.
            // ═══════════════════════════════════════════════════════════════
            <WhatsInsideTabs />

            // ═══════════════════════════════════════════════════════════════
            // COMPARISON - simulation software FIRST, then game engines
            // ═══════════════════════════════════════════════════════════════
            <section class="comparison-section">
                <div class="section-header">
                    <span class="section-tag">"THE TRUTH"</span>
                    <h2 class="section-title-epic">"How We Compare"</h2>
                    <p class="section-desc">"Honest, category by category. We do not win every row. We will show you the ones we lose."</p>
                </div>

                // ── Simulation software (FIRST) ──
                <h3 class="comparison-subtitle">"Simulation Software Comparison"</h3>
                <div class="comparison-table-wrapper">
                    <table class="comparison-table">
                        <thead>
                            <tr>
                                <th class="feature-col">"Capability"</th>
                                <th class="engine-col eustress">
                                    <img src="/assets/icons/eustress-gear.svg" alt="Eustress" class="engine-logo" />
                                    "Eustress"
                                </th>
                                <th class="engine-col">
                                    <img src="/assets/icons/omniverse.svg" alt="Omniverse" class="engine-logo" />
                                    "Omniverse"
                                </th>
                                <th class="engine-col">
                                    <img src="/assets/icons/anylogic.svg" alt="AnyLogic" class="engine-logo" />
                                    "AnyLogic"
                                </th>
                                <th class="engine-col">
                                    <img src="/assets/icons/matlab.svg" alt="MATLAB" class="engine-logo" />
                                    "MATLAB"
                                </th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr>
                                <td class="feature-name">"Real-time 3D world"</td>
                                <td class="eustress"><span class="check">"✓"</span>" 60+ FPS"</td>
                                <td><span class="check">"✓"</span>" RTX"</td>
                                <td><span class="warn">"~"</span>" Basic 3D"</td>
                                <td><span class="cross">"✗"</span>" Plots only"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Scale (entities)"</td>
                                <td class="eustress"><span class="check">"✓"</span>" 10M+"</td>
                                <td><span class="check">"✓"</span>" Large (USD)"</td>
                                <td><span class="warn">"~"</span>" 100Ks agents"</td>
                                <td><span class="warn">"~"</span>" Matrix-bound"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Time compression"</td>
                                <td class="eustress"><span class="check">"✓"</span>" ~1 yr/sec"</td>
                                <td><span class="warn">"~"</span>" Limited"</td>
                                <td><span class="check">"✓"</span>" Discrete-event"</td>
                                <td><span class="check">"✓"</span>" Numerical"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Agent-based modeling"</td>
                                <td class="eustress"><span class="check">"✓"</span>" ECS-native"</td>
                                <td><span class="warn">"~"</span>" Via Kit"</td>
                                <td><span class="check">"✓"</span>" Specialty"</td>
                                <td><span class="warn">"~"</span>" Toolbox"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Numerical / control libs"</td>
                                <td class="eustress"><span class="warn">"~"</span>" Growing"</td>
                                <td><span class="warn">"~"</span>" Partial"</td>
                                <td><span class="warn">"~"</span>" Partial"</td>
                                <td><span class="check">"✓"</span>" Gold standard"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Photoreal rendering"</td>
                                <td class="eustress"><span class="warn">"~"</span>" PBR"</td>
                                <td><span class="check">"✓"</span>" RTX path-traced"</td>
                                <td><span class="cross">"✗"</span>" None"</td>
                                <td><span class="cross">"✗"</span>" None"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"AI-native control"</td>
                                <td class="eustress"><span class="check">"✓"</span>" MCP bridge"</td>
                                <td><span class="warn">"~"</span>" Kit/USD"</td>
                                <td><span class="cross">"✗"</span>" None"</td>
                                <td><span class="warn">"~"</span>" Scripted"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Source open / forkable"</td>
                                <td class="eustress"><span class="check">"✓"</span>" PolyForm Shield"</td>
                                <td><span class="cross">"✗"</span>" Proprietary"</td>
                                <td><span class="cross">"✗"</span>" Commercial"</td>
                                <td><span class="cross">"✗"</span>" Commercial"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Pricing"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Free, no royalty"</td>
                                <td><span class="warn">"~"</span>" Free indie, paid ent."</td>
                                <td><span class="cross">"✗"</span>" $$$ license"</td>
                                <td><span class="cross">"✗"</span>" $$$ per seat"</td>
                            </tr>
                        </tbody>
                    </table>
                </div>

                // ── Game engines (SECOND) ──
                <h3 class="comparison-subtitle">"Game Engine Comparison"</h3>
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
                                <td class="feature-name">"Max Instances"</td>
                                <td class="eustress"><span class="check">"✓"</span>" 10M+"</td>
                                <td class="roblox"><span class="warn">"~"</span>" 100K"</td>
                                <td class="unity"><span class="warn">"~"</span>" 500K"</td>
                                <td class="unreal"><span class="check">"✓"</span>" 1M+"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Web Export"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Native WASM"</td>
                                <td class="roblox"><span class="cross">"✗"</span>" None"</td>
                                <td class="unity"><span class="warn">"~"</span>" WebGL"</td>
                                <td class="unreal"><span class="cross">"✗"</span>" Limited"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Source open / forkable"</td>
                                <td class="eustress"><span class="check">"✓"</span>" PolyForm Shield"</td>
                                <td class="roblox"><span class="cross">"✗"</span>" Closed"</td>
                                <td class="unity"><span class="cross">"✗"</span>" Closed"</td>
                                <td class="unreal"><span class="warn">"~"</span>" Source access"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Pricing"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Free, no royalty"</td>
                                <td class="roblox"><span class="warn">"~"</span>" Revenue share"</td>
                                <td class="unity"><span class="cross">"✗"</span>" Per seat"</td>
                                <td class="unreal"><span class="warn">"~"</span>" 5% royalty"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Scripting"</td>
                                <td class="eustress"><span class="check">"✓"</span>" Soul + Rune + Luau"</td>
                                <td class="roblox"><span class="check">"✓"</span>" Luau"</td>
                                <td class="unity"><span class="check">"✓"</span>" C#"</td>
                                <td class="unreal"><span class="check">"✓"</span>" Blueprint + C++"</td>
                            </tr>
                            <tr>
                                <td class="feature-name">"Studio Editor"</td>
                                <td class="eustress"><span class="warn">"~"</span>" Maturing"</td>
                                <td class="roblox"><span class="check">"✓"</span>" Polished"</td>
                                <td class="unity"><span class="check">"✓"</span>" Polished"</td>
                                <td class="unreal"><span class="check">"✓"</span>" AAA"</td>
                            </tr>
                        </tbody>
                    </table>
                </div>

                <div class="comparison-verdict">
                    <div class="verdict-card">
                        <h3>"🔬 Best for fidelity"</h3>
                        <p>"Game-grade 3D meets real physics and real data. The simulation does not cheat, and it still runs at interactive FPS."</p>
                    </div>
                    <div class="verdict-card">
                        <h3>"🔓 Best for ownership"</h3>
                        <p>"PolyForm Shield means no platform tax, no royalty, and no roadmap you do not control. Fork it, ship what you build, keep the work. The one thing it blocks is a competitor reselling the engine itself."</p>
                    </div>
                    <div class="verdict-card">
                        <h3>"🤖 Best AI Native Work"</h3>
                        <p>"Drive the live engine over MCP. An agent can build a scene, run a year of consequences, and read back the result."</p>
                    </div>
                </div>
            </section>

            // ═══════════════════════════════════════════════════════════════
            // WHAT EUSTRESS SIMULATES - real reference builds (no fake metrics)
            // ═══════════════════════════════════════════════════════════════
            <section class="showcase-industrial">
                <div class="showcase-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"REFERENCE BUILDS"</span>
                    <div class="header-line"></div>
                </div>
                <h2 class="showcase-title">"What Eustress Simulates"</h2>
                <p class="showcase-subtitle">"Real models running in the engine today"</p>

                <div class="showcase-grid-industrial">
                    <div class="showcase-card featured">
                        <div class="card-visual">
                            <div class="visual-icon">"🔋"</div>
                            <div class="visual-scanline"></div>
                        </div>
                        <div class="card-info">
                            <span class="info-tag">"DIGITAL TWIN"</span>
                            <h4>"V-Cell Battery"</h4>
                            <p>"Na-S solid-state cell, aged 10,000 charge cycles in seconds"</p>
                        </div>
                    </div>
                    <div class="showcase-card">
                        <div class="card-visual">
                            <div class="visual-icon">"⚡"</div>
                        </div>
                        <div class="card-info">
                            <h4>"Fusion Exosuit"</h4>
                            <span class="info-tag">"ENERGY"</span>
                        </div>
                    </div>
                    <div class="showcase-card">
                        <div class="card-visual">
                            <div class="visual-icon">"🌡️"</div>
                        </div>
                        <div class="card-info">
                            <h4>"Climate Model"</h4>
                            <span class="info-tag">"DATA"</span>
                        </div>
                    </div>
                    <div class="showcase-card">
                        <div class="card-visual">
                            <div class="visual-icon">"📦"</div>
                        </div>
                        <div class="card-info">
                            <h4>"Supply-Chain Twin"</h4>
                            <span class="info-tag">"EPCIS / GS1"</span>
                        </div>
                    </div>
                    <div class="showcase-card">
                        <div class="card-visual">
                            <div class="visual-icon">"☢️"</div>
                        </div>
                        <div class="card-info">
                            <h4>"Fission Reactor"</h4>
                            <span class="info-tag">"PID CONTROL"</span>
                        </div>
                    </div>
                </div>

                <a href="/gallery" class="btn-secondary-steel showcase-btn">
                    "Explore All Simulations"
                    <span class="btn-icon">"→"</span>
                </a>
            </section>

            // ═══════════════════════════════════════════════════════════════
            // PROOF - honest, source-backed (replaces fabricated testimonials)
            // ═══════════════════════════════════════════════════════════════
            <section class="testimonials-industrial">
                <div class="testimonials-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"THE PROOF IS THE SOURCE"</span>
                    <div class="header-line"></div>
                </div>
                <h2 class="testimonials-title">"Don't Take Our Word For It"</h2>

                <div class="systems-grid">
                    <div class="systems-card">
                        <div class="systems-icon">"🔓"</div>
                        <h3>"Read the code"</h3>
                        <p>"Source-available, top to bottom. The clock, the physics, the data layer, all of it readable under PolyForm Shield. If a claim on this page matters to you, go read the source."</p>
                    </div>
                    <div class="systems-card">
                        <div class="systems-icon">"🦀"</div>
                        <h3>"Built in Rust"</h3>
                        <p>"Memory safety is structural, not aspirational. The bug that eats other teams' weeks does not compile here."</p>
                    </div>
                    <div class="systems-card">
                        <div class="systems-icon">"▶️"</div>
                        <h3>"Runs today"</h3>
                        <p>"Not a render. Thousands of entities, interactive frame rates, click-select and raycast. A real engine you can launch right now."</p>
                    </div>
                </div>
            </section>

            // ═══════════════════════════════════════════════════════════════
            // OPEN SOURCE - build it with us (GitHub + contributor ladder)
            // ═══════════════════════════════════════════════════════════════
            <section class="github-section">
                <div class="github-bg">
                    <div class="github-grid-overlay"></div>
                </div>
                <div class="github-inner">
                    <div class="github-copy">
                        <span class="section-tag">"OPEN SOURCE"</span>
                        <h2 class="section-title-epic">"Build It With Us"</h2>
                        <p class="github-lead">"The best simulation engine should not be a walled garden. It should be free."</p>
                        <p class="github-sub">"Eustress ships under PolyForm Shield: free to clone, fork, and build on. The work you put in is the equity you keep."</p>

                        <div class="ladder">
                            <div class="ladder-step">
                                <span class="ladder-num">"1"</span>
                                <span class="ladder-text">"Open a Pull Request"</span>
                            </div>
                            <span class="ladder-arrow">"→"</span>
                            <div class="ladder-step">
                                <span class="ladder-num">"2"</span>
                                <span class="ladder-text">"Earn Points & Rank"</span>
                            </div>
                            <span class="ladder-arrow">"→"</span>
                            <div class="ladder-step">
                                <span class="ladder-num">"3"</span>
                                <span class="ladder-text">"Become a Contributor"</span>
                            </div>
                        </div>

                        <div class="github-cta">
                            <a href="https://github.com/WeaveITMeta/EustressEngine" target="_blank" rel="noopener" class="btn-primary-steel">
                                "★ Star on GitHub"
                                <span class="btn-icon">"→"</span>
                            </a>
                            <a href="https://github.com/WeaveITMeta/EustressEngine/pulls" target="_blank" rel="noopener" class="btn-secondary-steel">
                                "Open a Pull Request"
                            </a>
                        </div>
                    </div>

                    <div class="github-terminal">
                        <div class="term-header">
                            <span class="term-dots">
                                <span class="dot red"></span>
                                <span class="dot yellow"></span>
                                <span class="dot green"></span>
                            </span>
                            <span class="term-title">"bash"</span>
                        </div>
                        <div class="term-body">
                            <div class="term-line"><span class="term-prompt">"$ "</span>"git clone github.com/WeaveITMeta/EustressEngine"</div>
                            <div class="term-line term-dim">"Cloning into 'EustressEngine'..."</div>
                            <div class="term-line"><span class="term-prompt">"$ "</span>"cargo run"</div>
                            <div class="term-line term-ok">"✓ Eustress running. The world is yours."</div>
                        </div>
                        <div class="github-badges">
                            <span class="gh-badge">"PolyForm Shield"</span>
                            <span class="gh-badge">"100% Rust"</span>
                            <span class="gh-badge">"PRs welcome"</span>
                        </div>
                    </div>
                </div>
            </section>

            // ═══════════════════════════════════════════════════════════════
            // CTA
            // ═══════════════════════════════════════════════════════════════
            <section class="cta-industrial">
                <div class="cta-bg">
                    <div class="cta-grid-overlay"></div>
                    <div class="cta-glow-orb"></div>
                </div>
                <div class="cta-container">
                    <h2 class="cta-headline">"Ready to Model the "<span class="cta-accent">"Real World"</span>"?"</h2>
                    <p class="cta-subtext">"Join the builders simulating reality, and owning every bit of it."</p>

                    <a href="/login" class="btn-primary-steel cta-btn">
                        "Start Free Today"
                        <span class="btn-icon">"→"</span>
                    </a>

                    <div class="cta-features">
                        <div class="cta-feature">
                            <span class="feature-check">"✓"</span>
                            "Source-available, PolyForm Shield"
                        </div>
                        <div class="cta-feature">
                            <span class="feature-check">"✓"</span>
                            "Free forever, no platform tax"
                        </div>
                        <div class="cta-feature">
                            <span class="feature-check">"✓"</span>
                            "Fork it and ship your fork"
                        </div>
                    </div>
                </div>
            </section>

            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// "What's Inside" tabbed panel: consolidates Games & Worlds, Systems That
// Matter, Data Platform, and Superpowers into one standardized component.
// Every tab renders the exact same power-grid / power-card visual so
// switching tabs never jumps style. Auto-advances every 30s.
// -----------------------------------------------------------------------------

#[component]
fn WhatsInsideTabs() -> impl IntoView {
    let active = RwSignal::new(0usize);

    // Auto-carousel: advance to the next tab every 30 seconds. A manual click
    // (below) jumps immediately; the timer just keeps advancing from there.
    let timer = gloo_timers::callback::Interval::new(30_000, move || {
        active.update(|i| *i = (*i + 1) % 4);
    });
    timer.forget();

    let tab_label = |i: usize| match i {
        0 => "Games & Worlds",
        1 => "Systems That Matter",
        2 => "Data Platform",
        _ => "Superpowers",
    };

    view! {
        <section class="power-section tabs-section">
            <div class="section-header">
                <span class="section-tag">"WHAT'S INSIDE"</span>
                <h2 class="section-title-epic">"One Engine. Every Angle."</h2>
                <p class="section-desc">"Games, simulation, data, and the internals: same engine, same cards, one panel."</p>
            </div>

            <div class="tabs-bar" role="tablist">
                {(0..4).map(|i| {
                    let is_active = move || active.get() == i;
                    view! {
                        <button
                            class="tab-btn"
                            class:active=is_active
                            role="tab"
                            aria-selected=move || is_active().to_string()
                            on:click=move |_| active.set(i)
                        >
                            {tab_label(i)}
                        </button>
                    }
                }).collect_view()}
            </div>

            <div class="tabs-panel">
                {move || match active.get() {
                    0 => view! {
                        <div>
                            <div class="power-grid">
                                <div class="power-card">
                                    <div class="power-icon">"🌄"</div>
                                    <h3>"Photoreal Rendering"</h3>
                                    <p>"Bevy's PBR pipeline. Real-time lighting, shadows, atmosphere, reflections. Worlds that look shipped, not prototyped."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🗺️"</div>
                                    <h3>"Massive Open Worlds"</h3>
                                    <p>"Procedural terrain and seamless streaming. Walk for hours and never hit a loading screen."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🌐"</div>
                                    <h3>"Real-Time Multiplayer"</h3>
                                    <p>"QUIC networking, server-authoritative, low latency. Built in, not bolted on."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🥽"</div>
                                    <h3>"VR and XR Native"</h3>
                                    <p>"OpenXR out of the box. Quest, PSVR2, and desktop XR from one project."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"💥"</div>
                                    <h3>"Physics and Destruction"</h3>
                                    <p>"Avian physics with soft bodies, ragdolls, and things that break the way they should."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🚀"</div>
                                    <h3>"Ship Anywhere"</h3>
                                    <p>"One project, every screen. Windows, Mac, Linux, mobile, web, and headset."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🔊"</div>
                                    <h3>"Spatial Audio"</h3>
                                    <p>"Sound that lives in the world, not just the speakers. 3D positional audio, occlusion, and reverb that track the geometry."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🎆"</div>
                                    <h3>"Particles and VFX"</h3>
                                    <p>"Fire, smoke, sparks, and spells. GPU particle systems that hold their frame rate when the screen gets loud."</p>
                                </div>
                            </div>
                            <div class="systems-cta">
                                <a href="/gallery" class="btn-primary-glow">"See the Gallery →"</a>
                            </div>
                        </div>
                    }.into_any(),
                    1 => view! {
                        <div>
                            <div class="power-grid">
                                <div class="power-card">
                                    <div class="power-icon">"⚛️"</div>
                                    <h3>"Simulate Reality"</h3>
                                    <p>"Every material has real properties. Every force follows real physics. Model a single battery cell or a city's power grid; energy, emergency response, economies, supply chains. Realism is the foundation, not a setting."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"📊"</div>
                                    <h3>"Drive It With Data"</h3>
                                    <p>"Load millions of real rows. Query them. Feed them straight into a live model. Here, weather and economies run on data, not on timers and mocked ticks. The simulation does not fake it."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🌍"</div>
                                    <h3>"Source-Available, Free Forever"</h3>
                                    <p>"The most powerful simulation platform on Earth should not sit behind a paywall. Eustress ships under PolyForm Shield. Read it, fork it, ship what you build with it, keep every dollar. Climate, public safety, the systems we all live inside: those belong to everyone."</p>
                                </div>
                            </div>
                            <div class="systems-cta">
                                <p class="systems-urgency">"Every day without an accurate model is a day of decisions made on a guess. Build the model. Test the theory. Ship the solution."</p>
                                <a href="/download" class="btn-primary-glow">"Start Building Solutions →"</a>
                            </div>
                        </div>
                    }.into_any(),
                    2 => view! {
                        <div>
                            <div class="power-grid">
                                <div class="power-card">
                                    <div class="power-icon">"🗃️"</div>
                                    <h3>"Datasets as a Noun"</h3>
                                    <p>"Apache Arrow and Polars, in the engine. Load, filter, and join millions of rows from Parquet, CSV, or a live feed, right next to your 3D scene."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"📈"</div>
                                    <h3>"GPU-Accelerated Charts"</h3>
                                    <p>"Plot data at scene scale on the GPU. Hover for the exact value. Read the fit. Charts that keep up with millions of points."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🧩"</div>
                                    <h3>"Domain-General"</h3>
                                    <p>"One pipeline, any domain. The system does not care whether the rows are energy, supply chain, finance, or sensor streams. One inspector renders a Dataset the way it renders a Part."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🔗"</div>
                                    <h3>"Query, Then Simulate"</h3>
                                    <p>"Bind a column to a parameter and real numbers drive real physics. The gap between your spreadsheet and your simulation closes."</p>
                                </div>
                            </div>
                        </div>
                    }.into_any(),
                    _ => view! {
                        <div>
                            <div class="power-grid">
                                <div class="power-card">
                                    <div class="power-icon">"🦀"</div>
                                    <h3>"100% Rust"</h3>
                                    <p>"Memory-safe, fast, fearless concurrency. No garbage-collection pauses. A whole class of crash never compiles."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"⏱️"</div>
                                    <h3>"Time Compression"</h3>
                                    <p>"Simulated time is not wall-clock time. Built to compress a year of simulation into a second. Age a battery a decade before lunch."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🌐"</div>
                                    <h3>"Massive Scale"</h3>
                                    <p>"Persistence beyond live memory. Millions of entities, streamed by locality, culled on the GPU, stored in an LSM-tree world."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🎯"</div>
                                    <h3>"Avian Physics"</h3>
                                    <p>"ECS-native, deterministic physics. Soft bodies, ragdolls, constraints, destruction. Same inputs, same result, every run."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🤖"</div>
                                    <h3>"AI-Native Bridge"</h3>
                                    <p>"Drive the live engine over MCP. Inspect it, build in it, run simulations through it. Think Playwright, for a 3D world."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"✨"</div>
                                    <h3>"Soul Language"</h3>
                                    <p>"Write logic in plain Markdown. Soul compiles your intent to native Rust. Rune and Luau are there when you want the wheel."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"🎨"</div>
                                    <h3>"Bevy Rendering"</h3>
                                    <p>"Bevy's modern renderer. PBR, GPU-driven, clustered lighting, WebGPU-ready. 60+ FPS on real scenes."</p>
                                </div>
                                <div class="power-card">
                                    <div class="power-icon">"📦"</div>
                                    <h3>"Data Pipeline"</h3>
                                    <p>"Import anything. Mesh (GLTF, FBX), point clouds (PCD), CAD, and tables (Parquet, Arrow). One pipeline, every format."</p>
                                </div>
                            </div>
                        </div>
                    }.into_any(),
                }}
            </div>
        </section>
    }
}
