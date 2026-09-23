// =============================================================================
// Eustress Web - Scripting Documentation Page
// =============================================================================
// Scripting: Rune scripts that run in Play, the Luau runtime behind the command
// bar and Studio plugins, SoulScript summaries, and the API each one exposes.
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
                TocSubsection { id: "overview-what", title: "What a Script Is" },
                TocSubsection { id: "overview-languages", title: "Rune and Luau" },
            ],
        },
        TocSection {
            id: "disk",
            title: "Scripts on Disk",
            subsections: vec![
                TocSubsection { id: "disk-folder", title: "The Script Folder" },
                TocSubsection { id: "disk-loose", title: "Loose Files" },
                TocSubsection { id: "disk-classes", title: "Script Classes" },
            ],
        },
        TocSection {
            id: "run",
            title: "When Scripts Run",
            subsections: vec![
                TocSubsection { id: "run-lifecycle", title: "The Play Lifecycle" },
                TocSubsection { id: "run-state", title: "State Between Frames" },
                TocSubsection { id: "run-reload", title: "Editing While Playing" },
                TocSubsection { id: "run-errors", title: "Errors and Output" },
            ],
        },
        TocSection {
            id: "rune",
            title: "The Rune API",
            subsections: vec![
                TocSubsection { id: "rune-imports", title: "Imports" },
                TocSubsection { id: "rune-types", title: "Value Types" },
                TocSubsection { id: "rune-world", title: "Instances and Raycasts" },
                TocSubsection { id: "rune-more", title: "Simulation, UI and Files" },
            ],
        },
        TocSection {
            id: "luau",
            title: "Luau",
            subsections: vec![
                TocSubsection { id: "luau-runtime", title: "The Luau Runtime" },
                TocSubsection { id: "luau-globals", title: "Globals" },
                TocSubsection { id: "luau-example", title: "Building from the Command Bar" },
            ],
        },
        TocSection {
            id: "tools",
            title: "Tools",
            subsections: vec![
                TocSubsection { id: "tools-command-bar", title: "The Command Bar" },
                TocSubsection { id: "tools-editor", title: "The Script Editor" },
                TocSubsection { id: "tools-soul", title: "From Summary to Code" },
                TocSubsection { id: "tools-plugins", title: "Studio Plugins" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-luau", title: "Luau in Play" },
                TocSubsection { id: "roadmap-rune", title: "A Wider Rune API" },
            ],
        },
    ]
}

/// The Rune callbacks Studio calls, in order, across one Play session.
#[component]
fn LifecycleDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 170" role="img"
                aria-label="Pressing Play compiles every Rune script and calls on_init, then on_ready, then on_update every frame until Stop calls on_exit.">
                <defs>
                    <marker id="life-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>
                <rect x="10" y="80" width="96" height="48" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="58" y="109" class="dg-label" text-anchor="middle">"Play"</text>
                <line x1="106" y1="104" x2="138" y2="104" class="dg-line" marker-end="url(#life-arrow)"></line>
                <text x="122" y="150" class="dg-note" text-anchor="middle">"compile"</text>

                <rect x="140" y="80" width="96" height="48" rx="8" class="dg-box"></rect>
                <text x="188" y="109" class="dg-label" text-anchor="middle">"on_init"</text>
                <line x1="236" y1="104" x2="268" y2="104" class="dg-line" marker-end="url(#life-arrow)"></line>

                <rect x="270" y="80" width="96" height="48" rx="8" class="dg-box"></rect>
                <text x="318" y="109" class="dg-label" text-anchor="middle">"on_ready"</text>
                <line x1="366" y1="104" x2="398" y2="104" class="dg-line" marker-end="url(#life-arrow)"></line>

                <rect x="400" y="80" width="110" height="48" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="455" y="109" class="dg-label" text-anchor="middle">"on_update(dt)"</text>
                <path d="M 430 80 C 430 40, 480 40, 480 78" class="dg-line-dashed" fill="none" marker-end="url(#life-arrow)"></path>
                <text x="455" y="30" class="dg-note" text-anchor="middle">"every frame"</text>
                <line x1="510" y1="104" x2="538" y2="104" class="dg-line" marker-end="url(#life-arrow)"></line>
                <text x="524" y="150" class="dg-note" text-anchor="middle">"Stop"</text>

                <rect x="540" y="80" width="90" height="48" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="585" y="109" class="dg-label" text-anchor="middle">"on_exit"</text>
            </svg>
            <figcaption>
                "One Play session for a Rune script. on_init and on_ready run once in the first
                frame, on_update runs every frame after them, and Stop calls on_exit."
            </figcaption>
        </figure>
    }
}

