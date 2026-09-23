// =============================================================================
// Eustress Web - MCP Server Documentation Page
// =============================================================================
// MCP Server: the eustress-mcp binary that connects AI clients to a Universe,
// its setup, its tool and resource surface, and the disk and live paths.
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
                TocSubsection { id: "overview-what", title: "What It Is" },
                TocSubsection { id: "overview-paths", title: "Disk and Live" },
            ],
        },
        TocSection {
            id: "setup",
            title: "Setup",
            subsections: vec![
                TocSubsection { id: "setup-install", title: "Get the Binary" },
                TocSubsection { id: "setup-register", title: "Register It" },
                TocSubsection { id: "setup-resolve", title: "Universe and Space" },
                TocSubsection { id: "setup-verify", title: "Check the Connection" },
            ],
        },
        TocSection {
            id: "access",
            title: "Access & Safety",
            subsections: vec![
                TocSubsection { id: "access-classes", title: "What a Client May Call" },
                TocSubsection { id: "access-sandbox", title: "The Universe Sandbox" },
                TocSubsection { id: "access-review", title: "Review and Audit" },
            ],
        },
        TocSection {
            id: "world",
            title: "World Tools",
            subsections: vec![
                TocSubsection { id: "world-universes", title: "Universes and Spaces" },
                TocSubsection { id: "world-files", title: "Files and Scripts" },
                TocSubsection { id: "world-entities", title: "Entities" },
                TocSubsection { id: "world-history", title: "Git, Memory and Logs" },
            ],
        },
        TocSection {
            id: "live",
            title: "Live Tools",
            subsections: vec![
                TocSubsection { id: "live-bridge", title: "The Engine Bridge" },
                TocSubsection { id: "live-scene", title: "Scene and Editor" },
                TocSubsection { id: "live-camera", title: "AI Camera and Capture" },
                TocSubsection { id: "live-sim", title: "Simulation and Physics" },
            ],
        },
        TocSection {
            id: "specialist",
            title: "Specialist Tools",
            subsections: vec![
                TocSubsection { id: "specialist-cad", title: "CAD" },
                TocSubsection { id: "specialist-website", title: "Website and Moderation" },
                TocSubsection { id: "specialist-ai", title: "Generation and Network" },
            ],
        },
        TocSection {
            id: "resources",
            title: "Resources",
            subsections: vec![
                TocSubsection { id: "resources-uris", title: "Resource URIs" },
                TocSubsection { id: "resources-subscribe", title: "Subscriptions" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-live", title: "Editor Control over MCP" },
                TocSubsection { id: "roadmap-ship", title: "Packaging and Pass-Through Tools" },
            ],
        },
    ]
}

/// The two paths a tool call takes: straight to the Universe files, or over
/// the TCP bridge into a running engine that loads and watches those files.
#[component]
fn McpFlowDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 250" role="img"
                aria-label="An AI client talks to eustress-mcp over stdio. The server reads and writes the Universe files directly, and reaches a running engine over a TCP bridge on 127.0.0.1. The engine loads and watches the same files.">
                <defs>
                    <marker id="mcp-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>
                // AI client
                <rect x="16" y="97" width="136" height="56" rx="8" class="dg-box"></rect>
                <text x="84" y="122" class="dg-label" text-anchor="middle">"AI client"</text>
                <text x="84" y="140" class="dg-note" text-anchor="middle">"Claude Code, Cursor"</text>
                <line x1="152" y1="125" x2="226" y2="125" class="dg-line"
                    marker-start="url(#mcp-arrow)" marker-end="url(#mcp-arrow)"></line>
                <text x="189" y="115" class="dg-note" text-anchor="middle">"stdio"</text>

                // The server
                <rect x="226" y="97" width="150" height="56" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="301" y="122" class="dg-label" text-anchor="middle">"eustress-mcp"</text>
                <text x="301" y="140" class="dg-note" text-anchor="middle">"tools and resources"</text>

                // Disk path
                <rect x="470" y="16" width="154" height="56" rx="8" class="dg-box"></rect>
                <text x="547" y="41" class="dg-label" text-anchor="middle">"Universe files"</text>
                <text x="547" y="59" class="dg-note" text-anchor="middle">"Spaces, scripts, TOML"</text>
                <line x1="376" y1="112" x2="470" y2="52" class="dg-line" marker-end="url(#mcp-arrow)"></line>
                <text x="404" y="72" class="dg-note" text-anchor="middle">"disk tools"</text>

                // Live path
                <rect x="470" y="178" width="154" height="56" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="547" y="203" class="dg-label" text-anchor="middle">"Running engine"</text>
                <text x="547" y="221" class="dg-note" text-anchor="middle">"Studio or headless"</text>
                <line x1="376" y1="138" x2="470" y2="198" class="dg-line dg-line-accent" marker-end="url(#mcp-arrow)"></line>
                <text x="404" y="192" class="dg-note" text-anchor="middle">"TCP bridge"</text>

                // The engine loads and watches the same files
                <line x1="547" y1="72" x2="547" y2="178" class="dg-line dg-line-dashed"
                    marker-start="url(#mcp-arrow)" marker-end="url(#mcp-arrow)"></line>
                <text x="556" y="129" class="dg-note">"load, watch, save"</text>
            </svg>
            <figcaption>
                "Disk tools work on the Universe folder with or without an engine running. Live
                tools go through the engine's bridge and act on the world it has loaded."
            </figcaption>
        </figure>
    }
}

