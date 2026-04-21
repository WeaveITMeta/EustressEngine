// =============================================================================
// Eustress Web - Rune LSP Documentation Page
// =============================================================================
// The Rune language server bundled inside Eustress Engine. Powers both the
// in-engine Problems panel AND the @eustress/rune-lsp VS Code extension.
// Same binary, two transports.
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
            id: "intro",
            title: "Introduction",
            subsections: vec![
                TocSubsection { id: "intro-what", title: "What It Does" },
                TocSubsection { id: "intro-why", title: "Why It's Useful" },
            ],
        },
        TocSection {
            id: "how-to",
            title: "How to Use It",
            subsections: vec![
                TocSubsection { id: "how-bundled", title: "Bundled with the Engine" },
                TocSubsection { id: "how-stdio", title: "Standalone stdio" },
                TocSubsection { id: "how-tcp", title: "Standalone TCP" },
                TocSubsection { id: "how-editor", title: "Point an Editor at It" },
            ],
        },
        TocSection {
            id: "api",
            title: "API Reference",
            subsections: vec![
                TocSubsection { id: "api-cli", title: "Command-Line Flags" },
                TocSubsection { id: "api-capabilities", title: "Capabilities" },
                TocSubsection { id: "api-requests", title: "Request Handlers" },
                TocSubsection { id: "api-notifications", title: "Notifications" },
                TocSubsection { id: "api-diagnostics", title: "Diagnostics Pipeline" },
            ],
        },
        TocSection {
            id: "use-cases",
            title: "Use Cases",
            subsections: vec![
                TocSubsection { id: "uc-engine", title: "In-Engine Problems Panel" },
                TocSubsection { id: "uc-external", title: "External IDE Backend" },
                TocSubsection { id: "uc-ci", title: "CI Lint Checks" },
                TocSubsection { id: "uc-custom", title: "Custom Editors" },
            ],
        },
        TocSection {
            id: "conclusion",
            title: "Conclusion",
            subsections: vec![],
        },
    ]
}

