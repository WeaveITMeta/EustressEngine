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

            <HomeHero />

            // ═══════════════════════════════════════════════════════════════
            // WHAT'S INSIDE - consolidated tabbed panel (Worlds / Systems / Data / Superpowers)
            // Auto-carousels every 30s; a click jumps immediately and the timer
            // keeps advancing from wherever the user leaves it.
            // ═══════════════════════════════════════════════════════════════
            <WhatsInsideTabs />

            // ═══════════════════════════════════════════════════════════════
            // COMPARISON - simulation software FIRST, then game engines
            // ═══════════════════════════════════════════════════════════════
            <HomeComparison />

            // ═══════════════════════════════════════════════════════════════
            // WHAT EUSTRESS SIMULATES - real reference builds (no fake metrics)
            // ═══════════════════════════════════════════════════════════════
            <HomeShowcase />

            // ═══════════════════════════════════════════════════════════════
            // PROOF - the source is the evidence, and it is open to the reader.
            // One section: three ticked lines, the GitHub actions, the terminal.
            // ═══════════════════════════════════════════════════════════════
            <HomeProof />

            // ═══════════════════════════════════════════════════════════════
            // CTA
            // ═══════════════════════════════════════════════════════════════
            <HomeCta />

            <Footer />
        </div>
    }
}

/// The hero: headline, the Studio capture over the city backdrop, metrics and platforms.
#[component]
fn HomeHero() -> impl IntoView {
    view! {
        <section class="hero-industrial hero-city">
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
                        "Eustress is a source-available simulation and data platform "
                        <strong>"built in Rust"</strong>
                        "; it also builds games and interactive worlds."
                    </p>

                    // One primary action; the second is the way to look before installing.
                    <div class="hero-buttons">
                        <a href="/download" class="btn-primary-steel">
                            "Download Studio"
                            <span class="btn-icon">"→"</span>
                        </a>
                        <a href="/gallery" class="btn-secondary-steel">
                            "Browse the Gallery"
                        </a>
                    </div>
                </div>

                // The demo reel: real Studio captures and live simulation, no
                // mock application chrome around it.
                <div class="hero-visual-new">
                    <figure
                        class="hero-render"
                        on:mousemove=move |ev| on_tilt_move(&ev)
                        on:mouseleave=move |ev| on_tilt_leave(&ev)
                    >
                        <div class="hero-render-glow" aria-hidden="true"></div>
                        <img
                            src="/assets/hero-render.png"
                            alt="The Eustress Studio editor: a photographed bicycle reconstructed as a Gaussian splat, rendering live in the viewport alongside the scene explorer and properties panels"
                            class="hero-render-img"
                            width="1334"
                            height="714"
                            fetchpriority="high"
                            decoding="async"
                        />
                    </figure>
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
    }
}

/// "How We Compare": the simulation table, open, then the game engine table folded away.
#[component]
fn HomeComparison() -> impl IntoView {
    view! {
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
                            <td class="eustress"><span class="check">"✓"</span>" RTX"</td>
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
                            <td class="eustress"><span class="check">"✓"</span>" RTX ray-traced"</td>
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

            // ── Game engines: folded away, closed by default. The simulation table above is the main comparison. ──
            <details class="comparison-more">
                <summary class="comparison-more-summary">"Compare with Roblox, Unity and Unreal"</summary>
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
            </details>

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
    }
}


// -----------------------------------------------------------------------------
// Reference builds: "What Eustress Simulates". One card per build, each with an
// image slot. A build whose capture is None shows a quiet striped placeholder
// with its name; one with a capture shows the image, sized, with alt text.
// -----------------------------------------------------------------------------

/// One reference build on the home page.
struct ShowcaseBuild {
    /// Kebab-case name of the build; the capture file is named after it.
    slug: &'static str,
    /// The small label over the name.
    tag: &'static str,
    name: &'static str,
    /// One line under the name. Only what the build is, or a number that is
    /// measured. Nothing invented.
    line: &'static str,
    /// The capture, as a site path, or None while there is none yet.
    capture: Option<&'static str>,
    /// Figures shown in a row under the line, as (figure, label). Empty for most builds.
    stats: &'static [(&'static str, &'static str)],
}

