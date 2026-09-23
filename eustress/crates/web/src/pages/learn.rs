// =============================================================================
// Eustress Web - Learn Page
// =============================================================================
// The index of every Learn guide: a short path for newcomers, then every topic
// grouped into tracks (Build, Script, Simulate, Ship, Agents & Tools,
// Foundations), with each guide's level and reading time.
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// Data
// -----------------------------------------------------------------------------

/// One guide in the Learn index.
struct Topic {
    href: &'static str,
    title: &'static str,
    icon: &'static str,
    summary: &'static str,
    level: &'static str,
    minutes: u32,
    is_new: bool,
}

/// One step of the newcomer path.
struct Step {
    href: &'static str,
    title: &'static str,
    summary: &'static str,
}

const START_HERE: &[Step] = &[
    Step {
        href: "/docs/getting-started",
        title: "Getting Started",
        summary: "Install Eustress Engine, create a Universe and a Space, and press Play.",
    },
    Step {
        href: "/docs/studio",
        title: "Studio",
        summary: "Find your way around the editor: Explorer, Properties, tools and shortcuts.",
    },
    Step {
        href: "/docs/building",
        title: "Building",
        summary: "Build with parts, materials, CSG, terrain and light.",
    },
    Step {
        href: "/docs/scripting",
        title: "Scripting",
        summary: "Give the world behavior with Rune, and build from the command bar in Luau.",
    },
    Step {
        href: "/docs/simulation",
        title: "Simulation",
        summary: "Run it forward, measure it, and compare runs.",
    },
];

const BUILD: &[Topic] = &[
    Topic {
        href: "/docs/studio",
        title: "Studio",
        icon: "/assets/icons/monitor.svg",
        summary: "The editor: Explorer and its search, Properties, Insert Object, tools, History, shortcuts, modes and MindSpace.",
        level: "Beginner",
        minutes: 23,
        is_new: true,
    },
    Topic {
        href: "/docs/building",
        title: "Building",
        icon: "/assets/icons/cube.svg",
        summary: "Parts and their properties, materials, CSG, terrain, lights and atmosphere.",
        level: "Beginner",
        minutes: 19,
        is_new: false,
    },
    Topic {
        href: "/docs/perspective",
        title: "Perspective",
        icon: "/assets/icons/perspective.svg",
        summary: "2D and 3D views, perspective and orthographic, one world with no conversion.",
        level: "Beginner",
        minutes: 8,
        is_new: true,
    },
    Topic {
        href: "/docs/cad",
        title: "CAD",
        icon: "/assets/icons/grid.svg",
        summary: "Parametric parts on a B-rep kernel: features, variables, booleans and export.",
        level: "Intermediate",
        minutes: 18,
        is_new: true,
    },
    Topic {
        href: "/docs/importing",
        title: "Importing",
        icon: "/assets/icons/download.svg",
        summary: "Bring in Roblox places, glTF and GLB models, Gaussian splats, images, video and data.",
        level: "Intermediate",
        minutes: 19,
        is_new: true,
    },
    Topic {
        href: "/docs/ui",
        title: "UI Systems",
        icon: "/assets/icons/template.svg",
        summary: "Screen and in-world interfaces for the people inside a Space.",
        level: "Intermediate",
        minutes: 12,
        is_new: false,
    },
    Topic {
        href: "/docs/audio",
        title: "Audio",
        icon: "/assets/icons/audio.svg",
        summary: "The Sound object, SoundService, audio formats, and where playback stands.",
        level: "Beginner",
        minutes: 7,
        is_new: false,
    },
];

const SCRIPT: &[Topic] = &[
    Topic {
        href: "/docs/scripting",
        title: "Scripting",
        icon: "/assets/icons/code.svg",
        summary: "Luau, Rune and SoulScript: where scripts live, when they run, and the API they reach.",
        level: "Intermediate",
        minutes: 14,
        is_new: false,
    },
    Topic {
        href: "/docs/services",
        title: "Services",
        icon: "/assets/icons/settings.svg",
        summary: "Every service a Space starts with, what each one owns, and its properties.",
        level: "Intermediate",
        minutes: 14,
        is_new: false,
    },
];