#[component]
pub fn LearnLspPage() -> impl IntoView {
    let active_section = RwSignal::new("intro".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-networking"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/brain.svg" alt="LSP" class="toc-icon" />
                        <h2>"Rune LSP"</h2>
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
                            <span class="current">"Rune LSP"</span>
                        </div>
                        <h1 class="docs-title">"Rune LSP"</h1>
                        <p class="docs-subtitle">
                            "A standards-compliant Language Server for Rune scripts. Written in Rust,
                            built on " <code>"tower-lsp"</code> ", and powered by the same analyzer
                            that drives Eustress Engine's Problems panel. It ships inside the engine
                            binary bundle — no separate install — and speaks stdio or TCP."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "12 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Intermediate"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "v0.1.0"
                            </span>
                        </div>
                    </header>

                    // ── Introduction ─────────────────────────────────────
                    <section id="intro" class="docs-section">
                        <h2 class="section-anchor">"Introduction"</h2>

                        <div id="intro-what" class="docs-block">
                            <h3>"What It Does"</h3>
                            <p>
                                <code>"eustress-lsp"</code> " is the Rune language server. It parses
                                Rune source, compiles it through " <code>"rune::prepare"</code> ", and
                                exposes the resulting diagnostics, symbol index, and AST walker via the
                                standard Language Server Protocol. It does not ship its own text editor
                                and it does not embed the runtime — it's a pure analyzer behind an LSP
                                adapter."
                            </p>
                        </div>

                        <div id="intro-why" class="docs-block">
                            <h3>"Why It's Useful"</h3>
                            <p>
                                "Rune has first-class tooling inside Eustress Engine because the engine
                                is the analyzer's primary consumer. Shipping that same analyzer as an
                                LSP server means every editor — VS Code, Windsurf, Cursor, Zed, Neovim,
                                Helix — can have identical intelligence without re-implementing the
                                wheel. The server is also useful on its own in CI for typed lint checks
                                without spinning up the full engine."
                            </p>
                            <div class="docs-callout info">
                                <strong>"Single source of truth:"</strong>
                                " the LSP and the in-engine Problems panel call the same analyzer
                                functions. If diagnostics differ between surfaces, that's a bug, not a
                                feature difference."
                            </div>
                        </div>
                    </section>

                    // ── How to Use It ────────────────────────────────────
                    <section id="how-to" class="docs-section">
                        <h2 class="section-anchor">"How to Use It"</h2>

                        <div id="how-bundled" class="docs-block">
                            <h3>"Bundled with the Engine (normal path)"</h3>
                            <p>
                                "For almost everyone this is the only section that matters. The Rune
                                LSP is built into the Eustress Engine installer — there is no
                                separate download. Install the engine, launch it on a Universe, and
                                the LSP is automatically available to every IDE on your machine."
                            </p>
                            <ol class="docs-list numbered">
                                <li>"Install Eustress Engine from " <a href="/download">"/download"</a> "."</li>
                                <li>"Launch the engine and open a Universe (first launch scaffolds one for you)."</li>
                                <li>
                                    "Install the "
                                    <a href="https://open-vsx.org/extension/WeaveITMeta/rune-lsp" target="_blank" rel="noopener">
                                        <code>"WeaveITMeta.rune-lsp"</code>
                                    </a>
                                    " extension from Open VSX (works in VS Code, Windsurf, Cursor) — it finds the LSP over TCP automatically."
                                </li>
                            </ol>
                            <p>
                                "Under the hood: " <code>"cargo build --release"</code> " on the engine
                                workspace produces two binaries — " <code>"eustress-engine"</code> "
                                and " <code>"eustress-lsp"</code> " — and the installer ships both.
                                When the engine starts, its " <code>"LspLauncherPlugin"</code>
                                " spawns " <code>"eustress-lsp --tcp"</code> " as a child process
                                and writes the listening port to " <code>".eustress/lsp.port"</code>
                                " inside the active Universe. Closing the engine kills the child."
                            </p>
                        </div>

                        <div id="how-stdio" class="docs-block">
                            <h3>"Standalone stdio"</h3>
                            <p>"Run directly for a classic LSP-over-stdin/stdout client:"</p>
                            <pre class="code-block"><code>{"# Editors that expect a spawnable LSP
eustress-lsp"}</code></pre>
                            <p>
                                "Useful when you want language intelligence without a running engine —
                                CI, headless SSH sessions, or editors that refuse to speak TCP."
                            </p>
                        </div>

                        <div id="how-tcp" class="docs-block">
                            <h3>"Standalone TCP"</h3>
                            <pre class="code-block"><code>{"# OS-assigned port, written to stdout and to a file
eustress-lsp --tcp --port 0 --port-file /tmp/rune.port

# Fixed port, accepts multiple concurrent clients
eustress-lsp --tcp --port 8787"}</code></pre>
                            <p>
                                "TCP lets one server back many editors at once. The engine uses it so
                                editor extensions can attach even though the engine itself already has
                                the analyzer loaded internally."
                            </p>
                        </div>

                        <div id="how-editor" class="docs-block">
                            <h3>"Point an Editor at It"</h3>
                            <p>
                                "For VS Code / Windsurf / Cursor, install the "
                                <a href="https://open-vsx.org/extension/WeaveITMeta/rune-lsp" target="_blank" rel="noopener">
                                    <code>"WeaveITMeta.rune-lsp"</code>
                                </a>
                                " extension from "
                                <a href="https://open-vsx.org/extension/WeaveITMeta/rune-lsp" target="_blank" rel="noopener">
                                    "Open VSX"
                                </a>
                                " — it discovers the port file automatically. For other editors,
                                configure Rune as a file type and set the LSP command to "
                                <code>"eustress-lsp"</code> " (stdio) or a TCP dialer pointing at the port."
                            </p>
                        </div>
                    </section>

                    // ── API Reference ────────────────────────────────────
                    <section id="api" class="docs-section">
                        <h2 class="section-anchor">"API Reference"</h2>

                        <div id="api-cli" class="docs-block">
                            <h3>"Command-Line Flags"</h3>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"--tcp"</code>
                                    <span>"Switch from stdio to TCP transport"</span>
                                </div>
                                <div class="api-row">
                                    <code>"--port <N>"</code>
                                    <span>"TCP port (0 = OS-assigned); defaults to 0"</span>
                                </div>
                                <div class="api-row">
                                    <code>"--port-file <path>"</code>
                                    <span>"Write the listening port to this path for client discovery"</span>
                                </div>
                                <div class="api-row">
                                    <code>"--bind <addr>"</code>
                                    <span>"TCP bind address; defaults to 127.0.0.1 (loopback only)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"--log <level>"</code>
                                    <span>"\"off\" | \"info\" | \"debug\" | \"trace\" — tracing verbosity"</span>
                                </div>
                            </div>
                        </div>

                        <div id="api-capabilities" class="docs-block">
                            <h3>"Capabilities"</h3>
                            <p>"The server advertises the following in its " <code>"initialize"</code> " response:"</p>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"textDocumentSync"</code>
                                    <span>"Incremental — clients send ranged edits, not full buffers"</span>
                                </div>
                                <div class="api-row">
                                    <code>"hoverProvider"</code>
                                    <span>"Enabled — returns symbol metadata or diagnostic-under-cursor"</span>
                                </div>
                                <div class="api-row">
                                    <code>"definitionProvider"</code>
                                    <span>"Enabled — single-file go-to-definition"</span>
                                </div>
                                <div class="api-row">
                                    <code>"referencesProvider"</code>
                                    <span>"Enabled — all usages within the open document"</span>
                                </div>
                                <div class="api-row">
                                    <code>"completionProvider"</code>
                                    <span>"Trigger characters: \".\" and \"::\""</span>
                                </div>
                                <div class="api-row">
                                    <code>"renameProvider"</code>
                                    <span>"prepareProvider = true; validates the target is an identifier"</span>
                                </div>
                                <div class="api-row">
                                    <code>"codeActionProvider"</code>
                                    <span>"Emits quick fixes scoped to the current diagnostic range"</span>
                                </div>
                                <div class="api-row">
                                    <code>"documentSymbolProvider"</code>
                                    <span>"Outline: functions, structs, constants, modules"</span>
                                </div>
                            </div>
                        </div>

                        <div id="api-requests" class="docs-block">
                            <h3>"Request Handlers"</h3>
                            <p>"Every handler delegates to " <code>"script_editor::analyzer"</code> ":"</p>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"textDocument/hover"</code>
                                    <span>"identifier_at → symbol lookup; falls back to diagnostic message"</span>
                                </div>
                                <div class="api-row">
                                    <code>"textDocument/definition"</code>
                                    <span>"identifier_at + SymbolIndex → Location"</span>
                                </div>
                                <div class="api-row">
                                    <code>"textDocument/references"</code>
                                    <span>"Whole-document scan for identifier matches (comment/string aware)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"textDocument/completion"</code>
                                    <span>"prefix_at → complete(prefix, symbols, max_items)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"textDocument/rename"</code>
                                    <span>"analyzer::rename → WorkspaceEdit with TextEdits"</span>
                                </div>
                                <div class="api-row">
                                    <code>"textDocument/codeAction"</code>
                                    <span>"Range-overlap check against published diagnostics; returns fixes"</span>
                                </div>
                            </div>
                        </div>

                        <div id="api-notifications" class="docs-block">
                            <h3>"Notifications"</h3>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"textDocument/publishDiagnostics"</code>
                                    <span>"Pushed on open / change / save, debounced at 80 ms per document"</span>
                                </div>
                                <div class="api-row">
                                    <code>"window/logMessage"</code>
                                    <span>"Server-side tracing mirrored to the client log"</span>
                                </div>
                            </div>
                        </div>

                        <div id="api-diagnostics" class="docs-block">
                            <h3>"Diagnostics Pipeline"</h3>
                            <ol class="docs-list numbered">
                                <li>"Client sends " <code>"didChange"</code> " with incremental edits"</li>
                                <li>"Server reassembles the buffer in a " <code>"DashMap<Url, String>"</code></li>
                                <li>"An 80 ms debounce coalesces rapid keystrokes"</li>
                                <li>"After quiescence, " <code>"analyzer::analyze"</code> " runs: parse → compile → diagnostics"</li>
                                <li>"Results convert to LSP " <code>"Diagnostic"</code> " values and publish"</li>
                            </ol>
                            <div class="docs-callout info">
                                <strong>"Performance:"</strong>
                                " analysis runs on a dedicated Tokio task, so typing never blocks the main
                                IO loop. The 80 ms debounce is tuned to feel responsive without wasting
                                cycles on transient parse errors mid-keystroke."
                            </div>
                        </div>
                    </section>

                    // ── Use Cases ────────────────────────────────────────
                    <section id="use-cases" class="docs-section">
                        <h2 class="section-anchor">"Use Cases"</h2>

                        <div id="uc-engine" class="docs-block">
                            <h3>"In-Engine Problems Panel"</h3>
                            <p>
                                "Inside Eustress Engine, the Problems panel subscribes to the same
                                analyzer output that the LSP publishes. Click a row — jump to the
                                file and line. Click " <em>"Fix with Workshop"</em> " — the full
                                problem list seeds a new Workshop conversation so Claude can resolve
                                them."
                            </p>
                        </div>

                        <div id="uc-external" class="docs-block">
                            <h3>"External IDE Backend"</h3>
                            <p>
                                "Every feature of the " <a href="/learn/ide">"IDE extension"</a>
                                " is served by this LSP. The extension is a client, not a re-implementation."
                            </p>
                        </div>

                        <div id="uc-ci" class="docs-block">
                            <h3>"CI Lint Checks"</h3>
                            <p>
                                "Pipe the server diagnostics into your CI pipeline. A small wrapper can
                                " <code>"didOpen"</code> " every " <code>".rune"</code> " file and fail
                                the build on any " <code>"Error"</code> "-severity diagnostic, with zero
                                duplication of the analysis logic."
                            </p>
                            <pre class="code-block"><code>{"# pseudo-CI step
eustress-lsp --tcp --port 9999 &
LSP_PID=$!
rune-lint-runner --port 9999 --fail-on error Spaces/
kill $LSP_PID"}</code></pre>
                        </div>

                        <div id="uc-custom" class="docs-block">
                            <h3>"Custom Editors"</h3>
                            <p>
                                "Anyone can write a Zed, Helix, or Neovim client against this server.
                                There's nothing Eustress-specific in the protocol — it's pure LSP. The
                                repository's " <code>"infrastructure/extensions/"</code> " directory is
                                organized so additional editor packages can live alongside the VS Code one."
                            </p>
                        </div>
                    </section>

                    // ── Conclusion ───────────────────────────────────────
                    <section id="conclusion" class="docs-section">
                        <h2 class="section-anchor">"Conclusion"</h2>
                        <p>
                            "The Rune LSP is the connective tissue behind every Rune-editing experience
                            Eustress offers — the in-engine panels, the VS Code extension, any future
                            editor adapter. One analyzer, one binary, two transports, zero vendor
                            lock-in. Run it bundled with Eustress Engine, run it standalone in CI, or
                            run it behind your favorite editor — the semantics are always the same."
                        </p>
                        <div class="docs-callout info">
                            <strong>"See also:"</strong>
                            " the " <a href="/learn/ide">"IDE Integration"</a>
                            " guide for editor-side setup, and the " <a href="/learn/mcp">"MCP Server"</a>
                            " guide for exposing the full Universe (not just scripts) to AI clients."
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
                        <a href="/docs/audio" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Audio"</span>
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
