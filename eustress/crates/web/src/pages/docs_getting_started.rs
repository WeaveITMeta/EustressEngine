// =============================================================================
// Eustress Web - Getting Started Documentation Page
// =============================================================================
// Getting Started: install Eustress Engine, open a first Space, add and move a
// part, press Play, save, and run a first script.
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
                TocSubsection { id: "overview-what", title: "What Eustress Is" },
                TocSubsection { id: "overview-terms", title: "Universes and Spaces" },
            ],
        },
        TocSection {
            id: "install",
            title: "Install",
            subsections: vec![
                TocSubsection { id: "install-download", title: "Download" },
                TocSubsection { id: "install-updates", title: "Updates" },
                TocSubsection { id: "install-first-launch", title: "First Launch" },
                TocSubsection { id: "install-sign-in", title: "Signing In" },
            ],
        },
        TocSection {
            id: "first-space",
            title: "Your First Space",
            subsections: vec![
                TocSubsection { id: "first-space-universe", title: "Create a Universe" },
                TocSubsection { id: "first-space-space", title: "Add a Space" },
                TocSubsection { id: "first-space-open", title: "Open and Switch" },
            ],
        },
        TocSection {
            id: "build",
            title: "Build",
            subsections: vec![
                TocSubsection { id: "build-insert", title: "Insert a Part" },
                TocSubsection { id: "build-camera", title: "Look Around" },
                TocSubsection { id: "build-move", title: "Move It" },
                TocSubsection { id: "build-properties", title: "Change Its Properties" },
            ],
        },
        TocSection {
            id: "play",
            title: "Play",
            subsections: vec![
                TocSubsection { id: "play-run", title: "Run and Play" },
                TocSubsection { id: "play-stop", title: "What Stop Restores" },
            ],
        },
        TocSection {
            id: "save",
            title: "Save",
            subsections: vec![
                TocSubsection { id: "save-auto", title: "Saved as You Work" },
                TocSubsection { id: "save-manual", title: "Snapshots" },
                TocSubsection { id: "save-disk", title: "On Disk" },
            ],
        },
        TocSection {
            id: "script",
            title: "First Script",
            subsections: vec![
                TocSubsection { id: "script-bar", title: "The Command Bar" },
                TocSubsection { id: "script-luau", title: "Make a Part in Luau" },
                TocSubsection { id: "script-play", title: "Scripts That Run on Play" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-learn", title: "Keep Learning" },
                TocSubsection { id: "roadmap-coming", title: "Coming Next" },
            ],
        },
    ]
}

