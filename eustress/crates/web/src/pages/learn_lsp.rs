// =============================================================================
// Eustress Web - Rune LSP Documentation Page
// =============================================================================
// Rune LSP: the eustress-lsp language server. How to run it, how Studio starts
// it, what the shared analyzer checks, and how to point any editor at it.
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
                TocSubsection { id: "overview-what", title: "One Analyzer, Every Editor" },
                TocSubsection { id: "overview-binary", title: "Getting the Binary" },
            ],
        },
        TocSection {
            id: "running",
            title: "Running It",
            subsections: vec![
                TocSubsection { id: "running-flags", title: "Command Line" },
                TocSubsection { id: "running-stdio", title: "stdio" },
                TocSubsection { id: "running-tcp", title: "TCP" },
            ],
        },
        TocSection {
            id: "studio",
            title: "Inside Studio",
            subsections: vec![
                TocSubsection { id: "studio-launch", title: "How Studio Starts It" },
                TocSubsection { id: "studio-lifecycle", title: "Restarts and Logs" },
            ],
        },
        TocSection {
            id: "diagnostics",
            title: "Diagnostics",
            subsections: vec![
                TocSubsection { id: "diagnostics-passes", title: "Three Passes" },
                TocSubsection { id: "diagnostics-sources", title: "Sources and Timing" },
            ],
        },
        TocSection {
            id: "features",
            title: "Language Features",
            subsections: vec![
                TocSubsection { id: "features-capabilities", title: "Capabilities" },
                TocSubsection { id: "features-navigation", title: "Hover and Navigation" },
                TocSubsection { id: "features-editing", title: "Completion and Editing" },
                TocSubsection { id: "features-index", title: "The Universe Index" },
            ],
        },
        TocSection {
            id: "setup",
            title: "Editor Setup",
            subsections: vec![
                TocSubsection { id: "setup-vscode", title: "VS Code and Its Forks" },
                TocSubsection { id: "setup-neovim", title: "Neovim" },
                TocSubsection { id: "setup-helix", title: "Helix" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-symbols", title: "Richer Symbols" },
                TocSubsection { id: "roadmap-packages", title: "The Server in Every Package" },
            ],
        },
    ]
}

/// The analysis pipeline: parse and compile always run; the dry run of the
/// lifecycle functions runs only when neither pass found an error.
#[component]
fn AnalysisDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 180" role="img"
                aria-label="A Rune file is parsed, then compiled against the engine modules. Only when neither pass found an error are its lifecycle functions dry-run. All diagnostics then go to the editor, the Problems panel and the Output panel.">
                <defs>
                    <marker id="lsp-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                // The path taken when parse or compile reported an error.
                <line x1="235" y1="56" x2="235" y2="28" class="dg-line dg-line-dashed"></line>
                <line x1="235" y1="28" x2="560" y2="28" class="dg-line dg-line-dashed"></line>
                <line x1="560" y1="28" x2="560" y2="54" class="dg-line dg-line-dashed" marker-end="url(#lsp-arrow)"></line>
                <text x="397" y="20" class="dg-note" text-anchor="middle">"errors found: skip the dry run"</text>

                // The passes.
                <rect x="10" y="56" width="130" height="56" rx="8" class="dg-box"></rect>
                <text x="75" y="89" class="dg-label" text-anchor="middle">"Parse"</text>
                <rect x="170" y="56" width="130" height="56" rx="8" class="dg-box"></rect>
                <text x="235" y="89" class="dg-label" text-anchor="middle">"Compile"</text>
                <rect x="330" y="56" width="130" height="56" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="395" y="89" class="dg-label" text-anchor="middle">"Dry run"</text>
                <rect x="490" y="56" width="140" height="56" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="560" y="89" class="dg-label" text-anchor="middle">"Diagnostics"</text>
                <line x1="140" y1="84" x2="168" y2="84" class="dg-line" marker-end="url(#lsp-arrow)"></line>
                <line x1="300" y1="84" x2="328" y2="84" class="dg-line" marker-end="url(#lsp-arrow)"></line>
                <line x1="460" y1="84" x2="488" y2="84" class="dg-line" marker-end="url(#lsp-arrow)"></line>

                // What each pass does.
                <text x="75" y="136" class="dg-note" text-anchor="middle">"Rune parser"</text>
                <text x="75" y="152" class="dg-note" text-anchor="middle">"functions indexed"</text>
                <text x="235" y="136" class="dg-note" text-anchor="middle">"engine modules"</text>
                <text x="235" y="152" class="dg-note" text-anchor="middle">"names resolved"</text>
                <text x="395" y="136" class="dg-note" text-anchor="middle">"lifecycle functions"</text>
                <text x="395" y="152" class="dg-note" text-anchor="middle">"called once"</text>
                <text x="560" y="136" class="dg-note" text-anchor="middle">"editor, Problems"</text>
                <text x="560" y="152" class="dg-note" text-anchor="middle">"panel, Output"</text>
            </svg>
            <figcaption>
                "Parse and compile always run. The dry run happens only when neither found an error,
                so a file that does not compile is never executed."
            </figcaption>
        </figure>
    }
}

