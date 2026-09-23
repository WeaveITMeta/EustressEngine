// =============================================================================
// Eustress Web - IDE Integration Documentation Page
// =============================================================================
// IDE Integration: editing Eustress scripts in your own editor. Scripts are
// files Studio reloads on save, and the Eustress Rune LSP extension connects
// VS Code and editors built on it to the Rune language server Studio starts.
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
                TocSubsection { id: "overview-files", title: "Scripts Are Files" },
                TocSubsection { id: "overview-editors", title: "Choosing an Editor" },
            ],
        },
        TocSection {
            id: "disk",
            title: "Editing on Disk",
            subsections: vec![
                TocSubsection { id: "disk-open", title: "Open the Universe" },
                TocSubsection { id: "disk-reload", title: "Save and Reload" },
                TocSubsection { id: "disk-limits", title: "What the Watcher Skips" },
            ],
        },
        TocSection {
            id: "extension",
            title: "VS Code Extension",
            subsections: vec![
                TocSubsection { id: "extension-install", title: "Install" },
                TocSubsection { id: "extension-connect", title: "How It Finds Studio" },
                TocSubsection { id: "extension-status", title: "Status and Commands" },
                TocSubsection { id: "extension-standalone", title: "Without Studio" },
            ],
        },
        TocSection {
            id: "other",
            title: "Other Editors",
            subsections: vec![
                TocSubsection { id: "other-lsp", title: "Any LSP Client" },
                TocSubsection { id: "other-luau", title: "Luau Files" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-luau", title: "Luau in Your Editor" },
                TocSubsection { id: "roadmap-packages", title: "One Install Everywhere" },
            ],
        },
    ]
}

/// How an editor reaches the language server Studio starts: Studio launches
/// eustress-lsp, the server records its port inside the Universe, and the
/// extension reads that port and connects over loopback TCP.
#[component]
fn ConnectionDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 230" role="img"
                aria-label="Studio starts eustress-lsp. The server writes its port number to .eustress/lsp.port in the Universe folder. The editor extension reads that file and connects to the server over TCP on 127.0.0.1.">
                <defs>
                    <marker id="ide-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                // Top row: Studio starts the server, the server writes its port.
                <rect x="20" y="40" width="150" height="54" rx="8" class="dg-box"></rect>
                <text x="95" y="72" class="dg-label" text-anchor="middle">"Eustress Studio"</text>
                <rect x="245" y="40" width="150" height="54" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="320" y="72" class="dg-label" text-anchor="middle">"eustress-lsp"</text>
                <rect x="470" y="40" width="150" height="54" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="545" y="72" class="dg-label" text-anchor="middle">".eustress/lsp.port"</text>
                <line x1="170" y1="67" x2="243" y2="67" class="dg-line" marker-end="url(#ide-arrow)"></line>
                <text x="207" y="58" class="dg-note" text-anchor="middle">"starts"</text>
                <line x1="395" y1="67" x2="468" y2="67" class="dg-line" marker-end="url(#ide-arrow)"></line>
                <text x="432" y="58" class="dg-note" text-anchor="middle">"writes port"</text>

                // Bottom: the editor reads the port, then talks to the server.
                <rect x="245" y="160" width="150" height="54" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="320" y="192" class="dg-label" text-anchor="middle">"Your editor"</text>
                <line x1="530" y1="94" x2="397" y2="170" class="dg-line dg-line-dashed" marker-end="url(#ide-arrow)"></line>
                <text x="470" y="152" class="dg-note">"reads port"</text>
                <line x1="320" y1="158" x2="320" y2="96" class="dg-line dg-line-accent" marker-start="url(#ide-arrow)" marker-end="url(#ide-arrow)"></line>
                <text x="310" y="131" class="dg-note" text-anchor="end">"LSP over TCP, 127.0.0.1"</text>
            </svg>
            <figcaption>
                "Studio starts the server and records its port inside the Universe. The extension
                reads the port from that file and connects on the loopback address, so the server
                only accepts connections from your own machine."
            </figcaption>
        </figure>
    }
}