/// MCP Server documentation page.
#[component]
pub fn LearnMcpPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-mcp"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/sparkles.svg" alt="MCP Server" class="toc-icon" />
                        <h2>"MCP Server"</h2>
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
                            <span class="current">"MCP Server"</span>
                        </div>
                        <h1 class="docs-title">"MCP Server"</h1>
                        <p class="docs-subtitle">
                            "The Eustress MCP server, eustress-mcp, connects an AI client such as Claude
                            Code, Claude Desktop or Cursor to a Universe through the Model Context
                            Protocol. It reads and writes the Universe's files directly, and reaches a
                            running Eustress Engine through its local bridge for anything live."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "20 min read"
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
                            <h3>"What It Is"</h3>
                            <p>
                                "An MCP server is a program an AI client starts and talks to over a standard
                                protocol, so the model can call its tools and read its resources. "
                                <code>"eustress-mcp"</code>" is Eustress's server: one native binary that the
                                client launches as a child process and speaks to over standard input and
                                output, one JSON-RPC 2.0 message per line. It answers MCP protocol versions "
                                <code>"2025-06-18"</code>", "<code>"2025-03-26"</code>" and "
                                <code>"2024-11-05"</code>", echoing whichever the client asks for."
                            </p>
                            <p>
                                "It exposes about 120 tools and six kinds of resources. Most tools are the
                                same handlers the in-engine Workshop assistant uses, from the shared "
                                <code>"eustress-tools"</code>" crate; the others, most of them live tools,
                                exist only in the server. Each request runs on its own task, so a long call such as "
                                <code>"await_simulation"</code>" never blocks the others, and a client can
                                cancel it. In Studio, "<strong>"Help"</strong>" > "
                                <strong>"Setup MCP"</strong>" opens this page."
                            </p>
                        </div>

                        <div id="overview-paths" class="subsection">
                            <h3>"Disk and Live"</h3>
                            <p>
                                "Every tool reaches a Space in one of two ways. Disk tools read and write the
                                Universe folder, and a running engine picks the change up through its file
                                watcher, so they work with Studio closed. Live tools connect to the engine's
                                bridge, a JSON-RPC endpoint on "<code>"127.0.0.1"</code>", and act on the
                                world the engine has loaded right now."
                            </p>
                            <McpFlowDiagram />
                            <table class="docs-table">
                                <thead>
                                    <tr><th>""</th><th>"Disk tools"</th><th>"Live tools"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Needs an engine running"</td><td>"No"</td><td>"Yes: Studio or eustress-headless"</td></tr>
                                    <tr><td>"Sees"</td><td>"Files on disk"</td><td>"The loaded world, including parts that live only in the world database"</td></tr>
                                    <tr><td>"A change shows"</td><td>"When the engine's watcher reloads the file"</td><td>"On the engine's next frame"</td></tr>
                                    <tr><td>"Examples"</td><td><code>"read_file"</code>", "<code>"create_script"</code>", "<code>"git_status"</code></td><td><code>"inspect_scene"</code>", "<code>"scene_raycast"</code>", "<code>"sim_step"</code></td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The entity tools do both: they try the bridge first and fall back to the files
                                when no engine answers. The simulation tools are a third case, covered in "
                                <a href="#live-sim">"Simulation and Physics"</a>"."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // SETUP
                    // =========================================================
                    <section id="setup" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Setup"
                        </h2>

                        <div id="setup-install" class="subsection">
                            <h3>"Get the Binary"</h3>
                            <p>"Build the server from a source checkout, in the repository's "<code>"eustress"</code>" folder:"</p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"cargo build --release -p eustress-mcp-server

