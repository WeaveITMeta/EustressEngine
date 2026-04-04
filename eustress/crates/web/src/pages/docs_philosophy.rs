// =============================================================================
// Eustress Web - Philosophy Documentation Page
// =============================================================================
// The core philosophy of Eustress Engine: file-system-first, IDE agnostic,
// no vendor lock-in, your data is your data, and the power of vibe coding.
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// Table of Contents Data
// -----------------------------------------------------------------------------

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
            id: "manifesto",
            title: "Manifesto",
            subsections: vec![
                TocSubsection { id: "manifesto-vision", title: "Our Vision" },
                TocSubsection { id: "manifesto-principles", title: "Core Principles" },
                TocSubsection { id: "manifesto-different", title: "What Makes Us Different" },
            ],
        },
        TocSection {
            id: "filesystem",
            title: "File-System-First",
            subsections: vec![
                TocSubsection { id: "filesystem-why", title: "Why Files Matter" },
                TocSubsection { id: "filesystem-structure", title: "Project Structure" },
                TocSubsection { id: "filesystem-formats", title: "Open Formats" },
            ],
        },
        TocSection {
            id: "freedom",
            title: "Freedom & Ownership",
            subsections: vec![
                TocSubsection { id: "freedom-ide", title: "IDE Agnostic" },
                TocSubsection { id: "freedom-lockin", title: "No Vendor Lock-In" },
                TocSubsection { id: "freedom-data", title: "Your Data Is Your Data" },
            ],
        },
        TocSection {
            id: "performance",
            title: "Performance",
            subsections: vec![
                TocSubsection { id: "performance-rust", title: "100% Rust" },
                TocSubsection { id: "performance-scale", title: "Scale" },
                TocSubsection { id: "performance-benchmarks", title: "Benchmarks" },
            ],
        },
        TocSection {
            id: "vibe",
            title: "Vibe Coding",
            subsections: vec![
                TocSubsection { id: "vibe-what", title: "What Is Vibe Coding" },
                TocSubsection { id: "vibe-soul", title: "Soul Language" },
                TocSubsection { id: "vibe-ai", title: "AI Integration" },
            ],
        },
        TocSection {
            id: "community",
            title: "Community",
            subsections: vec![
                TocSubsection { id: "community-open", title: "Open Development" },
                TocSubsection { id: "community-contribute", title: "Contributing" },
                TocSubsection { id: "community-future", title: "The Future" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Philosophy documentation page.
#[component]
pub fn DocsPhilosophyPage() -> impl IntoView {
    let active_section = RwSignal::new("manifesto".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-philosophy"></div>
            </div>

            <div class="docs-layout">
                // Floating TOC Sidebar
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/brain.svg" alt="Philosophy" class="toc-icon" />
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

                // Main Content
                <main class="docs-content">
                    // Hero
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Philosophy"</span>
                        </div>
                        <h1 class="docs-title">"The Eustress Philosophy"</h1>
                        <p class="docs-subtitle">
                            "Your data is your data. Your tools are your choice. Your creativity 
                            is unlimited. We believe in freedom, performance, and the power of 
                            open standards."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "15 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "All Levels"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "v0.16.1"
                            </span>
                        </div>
                    </header>

                    // ─────────────────────────────────────────────────────
                    // Manifesto
                    // ─────────────────────────────────────────────────────
                    <section id="manifesto" class="docs-section">
                        <h2 class="section-anchor">"Manifesto"</h2>

                        <div id="manifesto-vision" class="docs-block">
                            <h3>"Our Vision"</h3>
                            <div class="manifesto-quote">
                                <blockquote>
                                    "We believe that creation should be as natural as thinking. 
                                    That your tools should amplify your vision, not constrain it. 
                                    That your work belongs to you, forever."
                                </blockquote>
                            </div>
                            <p>
                                "Eustress Engine was born from frustration with the status quo. 
                                Proprietary formats that lock you in. Bloated editors that slow you down. 
                                Licensing models that treat creators as renters, not owners."
                            </p>
                            <p>
                                "We built something different. Something that respects your time, 
                                your data, and your freedom. Something that scales from a single 
                                developer to a global team. Something that will still work in 50 years."
                            </p>
                        </div>

                        <div id="manifesto-principles" class="docs-block">
                            <h3>"Core Principles"</h3>
                            <div class="principles-grid">
                                <div class="principle-card">
                                    <div class="principle-number">"01"</div>
                                    <h4>"Files Over Databases"</h4>
                                    <p>
                                        "Plain text files are the universal interface. They work with 
                                        every tool, every version control system, every backup solution. 
                                        They will outlive any proprietary format."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"02"</div>
                                    <h4>"Standards Over Proprietary"</h4>
                                    <p>
                                        "We use glTF for 3D, TOML for config, PNG for images. Open 
                                        standards that anyone can read, write, and extend. No secret 
                                        sauce, no magic binaries."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"03"</div>
                                    <h4>"Performance Without Compromise"</h4>
                                    <p>
                                        "100% Rust means memory safety without garbage collection. 
                                        Fearless concurrency without data races. Native performance 
                                        on every platform."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"04"</div>
                                    <h4>"Simplicity Over Complexity"</h4>
                                    <p>
                                        "The best code is no code. The best feature is the one you 
                                        don't need. We ruthlessly eliminate complexity to focus on 
                                        what matters: your creation."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="manifesto-different" class="docs-block">
                            <h3>"What Makes Us Different"</h3>
                            <table class="docs-table comparison">
                                <thead>
                                    <tr>
                                        <th>"Aspect"</th>
                                        <th>"Others"</th>
                                        <th>"Eustress"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"Project Format"</td>
                                        <td>"Proprietary binary"</td>
                                        <td>"Plain folders + TOML"</td>
                                    </tr>
                                    <tr>
                                        <td>"Scene Format"</td>
                                        <td>"Custom binary"</td>
                                        <td>"glTF 2.0 (JSON)"</td>
                                    </tr>
                                    <tr>
                                        <td>"Asset Pipeline"</td>
                                        <td>"Import → Convert → Lock"</td>
                                        <td>"Use directly, cache derived"</td>
                                    </tr>
                                    <tr>
                                        <td>"Editor"</td>
                                        <td>"Required, proprietary"</td>
                                        <td>"Optional, any editor works"</td>
                                    </tr>
                                    <tr>
                                        <td>"Version Control"</td>
                                        <td>"Painful, needs LFS"</td>
                                        <td>"Native Git, text diffs"</td>
                                    </tr>
                                    <tr>
                                        <td>"Collaboration"</td>
                                        <td>"Cloud lock-in"</td>
                                        <td>"Any Git host"</td>
                                    </tr>
                                    <tr>
                                        <td>"Licensing"</td>
                                        <td>"Per-seat, royalties"</td>
                                        <td>"Open source core"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // File-System-First
                    // ─────────────────────────────────────────────────────
                    <section id="filesystem" class="docs-section">
                        <h2 class="section-anchor">"File-System-First"</h2>

                        <div id="filesystem-why" class="docs-block">
                            <h3>"Why Files Matter"</h3>
                            <p>
                                "Opening a folder "<strong>"is"</strong>" opening a project. No import 
                                wizards. No project files. No databases. Just a folder with your stuff."
                            </p>
                            <div class="docs-callout success">
                                <strong>"The Obsidian/VS Code Model:"</strong>
                                " Like Obsidian for notes or VS Code for code, Eustress treats your 
                                project folder as the source of truth. The engine reads files directly. 
                                What you see in your file explorer is what the engine sees."
                            </div>
                            <p>"This approach gives you:"</p>
                            <ul class="docs-list">
                                <li><strong>"Portability"</strong>" — Copy a folder, you've copied a project"</li>
                                <li><strong>"Transparency"</strong>" — See exactly what's in your project"</li>
                                <li><strong>"Tooling Freedom"</strong>" — Use any editor, any script, any tool"</li>
                                <li><strong>"Version Control"</strong>" — Git just works, no plugins needed"</li>
                                <li><strong>"Longevity"</strong>" — Files outlive software companies"</li>
                            </ul>
                        </div>

                        <div id="filesystem-structure" class="docs-block">
                            <h3>"Project Structure"</h3>
                            <pre class="code-block"><code>{"my-project/                    ← Open this folder = open project
├── .eustress/                  ← Engine metadata
│   ├── project.toml            ← Project settings (committed)
│   ├── settings.toml           ← Editor preferences (committed)
│   ├── cache/                  ← Derived assets (gitignored)
│   │   ├── textures/           ← GPU-optimized textures
│   │   ├── meshes/             ← Optimized geometry
│   │   └── scripts/            ← Compiled Rune bytecode
│   └── local/                  ← User-local state (gitignored)
│
├── Workspace/                  ← 3D scene content
│   ├── _service.toml           ← Service metadata
│   ├── Ground.part.toml        ← Part definition
│   ├── Building.model.toml     ← Model definition
│   └── assets/
│       └── building.glb        ← glTF model
│
├── SoulService/                ← Scripts
│   ├── _service.toml
│   └── main.soul               ← Soul script
│
├── StarterGui/                 ← UI definitions
│   ├── _service.toml
│   └── HUD/
│       ├── _instance.toml
│       └── Panel.frame.toml
│
├── assets/                     ← Raw assets
│   ├── textures/
│   ├── audio/
│   └── models/
│
└── .gitignore                  ← Standard Git ignore"}</code></pre>
                        </div>

                        <div id="filesystem-formats" class="docs-block">
                            <h3>"Open Formats"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Content"</th>
                                        <th>"Format"</th>
                                        <th>"Why"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"Configuration"</td>
                                        <td>"TOML"</td>
                                        <td>"Human-readable, Git-diffable"</td>
                                    </tr>
                                    <tr>
                                        <td>"3D Models"</td>
                                        <td>"glTF 2.0"</td>
                                        <td>"Industry standard, JSON scenes"</td>
                                    </tr>
                                    <tr>
                                        <td>"Textures"</td>
                                        <td>"PNG, JPEG, KTX2"</td>
                                        <td>"Universal support"</td>
                                    </tr>
                                    <tr>
                                        <td>"Audio"</td>
                                        <td>"OGG, WAV, FLAC"</td>
                                        <td>"Open codecs"</td>
                                    </tr>
                                    <tr>
                                        <td>"Scripts"</td>
                                        <td>".soul, .rune"</td>
                                        <td>"Plain text, any editor"</td>
                                    </tr>
                                    <tr>
                                        <td>"Data"</td>
                                        <td>"JSON, CSV"</td>
                                        <td>"Universal interchange"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Freedom & Ownership
                    // ─────────────────────────────────────────────────────
                    <section id="freedom" class="docs-section">
                        <h2 class="section-anchor">"Freedom & Ownership"</h2>

                        <div id="freedom-ide" class="docs-block">
                            <h3>"IDE Agnostic"</h3>
                            <p>
                                "Use whatever editor you love. Eustress doesn't care."
                            </p>
                            <div class="editor-grid">
                                <div class="editor-card">
                                    <h4>"VS Code"</h4>
                                    <p>"Full extension support, integrated terminal, Git"</p>
                                </div>
                                <div class="editor-card">
                                    <h4>"Neovim"</h4>
                                    <p>"LSP support, lightning fast, keyboard-driven"</p>
                                </div>
                                <div class="editor-card">
                                    <h4>"Zed"</h4>
                                    <p>"GPU-accelerated, collaborative, modern"</p>
                                </div>
                                <div class="editor-card">
                                    <h4>"Sublime Text"</h4>
                                    <p>"Fast, minimal, distraction-free"</p>
                                </div>
                                <div class="editor-card">
                                    <h4>"Eustress Studio"</h4>
                                    <p>"Integrated 3D viewport, visual editing"</p>
                                </div>
                                <div class="editor-card">
                                    <h4>"Notepad"</h4>
                                    <p>"Yes, even Notepad works. It's just text."</p>
                                </div>
                            </div>
                            <div class="docs-callout info">
                                <strong>"Hot Reload Everywhere:"</strong>
                                " Save a file in any editor, Eustress detects the change and 
                                hot-reloads automatically. No plugins required."
                            </div>
                        </div>

                        <div id="freedom-lockin" class="docs-block">
                            <h3>"No Vendor Lock-In"</h3>
                            <p>
                                "Your project is a folder. You can:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Copy it"</strong>" — Drag and drop, it's a folder"</li>
                                <li><strong>"Zip it"</strong>" — Standard archive, any tool"</li>
                                <li><strong>"Git it"</strong>" — Push to GitHub, GitLab, Bitbucket, your own server"</li>
                                <li><strong>"Sync it"</strong>" — Dropbox, OneDrive, Google Drive, rsync"</li>
                                <li><strong>"Back it up"</strong>" — Time Machine, Backblaze, tape drives"</li>
                                <li><strong>"Read it"</strong>" — In 50 years, TOML will still be readable"</li>
                            </ul>
                            <p>
                                "We will never hold your data hostage. If Eustress disappeared tomorrow, 
                                your projects would still be usable. glTF models open in Blender. TOML 
                                configs are human-readable. Your work is yours."
                            </p>
                        </div>

                        <div id="freedom-data" class="docs-block">
                            <h3>"Your Data Is Your Data"</h3>
                            <div class="data-ownership">
                                <div class="ownership-card">
                                    <h4>"🔒 No Cloud Required"</h4>
                                    <p>
                                        "Everything runs locally. No account needed. No internet required. 
                                        No telemetry by default. Your creations stay on your machine."
                                    </p>
                                </div>
                                <div class="ownership-card">
                                    <h4>"📤 Export Everything"</h4>
                                    <p>
                                        "Export to any format. glTF, FBX, OBJ for models. JSON, CSV for 
                                        data. No artificial limitations on getting your data out."
                                    </p>
                                </div>
                                <div class="ownership-card">
                                    <h4>"🔓 No DRM"</h4>
                                    <p>
                                        "Your builds are yours. Distribute them however you want. No 
                                        license checks, no online activation, no phone-home."
                                    </p>
                                </div>
                                <div class="ownership-card">
                                    <h4>"📜 Open Source Core"</h4>
                                    <p>
                                        "The engine core is open source. Fork it, modify it, learn from 
                                        it. Your investment in learning Eustress is never wasted."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Performance
                    // ─────────────────────────────────────────────────────
                    <section id="performance" class="docs-section">
                        <h2 class="section-anchor">"Performance"</h2>

                        <div id="performance-rust" class="docs-block">
                            <h3>"100% Rust"</h3>
                            <p>
                                "Eustress is written entirely in Rust. Not a wrapper around C++. 
                                Not a scripting layer on top of something else. Pure Rust, from 
                                the ground up."
                            </p>
                            <div class="rust-benefits">
                                <div class="benefit">
                                    <h4>"🦀 Memory Safety"</h4>
                                    <p>"No null pointers. No buffer overflows. No use-after-free. The compiler catches bugs before they ship."</p>
                                </div>
                                <div class="benefit">
                                    <h4>"⚡ Zero-Cost Abstractions"</h4>
                                    <p>"High-level code compiles to optimal machine code. No runtime overhead for safety."</p>
                                </div>
                                <div class="benefit">
                                    <h4>"🔄 Fearless Concurrency"</h4>
                                    <p>"Data races are compile-time errors. Multi-threading without fear."</p>
                                </div>
                                <div class="benefit">
                                    <h4>"🚫 No Garbage Collection"</h4>
                                    <p>"Predictable performance. No GC pauses. No frame drops from memory management."</p>
                                </div>
                            </div>
                        </div>

                        <div id="performance-scale" class="docs-block">
                            <h3>"Scale"</h3>
                            <p>
                                "Eustress scales from a single entity to millions:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Metric"</th>
                                        <th>"Capability"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"Entities"</td>
                                        <td>"Millions (ECS architecture)"</td>
                                    </tr>
                                    <tr>
                                        <td>"Physics Bodies"</td>
                                        <td>"100,000+ (Avian3D)"</td>
                                    </tr>
                                    <tr>
                                        <td>"Draw Calls"</td>
                                        <td>"Automatic batching"</td>
                                    </tr>
                                    <tr>
                                        <td>"Terrain"</td>
                                        <td>"Infinite (streaming chunks)"</td>
                                    </tr>
                                    <tr>
                                        <td>"Simulation Speed"</td>
                                        <td>"31M× real-time"</td>
                                    </tr>
                                    <tr>
                                        <td>"Hot Reload"</td>
                                        <td>"< 100ms"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="performance-benchmarks" class="docs-block">
                            <h3>"Benchmarks"</h3>
                            <div class="benchmark-cards">
                                <div class="benchmark-card">
                                    <div class="benchmark-value">"60+ FPS"</div>
                                    <div class="benchmark-label">"1M entities"</div>
                                    <div class="benchmark-detail">"On mid-range hardware"</div>
                                </div>
                                <div class="benchmark-card">
                                    <div class="benchmark-value">"< 50ms"</div>
                                    <div class="benchmark-label">"Startup time"</div>
                                    <div class="benchmark-detail">"Empty project"</div>
                                </div>
                                <div class="benchmark-card">
                                    <div class="benchmark-value">"< 100MB"</div>
                                    <div class="benchmark-label">"Memory baseline"</div>
                                    <div class="benchmark-detail">"Engine + editor"</div>
                                </div>
                                <div class="benchmark-card">
                                    <div class="benchmark-value">"< 50MB"</div>
                                    <div class="benchmark-label">"Binary size"</div>
                                    <div class="benchmark-detail">"Release build"</div>
                                </div>
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Vibe Coding
                    // ─────────────────────────────────────────────────────
                    <section id="vibe" class="docs-section">
                        <h2 class="section-anchor">"Vibe Coding"</h2>

                        <div id="vibe-what" class="docs-block">
                            <h3>"What Is Vibe Coding"</h3>
                            <p>
                                "Vibe coding is programming by intent. Instead of writing syntax, 
                                you describe what you want. The system figures out how to make it happen."
                            </p>
                            <div class="docs-callout success">
                                <strong>"The Future of Development:"</strong>
                                " Vibe coding isn't about replacing programmers — it's about 
                                amplifying them. Spend time on creative decisions, not boilerplate."
                            </div>
                            <p>
                                "With Eustress, vibe coding means:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Soul Language"</strong>" — Write behavior in plain English"</li>
                                <li><strong>"AI Assistance"</strong>" — Get suggestions, completions, explanations"</li>
                                <li><strong>"Hot Reload"</strong>" — See changes instantly, iterate rapidly"</li>
                                <li><strong>"Visual Feedback"</strong>" — Watch your words become reality"</li>
                            </ul>
                        </div>

                        <div id="vibe-soul" class="docs-block">
                            <h3>"Soul Language"</h3>
                            <p>
                                "Soul is our natural language scripting system. Write what you want, 
                                and Soul compiles it to efficient Rust code."
                            </p>
                            <pre class="code-block"><code>{"// player_controller.soul

When the player presses W, move them forward at 5 meters per second.
When the player presses Space and is on the ground, jump with force 8.
When the player touches a coin, add 10 points and destroy the coin.
Every 30 seconds, spawn a new enemy at a random spawn point.
When the player's health reaches zero, show the game over screen."}</code></pre>
                            <p>
                                "This compiles to type-safe Rust ECS code. No magic, no runtime 
                                interpretation — just efficient native code generated from your intent."
                            </p>
                        </div>

                        <div id="vibe-ai" class="docs-block">
                            <h3>"AI Integration"</h3>
                            <p>
                                "Eustress integrates with AI assistants to supercharge your workflow:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Code Generation"</strong>" — Describe a feature, get working code"</li>
                                <li><strong>"Bug Fixing"</strong>" — Paste an error, get a solution"</li>
                                <li><strong>"Documentation"</strong>" — Ask questions, get answers"</li>
                                <li><strong>"Asset Creation"</strong>" — Describe a model, generate it"</li>
                                <li><strong>"Testing"</strong>" — Describe a test case, generate the script"</li>
                            </ul>
                            <div class="docs-callout info">
                                <strong>"Works with Any AI:"</strong>
                                " Eustress's file-system-first approach means any AI assistant can 
                                read and modify your project. Claude, GPT, Gemini, local models — 
                                they all work because your project is just text files."
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Community
                    // ─────────────────────────────────────────────────────
                    <section id="community" class="docs-section">
                        <h2 class="section-anchor">"Community"</h2>

                        <div id="community-open" class="docs-block">
                            <h3>"Open Development"</h3>
                            <p>
                                "Eustress is developed in the open. Our roadmap is public. Our 
                                discussions are public. Our code is public."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"GitHub"</strong>" — Source code, issues, pull requests"</li>
                                <li><strong>"Discord"</strong>" — Real-time chat, help, showcase"</li>
                                <li><strong>"Forum"</strong>" — Long-form discussions, tutorials"</li>
                                <li><strong>"Blog"</strong>" — Development updates, deep dives"</li>
                            </ul>
                        </div>

                        <div id="community-contribute" class="docs-block">
                            <h3>"Contributing"</h3>
                            <p>
                                "We welcome contributions of all kinds:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Code"</strong>" — Bug fixes, features, optimizations"</li>
                                <li><strong>"Documentation"</strong>" — Tutorials, guides, translations"</li>
                                <li><strong>"Assets"</strong>" — Models, textures, sounds for the community"</li>
                                <li><strong>"Testing"</strong>" — Bug reports, feedback, benchmarks"</li>
                                <li><strong>"Teaching"</strong>" — Help others learn, answer questions"</li>
                            </ul>
                        </div>

                        <div id="community-future" class="docs-block">
                            <h3>"The Future"</h3>
                            <p>
                                "We're building Eustress for the long term. Our vision:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Universal Platform"</strong>" — Desktop, mobile, web, VR, AR"</li>
                                <li><strong>"Global Scale"</strong>" — Millions of concurrent users"</li>
                                <li><strong>"AI-Native"</strong>" — Deep integration with AI assistants"</li>
                                <li><strong>"Open Ecosystem"</strong>" — Plugins, assets, templates"</li>
                                <li><strong>"Education"</strong>" — Free for students and educators"</li>
                            </ul>
                            <div class="future-cta">
                                <p>
                                    <strong>"Join us in building the future of creation."</strong>
                                </p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary">"Download Eustress"</a>
                                    <a href="/community" class="btn-secondary">"Join the Community"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    // Navigation footer
                    <nav class="docs-nav-footer">
                        <a href="/docs/earning" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Earning"</span>
                            </div>
                        </a>
                        <a href="/learn" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Back to Learn"</span>
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
