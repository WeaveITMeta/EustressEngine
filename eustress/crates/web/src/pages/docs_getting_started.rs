// =============================================================================
// Eustress Web - Getting Started Documentation Page
// =============================================================================
// First page in the documentation — covers installation, first project,
// file system, scripting basics, identity setup, and next steps.
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
            id: "welcome",
            title: "Welcome",
            subsections: vec![
                TocSubsection { id: "welcome-what", title: "What is Eustress" },
                TocSubsection { id: "welcome-who", title: "Who It's For" },
                TocSubsection { id: "welcome-philosophy", title: "Philosophy" },
            ],
        },
        TocSection {
            id: "installation",
            title: "Installation",
            subsections: vec![
                TocSubsection { id: "installation-requirements", title: "System Requirements" },
                TocSubsection { id: "installation-download", title: "Download" },
                TocSubsection { id: "installation-verify", title: "Verify Installation" },
            ],
        },
        TocSection {
            id: "first-project",
            title: "Your First Project",
            subsections: vec![
                TocSubsection { id: "first-project-launch", title: "Launch the Engine" },
                TocSubsection { id: "first-project-universe", title: "Create a Universe" },
                TocSubsection { id: "first-project-space", title: "Create a Space" },
                TocSubsection { id: "first-project-primitives", title: "Add Primitives" },
            ],
        },
        TocSection {
            id: "filesystem",
            title: "File System",
            subsections: vec![
                TocSubsection { id: "filesystem-structure", title: "Project Structure" },
                TocSubsection { id: "filesystem-config", title: "Configuration" },
                TocSubsection { id: "filesystem-principles", title: "Design Principles" },
            ],
        },
        TocSection {
            id: "scripting",
            title: "Scripting Basics",
            subsections: vec![
                TocSubsection { id: "scripting-soul", title: "Soul (Natural Language)" },
                TocSubsection { id: "scripting-rune", title: "Rune" },
                TocSubsection { id: "scripting-luau", title: "Luau" },
            ],
        },
        TocSection {
            id: "identity",
            title: "Identity Setup",
            subsections: vec![
                TocSubsection { id: "identity-create", title: "Create Identity" },
                TocSubsection { id: "identity-link", title: "Link to Engine" },
            ],
        },
        TocSection {
            id: "next-steps",
            title: "Next Steps",
            subsections: vec![
                TocSubsection { id: "next-steps-learn", title: "Keep Learning" },
                TocSubsection { id: "next-steps-community", title: "Community" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Getting Started documentation page with floating TOC.
#[component]
pub fn DocsGettingStartedPage() -> impl IntoView {
    let active_section = RwSignal::new("welcome".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-getting-started"></div>
            </div>

            <div class="docs-layout">
                // Floating TOC Sidebar
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/rocket.svg" alt="Getting Started" class="toc-icon" />
                        <h2>"Getting Started"</h2>
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
                            <span class="current">"Getting Started"</span>
                        </div>
                        <h1 class="docs-title">"Getting Started"</h1>
                        <p class="docs-subtitle">
                            "Everything you need to go from zero to your first running project.
                            Install the engine, create a Universe, add objects, and write your first script."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "15 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Beginner"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/calendar.svg" alt="Updated" />
                                "Updated Apr 2026"
                            </span>
                        </div>
                    </header>

                    // =========================================================
                    // WELCOME SECTION
                    // =========================================================
                    <section id="welcome" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"01"</span>
                            "Welcome"
                        </h2>

                        <div id="welcome-what" class="subsection">
                            <h3>"What is Eustress Engine"</h3>
                            <p>
                                "Eustress Engine is an open, deterministic creation engine built on Rust and Bevy ECS.
                                It combines the performance of native code with the accessibility of natural-language
                                scripting, giving you a platform for building games, simulations, educational experiences,
                                and interactive art — all from the same set of tools."
                            </p>
                            <p>
                                "Unlike traditional engines that lock you into proprietary formats and closed ecosystems,
                                Eustress stores everything as plain files. Your projects are yours — version them with Git,
                                edit them with any text editor, and share them without friction."
                            </p>

                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/rocket.svg" alt="Performance" />
                                    </div>
                                    <h4>"Native Performance"</h4>
                                    <p>"Built on Rust and Bevy ECS — no garbage collector, no overhead"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/folder.svg" alt="Open Files" />
                                    </div>
                                    <h4>"Open File Formats"</h4>
                                    <p>"TOML configs, plain-text scripts, standard asset formats"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/brain.svg" alt="AI" />
                                    </div>
                                    <h4>"AI-Native Scripting"</h4>
                                    <p>"Soul language lets you describe behavior in natural language"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/network.svg" alt="Universe" />
                                    </div>
                                    <h4>"Universe Model"</h4>
                                    <p>"Organize worlds into Universes and Spaces with seamless switching"</p>
                                </div>
                            </div>
                        </div>

                        <div id="welcome-who" class="subsection">
                            <h3>"Who It's For"</h3>
                            <p>
                                "Eustress is designed for a broad range of creators:"
                            </p>

                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/edit.svg" alt="Creators" />
                                    </div>
                                    <h4>"Creators"</h4>
                                    <p>"Artists and designers who want to build interactive worlds without deep programming knowledge"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/code.svg" alt="Developers" />
                                    </div>
                                    <h4>"Developers"</h4>
                                    <p>"Programmers who want low-level control with Rust, Rune, or Luau scripting"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/book.svg" alt="Educators" />
                                    </div>
                                    <h4>"Educators"</h4>
                                    <p>"Teachers building interactive lessons, physics demos, and guided experiences"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/star.svg" alt="Students" />
                                    </div>
                                    <h4>"Students"</h4>
                                    <p>"Learners exploring game development, simulation, and creative coding"</p>
                                </div>
                            </div>
                        </div>

                        <div id="welcome-philosophy" class="subsection">
                            <h3>"Philosophy"</h3>
                            <p>
                                "Eustress is built around three principles:"
                            </p>
                            <ul class="feature-list">
                                <li>
                                    <strong>"Everything is a file"</strong>
                                    " — no proprietary binary blobs, no opaque databases. Projects are directories of human-readable files."
                                </li>
                                <li>
                                    <strong>"Determinism first"</strong>
                                    " — given the same inputs, the engine produces the same outputs. Debugging is tractable, replays are exact."
                                </li>
                                <li>
                                    <strong>"Progressive complexity"</strong>
                                    " — start with Soul (natural language), graduate to Luau or Rune, drop into Rust when you need maximum control."
                                </li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // INSTALLATION SECTION
                    // =========================================================
                    <section id="installation" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Installation"
                        </h2>

                        <div id="installation-requirements" class="subsection">
                            <h3>"System Requirements"</h3>
                            <p>
                                "Before installing, make sure your system meets the minimum requirements:"
                            </p>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Component"</th>
                                            <th>"Minimum"</th>
                                            <th>"Recommended"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td>"GPU"</td>
                                            <td>"Vulkan 1.1 compatible"</td>
                                            <td>"Vulkan 1.3 with compute shaders"</td>
                                        </tr>
                                        <tr>
                                            <td>"RAM"</td>
                                            <td>"8 GB"</td>
                                            <td>"16 GB"</td>
                                        </tr>
                                        <tr>
                                            <td>"Disk Space"</td>
                                            <td>"2 GB"</td>
                                            <td>"10 GB (with assets)"</td>
                                        </tr>
                                        <tr>
                                            <td>"OS"</td>
                                            <td>"Windows 10, macOS 12, Ubuntu 22.04"</td>
                                            <td>"Latest stable release"</td>
                                        </tr>
                                        <tr>
                                            <td>"CPU"</td>
                                            <td>"4 cores, x86_64 or ARM64"</td>
                                            <td>"8+ cores"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Vulkan Required"</strong>
                                    <p>"Eustress uses WGPU with a Vulkan backend. Integrated GPUs from 2018+
                                    generally support Vulkan 1.1. On macOS, MoltenVK provides Vulkan over Metal."</p>
                                </div>
                            </div>
                        </div>

                        <div id="installation-download" class="subsection">
                            <h3>"Download"</h3>
                            <p>
                                "Download the latest release for your platform from "
                                <a href="https://eustress.dev/download">"eustress.dev/download"</a>
                                ":"
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Windows (PowerShell)"</span>
                                </div>
                                <pre><code class="language-powershell">{r#"# Download and run the installer
winget install Eustress.Engine

# Or download the MSI directly
Invoke-WebRequest -Uri https://eustress.dev/dl/eustress-latest-x64.msi -OutFile eustress.msi
Start-Process msiexec -ArgumentList '/i eustress.msi' -Wait"#}</code></pre>
                            </div>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"macOS (Terminal)"</span>
                                </div>
                                <pre><code class="language-bash">{r#"# Homebrew
brew install eustress-engine

# Or download the .dmg
curl -LO https://eustress.dev/dl/eustress-latest-universal.dmg
open eustress-latest-universal.dmg"#}</code></pre>
                            </div>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Linux (Terminal)"</span>
                                </div>
                                <pre><code class="language-bash">{r#"# Flatpak (recommended)
flatpak install flathub dev.eustress.Engine

# Debian / Ubuntu
curl -fsSL https://eustress.dev/gpg | sudo gpg --dearmor -o /usr/share/keyrings/eustress.gpg
echo "deb [signed-by=/usr/share/keyrings/eustress.gpg] https://apt.eustress.dev stable main" \
  | sudo tee /etc/apt/sources.list.d/eustress.list
sudo apt update && sudo apt install eustress-engine

# Arch Linux (AUR)
paru -S eustress-engine"#}</code></pre>
                            </div>
                        </div>

                        <div id="installation-verify" class="subsection">
                            <h3>"Verify Installation"</h3>
                            <p>
                                "After installing, verify everything is working:"
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"# Check the version
eustress --version
# Eustress Engine 0.1.0 (rustc 1.85.0, bevy 0.15)

# Run the GPU diagnostics
eustress doctor
# ✓ Vulkan 1.3 — NVIDIA GeForce RTX 4070
# ✓ Compute shaders supported
# ✓ 16 GB system RAM
# ✓ Write access to ~/.eustress/
# All checks passed."#}</code></pre>
                            </div>

                            <div class="callout callout-tip">
                                <img src="/assets/icons/sparkles.svg" alt="Tip" />
                                <div>
                                    <strong>"Troubleshooting"</strong>
                                    <p>"If "<code>"eustress doctor"</code>" reports a Vulkan error, update your GPU drivers.
                                    For Mesa (Linux), ensure you have vulkan-icd-loader and your driver's Vulkan package installed."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // FIRST PROJECT SECTION
                    // =========================================================
                    <section id="first-project" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Your First Project"
                        </h2>

                        <div id="first-project-launch" class="subsection">
                            <h3>"Launch the Engine"</h3>
                            <p>
                                "Open Eustress from your application menu or run it from a terminal.
                                You'll see the Home screen with options to create a new project or open an existing one."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"# Launch from terminal (opens GUI)
eustress

# Or create a project directly
eustress new my-first-project
cd my-first-project
eustress open ."#}</code></pre>
                            </div>
                        </div>

                        <div id="first-project-universe" class="subsection">
                            <h3>"Create a Universe"</h3>
                            <p>
                                "A Universe is the top-level container for your project. It holds Spaces (scenes),
                                shared assets, global configuration, and your identity. Think of it as a
                                self-contained world."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"eustress new my-universe
# Created Universe at ./my-universe/
#   .eustress/
#   ├── config.toml
#   ├── spaces/
#   ├── assets/
#   └── scripts/"#}</code></pre>
                            </div>

                            <p>
                                "Or use the GUI: click "<strong>"New Universe"</strong>" on the Home screen,
                                choose a directory, and give it a name. The engine scaffolds the project
                                structure for you."
                            </p>
                        </div>

                        <div id="first-project-space" class="subsection">
                            <h3>"Create a Space"</h3>
                            <p>
                                "Spaces are individual scenes inside your Universe. A game might have
                                a main menu Space, a gameplay Space, and a settings Space."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"cd my-universe
eustress space new playground
# Created Space "playground" at .eustress/spaces/playground/
#   ├── space.toml       # Space metadata
#   ├── entities.toml    # Entity definitions
#   └── environment.toml # Lighting, skybox, fog"#}</code></pre>
                            </div>

                            <p>
                                "The engine automatically opens your new Space in the editor viewport.
                                You'll see an empty 3D scene with a default camera, directional light, and ground plane."
                            </p>
                        </div>

                        <div id="first-project-primitives" class="subsection">
                            <h3>"Add Primitives"</h3>
                            <p>
                                "Add your first objects using the Insert menu or the command palette
                                ("<code>"Ctrl+K"</code>"):"
                            </p>

                            <ol class="numbered-list">
                                <li>
                                    <strong>"Add a Cube"</strong>
                                    " — Insert > Primitives > Cube. A 1m cube appears at the origin."
                                </li>
                                <li>
                                    <strong>"Add a Sphere"</strong>
                                    " — Insert > Primitives > Sphere. Position it above the cube."
                                </li>
                                <li>
                                    <strong>"Press Play"</strong>
                                    " — Hit "<code>"F5"</code>" to enter Play mode. The sphere falls and collides with the cube."
                                </li>
                            </ol>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"entities.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r##"# This is what gets written to disk — human-readable entity definitions

[[entity]]
name = "MyCube"
type = "BasePart"
shape = "Cube"
position = [0.0, 0.5, 0.0]
scale = [1.0, 1.0, 1.0]
color = "#4a9eff"
anchored = true

[[entity]]
name = "MySphere"
type = "BasePart"
shape = "Sphere"
position = [0.0, 5.0, 0.0]
scale = [1.0, 1.0, 1.0]
color = "#ff6b6b"
anchored = false"##}</code></pre>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Everything is a File"</strong>
                                    <p>"Notice how the entities you add in the GUI are stored as plain TOML.
                                    You can edit this file directly — changes reload live in the editor."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // FILE SYSTEM SECTION
                    // =========================================================
                    <section id="filesystem" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "File System"
                        </h2>

                        <div id="filesystem-structure" class="subsection">
                            <h3>"Project Structure"</h3>
                            <p>
                                "Every Eustress project lives in a directory with a "<code>".eustress/"</code>
                                " folder at the root. Here is the full layout:"
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Directory Layout"</span>
                                </div>
                                <pre><code class="language-text">{r#"my-universe/
└── .eustress/
    ├── config.toml          # Universe-level settings
    ├── identity.toml         # Your creator identity (optional)
    ├── spaces/
    │   ├── playground/
    │   │   ├── space.toml        # Space metadata & physics settings
    │   │   ├── entities.toml     # All entities in this Space
    │   │   └── environment.toml  # Lighting, skybox, post-processing
    │   └── main-menu/
    │       ├── space.toml
    │       ├── entities.toml
    │       └── environment.toml
    ├── assets/
    │   ├── models/           # .gltf, .glb, .obj
    │   ├── textures/         # .png, .jpg, .ktx2
    │   ├── audio/            # .ogg, .wav, .mp3
    │   └── fonts/            # .ttf, .otf
    ├── scripts/
    │   ├── soul/             # .soul  (natural language)
    │   ├── rune/             # .rn    (Rune scripts)
    │   └── luau/             # .luau  (Luau scripts)
    └── .gitignore"#}</code></pre>
                            </div>
                        </div>

                        <div id="filesystem-config" class="subsection">
                            <h3>"Configuration"</h3>
                            <p>
                                "The "<code>"config.toml"</code>" at the Universe root controls global settings:"
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"config.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[universe]
name = "My First Universe"
version = "0.1.0"
default_space = "playground"

[physics]
gravity = [0.0, -9.81, 0.0]
timestep = "fixed"    # "fixed" or "variable"
substeps = 4

[rendering]
msaa = 4
shadows = true
ambient_occlusion = true
bloom = true

[scripting]
enabled = ["soul", "rune", "luau"]
hot_reload = true"#}</code></pre>
                            </div>
                        </div>

                        <div id="filesystem-principles" class="subsection">
                            <h3>"Design Principles"</h3>
                            <p>
                                "The file system is designed around transparency and interoperability:"
                            </p>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Principle"</th>
                                            <th>"What It Means"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><strong>"No proprietary formats"</strong></td>
                                            <td>"TOML, glTF, PNG, OGG — standard formats you can open anywhere"</td>
                                        </tr>
                                        <tr>
                                            <td><strong>"Git-friendly"</strong></td>
                                            <td>"All project files are text-based and merge cleanly"</td>
                                        </tr>
                                        <tr>
                                            <td><strong>"Hot-reloadable"</strong></td>
                                            <td>"Edit any file on disk and the engine picks up changes instantly"</td>
                                        </tr>
                                        <tr>
                                            <td><strong>"Self-contained"</strong></td>
                                            <td>"Copy the directory anywhere and it works — no registry, no database"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // SCRIPTING BASICS SECTION
                    // =========================================================
                    <section id="scripting" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Scripting Basics"
                        </h2>

                        <p>
                            "Eustress supports three scripting languages, each suited to different skill levels
                            and use cases. You can mix and match within a single project."
                        </p>

                        <div class="api-table">
                            <table>
                                <thead>
                                    <tr>
                                        <th>"Language"</th>
                                        <th>"Extension"</th>
                                        <th>"Best For"</th>
                                        <th>"Skill Level"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td><strong>"Soul"</strong></td>
                                        <td><code>".soul"</code></td>
                                        <td>"Describing behavior in plain English"</td>
                                        <td>"Beginner"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Rune"</strong></td>
                                        <td><code>".rn"</code></td>
                                        <td>"Rust-like scripting with pattern matching"</td>
                                        <td>"Intermediate"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Luau"</strong></td>
                                        <td><code>".luau"</code></td>
                                        <td>"Familiar Lua-family syntax, typed"</td>
                                        <td>"Intermediate"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="scripting-soul" class="subsection">
                            <h3>"Soul (Natural Language)"</h3>
                            <p>
                                "Soul is Eustress's natural-language scripting layer. Write what you want
                                to happen in plain English and the engine compiles it into executable behavior.
                                No syntax to memorize."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"hello.soul"</span>
                                </div>
                                <pre><code class="language-text">{r#"When the player touches the cube:
  Print "Hello from Eustress!" to the output log.
  Change the cube color to green.
  Play a chime sound."#}</code></pre>
                            </div>

                            <div class="callout callout-tip">
                                <img src="/assets/icons/sparkles.svg" alt="Tip" />
                                <div>
                                    <strong>"Soul + AI"</strong>
                                    <p>"Soul scripts are interpreted by a local language model. The more
                                    specific you are, the better the results. Use entity names exactly
                                    as they appear in your Space."</p>
                                </div>
                            </div>
                        </div>

                        <div id="scripting-rune" class="subsection">
                            <h3>"Rune"</h3>
                            <p>
                                "Rune is a dynamic language with Rust-like syntax. It supports pattern matching,
                                async/await, and direct access to engine APIs. Scripts are hot-reloaded on save."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"hello.rn"</span>
                                </div>
                                <pre><code class="language-rust">{r#"pub fn on_start(world) {
    let cube = world.get_entity("MyCube");
    println!("Hello from Rune!");

    cube.set_color(Color::GREEN);
    cube.on_touch(|other| {
        println!("Touched by {}", other.name());
    });
}"#}</code></pre>
                            </div>
                        </div>

                        <div id="scripting-luau" class="subsection">
                            <h3>"Luau"</h3>
                            <p>
                                "Luau is a typed Lua dialect originally developed at Roblox. It's familiar to
                                millions of developers and provides excellent tooling with type inference and
                                autocompletion."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"hello.luau"</span>
                                </div>
                                <pre><code class="language-lua">{r#"local cube = world:GetEntity("MyCube")
print("Hello from Luau!")

cube:SetColor(Color.new(0, 1, 0))
cube:OnTouch(function(other)
    print("Touched by " .. other:GetName())
end)"#}</code></pre>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Hot Reload"</strong>
                                    <p>"All three scripting languages support hot reload. Save a file and
                                    see your changes immediately — no restart needed."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // IDENTITY SETUP SECTION
                    // =========================================================
                    <section id="identity" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Identity Setup"
                        </h2>

                        <div id="identity-create" class="subsection">
                            <h3>"Create Your Identity"</h3>
                            <p>
                                "Your identity ties your creations to you. It's used for publishing,
                                collaboration, and attribution. Create one on "
                                <a href="https://eustress.dev">"eustress.dev"</a>":"
                            </p>

                            <ol class="numbered-list">
                                <li>
                                    <strong>"Sign up"</strong>
                                    " at "<a href="https://eustress.dev/signup">"eustress.dev/signup"</a>
                                </li>
                                <li>
                                    <strong>"Choose a handle"</strong>
                                    " — this is your public username (e.g., "<code>"@creator"</code>")"
                                </li>
                                <li>
                                    <strong>"Download your identity file"</strong>
                                    " — a signed "<code>"identity.toml"</code>" containing your public key"
                                </li>
                            </ol>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"identity.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[identity]
handle = "@creator"
display_name = "My Name"
public_key = "ed25519:abc123..."
created_at = "2026-04-03T00:00:00Z"

[identity.links]
website = "https://example.com"
repository = "https://github.com/creator""#}</code></pre>
                            </div>
                        </div>

                        <div id="identity-link" class="subsection">
                            <h3>"Link to Your Engine"</h3>
                            <p>
                                "Place your "<code>"identity.toml"</code>" in your project's "
                                <code>".eustress/"</code>" directory, or set it globally so every project
                                you create is automatically attributed to you:"
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Terminal"</span>
                                </div>
                                <pre><code class="language-bash">{r#"# Set identity globally (applies to all projects)
eustress identity set ~/.eustress/identity.toml

# Verify the link
eustress identity whoami
# @creator (My Name)
# Key: ed25519:abc123...
# Linked: 2026-04-03"#}</code></pre>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Optional but Recommended"</strong>
                                    <p>"Identity is not required to use the engine locally. You only need it
                                    when publishing Universes to the community or collaborating with others."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // NEXT STEPS SECTION
                    // =========================================================
                    <section id="next-steps" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "Next Steps"
                        </h2>

                        <div id="next-steps-learn" class="subsection">
                            <h3>"Keep Learning"</h3>
                            <p>
                                "Now that you have a running project, dive deeper into the topics that
                                interest you:"
                            </p>

                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/code.svg" alt="Scripting" />
                                    </div>
                                    <h4>"Scripting"</h4>
                                    <p>"Deep dive into Soul, Rune, and Luau with real-world examples"</p>
                                    <a href="/docs/scripting" class="feature-link">"Read the guide"</a>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/cube.svg" alt="Building" />
                                    </div>
                                    <h4>"Building"</h4>
                                    <p>"Learn the entity model, properties, constraints, and materials"</p>
                                    <a href="/docs/building" class="feature-link">"Read the guide"</a>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/physics.svg" alt="Physics" />
                                    </div>
                                    <h4>"Physics"</h4>
                                    <p>"Thermodynamics, fluids, materials science, GPU acceleration"</p>
                                    <a href="/docs/physics" class="feature-link">"Read the guide"</a>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/network.svg" alt="Networking" />
                                    </div>
                                    <h4>"Networking"</h4>
                                    <p>"Multiplayer, replication, streaming, and server architecture"</p>
                                    <a href="/docs/networking" class="feature-link">"Read the guide"</a>
                                </div>
                            </div>
                        </div>

                        <div id="next-steps-community" class="subsection">
                            <h3>"Community"</h3>
                            <p>
                                "Join the Eustress community to get help, share your creations, and contribute:"
                            </p>
                            <ul class="feature-list">
                                <li>
                                    <strong>"Discord"</strong>
                                    " — Real-time chat, help channels, and showcase"
                                </li>
                                <li>
                                    <strong>"GitHub"</strong>
                                    " — Source code, issue tracker, and pull requests"
                                </li>
                                <li>
                                    <strong>"Forum"</strong>
                                    " — Long-form discussions, tutorials, and announcements"
                                </li>
                                <li>
                                    <strong>"eustress.dev"</strong>
                                    " — Published Universes, asset packs, and creator profiles"
                                </li>
                            </ul>
                        </div>
                    </section>

                    // Next Navigation (no "Previous" — this is the first docs page)
                    <nav class="docs-nav-footer">
                        <a href="/docs/scripting" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Scripting"</span>
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