# Windows:        target/release/eustress-mcp.exe
# macOS, Linux:   target/release/eustress-mcp"#}</code></pre>
                            </div>
                            <p>
                                "The Windows installer copies "<code>"eustress-mcp.exe"</code>" into the
                                install folder only when the build that precedes it has produced the file. The
                                release pipeline builds only the "<code>"eustress-engine"</code>" package, so
                                installers it produces do not include the server: build it as above and point
                                your client at the file."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Build eustress-mcp-server, not eustress-mcp"</strong>
                                    <p>
                                        "The binary is named eustress-mcp, but its package is
                                        eustress-mcp-server. The workspace also holds a package called
                                        eustress-mcp: an older library that builds no program, so "
                                        <code>"cargo build -p eustress-mcp"</code>" finishes without producing
                                        a server."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="setup-register" class="subsection">
                            <h3>"Register It with Your AI Client"</h3>
                            <p>
                                "The server needs no arguments. Name the Universe it should work in with an
                                environment variable and register it under any name. For Claude Code, add it
                                to "<code>".mcp.json"</code>" in your project:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">".mcp.json"</span>
                                </div>
                                <pre><code class="language-json">{r#"{
  "mcpServers": {
    "eustress": {
      "type": "stdio",
      "command": "C:\\Eustress\\eustress\\target\\release\\eustress-mcp.exe",
      "args": [],
      "env": {
        "EUSTRESS_UNIVERSE": "C:\\Users\\you\\Documents\\Eustress\\Universe1"
      }
    }
  }
}"#}</code></pre>
                            </div>
                            <p>
                                "Other MCP clients take an equivalent entry, with the same command, arguments
                                and environment, in their own configuration file. Claude Code shows the tools
                                with its server prefix, for example "<code>"mcp__eustress__inspect_scene"</code>
                                ". Everything the server reads at startup:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Setting"</th><th>"Effect"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"--universe <path>"</code></td><td>"The Universe to open at startup. Checked before the variable below."</td></tr>
                                    <tr><td><code>"EUSTRESS_UNIVERSE"</code></td><td>"The same, as an environment variable: an absolute path to a folder that holds "<code>"Spaces"</code>"."</td></tr>
                                    <tr><td><code>"EUSTRESS_UNIVERSES_PATH"</code></td><td>"Folders to search for Universes and for a running engine, separated by "<code>";"</code>" on Windows and "<code>":"</code>" elsewhere. Default: "<code>"~/Eustress"</code>", "<code>"~/Documents/Eustress"</code>" and your home folder."</td></tr>
                                    <tr><td><code>"RUST_LOG"</code></td><td>"Level of the server's own log, which goes to stderr. Default: "<code>"info"</code>"."</td></tr>
                                    <tr><td><code>"EUSTRESS_MODERATOR_TOKEN"</code></td><td>"For Gallery moderators only: grants the Network capability (see "<a href="#access-classes">"Access"</a>")."</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="setup-resolve" class="subsection">
                            <h3>"Which Universe and Space"</h3>
                            <p>"At startup the server picks its Universe from the first of these that gives one:"</p>
                            <ol class="numbered-list">
                                <li>"The "<code>"--universe"</code>" argument."</li>
                                <li>"The "<code>"EUSTRESS_UNIVERSE"</code>" variable."</li>
                                <li>"The nearest folder above the working directory that contains a "<code>"Spaces"</code>" folder."</li>
                            </ol>
                            <p>
                                "If none does, the first resource request adopts the first Universe found in
                                the search folders. Resources always come from this Universe, and the "
                                <code>"set_active_universe"</code>" tool switches it mid-session."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Tools follow Studio's last Space"</strong>
                                    <p>
                                        "When Studio has run on this computer, every tool call works in the
                                        Space Studio last had open, read from "<code>"last_space_path"</code>
                                        " in "<code>"~/.eustress_engine/settings.json"</code>", and in that
                                        Space's Universe, even when "<code>"EUSTRESS_UNIVERSE"</code>" names
                                        another. Only without that setting does a tool fall back to the
                                        configured Universe and its first Space. Open the Space you want in
                                        Studio before you point an agent at it."
                                    </p>
                                </div>
                            </div>
                            <p>
                                "A live tool connects to the port in "<code>".eustress/engine.port"</code>" of
                                the Universe it works in, and falls back to the copy of that file in the folder
                                above. Without Studio's setting, the server first searches the search folders
                                for a Universe whose port file names a port that accepts a connection within
                                250 ms, and remembers the answer for 3 seconds, so closing one engine and
                                opening another needs no restart."
                            </p>
                        </div>

                        <div id="setup-verify" class="subsection">
                            <h3>"Check the Connection"</h3>
                            <p>
                                "On start the server logs its version, the Universe it resolved, its search
                                folders and its tool count to stderr, which your client may show in its MCP
                                log. Then ask the assistant to call "<code>"list_spaces"</code>": it names
                                the Spaces of the Universe it is working in. With no Universe resolved, "
                                <code>"resources/list"</code>" offers a single resource, "
                                <code>"eustress://help/setup"</code>", that explains how to set one."
                            </p>
                            <p>
                                "To test the live path, open a Space in Studio and call "
                                <code>"inspect_scene"</code>". It returns the loaded entities and the current
                                frame rate. With no engine running, a live tool answers that the engine is not
                                running and suggests opening Studio or starting eustress-headless."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // ACCESS & SAFETY
                    // =========================================================
                    <section id="access" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Access & Safety"
                        </h2>

                        <div id="access-classes" class="subsection">
                            <h3>"What a Client May Call"</h3>
                            <p>
                                "Every tool belongs to one capability class, set in a single table in the "
                                <code>"eustress-tools"</code>" crate, and the dispatcher checks the class
                                before the tool runs. An MCP client gets the standard grant, Read and Write.
                                Tools in the other classes still appear in "<code>"tools/list"</code>", and a
                                call returns a permission error that names the missing capability."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Class"</th><th>"Over MCP"</th><th>"Tools"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Read"</td><td>"Allowed"</td><td>"Queries, such as "<code>"read_file"</code>", "<code>"inspect_scene"</code>", "<code>"git_log"</code></td></tr>
                                    <tr><td>"Write"</td><td>"Allowed"</td><td>"Changes inside the Universe, such as "<code>"create_entity"</code>", "<code>"write_file"</code>", "<code>"run_simulation"</code></td></tr>
                                    <tr><td>"Destructive"</td><td>"Refused"</td><td><code>"delete_entity"</code>", "<code>"git_commit"</code>", "<code>"git_branch"</code>", "<code>"website_remove_reference"</code></td></tr>
                                    <tr><td>"Execute"</td><td>"Refused"</td><td><code>"run_bash"</code>", "<code>"execute_luau"</code>", "<code>"execute_rune"</code></td></tr>
                                    <tr><td>"Network"</td><td>"Only with "<code>"EUSTRESS_MODERATOR_TOKEN"</code></td><td><code>"http_request"</code>", "<code>"image_to_code"</code>", "<code>"image_to_geometry"</code>", "<code>"document_to_code"</code>", the four "<code>"moderation_"</code>" tools"</td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Twelve live tools are refused today"</strong>
                                    <p>
                                        "A tool missing from the capability table is refused even with full
                                        permissions. Twelve of the server's own live tools are missing from it: "
                                        <code>"equip_tool"</code>", "<code>"select_entity"</code>", "
                                        <code>"invoke_action"</code>", "<code>"capture_viewport"</code>", the four "
                                        <code>"ai_camera_"</code>" tools, "<code>"export_instances_toml"</code>", "
                                        <code>"data_bind"</code>", "<code>"data_bindings"</code>" and "
                                        <code>"data_unbind"</code>". They are listed, and every call returns a
                                        permission error. The bridge methods behind them work from the "
                                        <a href="/learn/cli">"eustress CLI"</a>"."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="access-sandbox" class="subsection">
                            <h3>"The Universe Sandbox"</h3>
                            <ul class="docs-list">
                                <li><strong>"File tools"</strong>": "<code>"read_file"</code>", "<code>"list_directory"</code>" and "<code>"write_file"</code>" take paths relative to the Universe root. A path containing "<code>".."</code>" is refused, and so is any path that resolves outside the Universe."</li>
                                <li><strong>"Entity files"</strong>": "<code>"write_file"</code>" will not overwrite an "<code>"_instance.toml"</code>"; entities change through "<code>"create_entity"</code>" and "<code>"update_entity"</code>"."</li>
                                <li><strong>"Size caps"</strong>": "<code>"read_file"</code>" returns the first 50,000 bytes of a file, resource reads stop at 256 KB, and a tool's structured result is cut at 120 KB with an explicit truncation note."</li>
                                <li><strong>"The protocol stream"</strong>" carries only protocol messages. The server's own log goes to stderr."</li>
                            </ul>
                        </div>

                        <div id="access-review" class="subsection">
                            <h3>"Review and Audit"</h3>
                            <p>
                                "Each tool in "<code>"tools/list"</code>" carries MCP annotations taken from its
                                own code: "<code>"readOnlyHint"</code>" on tools that only observe, "
                                <code>"destructiveHint"</code>" on tools that ask for approval in the Workshop,
                                and "<code>"openWorldHint"</code>" on "<code>"http_request"</code>". Clients
                                use them to decide which calls to confirm with you."
                            </p>
                            <p>
                                <code>"stage_file_change"</code>" writes nothing. It returns the proposed
                                create, modify or delete, with the file's current text for a modify, so the
                                client can show it to you before anything is written."
                            </p>
                            <p>
                                "Two logs show what happened in a Space. "<code>"query_audit_log"</code>" reads
                                the engine's record of its own Claude calls, one "<code>".log.toml"</code>
                                " file per call under "<code>"SoulService/Logs"</code>". "
                                <code>"oplog_tail"</code>" reads the engine's op-log of entity creates and
                                deletes, in order."
                            </p>
                            <div class="callout callout-warning">
                                <img src="/assets/icons/shield.svg" alt="Warning" />
                                <div>
                                    <strong>"The bridge trusts local programs"</strong>
                                    <p>
                                        "The engine bridge listens only on "<code>"127.0.0.1"</code>" but checks
                                        no identity: any program on your computer that reads "
                                        <code>"engine.port"</code>" can connect and call any bridge method,
                                        including "<code>"entity.delete"</code>" and "<code>"engine.shutdown"</code>
                                        ". Only its "<code>"tools.call"</code>" method is limited, to the same
                                        Read and Write set as the MCP server."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // WORLD TOOLS
                    // =========================================================
                    <section id="world" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "World Tools"
                        </h2>

                        <div id="world-universes" class="subsection">
                            <h3>"Universes and Spaces"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"set_active_universe"</code></td><td>"Switch the Universe this session resolves against. Handled by the server itself."</td></tr>
                                    <tr><td><code>"list_universes"</code></td><td>"List the Universe folders beside the current one."</td></tr>
                                    <tr><td><code>"list_spaces"</code></td><td>"List the Spaces of a Universe, the current one by default."</td></tr>
                                    <tr><td><code>"new_universe"</code></td><td>"Create a Universe folder beside the current one, with "<code>"Spaces"</code>" and "<code>".eustress"</code>" folders. Fails if it exists."</td></tr>
                                    <tr><td><code>"new_space"</code></td><td>"Create a Space from the standard service templates. The engine builds its database on first open."</td></tr>
                                    <tr><td><code>"rename_space"</code></td><td>"Rename a Space folder. Fails cleanly while a running engine holds it."</td></tr>
                                    <tr><td><code>"rename_universe"</code></td><td>"Rename a Universe folder and update the next-launch marker if it pointed there."</td></tr>
                                    <tr><td><code>"set_next_launch_universe"</code></td><td>"Write the marker the engine reads at startup to choose its Universe."</td></tr>
                                    <tr><td><code>"list_space_contents"</code></td><td>"A Space's services and top-level entities, or the children of one folder or Model."</td></tr>
                                    <tr><td><code>"generate_docs"</code></td><td>"Write a README.md describing the Space: services, entities, scripts and materials."</td></tr>
                                    <tr><td><code>"get_conversation"</code></td><td>"Read a saved Workshop conversation by session id."</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="world-files" class="subsection">
                            <h3>"Files and Scripts"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"read_file"</code></td><td>"Read a text file by its path from the Universe root."</td></tr>
                                    <tr><td><code>"list_directory"</code></td><td>"Every file and subfolder of a folder as it is on disk, with sizes."</td></tr>
                                    <tr><td><code>"write_file"</code></td><td>"Write a file under the Universe, creating folders as needed."</td></tr>
                                    <tr><td><code>"stage_file_change"</code></td><td>"Return a proposed create, modify or delete for review. Writes nothing."</td></tr>
                                    <tr><td><code>"search_universe"</code></td><td>"Search every Space's "<code>".toml"</code>", "<code>".rune"</code>", "<code>".lua"</code>" and "<code>".md"</code>" files; returns paths and line numbers."</td></tr>
                                    <tr><td><code>"list_assets"</code></td><td>"Meshes, textures and materials in the Space's MaterialService and Workspace."</td></tr>
                                    <tr><td><code>"list_scripts"</code></td><td>"The Soul scripts ("<code>".rune"</code>", "<code>".lua"</code>", "<code>".luau"</code>", "<code>".soul"</code>") in the Space's SoulService."</td></tr>
                                    <tr><td><code>"read_script"</code></td><td>"A Soul script's source, by name."</td></tr>
                                    <tr><td><code>"create_script"</code></td><td>"Create a script folder under SoulService, Rune by default or Luau; the engine's watcher loads it."</td></tr>
                                    <tr><td><code>"execute_rune"</code></td><td>"Write a Rune script into SoulService for the engine to run. Refused over MCP (Execute)."</td></tr>
                                    <tr><td><code>"execute_luau"</code></td><td>"Write a Luau script into SoulService for the engine to run. Refused over MCP (Execute)."</td></tr>
                                    <tr><td><code>"run_bash"</code></td><td>"Run a shell command in the Universe root. Refused over MCP (Execute)."</td></tr>
                                </tbody>
                            </table>
                            <p>"Scripting itself is covered in "<a href="/docs/scripting">"Scripting"</a>"."</p>
                        </div>

                        <div id="world-entities" class="subsection">
                            <h3>"Entities"</h3>
                            <p>
                                "Tools marked live first call the engine's bridge, where new parts go into the
                                world database, and fall back to writing "<code>"_instance.toml"</code>" folders
                                when no engine answers. An engine that is up but rejects the call reports the
                                error instead of a silent disk write."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"create_entity"</code></td><td>"Create a part or other instance with its size, color and material. Live first."</td></tr>
                                    <tr><td><code>"update_entity"</code></td><td>"Change an entity's properties. Live first."</td></tr>
                                    <tr><td><code>"delete_entity"</code></td><td>"Remove an entity. Refused over MCP (Destructive)."</td></tr>
                                    <tr><td><code>"query_entities"</code></td><td>"List entities, optionally of one class. Live first."</td></tr>
                                    <tr><td><code>"find_entity"</code></td><td>"Find entities whose name contains a text. Live first."</td></tr>
                                    <tr><td><code>"add_tag"</code></td><td>"Add a CollectionService tag to an entity. Live first."</td></tr>
                                    <tr><td><code>"remove_tag"</code></td><td>"Remove a CollectionService tag. Live first."</td></tr>
                                    <tr><td><code>"get_tagged_entities"</code></td><td>"Entities that carry a tag, read from the Workspace files."</td></tr>
                                    <tr><td><code>"insert_gaussian_splats"</code></td><td>"Import a "<code>".ply"</code>" Gaussian-splat cloud as a GaussianSplats instance."</td></tr>
                                    <tr><td><code>"particle_simulation"</code></td><td>"Describe, create, read or change a ParticleSimulation and its species."</td></tr>
                                    <tr><td><code>"promote_entity"</code></td><td>"Write a database-only part out as an "<code>"_instance.toml"</code>" folder, keeping its uuid. Needs a running engine."</td></tr>
                                    <tr><td><code>"demote_entity"</code></td><td>"Fold a bare part folder back into the database and delete the folder. Needs a running engine."</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="world-history" class="subsection">
                            <h3>"Git, Memory and Logs"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"git_status"</code></td><td>"Modified, staged and untracked files in the Universe's git repository."</td></tr>
                                    <tr><td><code>"git_log"</code></td><td>"Recent commits with hash, author, date and message."</td></tr>
                                    <tr><td><code>"git_diff"</code></td><td>"Uncommitted changes as a unified diff, optionally for one path."</td></tr>
                                    <tr><td><code>"feedback_diff"</code></td><td>"A structured diff between two git refs or two file paths."</td></tr>
                                    <tr><td><code>"git_commit"</code></td><td>"Stage everything and commit. Refused over MCP (Destructive)."</td></tr>
                                    <tr><td><code>"git_branch"</code></td><td>"List, create, switch, delete or merge branches. Refused over MCP (Destructive)."</td></tr>
                                    <tr><td><code>"list_rules"</code></td><td>"Workshop rules: "<code>".eustress/rules/*.md"</code>" for the Universe and "<code>".rules/*.md"</code>" in the Space."</td></tr>
                                    <tr><td><code>"list_workflows"</code></td><td>"Workshop workflows, the "<code>".md"</code>" files behind "<code>"/run"</code>" commands."</td></tr>
                                    <tr><td><code>"query_audit_log"</code></td><td>"The engine's Claude call log, newest first, up to 50 entries."</td></tr>
                                    <tr><td><code>"remember"</code></td><td>"Returns the memory as a request. Nothing is stored out of process."</td></tr>
                                    <tr><td><code>"recall"</code></td><td>"Returns the query as a request. The server holds no memories to search."</td></tr>
                                    <tr><td><code>"query_stream_events"</code></td><td>"Returns the query as a request. The event stream lives inside the engine."</td></tr>
                                </tbody>
                            </table>
                            <p>"How a Universe's history is kept is covered in "<a href="/docs/universes">"Universes"</a>"."</p>
                        </div>
                    </section>

                    // =========================================================
                    // LIVE TOOLS
                    // =========================================================
                    <section id="live" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Live Tools"
                        </h2>

                        <div id="live-bridge" class="subsection">
                            <h3>"The Engine Bridge"</h3>
                            <p>
                                "Every running engine, the Studio window and eustress-headless alike, hosts the
                                Engine Bridge: a JSON-RPC 2.0 endpoint on "<code>"127.0.0.1"</code>" at a port
                                the operating system picks. The engine writes that port to "
                                <code>".eustress/engine.port"</code>" in the Universe it has open, and a copy to
                                the same path in the folder above, the workspace root. Requests are handled on
                                the engine's main thread, up to 64 per frame, so a handler sees exactly the
                                world on screen."
                            </p>
                            <p>
                                "The server connects with a 2 second limit and gives most calls 2 seconds to
                                answer; "<code>"sim_step"</code>" waits longer in proportion to the ticks it
                                asked for. When several engines share one Universe, "<code>"engine.port"</code>
                                " names only one of them; "<a href="/learn/cli#multi-registry">"CLI & Headless"</a>
                                " covers the instance registry that lists them all."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Needs"</th><th>"Tools"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"A running engine"</td><td>"Everything in "<a href="#live-scene">"Scene and Editor"</a>" and "<a href="#live-camera">"AI Camera and Capture"</a>", plus "<code>"promote_entity"</code>" and "<code>"demote_entity"</code></td></tr>
                                    <tr><td>"An engine if one runs, else the files"</td><td><code>"create_entity"</code>", "<code>"update_entity"</code>", "<code>"query_entities"</code>", "<code>"find_entity"</code>", "<code>"add_tag"</code>", "<code>"remove_tag"</code></td></tr>
                                    <tr><td>"An engine with the Universe open, reached through files"</td><td>"The simulation tools in "<a href="#live-sim">"Simulation and Physics"</a></td></tr>
                                    <tr><td>"Nothing but the files"</td><td>"Every other tool"</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="live-scene" class="subsection">
                            <h3>"Scene and Editor"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"inspect_scene"</code></td><td>"Per-entity class, mesh, material, color, transform, visibility, physics flags, parent and source file, plus the frame rate. Filter by class, name, cell or region; 200 per page by default, 5,000 at most."</td></tr>
                                    <tr><td><code>"scene_overview"</code></td><td>"Entities grouped into 256 m Morton cells, densest first, each with bounds, a count and a class histogram."</td></tr>
                                    <tr><td><code>"partition_scene"</code></td><td>"Split the scene into balanced, spatially contiguous work units for parallel agents, 4 by default and 64 at most."</td></tr>
                                    <tr><td><code>"scene_raycast"</code></td><td>"Cast a ray against the live Avian colliders; hits nearest first, up to 1,000 m and 8 hits by default."</td></tr>
                                    <tr><td><code>"oplog_tail"</code></td><td>"Recent entity creates and deletes from the engine's op-log, 50 by default and 1,000 at most."</td></tr>
                                    <tr><td><code>"sim_step"</code></td><td>"Advance physics by exact 1/60 s ticks, up to 10,000 per call and one call at a time. Pause the simulation first."</td></tr>
                                    <tr><td><code>"get_editor_state"</code></td><td>"The active editor tool and the current selection."</td></tr>
                                    <tr><td><code>"sim_bindings"</code></td><td>"Forge placement records. Needs an engine built with the sim-orchestration feature."</td></tr>
                                    <tr><td><code>"equip_tool"</code></td><td>"Set the active tool: select, move, scale or rotate. Refused today."</td></tr>
                                    <tr><td><code>"select_entity"</code></td><td>"Replace the selection with entities by id. Refused today."</td></tr>
                                    <tr><td><code>"invoke_action"</code></td><td>"Run an editor action by name, as its shortcut would. Refused today."</td></tr>
                                    <tr><td><code>"export_instances_toml"</code></td><td>"Dump the world database to readable TOML under the Space's "<code>".eustress/exports"</code>". Refused today."</td></tr>
                                    <tr><td><code>"data_bind"</code></td><td>"Drive a simulation parameter from a Dataset column. Refused today."</td></tr>
                                    <tr><td><code>"data_bindings"</code></td><td>"List the active Dataset bindings. Refused today."</td></tr>
                                    <tr><td><code>"data_unbind"</code></td><td>"Remove a Dataset binding. Refused today."</td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-tip">
                                <img src="/assets/icons/sparkles.svg" alt="Tip" />
                                <div>
                                    <strong>"Large scenes: overview, partition, then page"</strong>
                                    <p>
                                        "Call "<code>"scene_overview"</code>" to see where things are, "
                                        <code>"partition_scene"</code>" to cut the world into one unit per
                                        agent, then page each unit's cells with "<code>"inspect_scene"</code>
                                        " and its "<code>"cell"</code>" argument, instead of paging one flat
                                        list of every entity."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="live-camera" class="subsection">
                            <h3>"AI Camera and Capture"</h3>
                            <p>
                                "The AI camera is a second, off-screen camera inside the engine, separate from
                                your view, that renders only when asked. All five tools here are refused over
                                MCP today, because they are missing from the capability table."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"capture_viewport"</code></td><td>"Screenshot the Studio window and return the PNG inline, up to 2,800,000 bytes."</td></tr>
                                    <tr><td><code>"ai_camera_set_pose"</code></td><td>"Place the AI camera by position plus a look-at point or a rotation."</td></tr>
                                    <tr><td><code>"ai_camera_orbit"</code></td><td>"Orbit the AI camera around a point: distance 15, yaw 45 degrees and pitch 30 degrees by default."</td></tr>
                                    <tr><td><code>"ai_camera_frame"</code></td><td>"Aim the AI camera at a named entity from a distance suited to its size."</td></tr>
                                    <tr><td><code>"ai_camera_capture"</code></td><td>"Render the AI camera and return the image inline."</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="live-sim" class="subsection">
                            <h3>"Simulation and Physics"</h3>
                            <p>
                                "The simulation tools reach an engine through files rather than the bridge,
                                because they also run inside the engine, where a call to its own bridge would
                                wait on itself. They queue a command in a file the engine reads every frame and
                                read the runtime snapshot it rewrites 4 times a second, so results need an
                                engine with the Universe open. With several engines on one Universe, pass "
                                <code>"pid"</code>" to choose one. The waiting tools stop early when the client
                                cancels. The clock and watchpoints themselves are covered in "
                                <a href="/docs/simulation">"Simulation"</a>"."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"run_simulation"</code></td><td>"Enter Play, like the Play button, with optional "<code>"time_scale"</code>" and "<code>"duration_s"</code>"; returns a ticket."</td></tr>
                                    <tr><td><code>"pause_simulation"</code></td><td>"Pause the run, keeping its state."</td></tr>
                                    <tr><td><code>"stop_simulation"</code></td><td>"Stop the run and return to Edit, like the Stop button."</td></tr>
                                    <tr><td><code>"get_simulation_state"</code></td><td>"Play state, watchpoint values, snapshot age, the current run and the last finished one."</td></tr>
                                    <tr><td><code>"await_simulation"</code></td><td>"Wait for a run to end, 300 s at most by default, and return its final values."</td></tr>
                                    <tr><td><code>"get_sim_value"</code></td><td>"Read one watchpoint value."</td></tr>
                                    <tr><td><code>"set_sim_value"</code></td><td>"Write a value into the simulation, such as an initial condition."</td></tr>
                                    <tr><td><code>"list_sim_values"</code></td><td>"All watchpoints, compactly, optionally under one prefix."</td></tr>
                                    <tr><td><code>"tail_telemetry"</code></td><td>"Recent watchpoint samples from the Universe's telemetry log."</td></tr>
                                    <tr><td><code>"run_experiment"</code></td><td>"Optionally create a git branch, then apply overrides, run for a duration, wait, and save the result under "<code>".eustress/experiments"</code>"."</td></tr>
                                    <tr><td><code>"compare_runs"</code></td><td>"Metric deltas between two saved experiments; "<code>"latest"</code>" and "<code>"latest-1"</code>" work as names."</td></tr>
                                    <tr><td><code>"list_experiments"</code></td><td>"Saved experiment results, newest first."</td></tr>
                                    <tr><td><code>"datastore_get"</code></td><td>"Read a key from a named DataStore file under the Universe's "<code>".eustress/datastore"</code>"."</td></tr>
                                    <tr><td><code>"datastore_set"</code></td><td>"Write a key to a named DataStore file."</td></tr>
                                    <tr><td><code>"query_material"</code></td><td>"Rendering and mechanical properties of a material preset."</td></tr>
                                    <tr><td><code>"calculate_physics"</code></td><td>"Evaluate a Realism equation, such as ideal gas pressure or drag force."</td></tr>
                                    <tr><td><code>"measure_distance"</code></td><td>"Straight-line distance between two world points, in meters."</td></tr>
                                    <tr><td><code>"raycast"</code></td><td>"Always returns an error pointing to live physics; use "<code>"scene_raycast"</code>"."</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // SPECIALIST TOOLS
                    // =========================================================
                    <section id="specialist" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Specialist Tools"
                        </h2>

                        <div id="specialist-cad" class="subsection">
                            <h3>"CAD"</h3>
                            <p>
                                "The CAD tools author and inspect parametric CadPart feature trees on disk,
                                with no engine needed. Each authoring call reports the part's state back, so an
                                agent sees a defect as soon as it makes one. The parts themselves are covered
                                in "<a href="/docs/cad">"CAD"</a>"."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"cad_list_templates"</code></td><td>"Built-in part templates with their variables, and the feature operations the kernel supports."</td></tr>
                                    <tr><td><code>"cad_create_part"</code></td><td>"Create a CadPart from a template (plate, box, cylinder) or as a placement of a published part."</td></tr>
                                    <tr><td><code>"cad_set_variable"</code></td><td>"Set a feature-tree variable, such as a height of 0.02 m."</td></tr>
                                    <tr><td><code>"cad_describe_part"</code></td><td>"The feature tree: variables in meters, per-feature status, sketch solve state and mesh statistics."</td></tr>
                                    <tr><td><code>"cad_validate_part"</code></td><td>"Pass or fail checks for empty bodies, open surfaces, non-manifold edges, degenerate triangles and bad volume."</td></tr>
                                    <tr><td><code>"cad_measure"</code></td><td>"Volume, surface area, center of mass and bounds; mass from a density; exact minimum distance to a second part."</td></tr>
                                    <tr><td><code>"cad_add_feature"</code></td><td>"Add a feature: extrude, revolve, hole, mirror, pattern, boolean, split, sweep, fillet, chamfer or shell."</td></tr>
                                    <tr><td><code>"cad_edit_feature"</code></td><td>"Suppress, unsuppress, rename or patch the feature at a tree index."</td></tr>
                                    <tr><td><code>"cad_delete_feature"</code></td><td>"Remove a feature; refused while later features reference it, unless forced."</td></tr>
                                    <tr><td><code>"cad_create_sketch"</code></td><td>"Add a named 2D sketch on the xy, xz or yz plane or on a face."</td></tr>
                                    <tr><td><code>"cad_add_sketch_entity"</code></td><td>"Add a point, line, circle, arc, rectangle or construction geometry, in meters."</td></tr>
                                    <tr><td><code>"cad_add_constraint"</code></td><td>"Add a geometric constraint and get the solver's verdict."</td></tr>
                                    <tr><td><code>"cad_dimension"</code></td><td>"Add a driving linear, radial or angular dimension from a value, a variable or an expression."</td></tr>
                                    <tr><td><code>"cad_solve_sketch"</code></td><td>"Run the 2D solver and report status, residual and remaining degrees of freedom."</td></tr>
                                    <tr><td><code>"cad_offset_sketch"</code></td><td>"Make a new sketch offset from a profile: negative shrinks it, positive grows it."</td></tr>
                                    <tr><td><code>"cad_publish_part"</code></td><td>"Publish a part to the Universe's shared CAD library so it can be placed many times."</td></tr>
                                    <tr><td><code>"cad_list_sources"</code></td><td>"The shared library's published parts and whether each still evaluates."</td></tr>
                                    <tr><td><code>"cad_export_glb"</code></td><td>"Export a CadPart to a binary glTF file with its parameters."</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="specialist-website" class="subsection">
                            <h3>"Website and Moderation"</h3>
                            <p>
                                "The Website tools edit a Space's Website service as TOML, so they work without
                                an engine; see "<a href="/docs/website">"Website Service"</a>". The moderation
                                tools serve Eustress's Gallery moderators and act on eustress.dev, so they need
                                the Network capability that "<code>"EUSTRESS_MODERATOR_TOKEN"</code>" grants."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"website_status"</code></td><td>"The Website service's namespace, schema version and every Reference."</td></tr>
                                    <tr><td><code>"website_setup"</code></td><td>"Create or update the service's namespace and schema version."</td></tr>
                                    <tr><td><code>"website_add_reference"</code></td><td>"Add or replace a Reference, a pointer that each publish resolves live."</td></tr>
                                    <tr><td><code>"website_remove_reference"</code></td><td>"Remove a Reference. Refused over MCP (Destructive)."</td></tr>
                                    <tr><td><code>"website_manifest_url"</code></td><td>"The manifest URL a site fetches, with the markup it needs."</td></tr>
                                    <tr><td><code>"moderation_queue"</code></td><td>"Gallery moderation cases by status."</td></tr>
                                    <tr><td><code>"moderation_case"</code></td><td>"One moderation case in full."</td></tr>
                                    <tr><td><code>"moderation_act"</code></td><td>"Approve, reject, hold or request changes, as the signed-in moderator."</td></tr>
                                    <tr><td><code>"moderation_backfill"</code></td><td>"Pass older Gallery listings through the moderation gate, a few per call."</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="specialist-ai" class="subsection">
                            <h3>"Generation and Network"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"image_to_code"</code></td><td>"Turn an image in the Universe into Rune code through the Claude vision API. Refused over MCP (Network)."</td></tr>
                                    <tr><td><code>"image_to_geometry"</code></td><td>"Rebuild a reference image as scene geometry with VIGA, a generate, render and verify loop. Refused over MCP (Network)."</td></tr>
                                    <tr><td><code>"document_to_code"</code></td><td>"Turn a design document into Rune or Luau code. Refused over MCP (Network)."</td></tr>
                                    <tr><td><code>"http_request"</code></td><td>"GET or POST to an external URL. Refused over MCP (Network)."</td></tr>
                                    <tr><td><code>"find_similar_entities"</code></td><td>"Entities most like a reference entity. Returns the request only."</td></tr>
                                    <tr><td><code>"suggest_swap_template"</code></td><td>"Toolbox templates ranked for a part. Returns the request only."</td></tr>
                                    <tr><td><code>"suggest_contextual_edits"</code></td><td>"A few edits that would improve a scene. Returns the request only."</td></tr>
                                    <tr><td><code>"suggest_tool_defaults"</code></td><td>"Options Bar defaults for a tool. Returns the request only."</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The last four hand their request to an engine-side lookup. Over MCP there is
                                no engine behind the call, so they answer with what was asked rather than
                                results."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // RESOURCES
                    // =========================================================
                    <section id="resources" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "Resources"
                        </h2>

                        <div id="resources-uris" class="subsection">
                            <h3>"Resource URIs"</h3>
                            <p>
                                "A resource is a document the client can read and pin. The server publishes six
                                URI templates; "<code>"{+path}"</code>" keeps its slashes, so nested folders
                                round-trip."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"URI"</th><th>"Returns"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"eustress://space/{space}"</code></td><td>"A Markdown overview of a Space: its service folders and scripts."</td></tr>
                                    <tr><td><code>"eustress://script/{space}/{+path}"</code></td><td>"A script folder's class, summary and source in one Markdown document."</td></tr>
                                    <tr><td><code>"eustress://entity/{space}/{+path}"</code></td><td>"An entity's "<code>"_instance.toml"</code>" with its name and class."</td></tr>
                                    <tr><td><code>"eustress://file/{space}/{+path}"</code></td><td>"Any text file under a Space. Binary files such as "<code>".png"</code>" and "<code>".glb"</code>" are refused."</td></tr>
                                    <tr><td><code>"eustress://conversation/{session_id}"</code></td><td>"A saved Workshop session from "<code>".eustress/knowledge/sessions"</code>"."</td></tr>
                                    <tr><td><code>"eustress://brief/{product}"</code></td><td>"An "<code>"ideation_brief.toml"</code>" found under the Spaces, by product name."</td></tr>
                                </tbody>
                            </table>
                            <p>
                                <code>"resources/list"</code>" returns the Spaces, their scripts (up to 500), the
                                20 newest conversations and the briefs, 100 per page with a cursor, up to 2,000
                                entries."
                            </p>
                        </div>

                        <div id="resources-subscribe" class="subsection">
                            <h3>"Subscriptions"</h3>
                            <p>
                                "Subscribe to a resource and the server sends "
                                <code>"notifications/resources/updated"</code>" when its file changes. A file
                                watcher starts with the first subscription and stops with the last. It watches
                                the Universe's "<code>"Spaces"</code>" folder and its saved sessions, lets a
                                burst of writes settle for 120 ms, and reacts to "<code>".rune"</code>", "
                                <code>".luau"</code>", "<code>".soul"</code>", "<code>".md"</code>", "
                                <code>".toml"</code>" and "<code>".json"</code>" files outside "
                                <code>".git"</code>", "<code>"target"</code>" and "<code>"node_modules"</code>"."
                            </p>
                            <p>
                                "Switching Universe with "<code>"set_active_universe"</code>" moves the watcher
                                and sends "<code>"notifications/resources/list_changed"</code>", so the client
                                drops a list that belongs to the Universe it left."
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

                        <div id="roadmap-live" class="subsection">
                            <h3>"Editor Control over MCP"</h3>
                            <p>
                                "The twelve refused live tools will become callable once they have entries in
                                the capability table: the AI camera, viewport capture, tool and selection
                                control, editor actions, Dataset bindings and database export. The engine side
                                of each already answers on the bridge, so an agent will see and steer Studio
                                through MCP the way the eustress CLI can today."
                            </p>
                        </div>

                        <div id="roadmap-ship" class="subsection">
                            <h3>"Packaging and Pass-Through Tools"</h3>
                            <p>
                                "The installer already has a place for eustress-mcp; a release build step for
                                its package will put the server beside Eustress Engine on every install. The
                                disk "<code>"raycast"</code>" tool will route to the bridge's live physics, and
                                the request-only tools will return results once they have a store or an engine
                                behind them out of process."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Point an agent at your Universe and let it read, build and test."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/learn/cli" class="btn-secondary-steel">"CLI & Headless"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/earning" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Earning"</span>
                            </div>
                        </a>
                        <a href="/learn/cli" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"CLI & Headless"</span>
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