/// Scripting documentation page.
#[component]
pub fn DocsScriptingPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-scripting"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/code.svg" alt="Scripting" class="toc-icon" />
                        <h2>"Scripting"</h2>
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
                            <span class="current">"Scripting"</span>
                        </div>
                        <h1 class="docs-title">"Scripting"</h1>
                        <p class="docs-subtitle">
                            "Scripts are source files inside a Space that Eustress compiles and runs.
                            Rune scripts run frame by frame while you play; a Roblox-compatible Luau runtime
                            powers the command bar and Studio plugins; and a SoulScript can start as a
                            plain-language summary that Claude turns into Rune."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "14 min read"
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

                        <div id="overview-what" class="subsection">
                            <h3>"What a Script Is"</h3>
                            <p>
                                "A script is a file of code that lives in a Space next to the parts it works on.
                                Eustress reads it when the Space opens, shows it in the Explorer, and runs Rune
                                scripts when you press Play. The file extension picks the language: "
                                <code>".rune"</code>" for Rune, "<code>".luau"</code>" or "<code>".lua"</code>" for Luau."
                            </p>
                            <p>
                                "Scripts you create are "<strong>"SoulScripts"</strong>": a folder holding the
                                code, an optional Markdown summary of what the code should do, and a small
                                "<code>"_instance.toml"</code>" that marks the folder as a script. Because every
                                piece is a plain file, scripts travel with the Space through git, diffs and
                                external editors."
                            </p>
                        </div>

                        <div id="overview-languages" class="subsection">
                            <h3>"Rune and Luau"</h3>
                            <p>
                                "Rune is the language Eustress runs during Play. Luau is available as a
                                Roblox-compatible runtime with Roblox-style globals, and today it runs from the
                                command bar and from Studio plugins."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Where code runs"</th><th>"Rune"</th><th>"Luau"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Play and Play Solo"</td><td>"Yes, every frame"</td><td>"Not yet (see What's Next)"</td></tr>
                                    <tr><td>"Command bar"</td><td>"Yes"</td><td>"Yes"</td></tr>
                                    <tr><td>"Studio plugins"</td><td>"Yes, "<code>".rune"</code></td><td>"Yes, "<code>".lua"</code></td></tr>
                                    <tr><td>"API style"</td><td>"Functions and types in the "<code>"eustress"</code>" module"</td><td>"Roblox globals such as "<code>"Instance"</code>" and "<code>"game"</code></td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // SCRIPTS ON DISK
                    // =========================================================
                    <section id="disk" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Scripts on Disk"
                        </h2>

                        <div id="disk-folder" class="subsection">
                            <h3>"The Script Folder"</h3>
                            <p>
                                "The Script button (in the ribbon of Modes that offer it) creates a script folder
                                in "<code>"SoulService"</code>", or inside the Explorer item you have selected.
                                The code and summary files take the folder's name:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Space folder"</span>
                                </div>
                                <pre><code class="language-text">{r#"SoulService/
  SoulScript/
    _instance.toml     marks the folder as a SoulScript
    SoulScript.rune    the code
    SoulScript.md      the summary"#}</code></pre>
                            </div>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"SoulService/SoulScript/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[metadata]
class_name = "SoulScript"
archivable = true

[script]
source = "SoulScript.rune""#}</code></pre>
                            </div>
                            <p>
                                "The loader opens the file named by "<code>"[script] source"</code>". Without that
                                key it takes the first "<code>".rune"</code>", "<code>".luau"</code>", "
                                <code>".soul"</code>" or "<code>".lua"</code>" file in the folder. Everything else
                                in the folder is left alone, so the summary and any notes never show up as
                                objects of their own. The "<a href="/learn/mcp">"MCP server"</a>"'s "
                                <code>"execute_rune"</code>" and "<code>"execute_luau"</code>" tools write
                                scripts in this same layout."
                            </p>
                        </div>

                        <div id="disk-loose" class="subsection">
                            <h3>"Loose Files"</h3>
                            <p>
                                "A bare "<code>".rune"</code>", "<code>".luau"</code>" or "<code>".lua"</code>" file
                                dropped into any service folder also loads as a script, so "
                                <code>"ServerScriptService/spawner.rune"</code>" works without a folder. Loose
                                script files that sit directly in "<code>"SoulService"</code>" are moved into
                                their own folders the next time the Space opens, with an "
                                <code>"_instance.toml"</code>" and an empty summary. If a folder with that name
                                already exists, the file is left where it is and a warning is logged."
                            </p>
                        </div>

                        <div id="disk-classes" class="subsection">
                            <h3>"Script Classes"</h3>
                            <p>
                                "The Luau classes carry Roblox's script types. The Roblox importer brings
                                "<code>"Script"</code>", "<code>"LocalScript"</code>" and "<code>"ModuleScript"</code>
                                " in as these, each with a "<code>"script.luau"</code>" source file; see "
                                <a href="/docs/importing">"Importing"</a>"."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Class"</th><th>"Roblox name accepted"</th><th>"Holds"</th><th>"Runs in Play"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"SoulScript"</code></td><td>"-"</td><td>"Rune code and a summary"</td><td>"Yes"</td></tr>
                                    <tr><td><code>"LuauScript"</code></td><td><code>"Script"</code></td><td>"Luau source"</td><td>"Not yet"</td></tr>
                                    <tr><td><code>"LuauLocalScript"</code></td><td><code>"LocalScript"</code></td><td>"Luau source"</td><td>"Not yet"</td></tr>
                                    <tr><td><code>"LuauModuleScript"</code></td><td><code>"ModuleScript"</code></td><td>"Luau source"</td><td>"Not yet"</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // WHEN SCRIPTS RUN
                    // =========================================================
                    <section id="run" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "When Scripts Run"
                        </h2>

                        <div id="run-lifecycle" class="subsection">
                            <h3>"The Play Lifecycle"</h3>
                            <p>
                                "Scripts do nothing while you edit. When you press Play ("<code>"F5"</code>") or
                                Play Solo ("<code>"F7"</code>"), Studio compiles every Rune script in the Space
                                and then calls these functions in each script that defines them. A function a
                                script leaves out is simply skipped."
                            </p>
                            <LifecycleDiagram />
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Function"</th><th>"Called"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"on_init()"</code></td><td>"Once, in the first Play frame, and again after the script is recompiled"</td></tr>
                                    <tr><td><code>"on_ready()"</code></td><td>"Once, right after the first "<code>"on_init"</code></td></tr>
                                    <tr><td><code>"on_update(dt)"</code></td><td>"Every frame while playing; "<code>"dt"</code>" is the frame time in seconds"</td></tr>
                                    <tr><td><code>"on_exit()"</code></td><td>"When you stop ("<code>"F8"</code>")"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Pause ("<code>"F6"</code>") stops the calls until you resume."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A script with only main() does nothing in Play"</strong>
                                    <p>
                                        <code>"main()"</code>" is the entry point for one-shot runs: the command bar
                                        and the MCP tools. Play only calls the lifecycle functions above. The starter
                                        file the Script button writes uses "<code>"main"</code>", so move its body
                                        into "<code>"on_init"</code>" when you want it to run on Play."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="run-state" class="subsection">
                            <h3>"State Between Frames"</h3>
                            <p>
                                "Every call starts a fresh Rune VM over the compiled script, and Rune has no mutable
                                globals, so a local variable does not survive from one frame to the next. Keep
                                running values in sim values, which persist across frames and are what
                                watchpoints, recordings and the MCP sim tools read:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"SoulService/Counter/Counter.rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::{log_info, get_sim_value, set_sim_value};

pub fn on_init() {
    set_sim_value("demo.elapsed", 0.0);
    log_info("Counter started");
}

pub fn on_update(dt) {
    let t = get_sim_value("demo.elapsed") + dt;
    set_sim_value("demo.elapsed", t);
}

pub fn on_exit() {
    log_info("Counter stopped");
}"#}</code></pre>
                            </div>
                            <p>
                                "Parts a script creates during Play exist only for that session: they are
                                marked as script-spawned and removed when you stop, and nothing is written to
                                disk. See "<a href="/docs/simulation">"Simulation"</a>" for watchpoints and
                                recordings."
                            </p>
                        </div>

                        <div id="run-reload" class="subsection">
                            <h3>"Editing While Playing"</h3>
                            <p>
                                "Save a "<code>".rune"</code>" file while the Space is playing, in Studio or in an
                                external editor, and the file watcher hands the new source to the running
                                session. The script recompiles in place on the next frame and "
                                <code>"on_init"</code>" runs again for the new code. If the new version fails to
                                compile, the last good version keeps running and the errors go to Output."
                            </p>
                        </div>

                        <div id="run-errors" class="subsection">
                            <h3>"Errors and Output"</h3>
                            <p>
                                "Compile errors appear in the Output panel one per line, in the form "
                                <code>"script:line:col: error: message"</code>". Errors thrown while "
                                <code>"on_init"</code>", "<code>"on_ready"</code>" or "<code>"on_update"</code>
                                " runs are reported there too, under the script's name. To print from a script, use "
                                <code>"log_info"</code>", "<code>"log_warn"</code>" or "<code>"log_error"</code>
                                "; they write to Output and to the engine log. Rune's own "<code>"println"</code>
                                " is captured into Output only in the command bar."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // THE RUNE API
                    // =========================================================
                    <section id="rune" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "The Rune API"
                        </h2>

                        <div id="rune-imports" class="subsection">
                            <h3>"Imports"</h3>
                            <p>
                                "Every Eustress function and type lives in the "<code>"eustress"</code>" module, so a
                                script file names what it uses at the top. The same module set compiles your
                                scripts, the command bar, the script editor's checks and the "
                                <a href="/learn/lsp">"Rune language server"</a>", so they agree on what exists."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::{log_info, Vector3, Instance};
// or everything at once:
use eustress::*;

// Physical-law libraries live one level down:
use eustress::realism::electrical;"#}</code></pre>
                            </div>
                            <p>
                                "The realism libraries are covered on the "<a href="/docs/realism">"Realism"</a>
                                " page."
                            </p>
                        </div>

                        <div id="rune-types" class="subsection">
                            <h3>"Value Types"</h3>
                            <p>
                                "The value types follow Roblox naming, with snake_case methods. Positions and sizes
                                are in meters; angles are in radians."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Type"</th><th>"Create"</th><th>"Fields and methods"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"Vector3"</code></td><td><code>"Vector3::new(x, y, z)"</code></td><td><code>"x y z"</code>", "<code>"magnitude unit dot cross lerp add sub mul div neg"</code></td></tr>
                                    <tr><td><code>"CFrame"</code></td><td><code>"new(x, y, z)"</code>", "<code>"from_position"</code>", "<code>"angles(rx, ry, rz)"</code>", "<code>"look_at(from, to)"</code></td><td><code>"position"</code>", "<code>"x y z look_vector right_vector up_vector inverse point_to_world_space point_to_object_space lerp mul add sub"</code></td></tr>
                                    <tr><td><code>"Color3"</code></td><td><code>"new(r, g, b)"</code>" (0 to 1), "<code>"from_rgb"</code>" (0 to 255), "<code>"from_hsv"</code></td><td><code>"r g b"</code>", "<code>"lerp to_hsv"</code></td></tr>
                                    <tr><td><code>"UDim"</code></td><td><code>"UDim::new(scale, offset)"</code></td><td><code>"scale offset"</code>", "<code>"add sub"</code></td></tr>
                                    <tr><td><code>"UDim2"</code></td><td><code>"new(xs, xo, ys, yo)"</code>", "<code>"from_scale"</code>", "<code>"from_offset"</code></td><td><code>"x_scale x_offset y_scale y_offset"</code>", "<code>"x y add sub lerp"</code></td></tr>
                                </tbody>
                            </table>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::{log_info, Vector3, CFrame, Color3};

pub fn on_init() {
    let target = Vector3::new(3.0, 0.0, 4.0);
    log_info(`distance ${target.magnitude()} m`);   // 5 m

    let eye = CFrame::look_at(Vector3::new(0.0, 2.0, 10.0), target);
    let ahead = eye.point_to_world_space(Vector3::new(0.0, 0.0, -5.0));
    log_info(`five meters ahead: ${ahead.x}, ${ahead.y}, ${ahead.z}`);

    let tint = Color3::from_hsv(0.6, 0.8, 1.0);
    log_info(`blue channel ${tint.b}`);
}"#}</code></pre>
                            </div>
                        </div>

                        <div id="rune-world" class="subsection">
                            <h3>"Instances and Raycasts"</h3>
                            <p>
                                <code>"Instance::new(class)"</code>" returns an instance handle that you name and set
                                properties on. During Play, Part-family classes ("<code>"Part"</code>", "
                                <code>"MeshPart"</code>", "<code>"SpherePart"</code>", "<code>"CylinderPart"</code>
                                ", "<code>"WedgePart"</code>", "<code>"CornerWedgePart"</code>") appear in the world
                                at the end of the frame; other classes are skipped with one warning per class."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Property"</th><th>"Value"</th><th>"Default"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"Position"</code></td><td><code>"Vector3"</code>", meters"</td><td>"0, 0.5, 0"</td></tr>
                                    <tr><td><code>"Size"</code></td><td><code>"Vector3"</code>", meters"</td><td>"4, 1, 2"</td></tr>
                                    <tr><td><code>"Orientation"</code></td><td><code>"Vector3"</code>", degrees"</td><td>"0, 0, 0"</td></tr>
                                    <tr><td><code>"Color"</code></td><td><code>"Color3"</code></td><td>"Gray"</td></tr>
                                    <tr><td><code>"Material"</code></td><td>"Material name, such as "<code>"Neon"</code></td><td><code>"Plastic"</code></td></tr>
                                    <tr><td><code>"Shape"</code></td><td><code>"Block"</code>", "<code>"Ball"</code>", "<code>"Cylinder"</code>", "<code>"Wedge"</code>", "<code>"CornerWedge"</code>", "<code>"Cone"</code></td><td><code>"Block"</code></td></tr>
                                    <tr><td><code>"Anchored"</code></td><td>"bool"</td><td>"false"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                <code>"workspace_raycast(origin, direction, None)"</code>" casts a ray from "
                                <code>"origin"</code>" along "<code>"direction"</code>" for up to 1,000 m and
                                returns the first hit, if any. The hit has "
                                <code>"instance"</code>" (the part's name), "<code>"position"</code>", "<code>"normal"</code>", "
                                <code>"distance"</code>" and "<code>"material"</code>". Raycasts answer one frame
                                late: each call returns the result of the same call on the previous frame, so the
                                first frame returns "<code>"None"</code>"."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"SoulService/Probe/Probe.rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::{set_sim_value, workspace_raycast, Instance, Vector3, Color3};

pub fn on_init() {
    if let Some(ball) = Instance::new("Part") {
        ball.set_name("Probe");
        ball.set("Shape", "Ball");
        ball.set("Size", Vector3::new(1.0, 1.0, 1.0));
        ball.set("Position", Vector3::new(0.0, 12.0, 0.0));
        ball.set("Color", Color3::new(1.0, 0.4, 0.1));
        ball.set("Anchored", true);
    }
}

pub fn on_update(dt) {
    let down = Vector3::new(0.0, -1.0, 0.0);
    if let Some(hit) = workspace_raycast(Vector3::new(0.0, 20.0, 0.0), down, None) {
        set_sim_value("probe.drop", hit.distance);
    }
}"#}</code></pre>
                            </div>
                        </div>

                        <div id="rune-more" class="subsection">
                            <h3>"Simulation, UI and Files"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Group"</th><th>"Functions"</th><th>"What they do"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Sim values"</td><td><code>"get_sim_value(key)"</code>", "<code>"set_sim_value(key, value)"</code>", "<code>"list_sim_values()"</code></td><td>"Read and publish named numbers; a missing key reads as 0"</td></tr>
                                    <tr><td>"Output"</td><td><code>"log_info"</code>", "<code>"log_warn"</code>", "<code>"log_error"</code></td><td>"Write a line to the Output panel"</td></tr>
                                    <tr><td>"UI"</td><td><code>"gui_set_text"</code>", "<code>"gui_set_visible"</code>", "<code>"gui_set_text_color"</code>", "<code>"gui_set_bg_color"</code>", "<code>"gui_set_border_color"</code>", "<code>"gui_set_font_size"</code></td><td>"Change a GUI element by name; see "<a href="/docs/ui">"UI Systems"</a></td></tr>
                                    <tr><td>"Physics"</td><td><code>"part_apply_impulse"</code>", "<code>"part_apply_angular_impulse"</code>", "<code>"part_set_velocity"</code></td><td>"Push a physics body by name, in kg m/s and m/s"</td></tr>
                                    <tr><td>"Tags"</td><td><code>"collection_add_tag"</code>", "<code>"collection_has_tag"</code>", "<code>"collection_get_tagged"</code></td><td>"Tag created instances; list ids of tagged objects"</td></tr>
                                    <tr><td>"Units"</td><td><code>"units_from_meters(v, unit)"</code>", "<code>"units_to_meters(v, unit)"</code></td><td>"Convert for display; the unit is a symbol such as "<code>"ft"</code></td></tr>
                                    <tr><td>"Space files"</td><td><code>"read_space_file"</code>", "<code>"write_space_file"</code></td><td>"Read and write text files by path relative to the Space; paths containing "<code>".."</code>" are refused"</td></tr>
                                    <tr><td>"HTTP"</td><td><code>"http_get_async(url)"</code>", "<code>"http_post_async(url, body)"</code></td><td>"Blocking request; the frame waits for the response"</td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"write_space_file changes the Space for good"</strong>
                                    <p>
                                        "Stop restores the scene, but it does not restore files. A file a script
                                        writes during Play is still there after you stop, and it is part of the
                                        Space from then on."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // LUAU
                    // =========================================================
                    <section id="luau" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Luau"
                        </h2>

                        <div id="luau-runtime" class="subsection">
                            <h3>"The Luau Runtime"</h3>
                            <p>
                                "The Luau runtime is a sandboxed Luau VM with a Roblox-style API installed as
                                globals, so ported Roblox code reads the way it did. It runs today in two places:
                                the command bar and Studio plugins. Luau files in a Space load into the Explorer,
                                but Play does not start them yet."
                            </p>
                            <p>
                                "In the command bar a script runs once from top to bottom on a fresh VM. The
                                coroutine scheduler is present, so "<code>"task.spawn"</code>" runs its function
                                right away. A "<code>"task.wait"</code>" at the top level returns at once, and a
                                function that waits inside "<code>"task.spawn"</code>" never resumes, because
                                nothing advances the scheduler outside Play."
                            </p>
                        </div>

                        <div id="luau-globals" class="subsection">
                            <h3>"Globals"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Group"</th><th>"Globals"</th><th>"Notes"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Values"</td><td><code>"Vector3"</code>", "<code>"CFrame"</code>", "<code>"Color3"</code>", "<code>"UDim"</code>", "<code>"UDim2"</code></td><td>"Roblox constructors ("<code>"Color3.fromRGB"</code>", "<code>"CFrame.lookAt"</code>", "<code>"UDim2.fromScale"</code>"), operators and methods"</td></tr>
                                    <tr><td>"Output"</td><td><code>"print"</code>", "<code>"warn"</code>", "<code>"typeof"</code>", "<code>"tick"</code></td><td>"In the command bar, print and warn go to Output"</td></tr>
                                    <tr><td>"Instances"</td><td><code>"Instance.new(class, parent)"</code></td><td>"Returns a table of properties; the parent argument is optional"</td></tr>
                                    <tr><td>"Services"</td><td><code>"game:GetService(name)"</code></td><td><code>"Players"</code>", "<code>"ReplicatedStorage"</code>", "<code>"ServerStorage"</code>", "<code>"ServerScriptService"</code>", "<code>"StarterGui"</code>", "<code>"StarterPlayer"</code>", "<code>"StarterPack"</code>", "<code>"Lighting"</code>", "<code>"CollectionService"</code></td></tr>
                                    <tr><td>"Tags"</td><td><code>"CollectionService"</code></td><td><code>"AddTag"</code>", "<code>"RemoveTag"</code>", "<code>"HasTag"</code>", "<code>"GetTagged"</code>"; GetTagged also returns ids of tagged objects already in the Space"</td></tr>
                                    <tr><td>"Scheduling"</td><td><code>"task"</code>", "<code>"wait"</code>", "<code>"spawn"</code>", "<code>"delay"</code></td><td>"Coroutine scheduler; waits resume only while the Play driver steps it"</td></tr>
                                    <tr><td>"Signals"</td><td><code>"RunService"</code>", "<code>"UserInputService"</code>", "<code>"Touched"</code></td><td>"Present; they fire only under the Luau Play driver (see What's Next)"</td></tr>
                                    <tr><td>"Helpers"</td><td><code>"Units"</code>", "<code>"Enum"</code></td><td><code>"Units.from_meters"</code>", "<code>"Units.to_meters"</code>"; "<code>"Enum.KeyCode.Space"</code>" reads as the string "<code>"KeyCode.Space"</code></td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Other Roblox services such as "<code>"RunService"</code>" and "
                                <code>"TweenService"</code>" are plain globals rather than "
                                <code>"GetService"</code>" results."
                            </p>
                        </div>

                        <div id="luau-example" class="subsection">
                            <h3>"Building from the Command Bar"</h3>
                            <p>
                                "When a command-bar run ends, every "<code>"Part"</code>" it created becomes a real
                                part: it is saved into the Space under "<code>"Workspace"</code>" and spawned, so
                                it stays after the run. "<code>"BillboardGui"</code>" and "
                                <code>"TextLabel"</code>" instances are materialized the same way. This builds a
                                tagged staircase:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Luau (command bar)"</span>
                                </div>
                                <pre><code class="language-lua">{r#"for i = 1, 10 do
    local step = Instance.new("Part")
    step.Name = "Step" .. i
    step.Size = Vector3.new(4, 0.4, 1.2)
    step.Position = Vector3.new(0, i * 0.4, i * 1.2)
    step.Color = Color3.fromHSV(i / 10, 0.6, 0.9)
    step.Anchored = true
    CollectionService:AddTag(step, "Stairs")
end
print("Built 10 steps")"#}</code></pre>
                            </div>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Stick to parts and labels"</strong>
                                    <p>
                                        "The command bar saves every created instance other than BillboardGui and
                                        TextLabel with a block mesh, whatever its class. Create folders, models and
                                        other classes from the Insert menu instead."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // TOOLS
                    // =========================================================
                    <section id="tools" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Tools"
                        </h2>

                        <div id="tools-command-bar" class="subsection">
                            <h3>"The Command Bar"</h3>
                            <p>
                                "The command bar runs code against the open Space, in Edit or in Play. Click the
                                language label at its left to switch between Rune and Luau. "
                                <code>"Enter"</code>" runs, "<code>"Shift+Enter"</code>" adds a new line, and
                                pasted multi-line code works as typed. Each run echoes the code and its output
                                to Output."
                            </p>
                            <p>
                                "Rune in the command bar takes plain statements: a snippet that declares no
                                function is wrapped in "<code>"main"</code>" and gets "<code>"use eustress::*;"</code>
                                " added, so "<code>"log_info(`hi`)"</code>" works on its own. A snippet that
                                declares items runs as written, starting at "<code>"main"</code>" or, if there is
                                none, "<code>"on_init"</code>"."
                            </p>
                        </div>

                        <div id="tools-editor" class="subsection">
                            <h3>"The Script Editor"</h3>
                            <p>
                                "Double-click a script in the Explorer to open it in a Studio tab. A SoulScript
                                tab switches between two views, "<strong>"Summary"</strong>" and "
                                <strong>"Code"</strong>". Summary edits are saved to the "<code>".md"</code>
                                " file as you type; in the Code view, "<strong>"Save"</strong>" writes the code
                                file."
                            </p>
                            <p>
                                "Rune code is checked as you type, 80 ms after you stop. Errors and warnings show
                                in the editor and in the Problems panel, and the same checks run over every
                                script when a Space opens. For editing in VS Code or another editor, see "
                                <a href="/learn/ide">"IDE Integration"</a>" and "<a href="/learn/lsp">"Rune LSP"</a>"."
                            </p>
                        </div>

                        <div id="tools-soul" class="subsection">
                            <h3>"From Summary to Code"</h3>
                            <p>
                                "A SoulScript's summary is a Markdown description of what the script should do.
                                In the Summary view, "<strong>"Build"</strong>" sends it to Claude together with
                                the names, classes, positions, sizes and colors of the objects in the scene.
                                Claude writes Rune, and the result is saved into the folder's code file. "
                                <strong>"Summarize"</strong>" goes the other way: it writes a summary of the code
                                into "<code>"<name>.md"</code>"."
                            </p>
                            <ol class="numbered-list">
                                <li>"Open "<strong>"File > Soul Settings..."</strong>" and enter your Anthropic API key under "<strong>"API Key"</strong>"."</li>
                                <li>"Click "<strong>"Save"</strong>". The key is stored on your machine in "<code>"~/.eustress_engine/soul_settings.json"</code>"."</li>
                                <li>"Open a SoulScript, write the summary in the Summary view, and click "<strong>"Build"</strong>"."</li>
                            </ol>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Builds need your own key"</strong>
                                    <p>
                                        "Without an API key, Build stops with an error in the script's status.
                                        Everything else on this page works without one: plain Rune and Luau never
                                        call a model."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="tools-plugins" class="subsection">
                            <h3>"Studio Plugins"</h3>
                            <p>
                                "A Studio plugin is a script that adds buttons to the ribbon's "
                                <strong>"Plugins"</strong>" tab. Put a "<code>".lua"</code>" file, a folder with an "
                                <code>"init.lua"</code>", or a "<code>".rune"</code>" file in the "
                                <code>"Eustress/Plugins"</code>" folder under your local data folder: "
                                <code>"%LOCALAPPDATA%"</code>" on Windows, "<code>"~/Library/Application Support"</code>
                                " on macOS, "<code>"~/.local/share"</code>" on Linux. Studio loads each one once per
                                session; "
                                <strong>"Reload Plugins"</strong>" on the Plugins tab reloads them all. A Luau
                                plugin runs top to bottom; a Rune plugin must define "
                                <code>"pub fn register()"</code>"."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Luau"</th><th>"Rune"</th><th>"Purpose"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"plugin:AddSection(tab, id, label)"</code></td><td><code>"plugin_add_section(id, label)"</code></td><td>"Add a group to the Plugins tab"</td></tr>
                                    <tr><td><code>"plugin:AddButton(...)"</code></td><td><code>"plugin_add_button(section, id, label, tooltip, callback)"</code></td><td>"Add a button that calls a function"</td></tr>
                                    <tr><td><code>"plugin:Notify(level, message)"</code></td><td><code>"plugin_notify(level, message)"</code></td><td>"Show a notification: info, success, warning or error"</td></tr>
                                    <tr><td><code>"plugin:GetSelection()"</code></td><td><code>"plugin_get_selection()"</code></td><td>"Ids of the selected objects"</td></tr>
                                </tbody>
                            </table>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Plugins/selection_counter.lua"</span>
                                </div>
                                <pre><code class="language-lua">{r#"plugin:AddSection("plugins", "selection-counter", "Selection Counter")

plugin:AddButton(
    "plugins",                  -- tab id (always "plugins")
    "selection-counter",        -- section id
    "count-selection",          -- button id
    "Count Selected",           -- label
    nil,                        -- icon
    "Show how many entities are selected",
    "selection_counter:count",  -- action id, unique across plugins
    "normal",                   -- "small", "normal" or "large"
    function()
        local selected = plugin:GetSelection()
        plugin:Notify("info", #selected .. " selected")
    end
)"#}</code></pre>
                            </div>
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

                        <div id="roadmap-luau" class="subsection">
                            <h3>"Luau in Play"</h3>
                            <p>
                                "The Luau Play driver is written and tested: a coroutine scheduler inside the VM
                                that makes "<code>"task.wait"</code>" and "<code>"Signal:Wait"</code>" yield,
                                RunService "<code>"Heartbeat"</code>", "<code>"Stepped"</code>" and "
                                <code>"RenderStepped"</code>" every frame, live "<code>"UserInputService"</code>
                                " state and input events, and "<code>"Touched"</code>" events from Avian collisions
                                on scene parts reached as "<code>"workspace.Name"</code>". Studio will switch it
                                on so Luau scripts start on Play and stop cleanly on Stop, and "
                                <code>"workspace:Raycast"</code>" will be connected to the engine's raycaster."
                            </p>
                        </div>

                        <div id="roadmap-rune" class="subsection">
                            <h3>"A Wider Rune API"</h3>
                            <p>
                                "Rune will gain handles to objects already in the scene, so a script can find an
                                existing part and move or recolor it during Play, and ScreenGui button clicks
                                will call an "<code>"on_button_click(name)"</code>" function (see "
                                <a href="/docs/ui">"UI Systems"</a>"). Keyboard and mouse queries, raycast
                                filters, tweens and data stores are declared in the module and will be connected
                                to the engine's input, physics, animation and storage."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Write it in a file. Press Play. Watch it run."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/simulation" class="btn-secondary-steel">"Simulation Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/audio" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Audio"</span>
                            </div>
                        </a>
                        <a href="/docs/services" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Services"</span>
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