/// Rune LSP documentation page.
#[component]
pub fn LearnLspPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-lsp"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/brain.svg" alt="Rune LSP" class="toc-icon" />
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
                            <code>"eustress-lsp"</code>" is the language server for Rune scripts in
                            Eustress. It runs the same analyzer as Studio's Problems panel and speaks the
                            Language Server Protocol over stdio or TCP, so any editor with an LSP client
                            gets diagnostics, hover, completion, navigation and rename."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "10 min read"
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
                            <h3>"One Analyzer, Every Editor"</h3>
                            <p>
                                <code>"eustress-lsp"</code>" is a small program that exposes the Rune
                                analyzer built into Eustress Engine as a standard language server. Studio's
                                script editor, its Problems panel and "<code>"eustress-lsp"</code>" all call
                                the same analyzer functions, so a script shows the same errors, in the same
                                words, wherever you open it."
                            </p>
                            <p>
                                "The server itself only translates. It turns editor positions into the
                                analyzer's line and column numbers, calls the analyzer, and turns the results
                                back into protocol messages, using the tower-lsp library. Improving the
                                analyzer improves every editor at once."
                            </p>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/shield.svg" alt="Diagnostics" />
                                    </div>
                                    <h4>"Diagnostics"</h4>
                                    <p>"Parse, compile and dry-run errors, pushed on every edit."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/book.svg" alt="Hover" />
                                    </div>
                                    <h4>"Hover"</h4>
                                    <p>"Signatures, descriptions and examples for the Eustress API."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/search.svg" alt="Navigation" />
                                    </div>
                                    <h4>"Navigation"</h4>
                                    <p>"Definitions, references and an outline, across the Universe."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/edit.svg" alt="Editing" />
                                    </div>
                                    <h4>"Editing"</h4>
                                    <p>"Completion, parameter hints, rename and a quick fix."</p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-binary" class="subsection">
                            <h3>"Getting the Binary"</h3>
                            <p>
                                <code>"eustress-lsp"</code>" is built from the engine package and carries the
                                engine's version. The Windows installer places "<code>"eustress-lsp.exe"</code>
                                " beside "<code>"eustress-engine.exe"</code>", where Studio finds it. The
                                Windows zip, the macOS disk image and the Linux archive do not include it
                                yet; on those, build the server from the Eustress source:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"# In the eustress/ folder of the repository
cargo build --release -p eustress-engine --bin eustress-lsp

# The binary lands in target/release/ (eustress-lsp.exe on Windows)"#}</code></pre>
                            </div>
                            <p>
                                "The "<code>"lsp"</code>" feature that enables it is on by default. To see
                                which version you have:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-text">{r#"$ eustress-lsp --version
