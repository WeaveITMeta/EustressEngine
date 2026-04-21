// =============================================================================
// Eustress Web - IDE Integration Documentation Page
// =============================================================================
// Covers the @eustress/rune-lsp VS Code extension: what it does, why it's
// useful, how to install it, the commands/settings it exposes, real-world
// use cases, and a closing recap.
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
                TocSubsection { id: "how-install", title: "Install the Extension" },
                TocSubsection { id: "how-launch", title: "Launch Eustress Engine" },
                TocSubsection { id: "how-open", title: "Open a Script" },
                TocSubsection { id: "how-multi", title: "Multiple Universes" },
            ],
        },
        TocSection {
            id: "api",
            title: "API Reference",
            subsections: vec![
                TocSubsection { id: "api-commands", title: "Commands" },
                TocSubsection { id: "api-settings", title: "Settings" },
                TocSubsection { id: "api-features", title: "Language Features" },
                TocSubsection { id: "api-protocol", title: "Transport Protocol" },
            ],
        },
        TocSection {
            id: "use-cases",
            title: "Use Cases",
            subsections: vec![
                TocSubsection { id: "uc-engine", title: "Alongside the Engine" },
                TocSubsection { id: "uc-headless", title: "Headless Script Editing" },
                TocSubsection { id: "uc-team", title: "Team Collaboration" },
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
pub fn LearnIdePage() -> impl IntoView {
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
                        <img src="/assets/icons/code.svg" alt="IDE" class="toc-icon" />
                        <h2>"IDE Integration"</h2>
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
                            <span class="current">"IDE Integration"</span>
                        </div>
                        <h1 class="docs-title">"IDE Integration"</h1>
                        <p class="docs-subtitle">
                            "Bring Rune script intelligence into VS Code, Windsurf, and Cursor.
                            The " <code>"@eustress/rune-lsp"</code> " extension attaches to any running Eustress
                            Engine instance over TCP and delivers diagnostics, hover, and navigation
                            for every Rune script in your Universe."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "10 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Beginner"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "v0.3.0"
                            </span>
                        </div>
                    </header>

                    // ── Introduction ─────────────────────────────────────
                    <section id="intro" class="docs-section">
                        <h2 class="section-anchor">"Introduction"</h2>

                        <div id="intro-what" class="docs-block">
                            <h3>"What It Does"</h3>
                            <p>
                                "The Eustress IDE extension is a thin shim around the " <code>"eustress-lsp"</code>
                                " binary that ships inside every engine install. When you open a "
                                <code>".rune"</code> " file in VS Code (or any VS Code-derivative like Windsurf
                                or Cursor), the extension finds the running engine, connects to its
                                language server over TCP, and surfaces:"
                            </p>
                            <ul class="docs-list">
                                <li>"Real-time parse and compile errors as red squiggles"</li>
                                <li>"Hover tooltips with types and documentation"</li>
                                <li>"Go-to-definition (F12) and find-references (Shift+F12)"</li>
                                <li>"Symbol-aware completion"</li>
                                <li>"Cross-file rename across the entire Universe"</li>
                            </ul>
                        </div>

                        <div id="intro-why" class="docs-block">
                            <h3>"Why It's Useful"</h3>
                            <p>
                                "Eustress Engine has a built-in script editor, but many creators prefer
                                their daily driver editor — VS Code for keyboard muscle memory, Windsurf
                                or Cursor for AI completion. The IDE extension lets you stay in that
                                editor while the engine keeps running in the background, hot-reloading
                                scripts as you save."
                            </p>
                            <div class="docs-callout info">
                                <strong>"No vendor lock-in:"</strong>
                                " the protocol is stock LSP over TCP. If tomorrow someone writes a Neovim
                                or Zed adapter, it will work against the same running engine."
                            </div>
                        </div>
                    </section>

                    // ── How to Use It ────────────────────────────────────
                    <section id="how-to" class="docs-section">
                        <h2 class="section-anchor">"How to Use It"</h2>

                        <div id="how-install" class="docs-block">
                            <h3>"Install the Extension"</h3>
                            <p>
                                "Install from "
                                <a href="https://open-vsx.org/extension/WeaveITMeta/rune-lsp" target="_blank" rel="noopener">"Open VSX"</a>
                                " (the registry Windsurf, Cursor, VSCodium, and most VS Code forks use), or drop in the VSIX that ships with Eustress Engine:"
                            </p>
                            <pre class="code-block"><code>{"# Windsurf / Cursor / VSCodium / any Open VSX-enabled editor:
#   UI:  Extensions panel → search 'Rune LSP' → Install
#   CLI:
code --install-extension WeaveITMeta.rune-lsp

# Or sideload the bundled VSIX from the engine install directory:
code --install-extension %INSTALL%/extensions/rune-lsp-0.3.6.vsix"}</code></pre>
                            <p>
                                "Direct link: "
                                <a href="https://open-vsx.org/extension/WeaveITMeta/rune-lsp" target="_blank" rel="noopener">
                                    "open-vsx.org/extension/WeaveITMeta/rune-lsp"
                                </a>
                            </p>
                        </div>

                        <div id="how-launch" class="docs-block">
                            <h3>"Launch Eustress Engine"</h3>
                            <p>
                                "Open your Universe in Eustress Engine. On startup the engine spawns "
                                <code>"eustress-lsp --tcp"</code> " and writes the assigned port to "
                                <code>".eustress/lsp.port"</code> " inside the Universe root. Nothing else to do."
                            </p>
                        </div>

                        <div id="how-open" class="docs-block">
                            <h3>"Open a Script"</h3>
                            <p>
                                "In VS Code, open any file under the Universe. The extension walks up
                                the directory tree looking for " <code>".eustress/lsp.port"</code> ", connects
                                to that port, and binds the document to the matching engine instance.
                                A status bar indicator (" <code>"Rune: connected"</code> ") confirms the link."
                            </p>
                        </div>

                        <div id="how-multi" class="docs-block">
                            <h3>"Multiple Universes"</h3>
                            <p>
                                "If you have two engines running in two different Universes, each writes
                                its own port file. The extension maintains one client per Universe, keyed
                                by the discovered port, and scopes each client's " <code>"documentSelector"</code>
                                " to that Universe's directory. Cross-Universe routing happens automatically —
                                no editor restart needed when you swap projects."
                            </p>
                        </div>
                    </section>

                    // ── API Reference ────────────────────────────────────
                    <section id="api" class="docs-section">
                        <h2 class="section-anchor">"API Reference"</h2>

                        <div id="api-commands" class="docs-block">
                            <h3>"Commands"</h3>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"rune.restart"</code>
                                    <span>"Stop every LSP client and re-scan for running engines"</span>
                                </div>
                                <div class="api-row">
                                    <code>"rune.showOutput"</code>
                                    <span>"Reveal the extension's Output channel for debugging"</span>
                                </div>
                                <div class="api-row">
                                    <code>"rune.reconnectActive"</code>
                                    <span>"Force-reconnect the client for the active file's Universe"</span>
                                </div>
                            </div>
                        </div>

                        <div id="api-settings" class="docs-block">
                            <h3>"Settings"</h3>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"rune.serverPath"</code>
                                    <span>"Override the eustress-lsp binary (defaults to bundled / engine-adjacent / PATH)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"rune.transport"</code>
                                    <span>"\"tcp\" (default — connect to running engine) or \"stdio\" (spawn a private server)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"rune.traceServer"</code>
                                    <span>"\"off\" | \"messages\" | \"verbose\" — LSP JSON-RPC tracing"</span>
                                </div>
                                <div class="api-row">
                                    <code>"rune.startupNotification"</code>
                                    <span>"Show the connection status toast on attach (default: true)"</span>
                                </div>
                            </div>
                        </div>

                        <div id="api-features" class="docs-block">
                            <h3>"Language Features"</h3>
                            <p>"The server advertises the following LSP capabilities:"</p>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"textDocument/publishDiagnostics"</code>
                                    <span>"Push-model diagnostics on every change (80 ms debounce)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"textDocument/hover"</code>
                                    <span>"Symbol kind, span, docstring, and diagnostic-under-cursor"</span>
                                </div>
                                <div class="api-row">
                                    <code>"textDocument/definition"</code>
                                    <span>"Jump to the declaration site of a symbol"</span>
                                </div>
                                <div class="api-row">
                                    <code>"textDocument/references"</code>
                                    <span>"Find every usage of a symbol across the open document"</span>
                                </div>
                                <div class="api-row">
                                    <code>"textDocument/completion"</code>
                                    <span>"Prefix-filtered completions drawn from the symbol index"</span>
                                </div>
                                <div class="api-row">
                                    <code>"textDocument/rename"</code>
                                    <span>"Comment- and string-aware rename across the document"</span>
                                </div>
                                <div class="api-row">
                                    <code>"textDocument/codeAction"</code>
                                    <span>"Quick fixes for the current diagnostic (when applicable)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"textDocument/documentSymbol"</code>
                                    <span>"Outline: functions, structs, constants, modules"</span>
                                </div>
                            </div>
                        </div>

                        <div id="api-protocol" class="docs-block">
                            <h3>"Transport Protocol"</h3>
                            <p>
                                "The extension uses stock " <code>"vscode-languageclient"</code> " talking to a "
                                <code>"tower-lsp"</code> " server. Transport is either:"
                            </p>
                            <ul class="docs-list">
                                <li>
                                    <strong>"TCP"</strong>
                                    " (default): the engine writes " <code>".eustress/lsp.port"</code>
                                    " at startup; the extension connects to " <code>"127.0.0.1:{port}"</code>
                                </li>
                                <li>
                                    <strong>"stdio"</strong>
                                    ": the extension spawns its own " <code>"eustress-lsp"</code>
                                    " subprocess and speaks LSP over stdin/stdout"
                                </li>
                            </ul>
                            <div class="docs-callout info">
                                <strong>"Privacy:"</strong>
                                " the TCP socket is bound to loopback only. Nothing leaves your machine."
                            </div>
                        </div>
                    </section>

                    // ── Use Cases ────────────────────────────────────────
                    <section id="use-cases" class="docs-section">
                        <h2 class="section-anchor">"Use Cases"</h2>

                        <div id="uc-engine" class="docs-block">
                            <h3>"Alongside the Engine"</h3>
                            <p>
                                "Keep Eustress Engine open on one monitor for the 3D viewport,
                                Explorer, and Properties panels. Drive script editing from VS Code on
                                the other monitor. Saves flow through the same file-system watcher the
                                engine uses, so hot-reload Just Works."
                            </p>
                        </div>

                        <div id="uc-headless" class="docs-block">
                            <h3>"Headless Script Editing"</h3>
                            <p>
                                "Writing Rune scripts on a low-powered laptop while your desktop runs
                                the simulation? SSH in, run " <code>"eustress-lsp --tcp --port 0"</code>
                                " bare (no engine), and point VS Code at the resulting port. You still get
                                full language intelligence without the Bevy render cost."
                            </p>
                        </div>

                        <div id="uc-team" class="docs-block">
                            <h3>"Team Collaboration"</h3>
                            <p>
                                "Every teammate can use their preferred editor. The extension's output
                                matches what the engine's Problems panel shows, so bug reports across editors
                                are consistent."
                            </p>
                        </div>
                    </section>

                    // ── Conclusion ───────────────────────────────────────
                    <section id="conclusion" class="docs-section">
                        <h2 class="section-anchor">"Conclusion"</h2>
                        <p>
                            "The IDE extension is the shortest path to Rune intelligence outside the
                            the engine. Install once, launch Eustress Engine, and your editor of choice becomes
                            a first-class Eustress workspace. Under the hood it's stock LSP — no magic,
                            no lock-in, and the same analyzer the engine runs internally."
                        </p>
                        <div class="docs-callout info">
                            <strong>"Next:"</strong>
                            " if you want your AI assistant to reason about the Universe (not just the
                            current file), read the " <a href="/learn/mcp">"MCP Server"</a>
                            " guide. If you want to understand the server itself, read the "
                            <a href="/learn/lsp">"Rune LSP"</a> " guide."
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/networking" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Networking"</span>
                            </div>
                        </a>
                        <a href="/learn/mcp" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"MCP Server"</span>
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