/// Getting Started documentation page.
#[component]
pub fn DocsGettingStartedPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-getting-started"></div>
            </div>

            <div class="docs-layout">
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

                <main class="docs-content">
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Getting Started"</span>
                        </div>
                        <h1 class="docs-title">"Getting Started"</h1>
                        <p class="docs-subtitle">
                            "Eustress is a source-available simulation and data platform built in Rust. This
                            page takes you from download to a first working Space: install Eustress Engine,
                            add and move a part, press Play, save, and run a first script."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "13 min read"
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

                        <div id="overview-what" class="subsection">
                            <h3>"What Eustress Is"</h3>
                            <p>
                                "Eustress is a source-available simulation and data platform built in Rust. You
                                build a world out of parts, run it with physics and scripts, and read what it
                                produces. The desktop application is "<strong>"Eustress Engine"</strong>", and the
                                window you work in is "<strong>"Studio"</strong>", its editor."
                            </p>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/physics.svg" alt="Physics" />
                                    </div>
                                    <h4>"Parts and Physics"</h4>
                                    <p>"Unanchored parts fall and collide when you press Run or Play."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/code.svg" alt="Scripting" />
                                    </div>
                                    <h4>"Luau and Rune"</h4>
                                    <p>"Two script languages, both runnable from the command bar."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/folder.svg" alt="Files" />
                                    </div>
                                    <h4>"Files You Own"</h4>
                                    <p>"A Space is a folder on your disk; each part you insert starts as a plain TOML file."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/archive.svg" alt="History" />
                                    </div>
                                    <h4>"Git Snapshots"</h4>
                                    <p>"Saves and autosaves commit the Space's files to git."</p>
                                </div>
                            </div>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Source-available"</strong>
                                    <p>
                                        "The source is public under the PolyForm Shield 1.0.0 license: you can read,
                                        build and use it, and you may not use it to build a competing product. See "
                                        <a href="/license">"License"</a>" for the full text."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-terms" class="subsection">
                            <h3>"Universes and Spaces"</h3>
                            <p>
                                "A Space is one world: its parts, lights, scripts and settings, kept together in one
                                folder. A Universe is a folder of related Spaces. Everything you build lives in a
                                Space, and every Space belongs to a Universe."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Term"</th><th>"What it is"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><strong>"Universe"</strong></td><td>"A folder under Documents/Eustress. Its Spaces live in its Spaces folder."</td></tr>
                                    <tr><td><strong>"Space"</strong></td><td>"One world, stored as a folder with one subfolder per service."</td></tr>
                                    <tr><td><strong>"Service"</strong></td><td>"A top-level container: Workspace holds the 3D world, Lighting the sun and sky, SoulService the scripts. A new Space gets 19 of them."</td></tr>
                                    <tr><td><strong>"Instance"</strong></td><td>"Any object in a Space: a part, a light, a script, a folder."</td></tr>
                                    <tr><td><strong>"Explorer"</strong></td><td>"The panel that lists every instance as a tree."</td></tr>
                                    <tr><td><strong>"Properties"</strong></td><td>"The panel that edits the selected instance."</td></tr>
                                    <tr><td><strong>"Output"</strong></td><td>"The log: messages from Studio and from your scripts."</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The "<a href="/docs/universes">"Universes"</a>" page covers how a Space is stored and
                                versioned in depth."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // INSTALL
                    // =========================================================
                    <section id="install" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Install"
                        </h2>

                        <div id="install-download" class="subsection">
                            <h3>"Download"</h3>
                            <p>
                                "Eustress Engine is in Public Alpha. Sign in on eustress.dev and open "
                                <a href="/download">"Download"</a>" for the latest build. The page needs a free
                                account; creating one includes an age and identity check."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Platform"</th><th>"File"</th><th>"Then"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Windows (x64)"</td><td><code>"eustress-engine-v<version>-windows-x64.zip"</code></td><td>"Extract it and run "<code>"eustress-engine.exe"</code></td></tr>
                                    <tr><td>"macOS (Apple Silicon)"</td><td><code>"eustress-engine-v<version>-macos-arm64.dmg"</code></td><td>"Open it; the app is Eustress Engine"</td></tr>
                                    <tr><td>"Linux (x64)"</td><td><code>"eustress-engine-v<version>-linux-x64.tar.gz"</code></td><td>"Extract it and run "<code>"./eustress-engine"</code></td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Keep the assets folder beside the program"</strong>
                                    <p>
                                        "The archives hold the program with an "<code>"assets"</code>" folder next to
                                        it, and Eustress Engine loads its part meshes from there. A copy of the program
                                        on its own starts without them: the Linux package's "<code>"install.sh"</code>
                                        " copies only the binary to "<code>"~/.local/bin"</code>". Run it from the
                                        folder you extracted."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="install-updates" class="subsection">
                            <h3>"Updates"</h3>
                            <p>
                                "Each time it starts, Eustress Engine asks downloads.eustress.dev for the latest
                                release. When a newer one exists, an "<em>"Update to"</em>" badge with the new
                                version appears at the right end of the menu bar. For now, get the new build from
                                the Download page and install it the same way as the first; the in-app install step
                                is listed under What's Next."
                            </p>
                        </div>

                        <div id="install-first-launch" class="subsection">
                            <h3>"First Launch"</h3>
                            <p>
                                "The first time Eustress Engine starts, it creates its working folder, "
                                <code>"Eustress"</code>" inside your Documents folder, with one Universe and one
                                Space in it, and opens that Space. On Windows the folder is "
                                <code>"%USERPROFILE%\\Documents\\Eustress"</code>", the local Documents folder, even
                                when OneDrive has redirected Documents, so a sync client never fights the file
                                watcher. To keep your Universes in another folder, set the "
                                <code>"EUSTRESS_WORKSPACE"</code>" environment variable to it before you start
                                Eustress Engine."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Documents/Eustress"</span>
                                </div>
                                <pre><code class="language-text">{r#"Eustress/
└── Universe1/
    ├── .eustress/assets/      parts and meshes shared by the Universe
    └── Spaces/
        └── Space1/
            ├── Workspace/     Baseplate, WelcomeCube
            ├── Lighting/      Sun, Moon, Sky, Atmosphere
            ├── SoulService/   scripts
            ├── ...            16 more service folders
            ├── space.toml
            └── simulation.toml"#}</code></pre>
                            </div>
                            <p>
                                "Space1 opens on a 512 m square gray Baseplate with a 4 m blue WelcomeCube at its
                                center. Later launches reopen the last Space you switched to from the Universes
                                panel, or else the first Space in alphabetical order."
                            </p>
                            <p>
                                "The window title names what is open, for example "<em>"Universe1 > Space1 - Eustress
                                Engine"</em>". Once per install, a notice says that anonymous usage stats are on:
                                Eustress counts which tools you click, never your content. Turn it off in Settings,
                                on the Notifications tab under Privacy. If something goes wrong, each run's log is in "
                                <code>"~/.eustress_engine/logs"</code>", which keeps the last 5 runs."
                            </p>
                        </div>

                        <div id="install-sign-in" class="subsection">
                            <h3>"Signing In"</h3>
                            <p>
                                "Studio works without an account. You need to sign in to publish a Space and to earn
                                Bliss. Your identity is a file: registering at "<a href="/login">"eustress.dev/login"</a>
                                " ends by downloading "<code>"eustress-<username>.toml"</code>", which holds your
                                Ed25519 key pair. In Studio, click "<strong>"Sign In"</strong>" at the right end of the
                                menu bar, choose "<strong>"Browse"</strong>", pick that file, and press "
                                <strong>"Sign In with Identity"</strong>". Studio remembers the file and signs you in
                                on later launches."
                            </p>
                            <div class="callout callout-warning">
                                <img src="/assets/icons/shield.svg" alt="Warning" />
                                <div>
                                    <strong>"The identity file is your private key"</strong>
                                    <p>"Keep it somewhere safe and never share it: anyone holding it can sign in as you."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // YOUR FIRST SPACE
                    // =========================================================
                    <section id="first-space" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Your First Space"
                        </h2>

                        <div id="first-space-universe" class="subsection">
                            <h3>"Create a Universe"</h3>
                            <ol class="numbered-list">
                                <li>"Open the "<strong>"File"</strong>" menu and choose "<strong>"New Universe..."</strong>"."</li>
                                <li>"Type a name for the Universe folder and press "<strong>"Create"</strong>"."</li>
                                <li>"A second dialog asks for the name of the first Space in it. Type one and press "<strong>"Create"</strong>"."</li>
                            </ol>
                            <p>
                                "Studio creates "<code>"Documents/Eustress/<Universe>/Spaces/<Space>"</code>", fills
                                the Space with its service folders, a Baseplate and a WelcomeCube, and opens it.
                                Characters that cannot appear in a file name become underscores."
                            </p>
                        </div>

                        <div id="first-space-space" class="subsection">
                            <h3>"Add a Space"</h3>
                            <p>
                                "To add another Space to the Universe you are in, choose "<strong>"File"</strong>
                                " > "<strong>"New Space"</strong>" or press "<code>"Ctrl+N"</code>". A save dialog
                                opens in the current Universe with the name "<em>"New Space"</em>" filled in: type
                                the name you want and confirm. The Space has to stay inside that Universe; Studio
                                creates it in the Universe's "<code>"Spaces"</code>" folder and switches to it."
                            </p>
                        </div>

                        <div id="first-space-open" class="subsection">
                            <h3>"Open and Switch"</h3>
                            <ul class="docs-list">
                                <li><strong>"Universes tab"</strong>": the left panel lists every Universe and its Spaces. Click a Space to open it."</li>
                                <li><strong>"File > Open File..."</strong>" ("<code>"Ctrl+O"</code>"): pick a Space folder anywhere on disk."</li>
                                <li><strong>"File > Recent"</strong>": the last 8 Spaces you opened."</li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // BUILD
                    // =========================================================
                    <section id="build" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Build"
                        </h2>

                        <div id="build-insert" class="subsection">
                            <h3>"Insert a Part"</h3>
                            <p>
                                "Open the "<strong>"Insert"</strong>" menu and choose "<strong>"Part (Block)"</strong>
                                ". A 4 by 1 by 2 m part named Block appears 10 m in front of the camera, already
                                selected. It goes into Workspace, or inside whatever you had selected in the
                                Explorer."
                            </p>
                            <p>
                                "To insert any other kind of object, press "<code>"Ctrl+I"</code>" for the "
                                <strong>"Insert Object"</strong>" dialog, type part of a class name ("
                                <em>"PointLight"</em>", "<em>"Folder"</em>", "<em>"TextLabel"</em>"), and press "
                                <code>"Enter"</code>". Either way the new object ends up selected, and "
                                <code>"Ctrl+Z"</code>" takes the insert back."
                            </p>
                        </div>

                        <div id="build-camera" class="subsection">
                            <h3>"Look Around"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Input"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Right-drag"</td><td>"Look around from where you stand"</td></tr>
                                    <tr><td><code>"W"</code>" / "<code>"S"</code></td><td>"Fly forward and back along the view"</td></tr>
                                    <tr><td><code>"A"</code>" / "<code>"D"</code></td><td>"Strafe left and right"</td></tr>
                                    <tr><td><code>"Q"</code>" / "<code>"E"</code></td><td>"Move down and up"</td></tr>
                                    <tr><td>"Mouse wheel"</td><td>"Fly toward the point under the cursor"</td></tr>
                                    <tr><td>"Middle-drag"</td><td>"Pan"</td></tr>
                                    <tr><td>"Alt + left-drag"</td><td>"Orbit"</td></tr>
                                    <tr><td><code>"F"</code></td><td>"Frame the selection"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The keys fly at 9.81 m/s; hold "<code>"Shift"</code>" to slow down for fine work.
                                The "<a href="/docs/perspective">"Perspective"</a>" page covers the 2D view, the
                                orthographic projection and the axis views."
                            </p>
                        </div>

                        <div id="build-move" class="subsection">
                            <h3>"Move It"</h3>
                            <p>
                                "With the part selected, pick a tool from the "<strong>"Tools"</strong>" group on the
                                Home tab, or press its key:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Key"</th><th>"Tool"</th><th>"Drag to"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"Alt+Z"</code></td><td>"Select"</td><td>"Slide the part across the surfaces under the cursor"</td></tr>
                                    <tr><td><code>"Alt+X"</code></td><td>"Move"</td><td>"Move along an axis arrow"</td></tr>
                                    <tr><td><code>"Alt+C"</code></td><td>"Scale"</td><td>"Resize from a face handle"</td></tr>
                                    <tr><td><code>"Alt+V"</code></td><td>"Rotate"</td><td>"Turn about a ring"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Moves snap in 1 m steps and rotations in 15 degree steps by default. Press "
                                <code>"2"</code>" for 0.2 m steps, "<code>"1"</code>" to go back to 1 m, and "
                                <code>"3"</code>" to turn move snapping off. "<code>"Ctrl+Z"</code>" undoes a move, and "
                                <code>"Ctrl+Y"</code>" redoes it. The "<a href="/docs/studio">"Studio"</a>" page covers
                                every tool."
                            </p>
                        </div>

                        <div id="build-properties" class="subsection">
                            <h3>"Change Its Properties"</h3>
                            <p>
                                "The Properties panel on the right edits whatever is selected. Change "
                                <code>"Color"</code>" or "<code>"Material"</code>" and the part updates at once. One
                                property matters before you press Play: "<code>"Anchored"</code>". An anchored part
                                stays where it is; an unanchored part is a physics body that falls. New parts start
                                unanchored, while the Baseplate and the WelcomeCube are anchored, so a Block you
                                raise above the cube will drop onto it."
                            </p>
                            <p>
                                "To anchor a selection from the keyboard, press "<code>"Alt+A"</code>". Press it again
                                to release it."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // PLAY
                    // =========================================================
                    <section id="play" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Play"
                        </h2>

                        <div id="play-run" class="subsection">
                            <h3>"Run and Play"</h3>
                            <p>
                                "The buttons at the left end of the ribbon's tab row start and stop the simulation:
                                the green play button is Run, the gamepad is Play, then Pause and Stop. The same
                                commands are in the "<strong>"Test"</strong>" menu:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Key"</th><th>"Command"</th><th>"What happens"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"F7"</code></td><td>"Run"</td><td>"Physics and scripts start. The camera stays free, with no character."</td></tr>
                                    <tr><td><code>"F5"</code></td><td>"Play"</td><td>"The same, plus a character at a SpawnLocation (or the default spawn point) and a camera that follows it."</td></tr>
                                    <tr><td><code>"F6"</code></td><td>"Pause"</td><td>"Freezes physics. Press again to resume."</td></tr>
                                    <tr><td><code>"F8"</code>" or "<code>"Esc"</code></td><td>"Stop"</td><td>"Returns to editing."</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Try it: raise your Block a few meters above the WelcomeCube, press "<code>"F7"</code>
                                ", and watch it fall onto the cube. Physics runs on "
                                <a href="/docs/physics">"Avian"</a>". Studio keeps physics paused while you edit, so
                                nothing moves until you press Run or Play."
                            </p>
                        </div>

                        <div id="play-stop" class="subsection">
                            <h3>"What Stop Restores"</h3>
                            <p>
                                "When you press Run or Play, Studio takes a snapshot of the Space. Stop puts it back:
                                every part returns to where it was, with its color, transparency and anchoring;
                                parts deleted during the run come back; anything created during the run is removed;
                                the lighting and the editor camera return to where you left them; and physics pauses
                                again. A run leaves what you built as it was."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // SAVE
                    // =========================================================
                    <section id="save" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Save"
                        </h2>

                        <div id="save-auto" class="subsection">
                            <h3>"Saved as You Work"</h3>
                            <p>
                                "Studio records your edits as you make them, in the Space's database, "
                                <code>"world.fjalldb"</code>". A simple part (a built-in shape with no children) keeps
                                its edits there only; a part with children or a custom mesh also has its "
                                <code>"_instance.toml"</code>" rewritten. Every 5 minutes an autosave commits the Space
                                folder's files to git, so there is a restore point even if you never save by hand."
                            </p>
                            <p>
                                "The database is kept out of git, so edits that live only in the database are not in
                                that history. The "<a href="/docs/universes#history-coverage">"Universes"</a>" page
                                has the full table of what a commit captures."
                            </p>
                            <div class="callout callout-warning">
                                <img src="/assets/icons/shield.svg" alt="Warning" />
                                <div>
                                    <strong>"Terrain waits for Ctrl+S"</strong>
                                    <p>
                                        "Terrain sculpting and painting stay in memory until you press "
                                        <code>"Ctrl+S"</code>". Close Studio without saving and those strokes are gone."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="save-manual" class="subsection">
                            <h3>"Snapshots"</h3>
                            <p>
                                "Press "<code>"Ctrl+S"</code>" (or "<strong>"File > Save Space"</strong>") to take a
                                snapshot now. Studio saves the scene the same database-first way, writes any terrain
                                to disk, then commits the Space folder's files to git with the message "
                                <em>"manual save"</em>" and the time. The first snapshot, manual or automatic, creates
                                the git repository in the Space folder."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Unsaved"</strong>": a badge at the right end of the menu bar appears when you have edits that no snapshot has recorded yet."</li>
                                <li><strong>"File menu"</strong>": shows the last snapshot, for example "<em>"Snapshot 14:32"</em>" or "<em>"Autosaved 14:37"</em>"."</li>
                                <li><strong>"Closing"</strong>": with unsaved edits, Studio asks before it exits."</li>
                            </ul>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Snapshots need git installed"</strong>
                                    <p>
                                        "Studio runs the "<code>"git"</code>" command to record snapshots. Without git on
                                        your machine every edit still reaches disk, but no snapshot is made, and a
                                        notification says so each time one fails."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="save-disk" class="subsection">
                            <h3>"On Disk"</h3>
                            <p>
                                "Inserting a part writes a folder holding an "<code>"_instance.toml"</code>" file that
                                you can read, diff and edit in any text editor. This is the WelcomeCube as a new
                                Space writes it:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Workspace/WelcomeCube/_instance.toml (excerpt)"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[metadata]
class_name = "Part"
archivable = true

[asset]
mesh = "parts/block.glb"
scene = "Scene0"

[transform]
position = [0.0, 2.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]   # quaternion x, y, z, w
scale = [4.0, 4.0, 4.0]           # a part's scale is its size in meters

[properties]
color = [0.388, 0.706, 1.0, 1.0]
transparency = 0.0
reflectance = 0.2
anchored = true
can_collide = true
locked = false"#}</code></pre>
                            </div>
                            <p>
                                "Later edits to a simple part like this one live in the database, so the file keeps
                                the values it was inserted with. The autosave keeps the database ("
                                <code>"world.fjalldb"</code>") and the Space's trash out of git on purpose. Back that
                                folder up with the rest of the Space: it holds edits that exist nowhere else, and it is
                                part of the Space, not a cache."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // FIRST SCRIPT
                    // =========================================================
                    <section id="script" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "First Script"
                        </h2>

                        <div id="script-bar" class="subsection">
                            <h3>"The Command Bar"</h3>
                            <p>
                                "The command bar runs a script the moment you press "<code>"Enter"</code>". It is the
                                single line docked along the bottom of the window. The label at its left says which
                                language it runs; click it to switch between "<strong>"Rune"</strong>" and "
                                <strong>"Luau"</strong>". "<code>"Shift+Enter"</code>" adds a line, and a pasted
                                script keeps its line breaks. Whatever the script prints appears in Output."
                            </p>
                        </div>

                        <div id="script-luau" class="subsection">
                            <h3>"Make a Part in Luau"</h3>
                            <p>"Switch the command bar to Luau, paste this, and press "<code>"Enter"</code>":"</p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Luau"</span>
                                </div>
                                <pre><code class="language-lua">{r#"local part = Instance.new("Part")
part.Name = "HelloPart"
part.Size = Vector3.new(2, 2, 2)          -- meters
part.Position = Vector3.new(0, 8, 0)      -- 4 m above the WelcomeCube
part.Color = Color3.fromRGB(0, 188, 212)
part.Anchored = false
print("Created " .. part.Name)"#}</code></pre>
                            </div>
                            <p>
                                "Output shows "<em>"Created HelloPart"</em>" and a line confirming the spawned part, and
                                a cyan cube hangs in the air above the WelcomeCube. The command bar runs in edit mode,
                                so the part is real: it appears in Workspace in the Explorer and is saved with the
                                Space like any other part. Press "<code>"F7"</code>" and it falls."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Meters everywhere"</strong>
                                    <p>
                                        "Scripts see every position and size in meters, whatever unit Studio is
                                        displaying. "<code>"Units.to_meters(5, 'ft')"</code>" returns 1.524 when you
                                        think in another unit."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="script-play" class="subsection">
                            <h3>"Scripts That Run on Play"</h3>
                            <p>
                                "A command-bar script runs once, in edit mode. Rune scripts saved in the Space run
                                every time you press Run or Play: each gets "<code>"on_init()"</code>" once and "
                                <code>"on_update(dt)"</code>" every frame, and Stop ends them. Luau files in a Space
                                show up in the Explorer, but Play does not start them yet, so for now Luau runs from
                                the command bar. The "<a href="/docs/scripting">"Scripting"</a>" page covers both
                                languages and the script API."
                            </p>
                            <p>
                                "A SoulScript can also begin as a plain-English summary. Its "<strong>"Build"</strong>
                                " button sends the summary to Anthropic's Claude, using your own API key saved in "
                                <strong>"Settings"</strong>" on the Soul tab, and writes the Rune code it gets back
                                beside the summary."
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

                        <div id="roadmap-learn" class="subsection">
                            <h3>"Keep Learning"</h3>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/monitor.svg" alt="Studio" />
                                    </div>
                                    <h4><a href="/docs/studio">"Studio"</a></h4>
                                    <p>"Every panel, tool, shortcut and search operator in the editor."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/cube.svg" alt="Building" />
                                    </div>
                                    <h4><a href="/docs/building">"Building"</a></h4>
                                    <p>"Parts, materials, lighting and terrain."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/code.svg" alt="Scripting" />
                                    </div>
                                    <h4><a href="/docs/scripting">"Scripting"</a></h4>
                                    <p>"Luau, Rune and the script API."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/play.svg" alt="Simulation" />
                                    </div>
                                    <h4><a href="/docs/simulation">"Simulation"</a></h4>
                                    <p>"The clock, recordings and experiments."</p>
                                </div>
                            </div>
                        </div>

                        <div id="roadmap-coming" class="subsection">
                            <h3>"Coming Next"</h3>
                            <ul class="docs-list">
                                <li><strong>"One-click updates"</strong>": the Update badge will download the new build, check its SHA-256 and restart into it on Windows and Linux. On macOS you will open the new disk image yourself."</li>
                                <li><strong>"A Windows installer"</strong>": the Download page will offer the Setup program the release pipeline already builds. It installs to Program Files, adds a Start Menu entry, and opens "<code>".eustress"</code>" files."</li>
                                <li><strong>"Luau on Play"</strong>": Luau scripts saved in a Space will start when you press Run or Play, the way Rune scripts do today."</li>
                            </ul>
                            <div class="future-cta">
                                <p><strong>"Download it, open Space1, and drop your first part."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/studio" class="btn-secondary-steel">"Studio Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/learn" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"All Topics"</span>
                            </div>
                        </a>
                        <a href="/docs/studio" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Studio"</span>
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