eustress-lsp 0.3.6
Usage: eustress-lsp [--tcp [--port <n>] [--port-file <path>]]"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // RUNNING IT
                    // =========================================================
                    <section id="running" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Running It"
                        </h2>

                        <div id="running-flags" class="subsection">
                            <h3>"Command Line"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Flag"</th><th>"Effect"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"(none)"</td><td>"Serve one editor over stdin and stdout"</td></tr>
                                    <tr><td><code>"--tcp"</code></td><td>"Serve over TCP on "<code>"127.0.0.1"</code>" instead, for any number of editors"</td></tr>
                                    <tr><td><code>"--port <n>"</code></td><td>"With "<code>"--tcp"</code>", listen on port n. The default, 0, lets the operating system pick a free port"</td></tr>
                                    <tr><td><code>"--port-file <path>"</code></td><td>"With "<code>"--tcp"</code>", write the port number to this file, creating its folders"</td></tr>
                                    <tr><td><code>"-h"</code>", "<code>"--help"</code>", "<code>"--version"</code></td><td>"Print the version and usage, then exit"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Any other argument is reported on stderr and ignored, and a "
                                <code>"--port"</code>" value that is not a number counts as 0."
                            </p>
                        </div>

                        <div id="running-stdio" class="subsection">
                            <h3>"stdio"</h3>
                            <p>
                                "With no flags, "<code>"eustress-lsp"</code>" reads protocol messages on stdin
                                and writes replies on stdout. This is how editors usually run a language
                                server: the editor starts the process, owns it, and is its only client."
                            </p>
                        </div>

                        <div id="running-tcp" class="subsection">
                            <h3>"TCP"</h3>
                            <p>
                                "With "<code>"--tcp"</code>", the server listens on the loopback address only,
                                so it accepts connections from your own machine. Once it is listening, it
                                prints "<code>"port=<n>"</code>" on stdout and writes the same number to the "
                                <code>"--port-file"</code>" path, if you gave one. Each connection gets its
                                own session, with its own open documents and Universe index, so several
                                editors can share one server."
                            </p>
                            <p>
                                <code>"Ctrl+C"</code>" stops the server and deletes the port file. Status
                                lines, such as the listening address and each new connection, go to stderr."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"# Let the operating system pick a port, and record it where editors look
eustress-lsp --tcp --port-file MyUniverse/.eustress/lsp.port