/// IDE Integration documentation page.
#[component]
pub fn LearnIdePage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-ide"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/edit.svg" alt="IDE Integration" class="toc-icon" />
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
                            "IDE integration means editing Eustress scripts in the code editor you already
                            use. Scripts are plain files in the Space folder, Studio reloads Rune scripts
                            when you save, and the Eustress Rune LSP extension connects VS Code and editors
                            built on it to the Rune language server that Studio starts."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "8 min read"
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

                        <div id="overview-files" class="subsection">
                            <h3>"Scripts Are Files"</h3>
                            <p>
                                "A script in Eustress is a text file inside a Space folder: "
                                <code>".rune"</code>" for Rune, "<code>".luau"</code>" (or "
                                <code>".lua"</code>") for Luau. Studio keeps each script as a file in the
                                Space folder and watches that folder while the Space is open, so you can edit
                                scripts in any editor that saves text files."
                            </p>
                            <p>
                                "Inserting a "<strong>"Script"</strong>" from the Studio ribbon writes a
                                folder of three files. With nothing selected in the Explorer, the folder goes
                                into the Space's "<code>"SoulService"</code>" folder; otherwise it goes into
                                the selected item's folder on disk."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Universe folder"</span>
                                </div>
                                <pre><code class="language-text">{r#"MyUniverse/
  .eustress/
    lsp.port                 port of the language server Studio started
    runtime-snapshot.json    simulation values, rewritten 4 times a second
  Spaces/
    MySpace/
      SoulService/
        SoulScript/
          _instance.toml     says what the folder is
          SoulScript.rune    the code you edit
          SoulScript.md      a short summary"#}</code></pre>
                            </div>
                            <p>
                                "The "<code>"_instance.toml"</code>" names the class and points at the source
                                file. The "<strong>"LocalScript"</strong>" and "<strong>"ModuleScript"</strong>
                                " inserts write "<code>"LuauLocalScript"</code>" and "
                                <code>"LuauModuleScript"</code>" folders with a "<code>".luau"</code>
                                " source instead."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"SoulScript/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[metadata]
class_name = "SoulScript"
archivable = true

[script]
source = "SoulScript.rune""#}</code></pre>
                            </div>
                        </div>

                        <div id="overview-editors" class="subsection">
                            <h3>"Choosing an Editor"</h3>
                            <p>
                                "Every option below edits the same files. They differ in how much the editor
                                understands about Rune:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Editor"</th><th>"Rune support"</th><th>"Setup"</th></tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"VS Code, Cursor, Windsurf"</td>
                                        <td>"Highlighting, diagnostics, hover, completion, go to definition, rename"</td>
                                        <td>"Install the Eustress Rune LSP extension"</td>
                                    </tr>
                                    <tr>
                                        <td>"Neovim, Helix, other LSP clients"</td>
                                        <td>"The same language features, from the same server"</td>
                                        <td>"Point the client at "<code>"eustress-lsp"</code></td>
                                    </tr>
                                    <tr>
                                        <td>"Studio's script editor"</td>
                                        <td>"Squiggles and the Problems panel, from the same analyzer"</td>
                                        <td>"Built in"</td>
                                    </tr>
                                    <tr>
                                        <td>"Any other text editor"</td>
                                        <td>"Plain editing; Studio still reloads Rune scripts on save"</td>
                                        <td>"None"</td>
                                    </tr>
                                </tbody>
                            </table>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"One analyzer everywhere"</strong>
                                    <p>
                                        "Studio's squiggles, its Problems panel and the external language server
                                        all run the same Rune analyzer, so a script reports the same errors, in
                                        the same words, in every editor."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // EDITING ON DISK
                    // =========================================================
                    <section id="disk" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Editing on Disk"
                        </h2>

                        <div id="disk-open" class="subsection">
                            <h3>"Open the Universe"</h3>
                            <p>
                                "Open the Universe folder, the one that contains "<code>"Spaces/"</code>
                                ", as your editor's workspace. The language server walks up from the
                                workspace to that folder and indexes every "<code>".rune"</code>" file below
                                it, which is what lets go to definition and rename reach across scripts."
                            </p>
                            <p>
                                "Opening a single Space folder works as well, because the server walks up
                                from there to the Universe. A folder outside any Universe limits those
                                features to the file you have open."
                            </p>
                        </div>

                        <div id="disk-reload" class="subsection">
                            <h3>"Save and Reload"</h3>
                            <p>
                                "Studio watches the folder of the Space that is open. After a save, the
                                watcher waits 300 ms for the file system to settle, then until 350 ms pass
                                with no new changes (1.2 s at most), and applies the whole batch at once, so
                                a single save reaches Studio in under a second. Editors that save by writing a
                                temporary file and renaming it over the original are treated as an ordinary
                                edit."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"When you save a Rune script"</th><th>"What Studio does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"During Play"</td>
                                        <td>"Recompiles the script right away and runs its "<code>"on_init"</code>" again against the new code. Compile errors appear in the Output panel."</td>
                                    </tr>
                                    <tr>
                                        <td>"While editing"</td>
                                        <td>"Keeps the new text and compiles it when you press Play."</td>
                                    </tr>
                                </tbody>
                            </table>
                            <p>"Live reload covers Rune scripts only in this build."</p>
                            <p>"To see the loop work:"</p>
                            <ol class="numbered-list">
                                <li>"Open a Space that has a Rune script and press "<strong>"Play"</strong>"."</li>
                                <li>"In your editor, delete a closing brace from the script and save."</li>
                                <li>"The Output panel shows the compile error, tagged "<code>"rune"</code>"."</li>
                                <li>"Put the brace back and save. The script recompiles and its "<code>"on_init"</code>" runs again."</li>
                            </ol>
                        </div>

                        <div id="disk-limits" class="subsection">
                            <h3>"What the Watcher Skips"</h3>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Saves in the first 5 seconds are ignored"</strong>
                                    <p>
                                        "For 5 seconds after a Space opens, the watcher ignores changes to
                                        files that already exist, because the operating system reports a burst
                                        of spurious edits while the watch starts. If you saved in that window,
                                        save again."
                                    </p>
                                </div>
                            </div>
                            <ul class="docs-list">
                                <li><strong>"Other Spaces."</strong>" Studio watches the Space that is open, not the whole Universe."</li>
                                <li><strong>"Hidden folders."</strong>" Anything inside a folder whose name starts with a dot, such as "<code>".eustress"</code>", is ignored."</li>
                                <li><strong>"Studio's own writes."</strong>" A file Studio wrote in the last 2 seconds is not reloaded, so its saves never loop back as edits."</li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // VS CODE EXTENSION
                    // =========================================================
                    <section id="extension" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "VS Code Extension"
                        </h2>

                        <div id="extension-install" class="subsection">
                            <h3>"Install"</h3>
                            <p>
                                "The extension is "<strong>"Eustress Rune LSP"</strong>", published on the
                                Open VSX registry as "<code>"WeaveITMeta.rune-lsp"</code>" (version 0.3.6). It
                                needs VS Code 1.85 or newer, or an editor built on it."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Editors that install from Open VSX"</strong>": search the Extensions view for Eustress Rune LSP."</li>
                                <li><strong>"VS Code"</strong>": Microsoft's marketplace does not list the extension. Download "<code>"WeaveITMeta.rune-lsp-0.3.6.vsix"</code>" from "<a href="https://open-vsx.org/extension/WeaveITMeta/rune-lsp">"its Open VSX page"</a>", then run "<strong>"Extensions: Install from VSIX"</strong>" from the Command Palette, or install it from a terminal."</li>
                            </ul>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"code --install-extension WeaveITMeta.rune-lsp-0.3.6.vsix"#}</code></pre>
                            </div>
                            <p>
                                "Besides the server connection, the extension registers a "<code>"rune"</code>
                                " language for "<code>".rune"</code>" files: a TextMate grammar that colors
                                Rune before the server connects, "<code>"//"</code>" and "<code>"/* */"</code>
                                " comments, bracket matching and auto-closing, and folding between "
                                <code>"// #region"</code>" and "<code>"// #endregion"</code>" markers."
                            </p>
                            <p>
                                "Its source lives in the Eustress repository under "
                                <code>"infrastructure/extensions/lsp/vscode"</code>", and "
                                <code>"infrastructure/extensions/lsp/scripts/build-vsix.sh"</code>
                                " packages a "<code>".vsix"</code>" of your own."
                            </p>
                        </div>

                        <div id="extension-connect" class="subsection">
                            <h3>"How It Finds Studio"</h3>
                            <ol class="numbered-list">
                                <li>"When a Space is open, Studio starts "<code>"eustress-lsp"</code>" in TCP mode, and the server writes its port number to "<code>".eustress/lsp.port"</code>" in the Universe folder."</li>
                                <li>"The extension activates in any workspace that contains a "<code>".rune"</code>" file or a "<code>".eustress/lsp.port"</code>" file."</li>
                                <li>"When you open a "<code>".rune"</code>" file, it walks up to the Universe folder, reads the port file and connects to "<code>"127.0.0.1"</code>" on that port. A connection attempt is abandoned after 1.5 seconds, so a stale port file never hangs the editor."</li>
                                <li>"Each Universe gets its own connection, limited to its own "<code>".rune"</code>" files, so one window can edit scripts from Universes open in two copies of Studio."</li>
                            </ol>
                            <ConnectionDiagram />
                            <div class="callout callout-tip">
                                <img src="/assets/icons/sparkles.svg" alt="Tip" />
                                <div>
                                    <strong>"After Studio replaces its server"</strong>
                                    <p>
                                        "Studio runs one server per Universe and replaces it when you open a
                                        Space in a different Universe. Run "
                                        <strong>"Eustress: Restart Rune Language Server"</strong>
                                        " to reconnect."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="extension-status" class="subsection">
                            <h3>"Status and Commands"</h3>
                            <p>"A status bar item on the right shows the connection:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Status bar"</th><th>"Meaning"</th><th>"Click to"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"Rune LSP"</code>" with a check mark"</td><td>"Connected"</td><td>"Restart the connections"</td></tr>
                                    <tr><td><code>"Rune LSP: starting…"</code></td><td>"Connecting to Studio or launching a server"</td><td>"Show the server path"</td></tr>
                                    <tr><td><code>"Rune LSP: engine not running"</code></td><td>"No port file, and no standalone server found"</td><td>"Open setup help"</td></tr>
                                    <tr><td><code>"Rune LSP: error"</code></td><td>"The connection or the server failed"</td><td>"Open the output"</td></tr>
                                </tbody>
                            </table>
                            <p>"The Command Palette has four commands:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Command"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><strong>"Eustress: Restart Rune Language Server"</strong></td><td>"Stops every connection, then reconnects each open Rune file"</td></tr>
                                    <tr><td><strong>"Eustress: Set up Rune Language Server…"</strong></td><td>"Offers the Eustress download page, this page, or the server path setting"</td></tr>
                                    <tr><td><strong>"Eustress: Show resolved server path"</strong></td><td>"Shows the standalone server the extension would launch, and the live connections"</td></tr>
                                    <tr><td><strong>"Eustress: Show Rune Language Server output"</strong></td><td>"Opens the output of the first connection; each Universe has its own channel"</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="extension-standalone" class="subsection">
                            <h3>"Without Studio"</h3>
                            <p>
                                "With no port file to follow, the extension launches a server of its own over
                                stdio, using the first of these that exists:"
                            </p>
                            <ol class="numbered-list">
                                <li>"The "<code>"eustress.serverPath"</code>" setting"</li>
                                <li>"The "<code>"EUSTRESS_LSP_PATH"</code>" environment variable"</li>
                                <li><code>"target/release/eustress-lsp"</code>", then "<code>"target/debug/eustress-lsp"</code>", in the first workspace folder (a source build of Eustress)"</li>
                                <li>"A server packaged inside the extension, if the package has one"</li>
                                <li><code>"eustress-lsp"</code>" on your "<code>"PATH"</code></li>
                            </ol>
                            <p>
                                "If "<code>"eustress.serverPath"</code>" names a file that does not exist, the
                                search stops there and the server is reported missing. When nothing is found,
                                the status bar shows "<code>"Rune LSP: engine not running"</code>" and a
                                prompt offers the download page."
                            </p>
                            <p>
                                "The Windows installer puts "<code>"eustress-lsp.exe"</code>" beside the
                                engine, in "<code>"C:\\Program Files\\Eustress Engine"</code>" by default, but
                                does not add that folder to "<code>"PATH"</code>". Point the setting at it:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"settings.json"</span>
                                </div>
                                <pre><code class="language-json">{r#"{
  "eustress.serverPath": "C:\\Program Files\\Eustress Engine\\eustress-lsp.exe"
}"#}</code></pre>
                            </div>
                            <p>
                                "A standalone server runs the same code as the one Studio starts, so every
                                language feature is the same."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // OTHER EDITORS
                    // =========================================================
                    <section id="other" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Other Editors"
                        </h2>

                        <div id="other-lsp" class="subsection">
                            <h3>"Any LSP Client"</h3>
                            <p>
                                "Editors with a built-in LSP client, such as Neovim and Helix, need no
                                extension. Tell the client three things:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Setting"</th><th>"Value"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Command"</td><td><code>"eustress-lsp"</code>", or its full path if its folder is not on "<code>"PATH"</code></td></tr>
                                    <tr><td>"Files"</td><td><code>".rune"</code>", as a language named "<code>"rune"</code></td></tr>
                                    <tr><td>"Root marker"</td><td><code>"Spaces"</code>", the folder that makes a directory a Universe"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The server talks over stdio unless you pass "<code>"--tcp"</code>". The "
                                <a href="/learn/lsp#setup">"Rune LSP"</a>" page has ready-to-paste Neovim and
                                Helix configurations, and the flags for sharing one server over TCP."
                            </p>
                        </div>

                        <div id="other-luau" class="subsection">
                            <h3>"Luau Files"</h3>
                            <p>
                                "Language features for Luau are not part of Eustress yet: "
                                <code>"eustress-lsp"</code>" analyzes Rune only, and Studio writes no Luau
                                type definitions or "<code>".luaurc"</code>" for other tools to read. Editor
                                support for "<code>".luau"</code>" files comes from whatever Luau tooling you
                                install, without knowledge of the Eustress API."
                            </p>
                            <p>
                                "The "<a href="/docs/scripting">"Scripting"</a>" page covers both languages
                                and the API they share."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // WHAT'S NEXT
                    // =========================================================
                    <section id="roadmap" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "What's Next"
                        </h2>

                        <div id="roadmap-luau" class="subsection">
                            <h3>"Luau in Your Editor"</h3>
                            <p>
                                "Luau will catch up with Rune on two fronts: a type-definition file for the
                                engine's Luau API, so standard Luau language servers can check Eustress
                                scripts, and live reload during Play, so a saved "<code>".luau"</code>" file
                                takes effect the way a saved "<code>".rune"</code>" file does."
                            </p>
                        </div>

                        <div id="roadmap-packages" class="subsection">
                            <h3>"One Install Everywhere"</h3>
                            <p>
                                "Today only the Windows installer places "<code>"eustress-lsp"</code>" beside
                                the engine; the Windows zip, the macOS disk image and the Linux archive do not
                                include it yet. Every package will include the server, so Studio starts it
                                on any platform without a source build, and the extension will be listed on
                                Microsoft's marketplace as well as Open VSX."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Write scripts in the editor you know. Press Play in Studio."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/learn/lsp" class="btn-secondary-steel">"Rune LSP Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/learn/cli" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"CLI & Headless"</span>
                            </div>
                        </a>
                        <a href="/learn/lsp" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Rune LSP"</span>
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