const SIMULATE: &[Topic] = &[
    Topic {
        href: "/docs/physics",
        title: "Physics",
        icon: "/assets/icons/physics.svg",
        summary: "Avian rigid bodies, colliders, constraints, queries, deformation and fracture.",
        level: "Intermediate",
        minutes: 20,
        is_new: false,
    },
    Topic {
        href: "/docs/simulation",
        title: "Simulation",
        icon: "/assets/icons/play.svg",
        summary: "The simulation clock, time compression, watchpoints, recordings and experiments.",
        level: "Intermediate",
        minutes: 16,
        is_new: false,
    },
    Topic {
        href: "/docs/realism",
        title: "Realism",
        icon: "/assets/icons/fire.svg",
        summary: "Material science, thermodynamics and electrochemistry models, with a worked case study.",
        level: "Advanced",
        minutes: 20,
        is_new: false,
    },
    Topic {
        href: "/docs/universes",
        title: "Universes",
        icon: "/assets/icons/star.svg",
        summary: "Where work lives: Space folders, the WorldDb, git history, and branches for what-if runs.",
        level: "Intermediate",
        minutes: 15,
        is_new: false,
    },
];

const SHIP: &[Topic] = &[
    Topic {
        href: "/docs/networking",
        title: "Networking",
        icon: "/assets/icons/network.svg",
        summary: "What runs today for multiplayer, and the transport that comes next.",
        level: "Intermediate",
        minutes: 12,
        is_new: false,
    },
    Topic {
        href: "/docs/publishing",
        title: "Publishing",
        icon: "/assets/icons/upload.svg",
        summary: "Publish a Universe from Studio: sign-in, the package, review, and its page on eustress.dev.",
        level: "Intermediate",
        minutes: 13,
        is_new: false,
    },
    Topic {
        href: "/docs/website",
        title: "Website Service",
        icon: "/assets/icons/web.svg",
        summary: "Mark values as References, publish once, and every number on your site updates from one fetch.",
        level: "Intermediate",
        minutes: 19,
        is_new: false,
    },
    Topic {
        href: "/docs/earning",
        title: "Earning",
        icon: "/assets/icons/bliss.svg",
        summary: "Bliss (BLS) for contributions, Tickets (TKT), and how payouts work.",
        level: "Intermediate",
        minutes: 17,
        is_new: false,
    },
];

const AGENTS: &[Topic] = &[
    Topic {
        href: "/learn/mcp",
        title: "MCP Server",
        icon: "/assets/icons/sparkles.svg",
        summary: "Give an AI agent tools to read, build and simulate inside your Spaces.",
        level: "Intermediate",
        minutes: 20,
        is_new: false,
    },
    Topic {
        href: "/learn/cli",
        title: "CLI & Headless",
        icon: "/assets/icons/list.svg",
        summary: "Open, list and close engines from a terminal, and run Spaces with no window.",
        level: "Intermediate",
        minutes: 17,
        is_new: true,
    },
    Topic {
        href: "/learn/ide",
        title: "IDE Integration",
        icon: "/assets/icons/edit.svg",
        summary: "Edit scripts in your own editor, with changes picked up by the running engine.",
        level: "Beginner",
        minutes: 8,
        is_new: false,
    },
    Topic {
        href: "/learn/lsp",
        title: "Rune LSP",
        icon: "/assets/icons/brain.svg",
        summary: "The Rune language server: diagnostics, hover and navigation for Rune scripts.",
        level: "Intermediate",
        minutes: 10,
        is_new: false,
    },
];

const FOUNDATIONS: &[Topic] = &[
    Topic {
        href: "/docs/getting-started",
        title: "Getting Started",
        icon: "/assets/icons/rocket.svg",
        summary: "From install to a first Space: a part, Play mode, a save and a first script.",
        level: "Beginner",
        minutes: 13,
        is_new: false,
    },
    Topic {
        href: "/docs/philosophy",
        title: "Philosophy",
        icon: "/assets/icons/book.svg",
        summary: "Why Eustress is built the way it is, and the mechanism behind each principle.",
        level: "Beginner",
        minutes: 11,
        is_new: false,
    },
];