# Or listen on a port of your choosing
eustress-lsp --tcp --port 7000"#}</code></pre>
                            </div>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Same code either way"</strong>
                                    <p>
                                        "A server you start yourself and the one Studio starts run the same
                                        code and read the same files, so their features are identical. TCP only
                                        changes who starts the process and how many editors share it."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // INSIDE STUDIO
                    // =========================================================
                    <section id="studio" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Inside Studio"
                        </h2>

                        <div id="studio-launch" class="subsection">
                            <h3>"How Studio Starts It"</h3>
                            <p>
                                "Studio starts "<code>"eustress-lsp"</code>" for you, so editors can connect
                                without launching their own. As soon as the open Space resolves to a Universe
                                (the nearest parent folder that contains "<code>"Spaces/"</code>"), Studio
                                runs:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Launched by Studio"</span>
                                </div>
                                <pre><code class="language-text">{r#"eustress-lsp --tcp --port-file <Universe>/.eustress/lsp.port"#}</code></pre>
                            </div>
                            <p>"It uses the first copy of the program it finds:"</p>
                            <ol class="numbered-list">
                                <li>"The file named by the "<code>"EUSTRESS_LSP_BIN"</code>" environment variable"</li>
                                <li><code>"eustress-lsp"</code>" beside the Studio executable, where the Windows installer puts it"</li>
                                <li>"The "<code>"target/release"</code>" and "<code>"target/debug"</code>" folders of the source tree Studio was built from"</li>
                            </ol>
                            <p>
                                "If there is none, Studio logs one message and stops looking until it
                                restarts. Editors can still start their own server over stdio."
                            </p>
                        </div>

                        <div id="studio-lifecycle" class="subsection">
                            <h3>"Restarts and Logs"</h3>
                            <ul class="docs-list">
                                <li><strong>"One server per Universe."</strong>" Moving to another Space in the same Universe keeps the server. Moving to a Space in a different Universe stops it, deletes its port file and starts a new one there."</li>
                                <li><strong>"Clean exit."</strong>" Closing Studio stops the server and deletes "<code>".eustress/lsp.port"</code>"."</li>
                                <li><strong>"Crash safety on Windows."</strong>" The server runs without a console window and is tied to Studio by a job object, so Windows ends it even if Studio crashes or is force-closed."</li>
                                <li><strong>"Logs."</strong>" The server's stderr goes to "<code>"eustress-lsp.log"</code>", and the launcher's latest status to "<code>"eustress-lsp-launcher.log"</code>", both in the system temp folder ("<code>"%TEMP%"</code>" on Windows)."</li>
                            </ul>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"A port file left behind"</strong>
                                    <p>
                                        "After a crash, the port file can outlive the server, since only a
                                        normal exit deletes it. Studio overwrites the file the next time it
                                        starts the server, and the VS Code extension abandons a dead port after
                                        1.5 seconds."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // DIAGNOSTICS
                    // =========================================================
                    <section id="diagnostics" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Diagnostics"
                        </h2>

                        <div id="diagnostics-passes" class="subsection">
                            <h3>"Three Passes"</h3>
                            <p>"Each analysis runs up to three passes over the text of one file:"</p>
                            <AnalysisDiagram />
                            <ol class="numbered-list">
                                <li><strong>"Parse."</strong>" Rune's own parser reads the file. A syntax error becomes a diagnostic, and each top-level function is recorded as a symbol for navigation."</li>
                                <li><strong>"Compile."</strong>" The file is compiled against Rune's standard modules plus the engine's Rune modules (the "<code>"eustress"</code>" module, "<code>"event_bus"</code>" and the realism laws), the same set Play mode compiles against. This catches unknown names and bad imports that parsing cannot."</li>
                                <li><strong>"Dry run."</strong>" If the first two passes found no errors, the analyzer calls each lifecycle function the script defines, once: "<code>"on_init"</code>", "<code>"on_ready"</code>", "<code>"on_update"</code>" and "<code>"on_tick"</code>" with a time step of 0.016 s, and "<code>"on_button_click"</code>" with the button name "<code>"TestButton"</code>". A failure is reported at the function's name, since it would fail the same way in Play. "<code>"on_exit"</code>" is never called."</li>
                            </ol>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"The dry run executes your code"</strong>
                                    <p>
                                        "The dry run calls your functions for real, in Studio and in every
                                        running "<code>"eustress-lsp"</code>", each time a file is analyzed: on
                                        every edit in an external editor, and for every script in the Universe
                                        when an editor connects. The HTTP functions in the Eustress API send
                                        real requests when called this way, so keep network calls out of "
                                        <code>"on_init"</code>" and "<code>"on_update"</code>", or guard them."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="diagnostics-sources" class="subsection">
                            <h3>"Sources and Timing"</h3>
                            <p>"Each diagnostic names the pass that produced it:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Source"</th><th>"Severity"</th><th>"Meaning"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"rune"</code></td><td>"Error"</td><td>"A syntax or compile error"</td></tr>
                                    <tr><td><code>"rune-warning"</code></td><td>"Warning"</td><td>"A compiler warning"</td></tr>
                                    <tr><td><code>"rune-link"</code></td><td>"Error"</td><td>"A link error, shown at the start of the file because the compiler gives it no location"</td></tr>
                                    <tr><td><code>"rune-runtime"</code></td><td>"Error"</td><td>"A lifecycle function failed during the dry run"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                <code>"eustress-lsp"</code>" analyzes a file when it opens, on every change and
                                on every save, and pushes the results with "
                                <code>"textDocument/publishDiagnostics"</code>". Document sync is Full: your
                                editor sends the whole file with each change, and the server analyzes it right
                                away."
                            </p>
                            <p>
                                "Inside Studio, the script editor waits until 80 ms after your last keystroke,
                                then feeds the same diagnostics to the squiggles, the Problems panel and the
                                Output panel."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // LANGUAGE FEATURES
                    // =========================================================
                    <section id="features" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Language Features"
                        </h2>

                        <div id="features-capabilities" class="subsection">
                            <h3>"Capabilities"</h3>
                            <p>
                                "The server's "<code>"initialize"</code>" response advertises these
                                capabilities and names the server "<code>"eustress-lsp"</code>", with the
                                engine's version:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Capability"</th><th>"Setting"</th><th>"What you get"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"textDocumentSync"</code></td><td>"Full"</td><td>"The editor sends the whole file on every change"</td></tr>
                                    <tr><td><code>"hoverProvider"</code></td><td>"On"</td><td>"API documentation, functions in the file, diagnostics"</td></tr>
                                    <tr><td><code>"completionProvider"</code></td><td>"Also opens on a period or a double quote"</td><td>"Keywords, functions, API names, simulation keys"</td></tr>
                                    <tr><td><code>"signatureHelpProvider"</code></td><td>"Opens on an opening parenthesis, advances on a comma"</td><td>"Parameter hints for Eustress API calls"</td></tr>
                                    <tr><td><code>"semanticTokensProvider"</code></td><td>"Whole document"</td><td>"Colors for keywords, API functions and types, your functions, strings, numbers and comments"</td></tr>
                                    <tr><td><code>"definitionProvider"</code></td><td>"On"</td><td>"The file, then the Universe, then an API page"</td></tr>
                                    <tr><td><code>"referencesProvider"</code></td><td>"On"</td><td>"Declarations that share the name"</td></tr>
                                    <tr><td><code>"documentSymbolProvider"</code></td><td>"On"</td><td>"The file's top-level functions"</td></tr>
                                    <tr><td><code>"renameProvider"</code></td><td>"On"</td><td>"Edits in the open file and in files that declare the name"</td></tr>
                                    <tr><td><code>"codeActionProvider"</code></td><td>"On"</td><td>"Insert missing semicolon"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Anything not in the table, such as formatting, workspace symbols or inlay
                                hints, is not implemented. After the handshake, the server logs "
                                <code>"eustress-lsp ready"</code>" to the editor."
                            </p>
                        </div>

                        <div id="features-navigation" class="subsection">
                            <h3>"Hover and Navigation"</h3>
                            <p>"Take a small script that reads and writes simulation values:"</p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::{get_sim_value, log_info, set_sim_value};

pub fn on_init() {
    log_info("cycle_life ready");
}

pub fn on_update(dt) {
    let soc = get_sim_value("battery.soc");
    set_sim_value("battery.soc_safe", clamp(soc, 0.0, 1.0));
}

fn clamp(value, lo, hi) {
    if value < lo { lo } else if value > hi { hi } else { value }
}"#}</code></pre>
                            </div>
                            <ul class="docs-list">
                                <li><strong>"Hover"</strong>" on "<code>"get_sim_value"</code>" shows its entry in the Eustress API catalog, which is built from the engine's own Rune module source: category, signature, description, and an example when there is one. On "<code>"clamp"</code>" it shows a function defined in this file and the line it starts on. On a name inside an error's range, it shows the error."</li>
                                <li><strong>"Go to definition"</strong>" on "<code>"clamp"</code>" jumps to it in this file. A name the file does not declare resolves to functions with that name in other "<code>".rune"</code>" files of the Universe. On "<code>"get_sim_value"</code>" it opens a generated Markdown page for the API entry, written to "<code>"eustress-lsp-api"</code>" in the temp folder and deleted when the server shuts down."</li>
                                <li><strong>"Find references"</strong>" on "<code>"clamp"</code>" lists the declarations named "<code>"clamp"</code>" in this file and across the Universe. Call sites are not listed yet."</li>
                                <li><strong>"Outline"</strong>" shows "<code>"on_init"</code>", "<code>"on_update"</code>" and "<code>"clamp"</code>"."</li>
                            </ul>
                        </div>

                        <div id="features-editing" class="subsection">
                            <h3>"Completion and Editing"</h3>
                            <ul class="docs-list">
                                <li><strong>"Completion"</strong>" lists Rune keywords first, then functions from the open file, then Eustress API functions and types with their signatures, up to 50 items."</li>
                                <li><strong>"Simulation keys."</strong>" Inside the quoted key of a "<code>"get_sim_value"</code>" or "<code>"set_sim_value"</code>" call, completion lists the keys in "<code>".eustress/runtime-snapshot.json"</code>", which Studio rewrites 4 times a second while it runs."</li>
                                <li><strong>"Parameter hints"</strong>" appear for Eustress API calls and highlight the parameter you are typing."</li>
                                <li><strong>"Rename"</strong>" replaces every occurrence of the name outside comments and strings, in the open file and in each other file that declares a function with that name. The new name must be a valid Rune identifier."</li>
                                <li><strong>"Quick fix."</strong>" When a diagnostic says a semicolon is expected, "<strong>"Insert missing semicolon"</strong>" adds it."</li>
                                <li><strong>"Suggestions."</strong>" Lines that call "<code>"get_sim_value"</code>", "<code>"set_sim_value"</code>", "<code>"http_request"</code>" or "<code>"datastore_get"</code>" also offer actions titled "<em>"Eustress: …"</em>". They run a command no editor implements yet, so choosing one leaves the file unchanged."</li>
                            </ul>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Rename matches names, not scripts"</strong>
                                    <p>
                                        "Each Rune script compiles on its own, yet a rename also edits every
                                        other script in the Universe that declares a function with the same
                                        name, including all uses of that name inside it. Before renaming a
                                        common name such as "<code>"clamp"</code>", review the other files the
                                        rename changed before you save them."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="features-index" class="subsection">
                            <h3>"The Universe Index"</h3>
                            <p>
                                "When an editor connects, the server takes the editor's workspace folder (or
                                its root path), walks up as many as 16 levels to the first folder that
                                contains "<code>"Spaces/"</code>", and indexes every "<code>".rune"</code>
                                " file below it, up to 12 folders deep. Folders whose names start with a dot, "
                                <code>"target"</code>" and "<code>"node_modules"</code>" are skipped."
                            </p>
                            <p>
                                "A file watcher with a 150 ms debounce keeps the index current when files
                                change on disk, including edits from other tools and a git checkout, and every
                                save re-indexes the saved file. With no Universe found, navigation stays
                                within the open file."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // EDITOR SETUP
                    // =========================================================
                    <section id="setup" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Editor Setup"
                        </h2>

                        <div id="setup-vscode" class="subsection">
                            <h3>"VS Code and Its Forks"</h3>
                            <p>
                                "Use the Eustress Rune LSP extension. It connects to the server Studio starts
                                and falls back to launching "<code>"eustress-lsp"</code>" itself. "
                                <a href="/learn/ide#extension">"IDE Integration"</a>" covers installing and
                                configuring it."
                            </p>
                        </div>

                        <div id="setup-neovim" class="subsection">
                            <h3>"Neovim"</h3>
                            <p>"Neovim 0.10 and newer can start the server from "<code>"init.lua"</code>" without plugins:"</p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"init.lua"</span>
                                </div>
                                <pre><code class="language-lua">{r#"-- Recognize .rune files, then start eustress-lsp for them.
vim.filetype.add({ extension = { rune = 'rune' } })

vim.api.nvim_create_autocmd('FileType', {
  pattern = 'rune',
  callback = function(args)
    vim.lsp.start({
      name = 'eustress-lsp',
      cmd = { 'eustress-lsp' },
      -- The Universe is the folder that contains Spaces/.
      root_dir = vim.fs.root(args.buf, 'Spaces'),
    })
  end,
})"#}</code></pre>
                            </div>
                        </div>

                        <div id="setup-helix" class="subsection">
                            <h3>"Helix"</h3>
                            <p>
                                "Add the server and a Rune language entry to "<code>"languages.toml"</code>"
                                in your Helix configuration folder:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"languages.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[language-server.eustress-lsp]
command = "eustress-lsp"

[[language]]
name = "rune"
scope = "source.rune"
file-types = ["rune"]
roots = ["Spaces"]
language-servers = ["eustress-lsp"]"#}</code></pre>
                            </div>
                            <p>
                                "In both editors, use the program's full path if its folder is not on "
                                <code>"PATH"</code>". The Windows installer does not add its folder to "
                                <code>"PATH"</code>"."
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

                        <div id="roadmap-symbols" class="subsection">
                            <h3>"Richer Symbols"</h3>
                            <p>
                                "The analyzer indexes functions today. Next it will record "<code>"use"</code>
                                " declarations, structs, enums, constants, "<code>"impl"</code>" blocks and
                                modules, so the outline, go to definition and rename cover them too, and a
                                period after a value will complete its members."
                            </p>
                        </div>

                        <div id="roadmap-packages" class="subsection">
                            <h3>"The Server in Every Package"</h3>
                            <p>
                                "The Windows zip, the macOS disk image and the Linux archive will ship "
                                <code>"eustress-lsp"</code>" beside the engine, as the Windows installer does,
                                so Studio starts the server on every platform without a source build."
                            </p>
                            <div class="future-cta">
                                <p><strong>"One analyzer. Every editor you like."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/scripting" class="btn-secondary-steel">"Scripting Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/learn/ide" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"IDE Integration"</span>
                            </div>
                        </a>
                        <a href="/docs/philosophy" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Philosophy"</span>
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