/// The reference builds, in card order. The first is the large featured card.
///
/// Captures go in assets/showcase/<slug>.webp, taken from the engine (Studio
/// or the Player running the build), 1280 x 800. When a file exists, set the
/// build's capture to Some("/assets/showcase/<slug>.webp"); until then the
/// card shows the placeholder, never a broken image.
/// Under the V-Cell figures: every one of them comes from the engine, and no cell has been built (the V-Cell session, spec 2.1).
const SIMULATED_NOTE: &str = "Simulated in Eustress. No V-Cell has been built yet.";

const SHOWCASE_BUILDS: &[ShowcaseBuild] = &[
    ShowcaseBuild {
        slug: "v-cell",
        tag: "DIGITAL TWIN",
        name: "V-Cell Battery",
        line: "An anode-lean lithium-sulfur cell that stacks 459 bipolar layers inside one 300 × 100 × 100 mm can, so a single cell delivers 987 V and 2,386 Wh.",
        capture: None,
        stats: &[
            ("544", "Wh/kg, pack"),
            ("987", "volts, one cell"),
            ("2,386", "watt-hours"),
            ("889", "cycles to 80%, device duty"),
            ("591", "kWh/kg over a service life, vehicle duty"),
        ],
    },
    ShowcaseBuild {
        slug: "fusion-exosuit",
        tag: "ENERGY",
        name: "Fusion Exosuit",
        line: "A fusion-powered exosuit, built and run in Studio.",
        capture: None,
        stats: &[],
    },
    ShowcaseBuild {
        slug: "climate-model",
        tag: "DATA",
        name: "Climate Model",
        line: "A climate model driven by data, built and run in Studio.",
        capture: None,
        stats: &[],
    },
    ShowcaseBuild {
        slug: "supply-chain-twin",
        tag: "EPCIS / GS1",
        name: "Supply-Chain Twin",
        line: "A supply chain twin that speaks EPCIS and GS1.",
        capture: None,
        stats: &[],
    },
    ShowcaseBuild {
        slug: "lenr-cold-fusion",
        tag: "LENR",
        name: "LENR / Cold Fusion",
        line: "A low-energy nuclear reaction model, built and run in Studio.",
        capture: None,
        stats: &[],
    },
];

/// Size of every capture, in pixels. Written on the <img> so the card does not
/// jump while the image loads.
const SHOWCASE_SHOT_WIDTH: u32 = 1280;
const SHOWCASE_SHOT_HEIGHT: u32 = 800;

/// The image slot of a card: the capture when there is one, otherwise the
/// striped placeholder carrying the build's name.
fn build_slot(build: &ShowcaseBuild) -> impl IntoView {
    let name = build.name;
    match build.capture {
        Some(path) => view! {
            <img
                class="build-shot"
                src=path
                alt=format!("{name} running in Eustress Studio")
                width=SHOWCASE_SHOT_WIDTH
                height=SHOWCASE_SHOT_HEIGHT
                loading="lazy"
                decoding="async"
            />
        }
        .into_any(),
        None => view! {
            <div class="build-slot build-slot-empty" aria-hidden="true">
                <span class="build-slot-name">{name}</span>
            </div>
        }
        .into_any(),
    }
}

#[component]
fn HomeShowcase() -> impl IntoView {
    view! {
        <section class="showcase-industrial">
            <div class="showcase-header">
                <div class="header-line"></div>
                <span class="header-tag">"REFERENCE BUILDS"</span>
                <div class="header-line"></div>
            </div>
            <h2 class="showcase-title">"What Eustress Simulates"</h2>
            <p class="showcase-subtitle">"Real models running in the engine today"</p>

            <div class="showcase-grid-industrial">
                {SHOWCASE_BUILDS
                    .iter()
                    .enumerate()
                    .map(|(i, build)| {
                        view! {
                            <article
                                class={if i == 0 { "showcase-card featured" } else { "showcase-card" }}
                                data-build={build.slug}
                            >
                                <div class="card-visual">{build_slot(build)}</div>
                                <div class="card-info">
                                    <span class="info-tag">{build.tag}</span>
                                    <h4>{build.name}</h4>
                                    <p>{build.line}</p>
                                    {(!build.stats.is_empty()).then(|| {
                                        view! {
                                            <ul class="build-stats">
                                                {build
                                                    .stats
                                                    .iter()
                                                    .map(|(figure, label)| {
                                                        view! {
                                                            <li class="build-stat">
                                                                <span class="build-stat-figure">{*figure}</span>
                                                                <span class="build-stat-label">{*label}</span>
                                                            </li>
                                                        }
                                                    })
                                                    .collect_view()}
                                            </ul>
                                            <p class="build-note">{SIMULATED_NOTE}</p>
                                        }
                                    })}
                                </div>
                            </article>
                        }
                    })
                    .collect_view()}
            </div>

            <a href="/gallery" class="btn-secondary-steel showcase-btn">
                "Explore All Simulations"
                <span class="btn-icon">"→"</span>
            </a>
        </section>
    }
}

