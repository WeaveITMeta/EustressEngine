// =============================================================================
// Eustress Web - Philosophy Documentation Page
// =============================================================================
// Philosophy: the principles behind Eustress, each tied to a mechanism the
// reader can inspect in the product and in the source.
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
                TocSubsection { id: "overview-purpose", title: "What Eustress Is For" },
                TocSubsection { id: "overview-principles", title: "Six Principles" },
            ],
        },
        TocSection {
            id: "agents",
            title: "Agents and People",
            subsections: vec![
                TocSubsection { id: "agents-same", title: "The Same Spaces" },
                TocSubsection { id: "agents-mcp", title: "The MCP Server" },
                TocSubsection { id: "agents-bridge", title: "The Engine Bridge" },
                TocSubsection { id: "agents-camera", title: "The AI Camera" },
            ],
        },
        TocSection {
            id: "ownership",
            title: "Your Work Is Yours",
            subsections: vec![
                TocSubsection { id: "ownership-files", title: "Files You Can Read" },
                TocSubsection { id: "ownership-history", title: "History You Keep" },
                TocSubsection { id: "ownership-scale", title: "A Database for Scale" },
                TocSubsection { id: "ownership-privacy", title: "What Leaves Your Machine" },
            ],
        },
        TocSection {
            id: "rust",
            title: "One Language",
            subsections: vec![
                TocSubsection { id: "rust-throughout", title: "Rust Throughout" },
                TocSubsection { id: "rust-slint", title: "Slint Is Rust" },
                TocSubsection { id: "rust-exceptions", title: "What Is Not Rust" },
            ],
        },
        TocSection {
            id: "units",
            title: "Measured in SI",
            subsections: vec![
                TocSubsection { id: "units-meters", title: "Meters Everywhere" },
                TocSubsection { id: "units-display", title: "Display Units" },
                TocSubsection { id: "units-si", title: "SI in the Physics" },
            ],
        },
        TocSection {
            id: "determinism",
            title: "Same Inputs, Same World",
            subsections: vec![
                TocSubsection { id: "determinism-pins", title: "The Pins" },
                TocSubsection { id: "determinism-seed", title: "One Seed" },
            ],
        },
        TocSection {
            id: "license",
            title: "Source-Available",
            subsections: vec![
                TocSubsection { id: "license-shield", title: "PolyForm Shield" },
                TocSubsection { id: "license-commercial", title: "The Commercial License" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-determinism", title: "A Determinism Gate" },
                TocSubsection { id: "roadmap-branches", title: "Branching the Whole World" },
            ],
        },
    ]
}