// -----------------------------------------------------------------------------
// Components
// -----------------------------------------------------------------------------

/// A grid of guide cards.
#[component]
fn TopicGrid(topics: &'static [Topic]) -> impl IntoView {
    view! {
        <div class="docs-grid">
            {topics.iter().map(|t| view! {
                <a href=t.href class="doc-card">
                    <div class="doc-card-top">
                        <img src=t.icon alt="" class="doc-icon" />
                        {t.is_new.then(|| view! { <span class="doc-badge">"NEW"</span> })}
                    </div>
                    <h3>{t.title}</h3>
                    <p>{t.summary}</p>
                    <div class="doc-card-meta">
                        <span>{t.level}</span>
                        <span>{format!("{} min read", t.minutes)}</span>
                    </div>
                </a>
            }).collect::<Vec<_>>()}
        </div>
    }
}

/// Learn page: the index of every guide.
#[component]
pub fn LearnPage() -> impl IntoView {
    let guide_count = BUILD.len() + SCRIPT.len() + SIMULATE.len() + SHIP.len() + AGENTS.len() + FOUNDATIONS.len();

    view! {
        <div class="page page-learn-industrial">
            <CentralNav active="learn".to_string() />

            <div class="learn-bg">
                <div class="learn-grid-overlay"></div>
                <div class="learn-glow glow-1"></div>
                <div class="learn-glow glow-2"></div>
            </div>

            // Hero
            <section class="learn-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"LEARN"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="learn-title">"Learn Eustress"</h1>
                <p class="learn-subtitle">
                    "Eustress is a source-available simulation and data platform built in Rust.
                    These guides cover building a Space, giving it behavior, simulating it,
                    publishing it, and handing it to AI agents."
                </p>
                <nav class="learn-jump" aria-label="Tracks">
                    <a href="#start" class="chip">"Start Here"</a>
                    <a href="#build" class="chip">"Build"</a>
                    <a href="#script" class="chip">"Script"</a>
                    <a href="#simulate" class="chip">"Simulate"</a>
                    <a href="#ship" class="chip">"Ship"</a>
                    <a href="#agents" class="chip">"Agents & Tools"</a>
                    <a href="#foundations" class="chip">"Foundations"</a>
                </nav>
                <p class="learn-count">{format!("{} guides, each ending with what comes next", guide_count)}</p>
            </section>

            // Start here
            <section id="start" class="learn-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/rocket.svg" alt="Start" class="section-icon" />
                    <h2>"Start Here"</h2>
                </div>
                <p class="learn-lede">
                    "New to Eustress? Read these five in order, and branch into any track below."
                </p>
                <ol class="learn-path">
                    {START_HERE.iter().enumerate().map(|(i, s)| view! {
                        <li>
                            <a href=s.href class="learn-step">
                                <span class="learn-step-num">{i + 1}</span>
                                <div class="learn-step-body">
                                    <h3>{s.title}</h3>
                                    <p>{s.summary}</p>
                                </div>
                            </a>
                        </li>
                    }).collect::<Vec<_>>()}
                </ol>
            </section>

            // Tracks
            <section id="build" class="learn-section learn-track">
                <div class="learn-track-header">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/cube.svg" alt="Build" class="section-icon" />
                        <h2>"Build"</h2>
                    </div>
                    <p class="learn-lede">
                        "Make the world: the editor, parts and materials, 2D and 3D views, parametric
                        CAD, imports, interfaces and sound."
                    </p>
                </div>
                <TopicGrid topics=BUILD />
            </section>

            <section id="script" class="learn-section learn-track">
                <div class="learn-track-header">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/code.svg" alt="Script" class="section-icon" />
                        <h2>"Script"</h2>
                    </div>
                    <p class="learn-lede">
                        "Give it behavior with scripts, through the services every Space starts with."
                    </p>
                </div>
                <TopicGrid topics=SCRIPT />
            </section>

            <section id="simulate" class="learn-section learn-track">
                <div class="learn-track-header">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/play.svg" alt="Simulate" class="section-icon" />
                        <h2>"Simulate"</h2>
                    </div>
                    <p class="learn-lede">
                        "Run it: physics, the simulation clock, physical-law models, and Universes
                        you can branch."
                    </p>
                </div>
                <TopicGrid topics=SIMULATE />
            </section>

            <section id="ship" class="learn-section learn-track">
                <div class="learn-track-header">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/upload.svg" alt="Ship" class="section-icon" />
                        <h2>"Ship"</h2>
                    </div>
                    <p class="learn-lede">
                        "Share it: multiplayer, publishing, live values for websites, and earning."
                    </p>
                </div>
                <TopicGrid topics=SHIP />
            </section>

            <section id="agents" class="learn-section learn-track">
                <div class="learn-track-header">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/sparkles.svg" alt="Agents and tools" class="section-icon" />
                        <h2>"Agents & Tools"</h2>
                    </div>
                    <p class="learn-lede">
                        "Drive it from outside: AI agents over MCP, the command line, and your own editor."
                    </p>
                </div>
                <TopicGrid topics=AGENTS />
            </section>

            <section id="foundations" class="learn-section learn-track">
                <div class="learn-track-header">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/book.svg" alt="Foundations" class="section-icon" />
                        <h2>"Foundations"</h2>
                    </div>
                    <p class="learn-lede">
                        "Where to begin, and why Eustress is built the way it is."
                    </p>
                </div>
                <TopicGrid topics=FOUNDATIONS />
            </section>

            // For AI assistants
            <section class="learn-section">
                <div class="learn-agents-card">
                    <img src="/assets/icons/brain.svg" alt="" class="learn-agents-icon" />
                    <div class="learn-agents-body">
                        <h3>"Reading this as an AI assistant?"</h3>
                        <p>
                            "Every guide is served as plain HTML, and "<a href="/llms.txt">"/llms.txt"</a>
                            " lists them with one-line summaries. To act inside a Space rather than read
                            about it, connect the "<a href="/learn/mcp">"MCP server"</a>"."
                        </p>
                    </div>
                </div>
            </section>

            // Resources
            <section class="resources-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/link.svg" alt="Resources" class="section-icon" />
                    <h2>"Resources"</h2>
                </div>
                <div class="resources-grid learn-resources">
                    <a href="https://github.com/WeaveITMeta/EustressEngine" class="resource-card" target="_blank" rel="noopener">
                        <img src="/assets/icons/github.svg" alt="" class="resource-icon" />
                        <div class="resource-content">
                            <h3>"Source Code"</h3>
                            <p>"Read and build Eustress on GitHub, under PolyForm Shield 1.0.0."</p>
                        </div>
                        <img src="/assets/icons/arrow-right.svg" alt="" class="resource-arrow" />
                    </a>
                    <a href="/download" class="resource-card">
                        <img src="/assets/icons/download.svg" alt="" class="resource-icon" />
                        <div class="resource-content">
                            <h3>"Download"</h3>
                            <p>"Eustress Engine for Windows, macOS (Apple Silicon) and Linux."</p>
                        </div>
                        <img src="/assets/icons/arrow-right.svg" alt="" class="resource-arrow" />
                    </a>
                    <a href="/license" class="resource-card">
                        <img src="/assets/icons/shield.svg" alt="" class="resource-icon" />
                        <div class="resource-content">
                            <h3>"License"</h3>
                            <p>"Source-available: free to use for anything except a competing product."</p>
                        </div>
                        <img src="/assets/icons/arrow-right.svg" alt="" class="resource-arrow" />
                    </a>
                </div>
            </section>

            // Help
            <section class="help-cta">
                <div class="help-card">
                    <img src="/assets/icons/help.svg" alt="Help" class="help-icon" />
                    <div class="help-content">
                        <h3>"Stuck on something?"</h3>
                        <p>"Ask other builders in the community, or reach the team through support."</p>
                    </div>
                    <div class="help-actions">
                        <a href="/community" class="btn-help">"Community"</a>
                        <a href="/support" class="btn-help secondary">"Support"</a>
                    </div>
                </div>
            </section>

            <Footer />
        </div>
    }
}