// -----------------------------------------------------------------------------
// The proof and the invitation, one section: the source is the evidence, and
// the source is open to the reader.
// -----------------------------------------------------------------------------

#[component]
fn HomeProof() -> impl IntoView {
    view! {
        <section class="github-section proof-section">
            <div class="github-bg">
                <div class="github-grid-overlay"></div>
            </div>
            <div class="github-inner">
                <div class="github-copy">
                    <span class="section-tag">"The proof is the source"</span>
                    <h2 class="section-title-epic">"Read every line. Build it with us."</h2>

                    <ul class="proof-points">
                        <li>
                            <span class="feature-check" aria-hidden="true">"✓"</span>
                            <span>
                                <strong>"Source-available:"</strong>
                                " the clock, the physics and the data layer, under PolyForm Shield."
                            </span>
                        </li>
                        <li>
                            <span class="feature-check" aria-hidden="true">"✓"</span>
                            <span>
                                <strong>"Rust end to end:"</strong>
                                " memory safety is structural."
                            </span>
                        </li>
                        <li>
                            <span class="feature-check" aria-hidden="true">"✓"</span>
                            <span>
                                <strong>"Contributors rank up:"</strong>
                                " open a pull request, earn points, become a contributor."
                            </span>
                        </li>
                    </ul>

                    <div class="github-cta">
                        <a href="https://github.com/WeaveITMeta/EustressEngine" target="_blank" rel="noopener" class="btn-primary-steel">
                            "★ Star on GitHub"
                            <span class="btn-icon">"→"</span>
                        </a>
                        <a href="https://github.com/WeaveITMeta/EustressEngine/pulls" target="_blank" rel="noopener" class="btn-secondary-steel">
                            "Open a pull request"
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
    }
}

