// =============================================================================
// Eustress Web - CLI & Headless Documentation Page
// =============================================================================
// CLI & Headless: the eustress command, the eustress-headless simulator, many
// engines on one Universe, the bridge protocol, and the utility programs.
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[derive(Clone, Debug, PartialEq)]
struct TocSection {
    id: &'static str,
    title: &'static str,
    subsections: Vec<TocSubsection>,
}

#[derive(Clone, Debug, PartialEq)]
struct TocSubsection {
    id: &'static str,
    title: &'static str,
}

fn get_toc() -> Vec<TocSection> {
    vec![
        TocSection {
            id: "overview",
            title: "Overview",
            subsections: vec![
                TocSubsection { id: "overview-programs", title: "The Programs" },
                TocSubsection { id: "overview-build", title: "Build Them" },
            ],
        },
        TocSection {
            id: "cli",
            title: "The eustress Command",
            subsections: vec![
                TocSubsection { id: "cli-verbs", title: "Verbs" },
                TocSubsection { id: "cli-open", title: "Open, List, Close" },
                TocSubsection { id: "cli-bridge", title: "Drive an Engine" },
                TocSubsection { id: "cli-run", title: "One-Shot Runs" },
            ],
        },
        TocSection {
            id: "headless",
            title: "Headless",
            subsections: vec![
                TocSubsection { id: "headless-what", title: "What It Runs" },
                TocSubsection { id: "headless-flags", title: "Flags" },
                TocSubsection { id: "headless-lifecycle", title: "A Run, Start to Finish" },
                TocSubsection { id: "headless-limits", title: "Limits" },
            ],
        },
        TocSection {
            id: "multi",
            title: "Many Engines",
            subsections: vec![
                TocSubsection { id: "multi-owner", title: "One Port File, One Owner" },
                TocSubsection { id: "multi-registry", title: "The Instance Registry" },
                TocSubsection { id: "multi-sim", title: "Simulations per Engine" },
            ],
        },
        TocSection {
            id: "bridge",
            title: "The Bridge",
            subsections: vec![
                TocSubsection { id: "bridge-wire", title: "Wire Format" },
                TocSubsection { id: "bridge-methods", title: "Methods" },
                TocSubsection { id: "bridge-client", title: "The Shared Client" },
            ],
        },
        TocSection {
            id: "utilities",
            title: "Utility Programs",
            subsections: vec![
                TocSubsection { id: "utilities-space", title: "eustress-space" },
                TocSubsection { id: "utilities-db", title: "Database Tools" },
                TocSubsection { id: "utilities-other", title: "Generators and the LSP" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-render", title: "Headless Capture" },
                TocSubsection { id: "roadmap-verbs", title: "Remaining Verbs" },
            ],
        },
    ]
}

/// Three engines on one Universe: each writes its own registry record, while
/// the Universe's single engine.port slot names only the owner.
#[component]
fn InstancesDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 250" role="img"
                aria-label="The eustress command reaches three engines on one Universe. Each engine writes its own record into the instance registry, while the Universe's engine.port file names only one of them, the owner.">
                <defs>
                    <marker id="cli-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>
                // The CLI
                <rect x="16" y="97" width="128" height="56" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="80" y="122" class="dg-label" text-anchor="middle">"eustress"</text>
                <text x="80" y="140" class="dg-note" text-anchor="middle">"open, bridge --pid"</text>
                <line x1="144" y1="118" x2="226" y2="44" class="dg-line" marker-end="url(#cli-arrow)"></line>
                <line x1="144" y1="125" x2="226" y2="125" class="dg-line" marker-end="url(#cli-arrow)"></line>
                <line x1="144" y1="132" x2="226" y2="206" class="dg-line" marker-end="url(#cli-arrow)"></line>

                // Three engines on one Universe
                <rect x="226" y="18" width="176" height="52" rx="8" class="dg-box"></rect>
                <text x="314" y="40" class="dg-label" text-anchor="middle">"Studio, pid 18020"</text>
                <text x="314" y="58" class="dg-note" text-anchor="middle">"port 52961"</text>
                <rect x="226" y="99" width="176" height="52" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="314" y="121" class="dg-label" text-anchor="middle">"headless, pid 18244"</text>
                <text x="314" y="139" class="dg-note" text-anchor="middle">"port 53117"</text>
                <rect x="226" y="180" width="176" height="52" rx="8" class="dg-box"></rect>
                <text x="314" y="202" class="dg-label" text-anchor="middle">"headless, pid 18410"</text>
                <text x="314" y="220" class="dg-note" text-anchor="middle">"port 53240"</text>

                // Registry: one record per engine
                <rect x="470" y="18" width="154" height="92" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="547" y="40" class="dg-label" text-anchor="middle">".eustress/instances"</text>
                <text x="547" y="60" class="dg-note" text-anchor="middle">"18020.json"</text>
                <text x="547" y="78" class="dg-note" text-anchor="middle">"18244.json"</text>
                <text x="547" y="96" class="dg-note" text-anchor="middle">"18410.json"</text>
                <line x1="402" y1="44" x2="470" y2="50" class="dg-line dg-line-dashed"></line>
                <line x1="402" y1="118" x2="470" y2="76" class="dg-line dg-line-dashed"></line>
                <line x1="402" y1="198" x2="470" y2="100" class="dg-line dg-line-dashed"></line>

                // The Universe's single slot names the owner
                <rect x="470" y="156" width="154" height="76" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="547" y="180" class="dg-label" text-anchor="middle">"engine.port"</text>
                <text x="547" y="198" class="dg-note" text-anchor="middle">"53117"</text>
                <text x="547" y="216" class="dg-note" text-anchor="middle">"one slot per Universe"</text>
                <line x1="470" y1="176" x2="402" y2="138" class="dg-line dg-line-accent" marker-end="url(#cli-arrow)"></line>
                <text x="446" y="146" class="dg-note" text-anchor="middle">"owner"</text>
            </svg>
            <figcaption>
                "Every engine writes its own registry record, so the registry lists all three. The
                Universe's engine.port holds one port, so it reaches only the owner; the CLI reaches
                any engine by pid or port."
            </figcaption>
        </figure>
    }
}

