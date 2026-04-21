// =============================================================================
// Eustress Web - MCP Server Documentation Page
// =============================================================================
// The eustress-mcp server surfaces an entire Universe — Spaces, scripts,
// entities, assets, conversations — to any MCP-compatible AI client (Claude
// Desktop, Cursor, Windsurf, and friends).
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
                TocSubsection { id: "how-install", title: "Install the Server" },
                TocSubsection { id: "how-config", title: "Configure Your AI Client" },
                TocSubsection { id: "how-universe", title: "Select a Universe" },
                TocSubsection { id: "how-verify", title: "Verify the Connection" },
            ],
        },
        TocSection {
            id: "api",
            title: "API Reference",
            subsections: vec![
                TocSubsection { id: "api-tools", title: "Tools" },
                TocSubsection { id: "api-resources", title: "Resources" },
                TocSubsection { id: "api-uri", title: "URI Scheme" },
                TocSubsection { id: "api-subscriptions", title: "Live Subscriptions" },
                TocSubsection { id: "api-env", title: "Environment Variables" },
            ],
        },
        TocSection {
            id: "use-cases",
            title: "Use Cases",
            subsections: vec![
                TocSubsection { id: "uc-review", title: "Universe-Wide Code Review" },
                TocSubsection { id: "uc-image", title: "Image-to-Entity Prompts" },
                TocSubsection { id: "uc-doc", title: "Scene Documentation" },
                TocSubsection { id: "uc-automation", title: "Automation via Claude" },
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
pub fn LearnMcpPage() -> impl IntoView {
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
                        <img src="/assets/icons/sparkles.svg" alt="MCP" class="toc-icon" />
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
                            "Expose your Universe to AI clients through the Model Context Protocol.
                            The " <code>"eustress-mcp"</code> " server maps Spaces, scripts, entities,
                            assets, and conversations to MCP tools and resources so Claude, Cursor,
                            Windsurf, and other agents can reason about your project end-to-end."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "15 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Intermediate"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "v0.3.0 (Rust)"
                            </span>
                        </div>
                    </header>

                    // ── Introduction ─────────────────────────────────────
                    <section id="intro" class="docs-section">
                        <h2 class="section-anchor">"Introduction"</h2>

                        <div id="intro-what" class="docs-block">
                            <h3>"What It Does"</h3>
                            <p>
                                "The Model Context Protocol is a transport-agnostic way for AI
                                assistants to read structured data and call tools on external systems.
                                " <code>"eustress-mcp"</code> " implements that protocol on top of an
                                Eustress Universe. Anything an engine user can click on — a Space, a
                                Rune script, a selected entity, an asset, a Workshop conversation — is
                                reachable by an AI client as either a tool invocation or a resource URI."
                            </p>
                        </div>

                        <div id="intro-why" class="docs-block">
                            <h3>"Why It's Useful"</h3>
                            <p>
                                "Without MCP, pasting context into Claude or Cursor is manual: copy
                                this script, paste that TOML, describe the scene hierarchy in English.
                                With MCP, the assistant reads the same canonical paths Workshop uses
                                (" <code>"@script:Space/Folder/Name"</code> "), walks folders,
                                searches by keyword, and subscribes to file changes so its answers stay
                                in sync with what you just edited."
                            </p>
                            <div class="docs-callout info">
                                <strong>"Workshop parity:"</strong>
                                " every MCP resource URI is a one-to-one map of a Workshop @mention. If
                                it works inside Eustress, the same reference works from Cursor."
                            </div>
                        </div>
                    </section>

                    // ── How to Use It ────────────────────────────────────
                    <section id="how-to" class="docs-section">
                        <h2 class="section-anchor">"How to Use It"</h2>

                        <div id="how-install" class="docs-block">
                            <h3>"Install the Server"</h3>
                            <p>
                                "The MCP server ships as a native Rust binary next to "
                                <code>"eustress-engine.exe"</code> " (~2.7 MB stripped, no Node.js
                                or bun runtime — just a single native executable). Installing
                                Eustress Engine puts it on disk automatically:"
                            </p>
                            <pre class="code-block"><code>{"# Bundled with the engine
C:/Program Files/Eustress Engine/eustress-mcp.exe      # Windows
/Applications/Eustress Engine.app/Contents/MacOS/eustress-mcp  # macOS
/opt/eustress-engine/eustress-mcp                      # Linux

# Or build from source
cd eustress
cargo build --release --bin eustress-mcp
# → eustress/target/release/eustress-mcp"}</code></pre>
                            <div class="docs-callout info">
                                <strong>"History:"</strong>
                                " v1 shipped as a TypeScript/bun-compiled binary on npm
                                (" <code>"@eustress/mcp-server"</code> "). That package is
                                deprecated as of v0.2.2 — the Rust rewrite is ~40× smaller,
                                starts ~40× faster, and ships with the engine."
                            </div>
                        </div>

                        <div id="how-config" class="docs-block">
                            <h3>"Configure Your AI Client"</h3>
                            <p>
                                "Each client has its own config file. Point " <code>"command"</code>
                                " at the bundled binary and set " <code>"EUSTRESS_UNIVERSES_PATH"</code>
                                " to one or more Universe roots separated by " <code>";"</code>
                                " on Windows, " <code>":"</code> " elsewhere. "
                                <code>"args"</code> " stays empty — the binary needs no CLI flags in
                                the normal case."
                            </p>
                            <pre class="code-block"><code>{"// Windsurf  — %APPDATA%/Codeium/windsurf/mcp_config.json
{
  \"mcpServers\": {
    \"eustress-engine\": {
      \"command\": \"C:/Program Files/Eustress Engine/eustress-mcp.exe\",
      \"args\": [],
      \"env\": {
        \"EUSTRESS_UNIVERSES_PATH\": \"C:/Users/you/Documents/Eustress\"
      }
    }
  }
}"}</code></pre>
                            <p>
                                "Eustress Engine's " <strong>"Help → Setup MCP"</strong>
                                " menu generates per-IDE snippets with the correct absolute path
                                pre-filled, so you don't have to look it up."
                            </p>
                        </div>

                        <div id="how-universe" class="docs-block">
                            <h3>"Select a Universe"</h3>
                            <p>"The server resolves the active Universe in this order:"</p>
                            <ol class="docs-list numbered">
                                <li><strong>"Explicit tool arg"</strong> " — e.g. " <code>"set_default_universe"</code> " sets it for the session"</li>
                                <li><strong>"Walk from resource/tool path"</strong> " — the first ancestor that contains " <code>"Spaces/"</code> " wins"</li>
                                <li><strong>"Session default"</strong> " — last one set via tool call"</li>
                                <li><strong>"Walk from process CWD"</strong> " — helpful when the client cd's into a Universe before launching"</li>
                                <li><strong>"EUSTRESS_UNIVERSES_PATH scan"</strong> " — if exactly one Universe is found, auto-select it"</li>
                            </ol>
                        </div>

                        <div id="how-verify" class="docs-block">
                            <h3>"Verify the Connection"</h3>
                            <p>
                                "Ask the assistant to run " <code>"list_spaces"</code> ". If you see a list
                                matching the folders under " <code>"{Universe}/Spaces"</code> ", the server
                                is wired correctly. If you see a single " <code>"eustress://help/setup"</code>
                                " resource, no Universe was resolved — set " <code>"EUSTRESS_UNIVERSE"</code>
                                " or call " <code>"set_default_universe"</code> "."
                            </p>
                        </div>
                    </section>

                    // ── API Reference ────────────────────────────────────
                    <section id="api" class="docs-section">
                        <h2 class="section-anchor">"API Reference"</h2>

                        <div id="api-tools" class="docs-block">
                            <h3>"Tools"</h3>
                            <p>"Thirteen tools grouped into discovery, editing, history, and configuration:"</p>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"list_spaces"</code>
                                    <span>"Enumerate every Space in the active Universe"</span>
                                </div>
                                <div class="api-row">
                                    <code>"list_scripts"</code>
                                    <span>"List all Rune scripts under a Space (recursively)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"read_script"</code>
                                    <span>"Read a script's source by canonical path"</span>
                                </div>
                                <div class="api-row">
                                    <code>"find_entity"</code>
                                    <span>"Locate an entity by name or path in a Space's TOML tree"</span>
                                </div>
                                <div class="api-row">
                                    <code>"list_assets"</code>
                                    <span>"Enumerate assets (models, textures, audio) with metadata"</span>
                                </div>
                                <div class="api-row">
                                    <code>"search_universe"</code>
                                    <span>"Substring search over scripts, entity TOML, and filenames"</span>
                                </div>
                                <div class="api-row">
                                    <code>"git_status"</code>
                                    <span>"Working-tree summary for the Universe's git repo"</span>
                                </div>
                                <div class="api-row">
                                    <code>"git_log"</code>
                                    <span>"Recent commits scoped to the Universe root"</span>
                                </div>
                                <div class="api-row">
                                    <code>"git_diff"</code>
                                    <span>"Unified diff of unstaged changes (capped at 64 KB)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"create_script"</code>
                                    <span>"Write a new Rune script at a canonical path"</span>
                                </div>
                                <div class="api-row">
                                    <code>"get_conversation"</code>
                                    <span>"Read a saved Workshop conversation JSON"</span>
                                </div>
                                <div class="api-row">
                                    <code>"list_universes"</code>
                                    <span>"Discovered Universes under EUSTRESS_UNIVERSES_PATH"</span>
                                </div>
                                <div class="api-row">
                                    <code>"set_default_universe"</code>
                                    <span>"Pin a Universe for the rest of the MCP session"</span>
                                </div>
                            </div>
                        </div>

                        <div id="api-resources" class="docs-block">
                            <h3>"Resources"</h3>
                            <p>"Six resource kinds, each addressable by URI and subscribable:"</p>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"eustress://space/{name}"</code>
                                    <span>"Top-level Space summary (manifest + entity counts)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"eustress://script/{space}/{+path}"</code>
                                    <span>"Rune script source (UTF-8 text)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"eustress://entity/{space}/{+path}"</code>
                                    <span>"Entity TOML with expanded properties"</span>
                                </div>
                                <div class="api-row">
                                    <code>"eustress://file/{space}/{+path}"</code>
                                    <span>"Any file under a Space — text or base64-encoded binary"</span>
                                </div>
                                <div class="api-row">
                                    <code>"eustress://conversation/{id}"</code>
                                    <span>"Saved Workshop conversation JSON"</span>
                                </div>
                                <div class="api-row">
                                    <code>"eustress://brief/{space}"</code>
                                    <span>"Auto-generated Space overview for quick AI onboarding"</span>
                                </div>
                            </div>
                        </div>

                        <div id="api-uri" class="docs-block">
                            <h3>"URI Scheme"</h3>
                            <p>
                                "The " <code>"{+path}"</code> " segment uses RFC 6570 reserved expansion —
                                slashes are allowed in the tail so nested folders round-trip cleanly.
                                The same canonical path format is what Workshop writes into the prompt
                                when you pick an @mention, so URIs are copy-pasteable between the engine
                                UI and your AI client."
                            </p>
                        </div>

                        <div id="api-subscriptions" class="docs-block">
                            <h3>"Live Subscriptions"</h3>
                            <p>"The server implements the MCP " <code>"resources/subscribe"</code> " flow:"</p>
                            <ul class="docs-list">
                                <li>"chokidar watches the active Universe root on first subscribe"</li>
                                <li>"Every change inside a subscribed URI fires " <code>"notifications/resources/updated"</code></li>
                                <li>"Swapping Universes retargets the watcher and drops stale subscriptions"</li>
                            </ul>
                        </div>

                        <div id="api-env" class="docs-block">
                            <h3>"Environment Variables"</h3>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"EUSTRESS_UNIVERSE"</code>
                                    <span>"Absolute path to a single Universe — forces it as the default"</span>
                                </div>
                                <div class="api-row">
                                    <code>"EUSTRESS_UNIVERSES_PATH"</code>
                                    <span>"Path-separator list of search roots; subfolders with a Spaces/ dir are discovered as Universes"</span>
                                </div>
                                <div class="api-row">
                                    <code>"EUSTRESS_MCP_LOG"</code>
                                    <span>"\"off\" | \"info\" | \"debug\" — server-side log verbosity"</span>
                                </div>
                            </div>
                        </div>
                    </section>

                    // ── Use Cases ────────────────────────────────────────
                    <section id="use-cases" class="docs-section">
                        <h2 class="section-anchor">"Use Cases"</h2>

                        <div id="uc-review" class="docs-block">
                            <h3>"Universe-Wide Code Review"</h3>
                            <p>
                                "Ask Claude to audit every Rune script for null-dereferences or unused
                                imports. It calls " <code>"list_scripts"</code> ", then "
                                <code>"read_script"</code> " on each, and returns a structured report.
                                No copy-pasting, no file-by-file hand-holding."
                            </p>
                        </div>

                        <div id="uc-image" class="docs-block">
                            <h3>"Image-to-Entity Prompts"</h3>
                            <p>
                                "Drop a reference image into a Workshop conversation, then ask Cursor
                                to generate matching entity TOML. The assistant reads the image through
                                the " <code>"eustress://file/..."</code> " resource and writes back via "
                                <code>"create_script"</code> " or the standard filesystem tools."
                            </p>
                        </div>

                        <div id="uc-doc" class="docs-block">
                            <h3>"Scene Documentation"</h3>
                            <p>
                                "Point the assistant at " <code>"eustress://brief/{space}"</code> " and
                                have it produce a human-readable README describing every entity,
                                behavior, and asset dependency. Great for onboarding collaborators."
                            </p>
                        </div>

                        <div id="uc-automation" class="docs-block">
                            <h3>"Automation via Claude"</h3>
                            <p>
                                "Scripted refactors across hundreds of entities — rename a component,
                                add a field, migrate a TOML schema — become one-shot prompts. The tools
                                are idempotent; the live subscription surface lets the assistant verify
                                its own edits landed."
                            </p>
                        </div>
                    </section>

                    // ── Conclusion ───────────────────────────────────────
                    <section id="conclusion" class="docs-section">
                        <h2 class="section-anchor">"Conclusion"</h2>
                        <p>
                            "The MCP server turns your Universe into a first-class knowledge source for
                            any modern AI client. Install once, wire up your config, and every prompt
                            has the full Space graph within reach. Because everything maps to Workshop's
                            @mention scheme, you can start in Eustress Engine, continue in Cursor, and end in
                            Claude Desktop without ever losing context."
                        </p>
                        <div class="docs-callout info">
                            <strong>"Related:"</strong>
                            " see the " <a href="/learn/ide">"IDE Integration"</a>
                            " guide for editor-native Rune intelligence, or the "
                            <a href="/learn/lsp">"Rune LSP"</a> " guide for the language server that backs it."
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