/// Philosophy documentation page.
#[component]
pub fn DocsPhilosophyPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-philosophy"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/book.svg" alt="Philosophy" class="toc-icon" />
                        <h2>"Philosophy"</h2>
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
                            <span class="current">"Philosophy"</span>
                        </div>
                        <h1 class="docs-title">"The Eustress Philosophy"</h1>
                        <p class="docs-subtitle">
                            "Eustress is a source-available simulation and data platform, written in Rust,
                            where people and AI agents build and run the same Spaces. These are the
                            principles behind it, each tied to a mechanism you can inspect yourself."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "11 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/cube.svg" alt="Level" />
                                "Beginner"
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

                        <div id="overview-purpose" class="subsection">
                            <h3>"What Eustress Is For"</h3>
                            <p>
                                "Eustress is for modeling physical systems, running them forward in time and
                                measuring what happens: a battery cell under load, heat moving through a part,
                                a structure an agent assembles and tests. The 3D view and the entity component
                                system underneath are how it does that. The product is the simulation and the
                                data it produces."
                            </p>
                            <p>
                                "Every principle below is a decision you can check in the product or in the
                                source, which is public. Where a principle is still partly a goal, the page
                                says which part."
                            </p>
                        </div>

                        <div id="overview-principles" class="subsection">
                            <h3>"Six Principles"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Principle"</th><th>"What to look at"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Agents and people work in the same Spaces"</td><td>"The MCP server, the engine bridge, the AI camera"</td></tr>
                                    <tr><td>"Your work is yours"</td><td>"Space folders, TOML files, git history, the WorldDb"</td></tr>
                                    <tr><td>"One language underneath"</td><td>"Rust in the engine, Studio, tools and website"</td></tr>
                                    <tr><td>"Measured in SI"</td><td>"Meters in every system, SI types in the physics"</td></tr>
                                    <tr><td>"Same inputs, same world"</td><td>"A fixed 60 Hz step, pinned physics settings, one seed"</td></tr>
                                    <tr><td>"Source-available"</td><td>"PolyForm Shield 1.0.0, plus a commercial license"</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // AGENTS AND PEOPLE
                    // =========================================================
                    <section id="agents" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Agents and People"
                        </h2>

                        <div id="agents-same" class="subsection">
                            <h3>"The Same Spaces"</h3>
                            <p>
                                "An agent in Eustress works in the Space you have open, not in a copy and not
                                by reading screenshots of the interface. The parts it creates appear in your
                                Explorer, and the editor actions your keyboard shortcuts run are one bridge call
                                away ("<code>"action.invoke"</code>")."
                            </p>
                            <p>
                                "Studio's Workshop panel and the MCP server take their tools from one crate, "
                                <code>"eustress-tools"</code>", so a tool added there is available to an agent in
                                either place."
                            </p>
                        </div>

                        <div id="agents-mcp" class="subsection">
                            <h3>"The MCP Server"</h3>
                            <p>
                                "The MCP server, "<code>"eustress-mcp"</code>", speaks the Model Context Protocol over
                                standard input and output, the way MCP clients such as Claude Desktop, Cursor and
                                Windsurf launch a server. Through it an agent lists Universes and Spaces, reads and
                                edits entities and scripts, runs simulations and experiments, and reads the
                                op-log."
                            </p>
                            <p>
                                "When an engine is running, entity edits go through it and appear in the open
                                Space at once. When none is, they are written to the Space's files instead. "
                                <a href="/learn/mcp">"MCP Server"</a>" lists the tools and how to connect a
                                client."
                            </p>
                        </div>

                        <div id="agents-bridge" class="subsection">
                            <h3>"The Engine Bridge"</h3>
                            <p>
                                "Every running engine, windowed or headless, opens a bridge: a JSON-RPC listener
                                on "<code>"127.0.0.1"</code>" whose port it writes to "<code>".eustress/engine.port"</code>
                                " in its Universe. The MCP server and the "<code>"eustress"</code>" command-line tool
                                are both clients of it. Each request and each reply is one line of JSON:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Bridge wire format"</span>
                                </div>
                                <pre><code class="language-json">{r#"{"jsonrpc":"2.0","id":1,"method":"ecs.inspect","params":{"limit":10}}
{"jsonrpc":"2.0","id":1,"result":{...}}"#}</code></pre>
                            </div>
                            <p>
                                "Its methods cover what acting in a world takes: query and edit entities ("
                                <code>"ecs.query"</code>", "<code>"entity.create"</code>", "<code>"entity.update"</code>
                                "), advance physics one fixed tick at a time ("<code>"sim.step"</code>"), cast rays
                                against live colliders ("<code>"scene.raycast"</code>"), run editor actions ("
                                <code>"action.invoke"</code>"), read the op-log ("<code>"oplog.tail"</code>") and
                                capture images ("<code>"viewport.capture"</code>", "<code>"ai_camera.capture"</code>")."
                            </p>
                        </div>

                        <div id="agents-camera" class="subsection">
                            <h3>"The AI Camera"</h3>
                            <p>
                                "An agent that builds something needs to look at it without taking over your
                                view. The AI camera is a second camera that renders 1280 by 720 images into an
                                off-screen texture instead of the window. It appears in the Explorer as a Camera
                                named AI Camera, and the "<code>"ai_camera_set_pose"</code>", "
                                <code>"ai_camera_orbit"</code>", "<code>"ai_camera_frame"</code>" and "
                                <code>"ai_camera_capture"</code>" tools move it and save what it sees, while your
                                viewport stays where you left it."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // YOUR WORK IS YOURS
                    // =========================================================
                    <section id="ownership" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Your Work Is Yours"
                        </h2>

                        <div id="ownership-files" class="subsection">
                            <h3>"Files You Can Read"</h3>
                            <p>
                                "A Space is a folder you can open without Eustress. Services, instances and
                                settings are TOML; scripts are "<code>".rune"</code>" and "<code>".luau"</code>" files;
                                meshes are "<code>".glb"</code>". Edit any of them in another editor and the file
                                watcher applies the save to the open Space. Rune scripts also get completions
                                from the "<a href="/learn/lsp">"Rune language server"</a>"."
                            </p>
                            <p>
                                <a href="/docs/universes">"Universes"</a>" walks through the folder layout, from
                                the Eustress folder down to a single "<code>"_instance.toml"</code>"."
                            </p>
                        </div>

                        <div id="ownership-history" class="subsection">
                            <h3>"History You Keep"</h3>
                            <p>
                                <code>"Ctrl+S"</code>" and autosave, every 300 seconds by default, each commit the
                                Space folder to git. The record of how a Space changed is ordinary git history on
                                your own disk: any git client reads it, and you can push it to any git host you
                                choose."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Git holds the files, not the database"</strong>
                                    <p>
                                        "Parts that live only in the Space's database are not in these commits. "
                                        <a href="/docs/universes#history-coverage">"What Git Captures"</a>" lists
                                        what a commit covers and how to export the rest as TOML first."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="ownership-scale" class="subsection">
                            <h3>"A Database for Scale"</h3>
                            <p>
                                "One file per part stops scaling long before a large world does, so each Space
                                also has a database, "<code>"world.fjalldb/"</code>", and Studio builds the scene
                                from it. Files are read into it when the Space opens. A Space with more than
                                100,000 binary parts streams them around the camera, 350 m out by default,
                                instead of loading them all."
                            </p>
                            <p>
                                "What lives only in the database can still be read as text. The "
                                <code>"export_instances_toml"</code>" tool writes it out as ordinary "
                                <code>"_instance.toml"</code>" files from a running engine, and "
                                <code>"eustress-space export"</code>" does the same with no engine at all."
                            </p>
                        </div>

                        <div id="ownership-privacy" class="subsection">
                            <h3>"What Leaves Your Machine"</h3>
                            <p>
                                "Studio sends anonymous usage statistics by default, so that the tools people
                                reach for get built first. Each ribbon click is recorded as the tool, the mode and
                                discipline it was clicked in, whether the tool is wired, and a timestamp. Scene
                                content, file names and entity names are not part of it."
                            </p>
                            <p>
                                "Clicks are kept as JSON lines in "<code>"%LOCALAPPDATA%\\Eustress\\telemetry"</code>
                                " on Windows (the local application data folder on other systems), so you can
                                read exactly what is collected. Only per-tool totals are sent: when a session
                                ends, or at the next launch if that send did not go through. To stop it, clear "
                                <strong>"Send anonymous usage statistics"</strong>" under "
                                <strong>"Settings > Notifications > Privacy"</strong>"."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // ONE LANGUAGE
                    // =========================================================
                    <section id="rust" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "One Language"
                        </h2>

                        <div id="rust-throughout" class="subsection">
                            <h3>"Rust Throughout"</h3>
                            <p>
                                "The engine, Studio, the WorldDb, the MCP server, the "<code>"eustress"</code>
                                " command-line tool and the headless runner are Rust, and so is this website,
                                written with Leptos and compiled to WebAssembly. That is about 574,000 lines of
                                Rust across 1,342 files under "<code>"eustress/crates"</code>"."
                            </p>
                            <p>
                                "One language means one compiler checks the whole path, from a record in the
                                database to a field in the Properties panel. Rust's ownership rules give memory
                                safety without a garbage collector, so no collector pause can land in the middle
                                of a fixed-rate simulation step."
                            </p>
                        </div>

                        <div id="rust-slint" class="subsection">
                            <h3>"Slint Is Rust"</h3>
                            <p>
                                "Studio's interface is written in Slint, a declarative language for user
                                interfaces, across 66 "<code>".slint"</code>" files. Slint is compiled, not
                                interpreted: the engine's build script turns the interface into Rust, and the
                                editor includes the result with "<code>"slint::include_modules!()"</code>"."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"crates/engine/build.rs"</span>
                                </div>
                                <pre><code class="language-rust">{r#"let slint_config = slint_build::CompilerConfiguration::new()
    .with_style("fluent-dark".into());

slint_build::compile_with_config(
    "ui/slint/main.slint",
    slint_config,
).expect("Failed to compile Slint UI");"#}</code></pre>
                            </div>
                            <p>
                                "A panel's properties and callbacks are therefore Rust types, checked by the same
                                compiler as the engine that feeds them."
                            </p>
                        </div>

                        <div id="rust-exceptions" class="subsection">
                            <h3>"What Is Not Rust"</h3>
                            <p>"Rust is the rule. These are the exceptions:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Part"</th><th>"Language"</th><th>"Why"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"The Luau virtual machine"</td><td>"C++"</td><td>"Luau scripts run in Roblox's Luau VM, built into the engine through the "<code>"mlua"</code>" crate. Rune scripts run in a VM written in Rust."</td></tr>
                                    <tr><td>"Workers behind "<code>"api.eustress.dev"</code></td><td>"JavaScript"</td><td>"They run on Cloudflare Workers. The content API server, "<code>"eustress-backend"</code>", is Rust."</td></tr>
                                    <tr><td>"The VS Code extension"</td><td>"TypeScript"</td><td>"VS Code loads extensions written in JavaScript or TypeScript."</td></tr>
                                    <tr><td>"Asset scripts"</td><td>"Python"</td><td>"They run inside Blender to build the avatar and export the primitive part meshes."</td></tr>
                                    <tr><td>"GPU shaders"</td><td>"WGSL"</td><td>"Five shader files, for billboards, the sun disc, the moon's phase and instanced part materials."</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // MEASURED IN SI
                    // =========================================================
                    <section id="units" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Measured in SI"
                        </h2>

                        <div id="units-meters" class="subsection">
                            <h3>"Meters Everywhere"</h3>
                            <p>
                                "One unit is one meter. The engine stores every length in meters ("
                                <code>"ENGINE_NATIVE_UNIT"</code>" is "<code>"Unit::Meter"</code>"), and gravity is
                                standard gravity, pointing down:"
                            </p>
                            <div class="equation-card">
                                <div class="equation">"g = 9.80665 m/s²"</div>
                                <div class="equation-label">"Standard gravity, the engine default"</div>
                            </div>
                            <p>
                                "Conversions happen only at the edges: when a file written in another unit loads,
                                and when the Properties panel shows or takes a value in your display unit. A
                                value read from a file, a script or a physics result is already in meters."
                            </p>
                        </div>

                        <div id="units-display" class="subsection">
                            <h3>"Display Units"</h3>
                            <p>
                                "If you think in other units, pick one from the unit badge, which reads "
                                <code>"m"</code>" until you change it: centimeters, millimeters, feet and inches
                                are among the choices. The Properties panel then shows and accepts lengths in that
                                unit and converts them to meters. The world itself does not change."
                            </p>
                            <p>
                                "A file can be written in another unit too. Name it with "<code>"unit"</code>" under "
                                <code>"[metadata]"</code>", and the loader converts the position and scale to meters
                                once, as it loads:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Workspace/Crate/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[metadata]
class_name = "Part"
unit = "cm"

[asset]
mesh = "parts/block.glb"
scene = "Scene0"

[transform]
position = [0.0, 50.0, 0.0]    # 0.5 m
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [10.0, 10.0, 10.0]     # a 10 cm cube"#}</code></pre>
                            </div>
                        </div>

                        <div id="units-si" class="subsection">
                            <h3>"SI in the Physics"</h3>
                            <p>
                                "The physics libraries use SI throughout, and each quantity is its own Rust type: "
                                <code>"Meters"</code>", "<code>"Kilograms"</code>", "<code>"Seconds"</code>", "
                                <code>"Kelvin"</code>", "<code>"Moles"</code>" and "<code>"Amperes"</code>", with derived
                                types such as "<code>"Newtons"</code>", "<code>"Pascals"</code>", "<code>"Joules"</code>
                                " and "<code>"Watts"</code>". A mass cannot be passed where a length is expected, and
                                the compiler says so."
                            </p>
                            <p>
                                "Physical constants such as the gravitational constant and the speed of light are
                                stored in SI too. "<a href="/docs/realism">"Realism"</a>" covers the laws built on
                                them."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // SAME INPUTS, SAME WORLD
                    // =========================================================
                    <section id="determinism" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Same Inputs, Same World"
                        </h2>

                        <div id="determinism-pins" class="subsection">
                            <h3>"The Pins"</h3>
                            <p>
                                "A result is worth comparing only if running it again gives the same answer, so
                                Eustress fixes every setting that shapes a physics step:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Setting"</th><th>"Value"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Fixed timestep"</td><td>"60 Hz"</td></tr>
                                    <tr><td>"Physics substeps"</td><td>"6 per step"</td></tr>
                                    <tr><td>"Solver settings"</td><td>"Avian's defaults, set explicitly"</td></tr>
                                    <tr><td>"Gravity"</td><td>"9.80665 m/s², downward"</td></tr>
                                    <tr><td>"Random seed"</td><td><code>"GlobalRngSeed"</code>", a fixed constant by default"</td></tr>
                                </tbody>
                            </table>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"crates/engine/src/app_core.rs"</span>
                                </div>
                                <pre><code class="language-rust">{r#".insert_resource(avian3d::prelude::Gravity(bevy::math::Vec3::NEG_Y * 9.80665))
.insert_resource(Time::<bevy::time::Fixed>::from_hz(60.0))
.insert_resource(avian3d::prelude::SubstepCount(6))
.insert_resource(avian3d::dynamics::solver::SolverConfig::default())
.add_plugins(eustress_common::physics::DeterminismPlugin)"#}</code></pre>
                            </div>
                            <p>
                                "The timestep and the substep count are written into the engine as values rather
                                than left to Avian's defaults, so a physics library update cannot change them
                                quietly."
                            </p>
                        </div>

                        <div id="determinism-seed" class="subsection">
                            <h3>"One Seed"</h3>
                            <p>
                                "Randomness that shapes a simulation comes from one seed. "<code>"GlobalRngSeed"</code>
                                " is a fixed constant unless you set another, and the particle simulation and the
                                scenario engine derive their random streams from it. A run that uses randomness can
                                be repeated from the Space alone, and changing the seed varies it on purpose."
                            </p>
                            <p>
                                "The headless runner makes a run a function of its inputs. This simulates 600
                                ticks, 10 seconds at 60 Hz, writes the recording and exits with a status code:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Bash"</span>
                                </div>
                                <pre><code class="language-bash">{r#"eustress run ~/Documents/Eustress/Universe1/Spaces/Space1 --ticks 600"#}</code></pre>
                            </div>
                            <p><a href="/learn/cli">"CLI & Headless"</a>" covers the runner and its options."</p>
                        </div>
                    </section>

                    // =========================================================
                    // SOURCE-AVAILABLE
                    // =========================================================
                    <section id="license" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "Source-Available"
                        </h2>

                        <div id="license-shield" class="subsection">
                            <h3>"PolyForm Shield"</h3>
                            <p>
                                "Eustress is source-available. The source is public on "
                                <a href="https://github.com/WeaveITMeta/EustressEngine">"GitHub"</a>", and the root "
                                <code>"LICENSE"</code>" is the PolyForm Shield License 1.0.0. It lets you use the
                                software for any purpose, change it, build new works on it and distribute copies,
                                with one exception: providing a product that competes with Eustress, or with a
                                product Eustress LLC provides using it. Copies you distribute must carry the
                                license terms and its "<code>"Required Notice"</code>" line."
                            </p>
                            <p>"At no cost, the license covers:"</p>
                            <ul class="docs-list">
                                <li><strong>"Products made with Eustress"</strong>": building, shipping and selling simulations, digital twins, training environments and visualizations."</li>
                                <li><strong>"Internal use"</strong>": at a company of any size, including in production."</li>
                                <li><strong>"Changes"</strong>": modifying and forking the engine for your own products."</li>
                                <li><strong>"Learning"</strong>": academic use, research, evaluation and personal projects."</li>
                            </ul>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Built with it, or a substitute for it"</strong>
                                    <p>
                                        "If your product is built with Eustress rather than being a substitute for
                                        it, you owe nothing and you do not need to ask."
                                    </p>
                                </div>
                            </div>
                            <p>
                                "Third-party crates such as Bevy, Slint and Avian keep their own licenses, and
                                one first-party crate, "<code>"eustress-embedvec"</code>", declares MIT in its
                                manifest."
                            </p>
                        </div>

                        <div id="license-commercial" class="subsection">
                            <h3>"The Commercial License"</h3>
                            <p>
                                "For rights the Shield license does not give, "<code>"LICENSE-COMMERCIAL.md"</code>
                                " describes a negotiated commercial license: to offer Eustress, a fork of it or a
                                substantially similar platform as your own product, or to get conventional terms
                                such as warranties, indemnification, support SLAs or a perpetual grant. Pricing
                                follows the rights granted, not your revenue from products the Shield license
                                already permits."
                            </p>
                            <p>
                                "Write to "<code>"licensing@eustress.dev"</code>", or read the full terms on the "
                                <a href="/license">"License"</a>" page."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // WHAT'S NEXT
                    // =========================================================
                    <section id="roadmap" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"08"</span>
                            "What's Next"
                        </h2>

                        <div id="roadmap-determinism" class="subsection">
                            <h3>"A Determinism Gate"</h3>
                            <p>
                                "The pins make determinism testable, and the next step is the test itself. The
                                headless runtime plan sets a gate: the same Space, run twice for the same number
                                of ticks, will have to produce byte-identical recordings."
                            </p>
                        </div>

                        <div id="roadmap-branches" class="subsection">
                            <h3>"Branching the Whole World"</h3>
                            <p>
                                "Agents already share your Spaces. Next, they will be able to fork one, database
                                included, try a change and keep it only if it wins. The storage library's
                                copy-on-write branches are built and tested, and "
                                <a href="/docs/universes#roadmap">"Universes"</a>" describes what is left to wire."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Build it, run it, measure it, and keep the files."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/learn/mcp" class="btn-secondary-steel">"MCP Server Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/learn/lsp" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Rune LSP"</span>
                            </div>
                        </a>
                        <a href="/learn" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"All Topics"</span>
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
