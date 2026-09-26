// =============================================================================
// Eustress Web - Home Page (Simulation & Data Platform positioning)
// Voice: McKale Olson (beats, no em dashes, stakes first).
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer, on_tilt_leave, on_tilt_move};

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
                            <br/>
                            "Zero compromises."
                        </p>

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

                    // The demo reel: real Studio captures and live simulation, no
                    // mock application chrome around it.
                    <div class="hero-visual-new">
                        <HeroReel />
                    </div>
                </div>

                    // Real platform metrics, given their own strip so the hero
                    // stays a single message rather than a spec sheet.
                    <div class="metric-strip">
                        <div class="metric">
                            <span class="metric-value">"10M+"</span>
                            <span class="metric-label">"Entities"</span>
                        </div>
                        <div class="metric">
                            <span class="metric-value">"1 yr/s"</span>
                            <span class="metric-label">"Sim Speed"</span>
                        </div>
                        <div class="metric">
                            <span class="metric-value">"60+"</span>
                            <span class="metric-label">"FPS"</span>
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
                        <h3>"Best for fidelity"</h3>
                        <p>"Game-grade 3D meets real physics and real data. The simulation does not cheat, and it still runs at interactive FPS."</p>
                    </div>
                    <div class="verdict-card">
                        <h3>"Best for ownership"</h3>
                        <p>"PolyForm Shield means no platform tax, no royalty, and a roadmap you control. Fork it, ship it, keep the work. The one limit is reselling the engine itself."</p>
                    </div>
                    <div class="verdict-card">
                        <h3>"Best for AI-native work"</h3>
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
                            <p>"Anode-free lithium-sulfur, 569 bipolar layers in one can: 1,223 V and 1,032 Wh/kg, aged 699 cycles in seconds"</p>
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
            // SOURCE-AVAILABLE - build it with us (GitHub + contributor ladder)
            // ═══════════════════════════════════════════════════════════════
            <section class="github-section">
                <div class="github-bg">
                    <div class="github-grid-overlay"></div>
                </div>
                <div class="github-inner">
                    <div class="github-copy">
                        <span class="section-tag">"SOURCE-AVAILABLE"</span>
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
// Hero demo reel. Starts muted and loops in the tilted frame. Click the video
// to pause or play, the speaker button for sound, and drag the timeline's head
// to scrub; the corner button goes full screen with the browser's controls.
// The timeline shows while a pointer or a finger is on the video and fades
// once it goes idle.
// -----------------------------------------------------------------------------

/// Chapter starts in the reel, in seconds: the timeline's tick marks and the
/// label over its head.
const REEL_CHAPTERS: [(f64, &str); 7] = [
    (0.0, "Intro"),
    (1.15, "Idea"),
    (7.7, "Build"),
    (14.3, "Simulate"),
    (21.05, "Iterate"),
    (33.5, "Reality"),
    (41.4, "Eustress"),
];
/// The reel's length before its metadata loads.
const REEL_SECONDS: f64 = 48.0;
/// How long the timeline stays up after the last pointer or touch activity
/// on the reel, in milliseconds.
const REEL_CONTROLS_IDLE_MS: f64 = 2500.0;

fn reel_chapter_at(seconds: f64) -> &'static str {
    REEL_CHAPTERS
        .iter()
        .rev()
        .find(|(start, _)| seconds >= *start)
        .map(|(_, name)| *name)
        .unwrap_or("Intro")
}