/// The closing band: the same backdrop treatment as the hero, one action.
#[component]
fn HomeCta() -> impl IntoView {
    view! {
        <section class="cta-industrial cta-horizon">
            <div class="cta-bg">
                <div class="cta-grid-overlay"></div>
                <div class="cta-glow-orb"></div>
            </div>
            <div class="cta-container">
                <h2 class="cta-headline">"Ready to Model the "<span class="cta-accent">"Real World"</span>"?"</h2>
                <p class="cta-subtext">"Join the builders simulating reality, and owning every bit of it."</p>

                <a href="/download" class="btn-primary-steel cta-btn">
                    "Download Studio"
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

// Not shown: the home page hero is the still image again. Kept for when the reel returns.
#[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn render<V: IntoView>(f: impl FnOnce() -> V) -> String {
        let owner = Owner::new();
        owner.with(|| f().to_html())
    }

    /// The visible text of rendered markup: tags gone, block tags and <br/> read as a space.
    fn text_of(html: &str) -> String {
        let mut out = String::new();
        let mut rest = html;
        while let Some(open) = rest.find('<') {
            out.push_str(&rest[..open]);
            let close = rest[open..].find('>').map(|i| open + i + 1).unwrap_or(rest.len());
            let name: String = rest[open + 1..close]
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if matches!(
                name.as_str(),
                "br" | "div" | "li" | "ul" | "p" | "h1" | "h2" | "h3" | "h4" | "section" | "summary"
                    | "details" | "tr" | "td" | "th" | "article" | "figure"
            ) {
                out.push(' ');
            }
            rest = &rest[close..];
        }
        out.push_str(rest);
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The first tag named NAME at or after FROM, up to and including its closing tag.
    fn element<'a>(html: &'a str, name: &str, from: usize) -> &'a str {
        let open = from + html[from..].find(&format!("<{name}")).unwrap_or_else(|| panic!("no <{name}> in {html}"));
        if matches!(name, "img" | "br") {
            // a void element has no closing tag
            return &html[open..open + html[open..].find('>').expect("end of tag") + 1];
        }
        let close_tag = format!("</{name}>");
        let close = open + html[open..].find(&close_tag).unwrap_or_else(|| panic!("no {close_tag}")) + close_tag.len();
        &html[open..close]
    }

    /// The element named NAME whose opening tag is the nearest one before MARKER, through its closing tag.
    fn around<'a>(html: &'a str, marker: &str, name: &str) -> &'a str {
        let at = html.find(marker).unwrap_or_else(|| panic!("no {marker:?} in {html}"));
        let open = html[..at].rfind(&format!("<{name}")).unwrap_or_else(|| panic!("no <{name}> before {marker:?}"));
        element(html, name, open)
    }

    fn hero() -> String {
        render(HomeHero)
    }

    fn no_dashes(label: &str, html: &str) {
        let text = text_of(html);
        assert!(!text.contains('\u{2014}') && !text.contains('\u{2013}'), "{label} has an em or en dash: {text}");
    }

    // ---- the hero ----

    #[test]
    fn the_hero_has_one_primary_action_and_it_downloads_studio() {
        let html = hero();
        let start = html.find("class=\"hero-buttons\"").expect("hero buttons");
        let end = start + html[start..].find("</div>").expect("end of buttons");
        let buttons = &html[start..end];
        let first = element(buttons, "a", 0);
        assert!(first.contains("href=\"/download\""), "primary goes to /download: {first}");
        assert!(first.contains("btn-primary-steel"), "primary is the filled button: {first}");
        assert!(text_of(first).starts_with("Download Studio"), "label: {first}");
        assert!(first.contains("btn-icon\">→<"), "arrow: {first}");
        let second = element(buttons, "a", buttons.find(first).unwrap() + first.len());
        assert!(second.contains("href=\"/gallery\"") && second.contains("btn-secondary-steel"), "{second}");
        assert_eq!(text_of(second), "Browse the Gallery");
        assert_eq!(buttons.matches("<a ").count(), 2, "two actions, no third: {buttons}");
    }

    #[test]
    fn no_hero_button_goes_to_about() {
        let html = hero();
        assert!(!html.contains("/about"), "the About button is gone from the hero");
        assert!(!text_of(&html).contains("About"), "{}", text_of(&html));
    }

    #[test]
    fn the_headline_and_metrics_are_unchanged_and_the_description_says_what_eustress_is() {
        let html = hero();
        assert_eq!(text_of(element(&html, "h1", 0)), "The Future of Creation");
        assert!(html.contains("<span class=\"headline-accent\">Creation</span>"), "{html}");
        let desc = text_of(around(&html, "hero-description", "p"));
        assert_eq!(desc, "Eustress is a source-available simulation and data platform built in Rust; it also builds games and interactive worlds.");
        no_dashes("the hero paragraph", around(&html, "hero-description", "p"));
        for (value, label) in [("10M+", "Entities"), ("1 yr/s", "Sim Speed"), ("60+", "FPS")] {
            let cell = format!("<span class=\"metric-value\">{value}</span><span class=\"metric-label\">{label}</span>");
            assert!(html.contains(&cell), "{value} {label}: {html}");
        }
        assert!(html.contains("RUNS ON"), "platform bar");
    }

    #[test]
    fn the_hero_keeps_the_real_studio_capture_and_the_city_backdrop_is_css_only() {
        let html = hero();
        let img = around(&html, "src=\"/assets/hero-render.png\"", "img");
        assert!(img.contains("src=\"/assets/hero-render.png\""), "{img}");
        assert!(img.contains("width=\"1334\"") && img.contains("height=\"714\"") && img.contains("alt=\"The Eustress Studio editor"), "{img}");
        // The backdrop is decoration: it is a CSS background, so no element, no alt text, nothing announced.
        assert!(html.contains("hero-industrial hero-city"));
        assert!(!html.contains("hero-city.webp") && !html.contains("cta-horizon.webp"), "no <img> for a backdrop");
        let css = include_str!("../../style/home.css");
        assert!(css.contains(".hero-industrial.hero-city"), "the hero backdrop rule");
        assert!(css.contains("url(\"/assets/home/hero-city.webp\")"), "hero backdrop file");
        assert!(css.contains("linear-gradient(90deg"), "dark gradient from the left");
        assert!(css.contains("background-position: right"), "anchored right");
        assert!(css.contains("background-size: cover"), "covers the hero");
        assert!(css.contains("@media (max-width: 768px)"), "a separate phone treatment");
    }

    #[test]
    fn the_new_stylesheet_is_linked_after_main_css() {
        let index = include_str!("../../index.html");
        let main = index.find("href=\"style/main.css\"").expect("main.css link");
        let home = index.find("<link data-trunk rel=\"css\" href=\"style/home.css\">").expect("home.css must be linked in index.html");
        assert!(home > main, "home.css after main.css so its rules win");
    }

    #[test]
    fn the_backdrop_images_exist_and_are_webp() {
        let hero: &[u8] = include_bytes!("../../assets/home/hero-city.webp");
        let cta: &[u8] = include_bytes!("../../assets/home/cta-horizon.webp");
        for (name, bytes) in [("hero-city", hero), ("cta-horizon", cta)] {
            assert!(bytes.len() > 10_000, "{name} is a real file: {} bytes", bytes.len());
            assert_eq!(&bytes[0..4], b"RIFF", "{name}");
            assert_eq!(&bytes[8..12], b"WEBP", "{name}");
        }
        let css = include_str!("../../style/home.css");
        assert!(css.contains("url(\"/assets/home/cta-horizon.webp\")"));
    }

    // ---- the comparison ----

    #[test]
    fn the_simulation_table_is_first_and_open_and_the_game_engine_table_is_folded_away() {
        let html = render(HomeComparison);
        let sim_title = html.find("Simulation Software Comparison").expect("sim title");
        let omniverse = html.find("Omniverse").expect("sim table");
        let details = html.find("<details").expect("a <details> holds the game engine table");
        assert!(sim_title < omniverse && omniverse < details, "the simulation table comes first, outside the <details>");
        assert_eq!(html.matches("<details").count(), 1);
        let open_tag = &html[details..details + html[details..].find('>').unwrap()];
        assert!(!open_tag.contains("open"), "closed by default: {open_tag}");
        let end = details + html[details..].find("</details>").expect("</details>");
        let inside = &html[details..end];
        assert!(text_of(element(inside, "summary", 0)) == "Compare with Roblox, Unity and Unreal", "{inside}");
        for needle in ["Game Engine Comparison", "Roblox", "Unity", "Unreal", "Lua VM"] {
            assert!(inside.contains(needle), "{needle} lives inside the <details>");
        }
        for needle in ["Omniverse", "AnyLogic", "MATLAB", "Simulation Software Comparison"] {
            assert!(!inside.contains(needle), "{needle} stays outside the <details>");
        }
        let verdict = html.find("comparison-verdict").expect("verdict cards");
        assert!(verdict > end, "the verdict cards follow the folded table");
        assert_eq!(html.matches("<table").count(), 2);
    }

    #[test]
    fn eustress_matches_omniverse_on_rtx_for_the_real_time_world_and_photoreal_rows() {
        // McKale (2026-10-15): Eustress supports RTX like Omniverse in these two rows.
        let html = render(HomeComparison);
        for (row, cell) in [("Real-time 3D world", " RTX"), ("Photoreal rendering", " RTX ray-traced")] {
            let at = html.find(row).unwrap_or_else(|| panic!("{row} row"));
            let tr = &html[at..at + html[at..].find("</tr>").expect("</tr>")];
            let eustress = &tr[tr.find("class=\"eustress\"").expect("eustress cell")..];
            let eustress = &eustress[..eustress.find("</td>").unwrap()];
            assert!(eustress.contains("✓") && eustress.ends_with(cell), "{row}: {eustress}");
        }
    }

    // ---- the reference builds ----

    #[test]
    fn the_showcase_has_five_builds_and_v_cell_is_the_featured_one_with_its_real_line() {
        assert_eq!(SHOWCASE_BUILDS.len(), 5);
        let names: Vec<_> = SHOWCASE_BUILDS.iter().map(|b| (b.name, b.tag)).collect();
        assert_eq!(
            names,
            [
                ("V-Cell Battery", "DIGITAL TWIN"),
                ("Fusion Exosuit", "ENERGY"),
                ("Climate Model", "DATA"),
                ("Supply-Chain Twin", "EPCIS / GS1"),
                ("LENR / Cold Fusion", "LENR"),
            ]
        );
        let slugs: Vec<_> = SHOWCASE_BUILDS.iter().map(|b| b.slug).collect();
        assert_eq!(slugs, ["v-cell", "fusion-exosuit", "climate-model", "supply-chain-twin", "lenr-cold-fusion"]);
        assert_eq!(
            SHOWCASE_BUILDS[0].line,
            "An anode-lean lithium-sulfur cell that stacks 459 bipolar layers inside one 300 × 100 × 100 mm can, so a single cell delivers 987 V and 2,386 Wh."
        );
        let html = render(HomeShowcase);
        assert_eq!(html.matches("showcase-card featured").count(), 1);
        let featured = around(&html, "showcase-card featured", "article");
        assert!(featured.contains("data-build=\"v-cell\""), "{featured}");
        assert_eq!(html.matches("<article").count(), 5);
    }

    #[test]
    fn the_v_cell_card_shows_its_five_figures_in_order_and_none_of_the_old_ones() {
        let html = render(HomeShowcase);
        let card = around(&html, "data-build=\"v-cell\"", "article");
        let want = [
            ("544", "Wh/kg, pack"),
            ("987", "volts, one cell"),
            ("2,386", "watt-hours"),
            ("889", "cycles to 80%, device duty"),
            ("591", "kWh/kg over a service life, vehicle duty"),
        ];
        assert_eq!(SHOWCASE_BUILDS[0].stats, &want[..]);
        assert!(card.contains(&format!("<p class=\"build-note\">{SIMULATED_NOTE}</p>")), "the figures say they are simulated");
        assert_eq!(html.matches("class=\"build-note\"").count(), 1, "only the card with figures carries the note");
        let mut last = 0;
        for (figure, label) in want {
            let pair = format!("<span class=\"build-stat-figure\">{figure}</span><span class=\"build-stat-label\">{label}</span>");
            let at = card.find(&pair).unwrap_or_else(|| panic!("missing {figure} / {label} in {card}"));
            assert!(at > last, "{figure} is out of order");
            last = at;
        }
        assert_eq!(card.matches("build-stat-figure").count(), 5);
        assert_eq!(html.matches("build-stat-figure").count(), 5, "only the V-Cell card has figures");
        let text = text_of(&html);
        for old in ["569", "1,223", "1,032", "699", "Anode-free", "2,930", "station, projected"] {
            assert!(!text.contains(old), "the old V-Cell figure {old} is still on the page");
        }
        assert!(text.contains("so a single cell delivers 987 V and 2,386 Wh."), "{text}");
        let css = include_str!("../../style/home.css");
        assert!(css.contains("font-variant-numeric: tabular-nums"), "figures use tabular numbers");
        assert!(css.contains(".build-stat-figure") && css.contains("var(--font-mono)"), "figures use the mono face");
    }

    #[test]
    fn the_four_other_builds_carry_no_invented_numbers() {
        for b in &SHOWCASE_BUILDS[1..] {
            assert!(b.stats.is_empty(), "{} has figures nobody gave", b.name);
            // A word with a digit may only be one the card already names (GS1).
            for word in b.line.split_whitespace().filter(|w| w.chars().any(|c| c.is_ascii_digit())) {
                let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
                assert!(b.tag.contains(word) || b.name.contains(word), "{}: an invented number {word:?} in {}", b.name, b.line);
            }
        }
    }

    #[test]
    fn a_card_without_a_capture_shows_the_placeholder_and_never_a_bare_image() {
        let html = render(HomeShowcase);
        let without = SHOWCASE_BUILDS.iter().filter(|b| b.capture.is_none()).count();
        let with = SHOWCASE_BUILDS.len() - without;
        assert_eq!(html.matches("build-slot-empty").count(), without, "one placeholder per card with no capture");
        assert_eq!(html.matches("<img").count(), with, "an image only where there is a capture");
        for chunk in html.split("<img").skip(1) {
            let tag = &chunk[..chunk.find('>').unwrap()];
            assert!(tag.contains("src=\"") && !tag.contains("src=\"\""), "an <img> without a src: {tag}");
        }
        for b in SHOWCASE_BUILDS.iter().filter(|b| b.capture.is_none()) {
            let marker = format!("data-build=\"{}\"", b.slug);
            let card = around(&html, &marker, "article");
            assert!(card.contains("build-slot-empty"), "{}: {card}", b.name);
            assert!(card.contains(&format!("<span class=\"build-slot-name\">{}</span>", b.name)), "{}: the placeholder names the build", b.name);
            assert!(card.contains("aria-hidden=\"true\""), "the placeholder is decoration");
        }
    }

    #[test]
    fn a_card_with_a_capture_shows_the_image_with_size_and_alt_text() {
        let build = ShowcaseBuild {
            slug: "test-build",
            tag: "TEST",
            name: "Test Build",
            line: "A line.",
            capture: Some("/assets/showcase/test-build.webp"),
            stats: &[],
        };
        let html = render(|| build_slot(&build));
        let img = element(&html, "img", 0);
        assert!(img.contains("src=\"/assets/showcase/test-build.webp\""), "{img}");
        assert!(img.contains(&format!("width=\"{SHOWCASE_SHOT_WIDTH}\"")) && img.contains(&format!("height=\"{SHOWCASE_SHOT_HEIGHT}\"")), "{img}");
        assert!(img.contains("alt=\"Test Build running in Eustress Studio\""), "{img}");
        assert!(!html.contains("build-slot-empty"), "{html}");
    }

    #[test]
    fn every_capture_path_follows_the_slug_and_the_file_exists() {
        let mut slugs = std::collections::HashSet::new();
        for b in SHOWCASE_BUILDS {
            assert!(slugs.insert(b.slug), "duplicate slug {}", b.slug);
            assert!(b.slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{}", b.slug);
            if let Some(path) = b.capture {
                assert_eq!(path, format!("/assets/showcase/{}.webp", b.slug));
                let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path.trim_start_matches('/'));
                assert!(file.exists(), "{} names a capture that is not in assets/showcase: {}", b.name, file.display());
            }
        }
    }

    // ---- the proof and the invitation ----

    #[test]
    fn the_proof_and_the_github_invitation_are_one_section() {
        let html = render(HomeProof);
        assert_eq!(html.matches("<section").count(), 1);
        let text = text_of(&html);
        assert!(text.contains("The proof is the source"), "{text}");
        assert_eq!(text_of(element(&html, "h2", 0)), "Read every line. Build it with us.");
        for line in [
            "Source-available: the clock, the physics and the data layer, under PolyForm Shield.",
            "Rust end to end: memory safety is structural.",
            "Contributors rank up: open a pull request, earn points, become a contributor.",
        ] {
            assert!(text.contains(line), "missing tick line {line:?} in {text}");
        }
        assert_eq!(html.matches("feature-check").count(), 3, "three ticked lines");
        let star = around(&html, "Star on GitHub", "a");
        assert!(star.contains("href=\"https://github.com/WeaveITMeta/EustressEngine\""), "{star}");
        let pr = around(&html, "Open a pull request", "a");
        assert!(pr.contains("href=\"https://github.com/WeaveITMeta/EustressEngine/pulls\""), "{pr}");
        assert!(text.contains("$ git clone github.com/WeaveITMeta/EustressEngine"), "the terminal stays: {text}");
        assert!(text.contains("✓ Eustress running. The world is yours."), "{text}");
        // the old pieces are gone
        for gone in ["Don't Take Our Word For It", "Build It With Us", "ladder", "testimonials-industrial", "Runs today"] {
            assert!(!html.contains(gone), "{gone} should be gone");
        }
    }

    // ---- the closing band ----

    #[test]
    fn the_closing_band_asks_for_the_download_and_matches_the_hero() {
        let html = render(HomeCta);
        assert_eq!(text_of(element(&html, "h2", 0)), "Ready to Model the Real World?");
        assert!(html.contains("<span class=\"cta-accent\">Real World</span>"), "{html}");
        let text = text_of(&html);
        assert!(text.contains("Join the builders simulating reality, and owning every bit of it."), "{text}");
        for tick in ["Source-available, PolyForm Shield", "Free forever, no platform tax", "Fork it and ship your fork"] {
            assert!(text.contains(tick), "{tick}");
        }
        let button = element(&html, "a", 0);
        assert!(button.contains("href=\"/download\""), "{button}");
        assert!(button.contains("btn-primary-steel") && button.contains("cta-btn"), "{button}");
        assert!(text_of(button).starts_with("Download Studio") && button.contains("btn-icon\">→<"), "{button}");
        assert_eq!(html.matches("<a ").count(), 1, "one action");
        assert!(!html.contains("/login") && !html.contains("Start Free Today"));
        assert!(html.contains("cta-industrial cta-horizon"), "the band carries the horizon backdrop class");
        assert!(include_str!("../../style/home.css").contains(".cta-industrial.cta-horizon"));
    }

    // ---- the page as a whole ----

    #[test]
    fn the_page_is_the_nav_then_six_sections_in_order_then_the_footer() {
        let src = include_str!("home.rs");
        let start = src.find("pub fn HomePage()").expect("HomePage");
        let end = src.find("fn HomeHero(").expect("HomeHero");
        let page = &src[start..end];
        assert!(!page.contains("<section"), "every section is its own component");
        let mut last = 0;
        for name in ["CentralNav", "HomeHero", "WhatsInsideTabs", "HomeComparison", "HomeShowcase", "HomeProof", "HomeCta", "Footer"] {
            let tag = format!("<{name} ");
            assert_eq!(page.matches(tag.as_str()).count(), 1, "{name} appears once in HomePage");
            let at = page.find(tag.as_str()).unwrap();
            assert!(at > last, "{name} is out of order");
            last = at;
        }
    }

    /// The whole home page, rendered as the prerender does: nav, every section and the footer.
    #[cfg(feature = "ssr")]
    fn render_home_page() -> String {
        use hydration_context::{SharedContext, SsrSharedContext};
        use leptos_router::location::RequestUrl;
        let shared: std::sync::Arc<dyn SharedContext + Send + Sync> = std::sync::Arc::new(SsrSharedContext::new());
        let owner = Owner::new_root(Some(shared));
        let html = owner.with(|| {
            provide_context(RequestUrl::new("/"));
            crate::app::App().to_html()
        });
        owner.cleanup();
        html
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_home_page_renders_the_footer_after_the_closing_band() {
        let html = render_home_page();
        assert!(html.contains("The Future of"), "this is the home page");
        let cta = html.find("Ready to Model the").expect("closing band");
        let footer = html.find("<footer").expect("the home page renders its footer");
        assert!(html.contains("footer-industrial"), "the site footer");
        assert!(cta < footer, "the footer comes after the closing band");
        assert_eq!(html.matches("<footer").count(), 1);
        let end = footer + html[footer..].find("</footer>").expect("</footer>") + "</footer>".len();
        assert!(!text_of(&html[end..]).contains("Ready to Model"), "nothing of the page follows the footer");
    }

    #[test]
    fn no_dashes_in_the_text_people_read() {
        no_dashes("hero", &hero());
        no_dashes("comparison", &render(HomeComparison));
        no_dashes("showcase", &render(HomeShowcase));
        no_dashes("proof", &render(HomeProof));
        no_dashes("cta", &render(HomeCta));
        for b in SHOWCASE_BUILDS {
            assert!(!b.line.contains('\u{2014}') && !b.line.contains('\u{2013}'), "{}", b.line);
        }
    }
}