/// CLI & Headless documentation page.
#[component]
pub fn LearnCliPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-cli"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/list.svg" alt="CLI & Headless" class="toc-icon" />
                        <h2>"CLI & Headless"</h2>
                    </div>
                    <nav class="toc-nav">
                        {get_toc().into_iter().map(|section| {
                            let section_id = section.id.to_string();
                            let is_active = {
                                let section_id = section_id.clone();
                                move || active_section.get() == section_id
                            };
                            view! {
                                <div class="toc-section">
                                    <a
                                        href=format!("#{}", section.id)
                                        class="toc-section-title"
                                        class:active=is_active
                                    >
                                        {section.title}
                                    </a>
                                    <div class="toc-subsections">
                                        {section.subsections.into_iter().map(|sub| {
                                            view! {
                                                <a href=format!("#{}", sub.id) class="toc-subsection">
                                                    {sub.title}
                                                </a>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                </div>
                            }
                        }).collect::<Vec<_>>()}
                    </nav>

                    <div class="toc-footer">
                        <a href="/learn" class="toc-back">
                            <img src="/assets/icons/arrow-left.svg" alt="Back" />
                            "Back to Learn"
                        </a>
                    </div>
                </aside>

                <main class="docs-content">
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"CLI & Headless"</span>
                        </div>
                        <h1 class="docs-title">"CLI & Headless"</h1>
                        <p class="docs-subtitle">
                            "Eustress runs without its editor window. The eustress command opens, lists,
                            drives and closes engines from a terminal, and eustress-headless simulates a
                            Space with no window at all, so scripts, CI jobs and AI agents can run a Space
                            and read the results."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "17 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/cube.svg" alt="Level" />
                                "Intermediate"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "Updated Sep 2026"
                            </span>
                        </div>
                    </header>

                    // =========================================================
                    // OVERVIEW
                    // =========================================================
                    <section id="overview" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"01"</span>
                            "Overview"
                        </h2>

                        <div id="overview-programs" class="subsection">
                            <h3>"The Programs"</h3>
                            <p>
                                "The command-line side of Eustress is a set of programs built from the same
                                source as Eustress Engine. Two of them do most of the work: "
                                <code>"eustress"</code>", which manages and drives engines, and "
                                <code>"eustress-headless"</code>", which is an engine with no window."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Program"</th><th>"Package"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"eustress"</code></td><td>"eustress-cli"</td><td>"Opens, lists and closes engines, drives any engine over its bridge, and runs a Space headless in one call."</td></tr>
                                    <tr><td><code>"eustress-headless"</code></td><td>"eustress-engine"</td><td>"Loads a Space and runs its simulation with no window: physics, scripts, the world database and the bridge."</td></tr>
                                    <tr><td><code>"eustress-engine"</code></td><td>"eustress-engine"</td><td>"The Studio window. "<code>"--space <dir>"</code>", "<code>"--universe <dir>"</code>" or "<code>"--open <file>"</code>" opens a Space directly."</td></tr>
                                    <tr><td><code>"eustress-mcp"</code></td><td>"eustress-mcp-server"</td><td>"The MCP server for AI clients; see "<a href="/learn/mcp">"MCP Server"</a>"."</td></tr>
                                    <tr><td><code>"eustress-space"</code></td><td>"eustress-space"</td><td>"Opens, verifies and exports a Space's database without the engine."</td></tr>
                                    <tr><td><code>"convert-to-eustress"</code>", "<code>"reseed-space-subtree"</code></td><td>"eustress-engine"</td><td>"Seed a Space's database from its TOML files."</td></tr>
                                    <tr><td><code>"purge_tree_path"</code></td><td>"eustress-worlddb"</td><td>"Remove one entity from every store of a closed Space's database."</td></tr>
                                    <tr><td><code>"generate-benchmark-map"</code></td><td>"eustress-engine"</td><td>"Fill a Space with a grid of test parts."</td></tr>
                                    <tr><td><code>"texture-gen"</code></td><td>"texture-gen"</td><td>"Regenerate the bundled material textures in a source checkout."</td></tr>
                                    <tr><td><code>"eustress-lsp"</code></td><td>"eustress-engine"</td><td>"The Rune language server; see "<a href="/learn/lsp">"Rune LSP"</a>"."</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="overview-build" class="subsection">
                            <h3>"Build Them"</h3>
                            <p>
                                "The Windows installer installs Eustress Engine and, when they were built
                                beside it, eustress-lsp and eustress-mcp. Build the other programs in the "
                                <code>"eustress"</code>" folder of a source checkout; each lands in "
                                <code>"target/release"</code>"."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"cargo build --release -p eustress-cli      # eustress
cargo build --release -p eustress-engine   # eustress-engine, eustress-headless and the engine's tools
cargo build --release -p eustress-space    # eustress-space"#}</code></pre>
                            </div>
                            <p>
                                <code>"eustress open"</code>" and "<code>"eustress run"</code>" look for "
                                <code>"eustress-engine"</code>" and "<code>"eustress-headless"</code>" next to
                                their own executable first and on your "<code>"PATH"</code>" second, so
                                building everything into the same "<code>"target"</code>" folder is enough."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // THE EUSTRESS COMMAND
                    // =========================================================
                    <section id="cli" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "The eustress Command"
                        </h2>

                        <div id="cli-verbs" class="subsection">
                            <h3>"Verbs"</h3>
                            <p>
                                <code>"eustress"</code>" is one program with a verb per job. "
                                <code>"-v"</code>" or "<code>"--verbose"</code>" turns on debug logging for
                                any verb, and "<code>"--help"</code>" prints each verb's flags."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Verb"</th><th>"What it does"</th><th>"Status"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"open"</code></td><td>"Start an engine on a Space, detached, and print its pid and port."</td><td>"Available"</td></tr>
                                    <tr><td><code>"instances"</code></td><td>"List running engines, dropping records of engines that are gone."</td><td>"Available"</td></tr>
                                    <tr><td><code>"close"</code></td><td>"Shut engines down cleanly by pid, by Space or all at once."</td><td>"Available"</td></tr>
                                    <tr><td><code>"bridge"</code></td><td>"Send one command to a running engine over its bridge."</td><td>"Available"</td></tr>
                                    <tr><td><code>"run"</code></td><td>"Run a Space in eustress-headless and wait for it to finish."</td><td>"Available"</td></tr>
                                    <tr><td><code>"server"</code></td><td>"Start the multiplayer host."</td><td>"Not yet available"</td></tr>
                                    <tr><td><code>"publish"</code></td><td>"Upload a Space from the command line."</td><td>"Not yet available"</td></tr>
                                    <tr><td><code>"sim"</code></td><td>"Replay simulation history."</td><td>"Not yet available"</td></tr>
                                    <tr><td><code>"fork"</code></td><td>"Register a fork with the Trust Registry."</td><td>"Not yet available"</td></tr>
                                </tbody>
                            </table>
                            <p>"Why the last four are not usable yet is in "<a href="#roadmap-verbs">"Remaining Verbs"</a>"."</p>
                        </div>

                        <div id="cli-open" class="subsection">
                            <h3>"Open, List, Close"</h3>
                            <p>
                                <code>"eustress open"</code>" starts a new engine on a Space and returns as soon
                                as its bridge is up, printing the engine's instance record with its pid and
                                port. The engine runs detached, so closing the terminal leaves it running, and
                                capturing the output with "<code>"$(...)"</code>" returns as soon as the record
                                is printed."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"cd ~/Documents/Eustress/Lab
eustress open Spaces/Bracket-A --headless --json
eustress open Spaces/Bracket-B
eustress instances
eustress close --pid 18244
eustress close --all"#}</code></pre>
                            </div>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Flag"</th><th>"Verb"</th><th>"Effect"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"<space>"</code></td><td>"open"</td><td>"The Space folder to open. Required."</td></tr>
                                    <tr><td><code>"--headless"</code></td><td>"open"</td><td>"Start eustress-headless instead of a Studio window."</td></tr>
                                    <tr><td><code>"--play"</code></td><td>"open"</td><td>"Start the run as soon as the Space loads. Applies to headless engines; Studio opens in Edit either way."</td></tr>
                                    <tr><td><code>"--wait-secs <n>"</code></td><td>"open"</td><td>"How long to wait for the bridge, default 45. The engine keeps running if this runs out, and the command exits with an error."</td></tr>
                                    <tr><td><code>"--json"</code></td><td>"open, instances"</td><td>"Print only JSON, for scripts."</td></tr>
                                    <tr><td><code>"--pid <n>"</code>", "<code>"--space <dir>"</code>", "<code>"--all"</code></td><td>"close"</td><td>"Which engines to close."</td></tr>
                                    <tr><td><code>"--force"</code></td><td>"close"</td><td>"Kill the process if the clean shutdown is refused or times out, and delete its record."</td></tr>
                                    <tr><td><code>"--workspace <dir>"</code></td><td>"all three"</td><td>"Where the instance registry lives. Default: "<code>"EUSTRESS_WORKSPACE"</code>", else "<code>"Documents/Eustress"</code>"."</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Without "<code>"--play"</code>", a headless engine opens in Edit and waits for a
                                run command, so you can set its values first. "<code>"close"</code>" sends the "
                                <code>"engine.shutdown"</code>" bridge method, which exits the way closing the
                                window does: the port file and instance record are removed and the world
                                database is released."
                            </p>
                        </div>

                        <div id="cli-bridge" class="subsection">
                            <h3>"Drive an Engine"</h3>
                            <p>
                                <code>"eustress bridge"</code>" sends one request to a running engine, Studio or
                                headless, and prints a one-line summary followed by the full JSON result, so its
                                output pipes into "<code>"jq"</code>". Choose the engine before the subcommand:
                                with no flag it reaches the engine whose port is in "
                                <code>".eustress/engine.port"</code>" under "<code>"--universe <dir>"</code>" (the
                                current folder by default); "<code>"--port <n>"</code>" and "
                                <code>"--pid <n>"</code>" reach one engine directly."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Subcommand"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"ping"</code></td><td>"Check that the engine answers."</td></tr>
                                    <tr><td><code>"inspect [--class] [--name-contains] [--limit 50]"</code></td><td>"Entities with mesh, material, transform and physics flags, plus the frame rate."</td></tr>
                                    <tr><td><code>"ecs-query [--class] [--offset] [--limit 100]"</code></td><td>"Entity ids, names and classes, a page at a time."</td></tr>
                                    <tr><td><code>"sim-read [--keys a,b]"</code></td><td>"Simulation values, all of them or the listed keys."</td></tr>
                                    <tr><td><code>"sim-step [--ticks 1]"</code></td><td>"Advance physics by exact 1/60 s ticks."</td></tr>
                                    <tr><td><code>"sim-run [--time-scale] [--duration] [--wait]"</code></td><td>"Start or resume a run, or retune a running one. "<code>"--wait"</code>" prints the run's final values when it ends, giving up after "<code>"--timeout"</code>" seconds (300)."</td></tr>
                                    <tr><td><code>"sim-pause"</code>", "<code>"sim-stop"</code></td><td>"Pause the run, or stop it and restore the scene like the Stop button."</td></tr>
                                    <tr><td><code>"sim-set KEY=VALUE ..."</code></td><td>"Write numeric simulation values."</td></tr>
                                    <tr><td><code>"sim-state"</code></td><td>"Play state, simulation clock and the run ledger."</td></tr>
                                    <tr><td><code>"sim-await [--run-id] [--timeout 300]"</code></td><td>"Wait for a run to end and print its outcome."</td></tr>
                                    <tr><td><code>"raycast [--origin X Y Z] [--direction X Y Z]"</code></td><td>"Cast a ray against the live colliders; "<code>"--max-distance"</code>" and "<code>"--max-hits"</code>" bound it."</td></tr>
                                    <tr><td><code>"oplog [--limit 50]"</code></td><td>"Recent entity creates and deletes."</td></tr>
                                    <tr><td><code>"entity create|read|update|delete|find"</code></td><td>"Parts in the world database, by uuid or name."</td></tr>
                                    <tr><td><code>"call <method> [--params '{...}']"</code></td><td>"Any bridge method by name, with JSON parameters."</td></tr>
                                </tbody>
                            </table>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"eustress bridge ping
eustress bridge --pid 18244 inspect --class Part --limit 20
eustress bridge --port 53117 entity create --name Crate --position 0 2 0 --color 0.8 0.5 0.2
eustress bridge --pid 18244 sim-run --duration 60 --time-scale 10 --wait
eustress bridge call viewport.capture"#}</code></pre>
                            </div>
                            <p>
                                <code>"entity create"</code>" makes a 1 m anchored Plastic block unless you say
                                otherwise; "<code>"--shape"</code>" takes block, ball, cylinder, wedge,
                                cornerwedge or cone, and colors are red, green and blue from 0 to 1."
                            </p>
                        </div>

                        <div id="cli-run" class="subsection">
                            <h3>"One-Shot Runs"</h3>
                            <p>
                                <code>"eustress run"</code>" starts eustress-headless on a Space in the
                                foreground and waits for it. With "<code>"--ticks"</code>" it is a batch job:
                                run that many simulation ticks, export the recording, exit. It succeeds when
                                eustress-headless exits with status 0 and fails otherwise, so a CI step can
                                check it directly."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"eustress run ~/Documents/Eustress/Lab/Spaces/Bracket-A --ticks 600
echo $?"#}</code></pre>
                            </div>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Flag"</th><th>"Effect"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"<space>"</code></td><td>"The Space folder to simulate. Required."</td></tr>
                                    <tr><td><code>"--ticks <n>"</code></td><td>"Stop after N ticks and export the recording. Without it, the run lasts until the process is closed."</td></tr>
                                    <tr><td><code>"--tick-rate <hz>"</code></td><td>"Main loop rate, default 60."</td></tr>
                                    <tr><td><code>"--no-autoplay"</code></td><td>"Stay in Edit and wait for a run command over the bridge."</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // HEADLESS
                    // =========================================================
                    <section id="headless" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Headless"
                        </h2>

                        <div id="headless-what" class="subsection">
                            <h3>"What It Runs"</h3>
                            <p>
                                "eustress-headless is the simulation half of Eustress Engine without the window.
                                It adds the same core set of plugins Studio does (the Space loader, the world
                                database, Avian physics, the Realism and simulation systems, the Rune script
                                runtime, and the Engine Bridge) on a plain run loop, with no window, no GPU and no
                                editor interface. Luau scripts start on Play once Studio does; see "
                                <a href="/docs/scripting">"Scripting"</a>"."
                            </p>
                            <p>
                                "Because it hosts the same bridge, every "<code>"eustress bridge"</code>
                                " subcommand and every live tool of the "<a href="/learn/mcp">"MCP server"</a>
                                " works against it, apart from the few listed under "
                                <a href="#headless-limits">"Limits"</a>". It registers itself with kind "
                                <code>"headless"</code>", so "<code>"eustress instances"</code>" can tell it
                                from a Studio window."
                            </p>
                        </div>

                        <div id="headless-flags" class="subsection">
                            <h3>"Flags"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Flag"</th><th>"Effect"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"--space <dir>"</code></td><td>"The Space to open: the folder that holds "<code>"Workspace"</code>" and "<code>"world.fjalldb"</code>"."</td></tr>
                                    <tr><td><code>"--universe <dir>"</code></td><td>"Open the first Space inside a Universe instead."</td></tr>
                                    <tr><td><code>"--ticks <n>"</code></td><td>"Stop after N simulation ticks, export the recording and exit."</td></tr>
                                    <tr><td><code>"--tick-rate <hz>"</code></td><td>"Main loop rate, default 60. The simulation step stays fixed at 60 Hz."</td></tr>
                                    <tr><td><code>"--no-autoplay"</code></td><td>"Stay in Edit and wait for a run command over the bridge or MCP."</td></tr>
                                    <tr><td><code>"--autoplay-delay-frames <n>"</code></td><td>"Frames to let the Space load before entering Play, default 120, about 2 s at 60 Hz."</td></tr>
                                    <tr><td><code>"-h"</code>", "<code>"--help"</code></td><td>"Print usage."</td></tr>
                                </tbody>
                            </table>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"eustress-headless --space ~/Documents/Eustress/Lab/Spaces/Bracket-A --ticks 600
eustress-headless --universe ~/Documents/Eustress/Lab --no-autoplay"#}</code></pre>
                            </div>
                        </div>

                        <div id="headless-lifecycle" class="subsection">
                            <h3>"A Run, Start to Finish"</h3>
                            <ol class="numbered-list">
                                <li>"It loads the Space, binds the bridge and writes "<code>"engine.port"</code>" and its instance record."</li>
                                <li>"After the autoplay delay it switches to Play, and the run's watchpoint recording starts."</li>
                                <li>"With "<code>"--ticks"</code>", it returns to Edit on the first frame its simulation clock has reached N ticks, so the recording can count slightly more than N. Leaving Play exports the recording to "<code>"<Universe>/.eustress/knowledge/recordings/<Space>/"</code>" as a "<code>"sim_"</code>" JSON file named by time."</li>
                                <li>"It gives that work four frames to finish, then exits."</li>
                            </ol>
                            <p>
                                "Without "<code>"--ticks"</code>" it runs until it is closed, by "
                                <code>"eustress close"</code>", by the "<code>"engine.shutdown"</code>" bridge
                                method or by ending the process."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Exit status"</th><th>"Meaning"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"0"</code></td><td>"The run finished, the engine was closed cleanly, or "<code>"--help"</code>" was asked for."</td></tr>
                                    <tr><td><code>"2"</code></td><td>"An argument was wrong: an unknown flag, a missing value, or a Space folder that does not exist. The reason goes to stderr."</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="headless-limits" class="subsection">
                            <h3>"Limits"</h3>
                            <ul class="docs-list">
                                <li><strong>"No glTF loader"</strong>": parts built from primitive shapes, physics, scripts, simulation values and the bridge all work, but custom "<code>".glb"</code>" meshes do not load."</li>
                                <li><strong>"No renderer"</strong>": the bridge's "<code>"viewport.capture"</code>" and "<code>"ai_camera"</code>" methods produce no image."</li>
                                <li><strong>"No Workshop"</strong>": "<code>"tools.list"</code>" and "<code>"tools.call"</code>" answer that the tool registry is not available."</li>
                                <li><strong>"No input"</strong>": with no window, scripts that poll input see nothing pressed."</li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // MANY ENGINES
                    // =========================================================
                    <section id="multi" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Many Engines"
                        </h2>

                        <div id="multi-owner" class="subsection">
                            <h3>"One Port File, One Owner"</h3>
                            <p>
                                "A Universe has one "<code>".eustress/engine.port"</code>" file, so it names one
                                engine: the Universe's owner, whichever engine wrote it last. An engine removes
                                the file on exit only while it still holds its own port, and a surviving engine
                                takes a released slot back within a second, including one left by an engine
                                that crashed."
                            </p>
                            <p>
                                "Anything that addresses the Universe, such as "
                                <code>"eustress bridge --universe"</code>" or the MCP server's live tools,
                                reaches the owner. To reach one engine among several, address it by port or
                                pid."
                            </p>
                            <InstancesDiagram />
                        </div>

                        <div id="multi-registry" class="subsection">
                            <h3>"The Instance Registry"</h3>
                            <p>
                                "Every engine, Studio or headless, also writes a record of its own once its
                                bridge is up, at "<code>"<workspace>/.eustress/instances/<pid>.json"</code>",
                                where the workspace is the folder that holds its Universe. The record is
                                rewritten when the engine switches Space and removed when it exits cleanly. The
                                eustress command reads records from "<code>"--workspace"</code>", else "
                                <code>"EUSTRESS_WORKSPACE"</code>", else "<code>"Documents/Eustress"</code>" (on
                                Windows, the Documents folder in your user profile even when OneDrive redirects
                                it), so pass "<code>"--workspace"</code>" when your Universes live elsewhere."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">".eustress/instances/18244.json"</span>
                                </div>
                                <pre><code class="language-json">{r#"{
  "pid": 18244,
  "port": 53117,
  "kind": "headless",
  "space": "C:\\Users\\you\\Documents\\Eustress\\Lab\\Spaces\\Bracket-A",
  "universe": "C:\\Users\\you\\Documents\\Eustress\\Lab",
  "started_at": "2026-09-22T14:03:11.482917300+00:00"
}"#}</code></pre>
                            </div>
                            <p>
                                <code>"eustress instances"</code>" reads these records, checks that each port
                                still answers, and deletes the records of engines that are gone, so what it
                                lists is what you can drive. "<code>"--json"</code>" adds an "
                                <code>"owns_universe"</code>" field that marks each Universe's owner."
                            </p>
                        </div>

                        <div id="multi-sim" class="subsection">
                            <h3>"Simulations per Engine"</h3>
                            <p>
                                "Beside its record, each engine keeps a private folder, "
                                <code>"instances/<pid>/"</code>", with its simulation command queue ("
                                <code>"sim-commands.jsonl"</code>") and its runtime snapshot ("
                                <code>"snapshot.json"</code>"). Only that engine reads the queue and writes the
                                snapshot, so engines on one Universe run their own simulations side by side.
                                The Universe's own queue and "<code>"runtime-snapshot.json"</code>" belong to
                                the owner, and every engine appends to the shared "
                                <code>".eustress/telemetry.jsonl"</code>", tagging each line with its pid,
                                Space and run id."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"cd ~/Documents/Eustress/Lab
A=$(eustress open Spaces/Bracket-A --headless --json | jq .pid)
B=$(eustress open Spaces/Bracket-B --headless --json | jq .pid)

eustress bridge --pid $A sim-set load.current_a=2.5
eustress bridge --pid $A sim-run --duration 60 --time-scale 100
eustress bridge --pid $B sim-run --duration 60 --time-scale 100

eustress bridge --pid $A sim-await    # run id, end reason, final values
eustress bridge --pid $B sim-await
eustress close --all"#}</code></pre>
                            </div>
                            <p>
                                "The MCP server's simulation tools take a "<code>"pid"</code>" the same way;
                                without one they reach the owner and name the other engines that share the
                                Universe."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // THE BRIDGE
                    // =========================================================
                    <section id="bridge" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "The Bridge"
                        </h2>

                        <div id="bridge-wire" class="subsection">
                            <h3>"Wire Format"</h3>
                            <p>
                                "The Engine Bridge speaks JSON-RPC 2.0 over TCP on "<code>"127.0.0.1"</code>",
                                one newline-terminated JSON object per request and per reply. Connect, write a
                                line, read a line:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"JSON-RPC"</span>
                                </div>
                                <pre><code class="language-json">{r#"{"jsonrpc":"2.0","id":1,"method":"ecs.inspect","params":{"limit":10}}
{"jsonrpc":"2.0","id":1,"result":{ ... }}
{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"method not found: ecs.inspekt"}}"#}</code></pre>
                            </div>
                            <p>
                                "Requests run on the engine's main thread, up to 64 per frame. "
                                <code>"sim.step"</code>" is spread across frames, at most 100 ms of stepping per
                                frame, and "<code>"db.export_toml"</code>" writes its files on a worker thread,
                                so the engine keeps rendering and answering while they work; only one "
                                <code>"sim.step"</code>" runs at a time. Error codes follow JSON-RPC: "
                                <code>"-32601"</code>" for an unknown method, "<code>"-32602"</code>" for bad
                                parameters, "<code>"-32603"</code>" for a failure inside the engine."
                            </p>
                        </div>

                        <div id="bridge-methods" class="subsection">
                            <h3>"Methods"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Method"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"ping"</code></td><td>"Health check."</td></tr>
                                    <tr><td><code>"ecs.query"</code></td><td>"Entity ids, names and classes, paginated."</td></tr>
                                    <tr><td><code>"ecs.inspect"</code></td><td>"Detailed live entities and the frame rate; filters by class, name, cell or region."</td></tr>
                                    <tr><td><code>"scene.overview"</code></td><td>"Entity counts and class histograms per 256 m Morton cell."</td></tr>
                                    <tr><td><code>"scene.raycast"</code></td><td>"A ray against the live Avian colliders, hits nearest first."</td></tr>
                                    <tr><td><code>"sim.read"</code></td><td>"Current simulation values, all of them or by key."</td></tr>
                                    <tr><td><code>"sim.step"</code></td><td>"Advance physics by N fixed 1/60 s ticks, up to 10,000."</td></tr>
                                    <tr><td><code>"sim.run"</code>", "<code>"sim.pause"</code>", "<code>"sim.stop"</code></td><td>"Start, pause and stop a run; "<code>"sim.run"</code>" answers with the run id."</td></tr>
                                    <tr><td><code>"sim.set"</code>", "<code>"sim.state"</code></td><td>"Write simulation values; read play state, clock and the run ledger."</td></tr>
                                    <tr><td><code>"hil.ingest"</code></td><td>"Append hardware samples to a run's log. It never writes simulation values, and it rejects samples from instruments past their calibration date."</td></tr>
                                    <tr><td><code>"oplog.tail"</code></td><td>"Recent entity creates and deletes, in order."</td></tr>
                                    <tr><td><code>"entity.create"</code>", "<code>".read"</code>", "<code>".update"</code>", "<code>".delete"</code>", "<code>".find"</code></td><td>"Parts in the world database."</td></tr>
                                    <tr><td><code>"entity.add_tag"</code>", "<code>"entity.remove_tag"</code></td><td>"CollectionService tags."</td></tr>
                                    <tr><td><code>"entity.promote"</code>", "<code>"entity.demote"</code></td><td>"Move a part between the database and an "<code>"_instance.toml"</code>" folder."</td></tr>
                                    <tr><td><code>"db.export_toml"</code></td><td>"Dump the world database to readable TOML files."</td></tr>
                                    <tr><td><code>"tool.equip"</code>", "<code>"selection.set"</code>", "<code>"state.get"</code></td><td>"The editor's active tool and selection."</td></tr>
                                    <tr><td><code>"action.invoke"</code></td><td>"Run an editor action by name, as its shortcut would."</td></tr>
                                    <tr><td><code>"viewport.capture"</code></td><td>"Screenshot the window to "<code>"<Space>/.eustress/capture.png"</code>"; the file lands a frame or two after the reply."</td></tr>
                                    <tr><td><code>"ai_camera.set_pose"</code>", "<code>".orbit"</code>", "<code>".frame"</code>", "<code>".capture"</code></td><td>"The off-screen AI camera; captures go to "<code>"<Space>/.eustress/ai_camera.png"</code>"."</td></tr>
                                    <tr><td><code>"data.bind"</code>", "<code>"data.bindings"</code>", "<code>"data.unbind"</code></td><td>"Dataset columns driving simulation parameters."</td></tr>
                                    <tr><td><code>"tools.list"</code>", "<code>"tools.call"</code></td><td>"The Workshop tool registry, called with the Read and Write grant. Studio only."</td></tr>
                                    <tr><td><code>"engine.shutdown"</code></td><td>"Exit cleanly at the end of the frame."</td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-warning">
                                <img src="/assets/icons/shield.svg" alt="Warning" />
                                <div>
                                    <strong>"Any local program can call these"</strong>
                                    <p>
                                        "The bridge listens only on "<code>"127.0.0.1"</code>" and checks no
                                        identity. Every method above, "<code>"entity.delete"</code>" and "
                                        <code>"engine.shutdown"</code>" included, is open to any program on your
                                        computer that reads the port file."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="bridge-client" class="subsection">
                            <h3>"The Shared Client"</h3>
                            <p>
                                "The eustress command and the MCP server share one client crate, "
                                <code>"eustress-bridge-client"</code>": synchronous TCP from the standard
                                library, a 2 second connect limit and friendly errors when no engine answers. A
                                Rust tool inside the Eustress workspace can depend on it by path:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress_bridge_client::{call_engine, call_port, default_workspace_root, list_instances};
use serde_json::json;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The Universe's owner, found through its engine.port file.
    let universe = Path::new("C:/Users/you/Documents/Eustress/Lab");
    let scene = call_engine(universe, "ecs.inspect", json!({ "limit": 10 }))?;
    println!("{scene}");

    // Every live engine in the workspace, each by its own port.
    for rec in list_instances(&default_workspace_root()) {
        let pong = call_port(rec.port, "ping", json!({}))?;
        println!("{} {} {:?} {}", rec.pid, rec.kind.as_str(), rec.space, pong);
    }
    Ok(())
}"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // UTILITY PROGRAMS
                    // =========================================================
                    <section id="utilities" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Utility Programs"
                        </h2>

                        <div id="utilities-space" class="subsection">
                            <h3>"eustress-space"</h3>
                            <p>
                                "eustress-space reads a Space's world database with only the storage crate
                                linked, no engine, which makes it the quickest way to see what a Space holds.
                                Its path is the Space folder or its "<code>"world.fjalldb"</code>" folder. It
                                opens the same database the engine writes, so point it at a Space no engine
                                has open (see "<a href="#utilities-db">"Database Tools"</a>")."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Command"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"eustress-space open <path>"</code></td><td>"Entity count, class histogram, world bounds and the database header."</td></tr>
                                    <tr><td><code>"eustress-space verify <path>"</code></td><td>"Validates every stored entity and exits non-zero if any fails."</td></tr>
                                    <tr><td><code>"eustress-space export <path> [--out <dir>]"</code></td><td>"Writes each entity as a readable "<code>".instance.toml"</code>", grouped by class, to "<code>"<path>/export_toml"</code>" by default."</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="utilities-db" class="subsection">
                            <h3>"Database Tools"</h3>
                            <div class="callout callout-warning">
                                <img src="/assets/icons/shield.svg" alt="Warning" />
                                <div>
                                    <strong>"Close the Space first"</strong>
                                    <p>
                                        "Fjall, the database under a Space, takes no lock between processes, so
                                        nothing stops two programs from writing one Space's "
                                        <code>"world.fjalldb"</code>" at once, and the result is a corrupted
                                        database. Run the tools below only on a Space that no engine has open."
                                    </p>
                                </div>
                            </div>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Program"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"convert-to-eustress [--space <dir>] [--eustress-root <dir>] [--dry-run]"</code></td><td>"Seed each Space's database from its TOML files, keeping the files. Every Space under "<code>"Documents/Eustress"</code>" by default. The engine also does this for a Space when it opens it; see "<a href="/docs/importing">"Importing"</a>"."</td></tr>
                                    <tr><td><code>"reseed-space-subtree --space <dir> [--subdir Workspace]"</code></td><td>"Re-read one folder's TOML files into the database after edits made while the engine was closed. Always pass "<code>"--space"</code>": the default is "<code>"Universe1/Spaces/Space1"</code>"."</td></tr>
                                    <tr><td><code>"purge_tree_path --space <dir> --match <text> [--apply]"</code></td><td>"Remove every stored record whose path contains the text. A dry run unless "<code>"--apply"</code>" is given, which first copies each value to "<code>"<Space>/.eustress/trash/db-purge-<seconds>/"</code>", then deletes and re-reads to prove it."</td></tr>
                                </tbody>
                            </table>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"SPACE=~/Documents/Eustress/Lab/Spaces/Bracket-A
cargo run --release -p eustress-worlddb --bin purge_tree_path -- --space "$SPACE" --match Crate
cargo run --release -p eustress-worlddb --bin purge_tree_path -- --space "$SPACE" --match Crate --apply"#}</code></pre>
                            </div>
                        </div>

                        <div id="utilities-other" class="subsection">
                            <h3>"Generators and the LSP"</h3>
                            <p>
                                <code>"generate-benchmark-map"</code>" fills a Space with an N by N grid of test
                                parts, written straight into the Space's database, so the same "
                                <a href="#utilities-db">"closed Space"</a>" rule applies. In that default mode, "
                                <code>"--output"</code>" must be a path inside the Space's "
                                <code>"Workspace"</code>" folder; "<code>"--disk"</code>" writes one folder per
                                part instead, and "<code>"--binary-ecs"</code>" writes binary parts."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"# 316 x 316 = 99,856 parts, 4 m apart
cargo run --release -p eustress-engine --bin generate-benchmark-map -- \
  --grid-size 316 --spacing 4 --seed 42 --output "$SPACE/Workspace/BenchmarkGrid""#}</code></pre>
                            </div>
                            <p>
                                "Its other flags are "<code>"--active-pct"</code>" (the share of parts given a
                                velocity, default 0.10) and the defaults shown above for "
                                <code>"--grid-size"</code>" (100), "<code>"--spacing"</code>" (4) and "
                                <code>"--seed"</code>" (42)."
                            </p>
                            <p>
                                <code>"texture-gen"</code>" regenerates the bundled PBR material library (base
                                color, normal and ORM maps for 18 materials) into the source tree: run "
                                <code>"cargo run -p texture-gen"</code>", add "<code>"--only brick,grass"</code>
                                " for a subset or "<code>"--size"</code>" for another resolution (2048 by
                                default), and use its "<code>"check"</code>" subcommand to report seams. "
                                <code>"eustress-lsp"</code>" serves Rune language intelligence over stdio or TCP;
                                see "<a href="/learn/lsp">"Rune LSP"</a>"."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // WHAT'S NEXT
                    // =========================================================
                    <section id="roadmap" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "What's Next"
                        </h2>

                        <div id="roadmap-render" class="subsection">
                            <h3>"Headless Capture"</h3>
                            <p>
                                "A GPU tier for eustress-headless will keep the renderer while still opening no
                                window, so "<code>"viewport.capture"</code>" and the AI camera will produce
                                images on a machine with no desktop, such as a CI runner with a graphics
                                adapter."
                            </p>
                        </div>

                        <div id="roadmap-verbs" class="subsection">
                            <h3>"Remaining Verbs"</h3>
                            <p>
                                "Four verbs are defined but not usable yet, and will be documented here once they
                                are. "<code>"server"</code>" starts eustress-server, whose multiplayer loop is
                                still empty; see "<a href="/docs/networking">"Networking"</a>". "
                                <code>"publish"</code>" calls Wrangler with an upload command that names no
                                file; see "<a href="/docs/publishing">"Publishing"</a>" for how Spaces are
                                published. "<code>"sim"</code>" reads a history stream that starts empty in its
                                own process, so it has no records to show. "<code>"fork"</code>" posts to
                                registry endpoints that no Eustress service implements yet."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Open a Space from a script, run it, read the numbers, close it."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/simulation" class="btn-secondary-steel">"Simulation Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/learn/mcp" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"MCP Server"</span>
                            </div>
                        </a>
                        <a href="/learn/ide" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"IDE Integration"</span>
                            </div>
                            <img src="/assets/icons/arrow-right.svg" alt="Next" />
                        </a>
                    </nav>
                </main>
            </div>

            <Footer />
        </div>
    }
}