fn reel_clock(seconds: f64) -> String {
    let s = seconds.max(0.0).floor() as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Whether the reel's timeline is showing, and what holds it up. Shared by
/// the reel's event handlers and its idle timer.
#[derive(Clone, Copy)]
struct ReelControls {
    shown: RwSignal<bool>,
    dragging: RwSignal<bool>,
    /// `Date.now()` at the last pointer or touch activity on the reel.
    last_activity: StoredValue<f64>,
    /// A mouse resting on the timeline keeps it up.
    over_timeline: StoredValue<bool>,
    /// At most one idle check is pending, however fast the pointer moves.
    timer_armed: StoredValue<bool>,
}

impl ReelControls {
    /// A pointer moved or a finger touched: show the timeline and restart
    /// the idle countdown.
    fn wake(self) {
        self.last_activity.set_value(js_sys::Date::now());
        if !self.shown.get_untracked() {
            self.shown.set(true);
        }
        if !self.timer_armed.get_value() {
            self.timer_armed.set_value(true);
            self.check_after(REEL_CONTROLS_IDLE_MS);
        }
    }

    /// The mouse left the reel: hide now, unless a drag is holding it.
    fn hide(self) {
        if !self.dragging.get_untracked() {
            self.shown.set(false);
        }
    }

    fn check_after(self, ms: f64) {
        set_timeout(
            move || self.check_idle(),
            std::time::Duration::from_millis(ms.max(16.0) as u64),
        );
    }

    /// Hide once the reel has gone REEL_CONTROLS_IDLE_MS without activity,
    /// otherwise wait out the rest. A drag or a resting mouse holds it up
    /// until the next pointer event starts a fresh countdown.
    fn check_idle(self) {
        // The reel may have unmounted since this was scheduled.
        let Some(last) = self.last_activity.try_get_value() else { return };
        let remaining = REEL_CONTROLS_IDLE_MS - (js_sys::Date::now() - last);
        if remaining > 0.0 {
            self.check_after(remaining);
            return;
        }
        self.timer_armed.set_value(false);
        if !self.dragging.get_untracked() && !self.over_timeline.get_value() {
            self.shown.set(false);
        }
    }
}

#[component]
fn HeroReel() -> impl IntoView {
    let video_ref = NodeRef::<leptos::html::Video>::new();
    let track_ref = NodeRef::<leptos::html::Div>::new();
    let muted = RwSignal::new(true);
    let progress = RwSignal::new(0.0_f64);
    let duration = RwSignal::new(REEL_SECONDS);
    let dragging = RwSignal::new(false);
    // Set for one update when playback jumps back (the loop wrapping), so the
    // head snaps to the start instead of sliding backwards across the track.
    let jumped = RwSignal::new(false);
    let controls = ReelControls {
        shown: RwSignal::new(false),
        dragging,
        last_activity: StoredValue::new(0.0),
        over_timeline: StoredValue::new(false),
        timer_armed: StoredValue::new(false),
    };
    // Set by a touch that lands while the timeline is hidden. That tap only
    // brings the timeline up, so reaching for it never pauses the reel.
    let tap_reveals = StoredValue::new(false);

    // Browser only: honour reduced motion, start the loop once it has a frame,
    // and hide the browser's controls again when full screen ends (standard
    // API on the element, WebKit's own event on iOS, where only the video
    // element can go full screen).
    #[cfg(not(feature = "ssr"))]
    Effect::new(move |_| {
        use wasm_bindgen::{closure::Closure, JsCast};

        let Some(video) = video_ref.get() else { return };
        // Autoplay policy reads the `muted` PROPERTY. An element built in the
        // browser gets the attribute but starts unmuted, so the browser would
        // refuse to autoplay it.
        video.set_muted(true);
        if crate::components::tilt::prefers_reduced_motion() {
            video.set_autoplay(false);
            let _ = video.pause();
        } else {
            // Start it once there is a frame to show. Calling play() here, at
            // mount, runs before the <source> children exist: it fails, and a
            // play() call also clears the element's autoplay and poster
            // flags, which left a paused black frame.
            let on_ready = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
                if let Some(v) = video_ref.get_untracked() {
                    if v.paused() && document().fullscreen_element().is_none() {
                        let _ = v.play();
                    }
                }
            });
            let _ = video.add_event_listener_with_callback("loadeddata", on_ready.as_ref().unchecked_ref());
            on_ready.forget();
            // HAVE_CURRENT_DATA already: `loadeddata` has fired, start now.
            if video.ready_state() >= 2 {
                let _ = video.play();
            }
        }
        let on_exit = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            if document().fullscreen_element().is_some() {
                return;
            }
            if let Some(v) = video_ref.get_untracked() {
                v.set_controls(false);
            }
        });
        for event in ["fullscreenchange", "webkitfullscreenchange", "webkitendfullscreen"] {
            let _ = video.add_event_listener_with_callback(event, on_exit.as_ref().unchecked_ref());
        }
        on_exit.forget();
    });

    let open_fullscreen = move |_| {
        #[cfg(not(feature = "ssr"))]
        {
            use wasm_bindgen::JsCast;

            let Some(video) = video_ref.get_untracked() else { return };
            // Keeps the viewer's place and sound choice; the browser's own
            // controls take over while full screen.
            video.set_controls(true);
            if video.request_fullscreen().is_err() {
                // iOS Safari has no element full screen; its video player does.
                if let Ok(enter) = js_sys::Reflect::get(&video, &"webkitEnterFullscreen".into()) {
                    if let Some(enter) = enter.dyn_ref::<js_sys::Function>() {
                        let _ = enter.call0(&video);
                    }
                }
            }
        }
    };

    let toggle_sound = move |_| {
        if let Some(v) = video_ref.get_untracked() {
            let now_muted = !v.muted();
            v.set_muted(now_muted);
            muted.set(now_muted);
            // Asking for sound means wanting to hear it: start a paused reel.
            if !now_muted && v.paused() {
                let _ = v.play();
            }
        }
    };

    // Move the head to a pointer's x position and seek there.
    let seek_to_x = move |client_x: f64| {
        let Some(track) = track_ref.get_untracked() else { return };
        let rect = track.get_bounding_client_rect();
        if rect.width() <= 0.0 {
            return;
        }
        let frac = ((client_x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        progress.set(frac);
        if let Some(v) = video_ref.get_untracked() {
            let d = v.duration();
            if d.is_finite() && d > 0.0 {
                v.set_current_time(frac * d);
            }
        }
    };

    let seek_by = move |delta: f64| {
        if let Some(v) = video_ref.get_untracked() {
            let d = v.duration();
            if d.is_finite() && d > 0.0 {
                let t = (v.current_time() + delta).clamp(0.0, d - 0.05);
                v.set_current_time(t);
                progress.set(t / d);
            }
        }
    };

    let head_left = move || format!("{:.3}%", progress.get() * 100.0);
    let head_label = move || {
        let t = progress.get() * duration.get();
        format!("{} · {}", reel_chapter_at(t), reel_clock(t))
    };

    view! {
        <figure
            class="hero-render hero-reel"
            class:show-controls=move || controls.shown.get()
            on:pointerdown=move |ev: web_sys::PointerEvent| {
                tap_reveals.set_value(ev.pointer_type() == "touch" && !controls.shown.get_untracked());
                controls.wake();
            }
            on:pointermove=move |_| controls.wake()
            on:pointerup=move |_| controls.wake()
            on:mousemove=move |ev| {
                // Hold the tilt still while the timeline head is being dragged.
                if !dragging.get_untracked() {
                    on_tilt_move(&ev)
                }
            }
            on:mouseleave=move |ev| {
                on_tilt_leave(&ev);
                controls.hide();
            }
        >
            <div class="hero-render-glow" aria-hidden="true"></div>
            <video
                node_ref=video_ref
                class="hero-render-video"
                poster="/assets/demo/eustress-demo-poster.jpg"
                muted=true
                prop:muted=true
                on:click=move |_| {
                    // On touch, the tap that brings the timeline up does
                    // only that.
                    if tap_reveals.get_value() {
                        return;
                    }
                    // Click toggles play. In full screen the browser's own
                    // controls handle it.
                    #[cfg(not(feature = "ssr"))]
                    if let Some(v) = video_ref.get_untracked() {
                        if document().fullscreen_element().is_none() {
                            if v.paused() {
                                let _ = v.play();
                            } else {
                                let _ = v.pause();
                            }
                        }
                    }
                }
                on:loadedmetadata=move |_| {
                    if let Some(v) = video_ref.get_untracked() {
                        let d = v.duration();
                        if d.is_finite() && d > 0.0 {
                            duration.set(d);
                        }
                    }
                }
                on:timeupdate=move |_| {
                    if dragging.get_untracked() {
                        return;
                    }
                    if let Some(v) = video_ref.get_untracked() {
                        let d = v.duration();
                        if d.is_finite() && d > 0.0 {
                            let next = v.current_time() / d;
                            jumped.set(next < progress.get_untracked());
                            progress.set(next);
                        }
                    }
                }
                on:volumechange=move |_| {
                    if let Some(v) = video_ref.get_untracked() {
                        muted.set(v.muted());
                    }
                }
                autoplay=true
                loop=true
                playsinline=true
                preload="auto"
                width="1920"
                height="1080"
                aria-label="Eustress demo: an idea for a canyon footbridge is built in Eustress Studio, fails a live rockfall simulation, gets a redesigned canopy, passes the same test, and comes out costed and ready to build"
            >
                <source src="/assets/demo/eustress-demo.webm" type="video/webm" />
                <source src="/assets/demo/eustress-demo.mp4" type="video/mp4" />
            </video>
            <div class="hero-reel-scrim" aria-hidden="true"></div>
            <div
                class="hero-timeline"
                class:dragging=move || dragging.get()
                class:jumped=move || jumped.get()
                role="slider"
                tabindex="0"
                aria-label="Demo timeline"
                aria-valuemin="0"
                aria-valuemax="100"
                aria-valuenow=move || format!("{:.0}", progress.get() * 100.0)
                aria-valuetext=head_label
                on:pointerdown=move |ev: web_sys::PointerEvent| {
                    ev.prevent_default();
                    dragging.set(true);
                    if let Some(track) = track_ref.get_untracked() {
                        let _ = track.set_pointer_capture(ev.pointer_id());
                    }
                    seek_to_x(ev.client_x() as f64);
                }
                on:pointermove=move |ev: web_sys::PointerEvent| {
                    if dragging.get_untracked() {
                        seek_to_x(ev.client_x() as f64);
                    }
                }
                on:pointerup=move |_| dragging.set(false)
                on:pointercancel=move |_| dragging.set(false)
                on:lostpointercapture=move |_| dragging.set(false)
                on:pointerenter=move |ev: web_sys::PointerEvent| {
                    if ev.pointer_type() == "mouse" {
                        controls.over_timeline.set_value(true);
                    }
                }
                on:pointerleave=move |_| controls.over_timeline.set_value(false)
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    controls.wake();
                    match ev.key().as_str() {
                        "ArrowLeft" => {
                            ev.prevent_default();
                            seek_by(-5.0);
                        }
                        "ArrowRight" => {
                            ev.prevent_default();
                            seek_by(5.0);
                        }
                        "Home" => {
                            ev.prevent_default();
                            seek_by(-1.0e6);
                        }
                        "End" => {
                            ev.prevent_default();
                            seek_by(1.0e6);
                        }
                        _ => {}
                    }
                }
            >
                <div class="hero-timeline-track" node_ref=track_ref>
                    <div class="hero-timeline-fill" style:width=head_left></div>
                    {REEL_CHAPTERS
                        .iter()
                        .skip(1)
                        .map(|&(start, name)| {
                            view! {
                                <span
                                    class="hero-timeline-tick"
                                    style:left=move || format!("{:.3}%", start / duration.get() * 100.0)
                                    title=name
                                ></span>
                            }
                        })
                        .collect_view()}
                    <div class="hero-timeline-head" style:left=head_left>
                        <span class="hero-timeline-label">{head_label}</span>
                    </div>
                </div>
            </div>
            <button
                type="button"
                class="hero-fullscreen hero-sound"
                class:is-muted=move || muted.get()
                aria-label=move || if muted.get() { "Turn the sound on" } else { "Mute" }
                aria-pressed=move || if muted.get() { "false" } else { "true" }
                title=move || if muted.get() { "Sound on" } else { "Mute" }
                on:click=toggle_sound
            >
                <svg viewBox="0 0 24 24" aria-hidden="true">
                    <path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z" />
                    {move || if muted.get() {
                        view! { <path d="M16 9.5l5 5M21 9.5l-5 5" /> }.into_any()
                    } else {
                        view! { <path d="M15.5 9a4.2 4.2 0 0 1 0 6M18 6.5a7.8 7.8 0 0 1 0 11" /> }.into_any()
                    }}
                </svg>
            </button>
            <button
                type="button"
                class="hero-fullscreen"
                aria-label="Watch the demo full screen"
                title="Watch full screen"
                on:click=open_fullscreen
            >
                <svg viewBox="0 0 24 24" aria-hidden="true">
                    <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />
                </svg>
            </button>
        </figure>
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
    // Browser only: the prerender renders the first tab and has no clock.
    #[cfg(not(feature = "ssr"))]
    {
        let timer = gloo_timers::callback::Interval::new(30_000, move || {
            active.update(|i| *i = (*i + 1) % 4);
        });
        timer.forget();
    }

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
