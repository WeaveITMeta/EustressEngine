// =============================================================================
// Eustress Web - Services Documentation Page
// =============================================================================
// Comprehensive documentation on the default services that every Eustress Space
// ships with. Covers core (Workspace, Lighting, Chat), player (Players,
// StarterPlayer, StarterGui, StarterPack, Teams), data (ReplicatedStorage,
// ServerStorage), scripting (ServerScriptService, SoulService), and rendering
// (MaterialService, SoundService, AdornmentService) services.
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
            id: "overview",
            title: "Overview",
            subsections: vec![
                TocSubsection { id: "overview-intro", title: "Introduction" },
                TocSubsection { id: "overview-categories", title: "Categories" },
            ],
        },
        TocSection {
            id: "core",
            title: "Core Services",
            subsections: vec![
                TocSubsection { id: "core-workspace", title: "Workspace" },
                TocSubsection { id: "core-lighting", title: "Lighting" },
                TocSubsection { id: "core-chat", title: "Chat" },
            ],
        },
        TocSection {
            id: "player",
            title: "Player Services",
            subsections: vec![
                TocSubsection { id: "player-players", title: "Players" },
                TocSubsection { id: "player-starter-player", title: "StarterPlayer" },
                TocSubsection { id: "player-starter-gui", title: "StarterGui" },
                TocSubsection { id: "player-starter-pack", title: "StarterPack" },
                TocSubsection { id: "player-teams", title: "Teams" },
            ],
        },
        TocSection {
            id: "data",
            title: "Data Services",
            subsections: vec![
                TocSubsection { id: "data-replicated-storage", title: "ReplicatedStorage" },
                TocSubsection { id: "data-server-storage", title: "ServerStorage" },
            ],
        },
        TocSection {
            id: "scripting",
            title: "Scripting Services",
            subsections: vec![
                TocSubsection { id: "scripting-server-script-service", title: "ServerScriptService" },
                TocSubsection { id: "scripting-soul-service", title: "SoulService" },
            ],
        },
        TocSection {
            id: "rendering",
            title: "Rendering Services",
            subsections: vec![
                TocSubsection { id: "rendering-material-service", title: "MaterialService" },
                TocSubsection { id: "rendering-sound-service", title: "SoundService" },
                TocSubsection { id: "rendering-adornment-service", title: "AdornmentService" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Services documentation page — all default services in an Eustress Space.
#[component]
pub fn DocsServicesPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-services"></div>
            </div>

            <div class="docs-layout">
                // Floating TOC Sidebar
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/network.svg" alt="Services" class="toc-icon" />
                        <h2>"Services"</h2>
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
                            <span class="current">"Services"</span>
                        </div>
                        <h1 class="docs-title">"Services"</h1>
                        <p class="docs-subtitle">
                            "Every Eustress Space ships with default services that manage physics,
                            lighting, players, scripting, data storage, audio, and rendering.
                            Services are singleton objects — each Space has exactly one instance
                            of each, created automatically when the Space loads."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "20 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Beginner"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/calendar.svg" alt="Updated" />
                                "Updated May 2026"
                            </span>
                        </div>
                    </header>

                    // =============================================================
                    // OVERVIEW SECTION
                    // =============================================================
                    <section id="overview" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"01"</span>
                            "Overview"
                        </h2>

                        <div id="overview-intro" class="subsection">
                            <h3>"Introduction"</h3>
                            <p>
                                "Services are the backbone of every Eustress Space. They manage
                                everything from gravity and lighting to player spawning and audio.
                                Unlike regular entities that you create and destroy, services are
                                singletons — each Space has exactly one instance of each service,
                                and they exist for the entire lifetime of the Space."
                            </p>
                            <p>
                                "You access services from scripts via "
                                <code>"game.get_service(\"ServiceName\")"</code>
                                ". Properties can be configured in Studio via the Services Browser
                                (Help → Services Browser) or set at runtime from server scripts."
                            </p>
                        </div>

                        <div id="overview-categories" class="subsection">
                            <h3>"Categories"</h3>
                            <p>
                                "Services are organized into five categories based on their
                                responsibilities:"
                            </p>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/cube.svg" alt="Core" />
                                    </div>
                                    <h4>"Core"</h4>
                                    <p>"Workspace, Lighting, Chat — the foundation of every Space"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/star.svg" alt="Player" />
                                    </div>
                                    <h4>"Player"</h4>
                                    <p>"Players, StarterPlayer, StarterGui, StarterPack, Teams"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/folder.svg" alt="Data" />
                                    </div>
                                    <h4>"Data"</h4>
                                    <p>"ReplicatedStorage, ServerStorage — shared and secure containers"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/code.svg" alt="Scripting" />
                                    </div>
                                    <h4>"Scripting"</h4>
                                    <p>"ServerScriptService, SoulService — script execution and AI"</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =============================================================
                    // CORE SERVICES SECTION
                    // =============================================================
                    <section id="core" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Core Services"
                        </h2>

                        <div id="core-workspace" class="subsection">
                            <h3>"Workspace"</h3>
                            <p>
                                "Workspace is the top-level service that holds every Part, Model,
                                and Script in the 3D world. It controls global physics (gravity,
                                air density, wind), the fallen-parts destroy height, streaming
                                behavior, and collision groups. Every Space has exactly one
                                Workspace."
                            </p>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Property"</th>
                                            <th>"Type"</th>
                                            <th>"Default"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"Gravity"</code></td>
                                            <td>"float"</td>
                                            <td><code>"196.2"</code></td>
                                            <td>"Acceleration due to gravity in studs/s²"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"FallenPartsDestroyHeight"</code></td>
                                            <td>"float"</td>
                                            <td><code>"-500"</code></td>
                                            <td>"Y coordinate below which parts are destroyed"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"AirDensity"</code></td>
                                            <td>"float"</td>
                                            <td><code>"0.0012"</code></td>
                                            <td>"Air density for aerodynamic drag calculations"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"GlobalWind"</code></td>
                                            <td>"Vector3"</td>
                                            <td><code>"0, 0, 0"</code></td>
                                            <td>"Wind vector affecting particles and cloth"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"TouchesUseCollisionGroups"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"false"</code></td>
                                            <td>"Whether .Touched events respect collision groups"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"let workspace = game.get_service("Workspace");

// Moon gravity
workspace.Gravity = 32.7;

// Enable wind for particle effects
workspace.GlobalWind = Vector3.new(10, 0, 5);"#}</code></pre>
                            </div>
                        </div>

                        <div id="core-lighting" class="subsection">
                            <h3>"Lighting"</h3>
                            <p>
                                "Lighting is the global illumination service. It drives the sun
                                position (via ClockTime and GeographicLatitude), ambient color,
                                shadow quality, exposure, fog, and atmosphere effects. It also
                                hosts child objects like Atmosphere, Sky, and post-processing
                                effects."
                            </p>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Property"</th>
                                            <th>"Type"</th>
                                            <th>"Default"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"ClockTime"</code></td>
                                            <td>"float"</td>
                                            <td><code>"14.0"</code></td>
                                            <td>"Time of day (0–24). Controls sun position and sky color"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Brightness"</code></td>
                                            <td>"float"</td>
                                            <td><code>"2"</code></td>
                                            <td>"Overall scene brightness multiplier"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"GlobalShadows"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"true"</code></td>
                                            <td>"Whether shadow maps are computed"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"ShadowSoftness"</code></td>
                                            <td>"float"</td>
                                            <td><code>"0.2"</code></td>
                                            <td>"Shadow edge blur radius (0 = sharp, 1 = very soft)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"FogStart"</code></td>
                                            <td>"float"</td>
                                            <td><code>"0"</code></td>
                                            <td>"Distance at which fog begins"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"FogEnd"</code></td>
                                            <td>"float"</td>
                                            <td><code>"100000"</code></td>
                                            <td>"Distance at which fog reaches full opacity"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"FogColor"</code></td>
                                            <td>"Color3"</td>
                                            <td><code>"0.75, 0.75, 0.75"</code></td>
                                            <td>"Color of distance fog"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"ExposureCompensation"</code></td>
                                            <td>"float"</td>
                                            <td><code>"0"</code></td>
                                            <td>"EV bias for HDR tone mapping"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"GeographicLatitude"</code></td>
                                            <td>"float"</td>
                                            <td><code>"41.7"</code></td>
                                            <td>"Latitude for sun angle calculation"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <p>"Lighting can contain child objects for advanced effects:"</p>
                            <ul class="docs-list">
                                <li><strong>"Atmosphere"</strong>" — Realistic atmospheric scattering (Rayleigh + Mie)"</li>
                                <li><strong>"Sky"</strong>" — Custom skybox with six-face textures or procedural sky"</li>
                                <li><strong>"BloomEffect"</strong>" — HDR bloom post-processing"</li>
                                <li><strong>"ColorCorrectionEffect"</strong>" — Color grading and LUT"</li>
                                <li><strong>"SunRaysEffect"</strong>" — God rays from the sun"</li>
                                <li><strong>"DepthOfFieldEffect"</strong>" — Camera focal blur"</li>
                            </ul>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"let lighting = game.get_service("Lighting");

// Sunset scene
lighting.ClockTime = 18.5;
lighting.FogColor = Color3.new(1.0, 0.6, 0.3);
lighting.FogEnd = 500;
lighting.ShadowSoftness = 0.5;"#}</code></pre>
                            </div>
                        </div>

                        <div id="core-chat" class="subsection">
                            <h3>"Chat"</h3>
                            <p>
                                "The Chat service provides the default text chat system with
                                support for bubble chat (speech bubbles above characters), message
                                filtering for safety, and customizable chat windows. It can be
                                extended with ChatModules for custom commands and behaviors."
                            </p>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Property"</th>
                                            <th>"Type"</th>
                                            <th>"Default"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"BubbleChatEnabled"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"true"</code></td>
                                            <td>"Show speech bubble above character when they chat"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"LoadDefaultChat"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"true"</code></td>
                                            <td>"Load the built-in chat GUI"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"FilteringEnabled"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"true"</code></td>
                                            <td>"Enable server-side text filtering for safety"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"let chat = game.get_service("Chat");

// Use a custom chat system
chat.LoadDefaultChat = false;
chat.BubbleChatEnabled = false;"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =============================================================
                    // PLAYER SERVICES SECTION
                    // =============================================================
                    <section id="player" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Player Services"
                        </h2>

                        <div id="player-players" class="subsection">
                            <h3>"Players"</h3>
                            <p>
                                "Players is a runtime-only service that contains a Player object
                                for each connected client. It provides events for
                                PlayerAdded/PlayerRemoving and methods like GetPlayers(). In
                                Studio, it shows no players since you are in edit mode."
                            </p>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Property"</th>
                                            <th>"Type"</th>
                                            <th>"Default"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"MaxPlayers"</code></td>
                                            <td>"int"</td>
                                            <td><code>"50"</code></td>
                                            <td>"Maximum concurrent players in the server"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"RespawnTime"</code></td>
                                            <td>"float"</td>
                                            <td><code>"5.0"</code></td>
                                            <td>"Seconds to wait before respawning after death"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"CharacterAutoLoads"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"true"</code></td>
                                            <td>"Automatically load character model on join"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Event"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"PlayerAdded(player)"</code></td>
                                            <td>"Fires when a new player connects"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"PlayerRemoving(player)"</code></td>
                                            <td>"Fires just before a player disconnects"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"let players = game.get_service("Players");

players.PlayerAdded.connect(|player| {
    print("Welcome, " + player.Name);
});

players.PlayerRemoving.connect(|player| {
    print("Goodbye, " + player.Name);
});"#}</code></pre>
                            </div>
                        </div>

                        <div id="player-starter-player" class="subsection">
                            <h3>"StarterPlayer"</h3>
                            <p>
                                "StarterPlayer defines the default configuration applied to every
                                player when they join. It contains two sub-containers:
                                StarterPlayerScripts (scripts cloned into each Player) and
                                StarterCharacterScripts (scripts cloned into each Character)."
                            </p>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Property"</th>
                                            <th>"Type"</th>
                                            <th>"Default"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"CameraMode"</code></td>
                                            <td>"enum"</td>
                                            <td><code>"Classic"</code></td>
                                            <td>"Camera behavior: Classic, LockFirstPerson, LockThirdPerson"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"CameraMaxZoomDistance"</code></td>
                                            <td>"float"</td>
                                            <td><code>"128"</code></td>
                                            <td>"Maximum camera distance from character"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"CharacterWalkSpeed"</code></td>
                                            <td>"float"</td>
                                            <td><code>"16"</code></td>
                                            <td>"Default walk speed in studs per second"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"CharacterJumpHeight"</code></td>
                                            <td>"float"</td>
                                            <td><code>"7.2"</code></td>
                                            <td>"Maximum jump height in studs"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"EnableMouseLockOption"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"true"</code></td>
                                            <td>"Allow players to toggle mouse lock (Shift-Lock)"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"let starter = game.get_service("StarterPlayer");

// First-person only
starter.CameraMode = "LockFirstPerson";
starter.CameraMinZoomDistance = 0;
starter.CameraMaxZoomDistance = 0;

// Slower, more deliberate movement
starter.CharacterWalkSpeed = 10;
starter.CharacterJumpHeight = 5;"#}</code></pre>
                            </div>
                        </div>

                        <div id="player-starter-gui" class="subsection">
                            <h3>"StarterGui"</h3>
                            <p>
                                "StarterGui holds ScreenGui, BillboardGui, and SurfaceGui objects
                                that are automatically copied into each player's PlayerGui when
                                they join or respawn. This is the standard way to create HUD
                                elements, health bars, inventory screens, and menus."
                            </p>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Property"</th>
                                            <th>"Type"</th>
                                            <th>"Default"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"ResetOnSpawn"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"true"</code></td>
                                            <td>"Reset player GUI on respawn"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"ScreenCompatibilityMode"</code></td>
                                            <td>"enum"</td>
                                            <td><code>"TextScaleDpi"</code></td>
                                            <td>"GUI scaling mode for different screens"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"ShowDevelopmentGui"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"false"</code></td>
                                            <td>"Display developer-only GUI elements"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <div class="callout callout-tip">
                                <img src="/assets/icons/sparkles.svg" alt="Tip" />
                                <div>
                                    <strong>"Persistent HUD"</strong>
                                    <p>"Set " <code>"ResetOnSpawn = false"</code> " for GUI
                                    elements that should survive character death, like inventory
                                    screens or settings panels."</p>
                                </div>
                            </div>
                        </div>

                        <div id="player-starter-pack" class="subsection">
                            <h3>"StarterPack"</h3>
                            <p>
                                "StarterPack contains Tool objects that are cloned into every
                                player's Backpack when they join or respawn. Use this for default
                                weapons, building tools, or any item players should start with."
                            </p>
                            <p>
                                "StarterPack has no configurable properties — it is a pure
                                container. Place Tool objects inside it and they are automatically
                                distributed to every player."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"let starter_pack = game.get_service("StarterPack");
let sword = create_tool("Sword");
sword.Parent = starter_pack;
// Now all new players will receive this sword"#}</code></pre>
                            </div>
                        </div>

                        <div id="player-teams" class="subsection">
                            <h3>"Teams"</h3>
                            <p>
                                "The Teams service holds Team objects that players can be assigned
                                to. Each Team has a TeamColor and AutoAssignable flag. When teams
                                exist, player names appear in their team color and the leaderboard
                                shows team groupings."
                            </p>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Property (per Team)"</th>
                                            <th>"Type"</th>
                                            <th>"Default"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"TeamColor"</code></td>
                                            <td>"BrickColor"</td>
                                            <td>"varies"</td>
                                            <td>"Color identifier for this team"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"AutoAssignable"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"true"</code></td>
                                            <td>"Whether new players are auto-assigned"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Name"</code></td>
                                            <td>"string"</td>
                                            <td><code>"Team"</code></td>
                                            <td>"Display name in leaderboard and nametags"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"let teams = game.get_service("Teams");

let red = Instance.new("Team");
red.Name = "Red Team";
red.TeamColor = BrickColor.new("Bright red");
red.AutoAssignable = true;
red.Parent = teams;"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =============================================================
                    // DATA SERVICES SECTION
                    // =============================================================
                    <section id="data" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Data Services"
                        </h2>

                        <div id="data-replicated-storage" class="subsection">
                            <h3>"ReplicatedStorage"</h3>
                            <p>
                                "ReplicatedStorage is a container whose contents are visible to
                                both server and all clients. Place ModuleScripts, models,
                                animations, sounds, and other assets here that need to be
                                accessible from both server scripts and local scripts."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Shared Assets"</strong>" — Models, sounds, animations accessible from both server and client"</li>
                                <li><strong>"Shared ModuleScripts"</strong>" — Utility libraries and configuration"</li>
                                <li><strong>"RemoteEvents"</strong>" — Standard location for client-server communication objects"</li>
                            </ul>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Warning" />
                                <div>
                                    <strong>"Not Secure"</strong>
                                    <p>"Clients can read everything in ReplicatedStorage. Never store
                                    secret configuration values, API keys, or server-only logic here.
                                    Use ServerStorage instead."</p>
                                </div>
                            </div>
                        </div>

                        <div id="data-server-storage" class="subsection">
                            <h3>"ServerStorage"</h3>
                            <p>
                                "ServerStorage is a container whose contents are only accessible to
                                server-side scripts. Clients cannot see, access, or replicate
                                anything stored here. Use it for server-only modules, secret
                                configurations, and assets that should not be downloaded by
                                clients."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Server-Only Modules"</strong>" — Anti-cheat, economy calculations, matchmaking"</li>
                                <li><strong>"Secret Configuration"</strong>" — API keys, admin lists, server-side constants"</li>
                                <li><strong>"Templates"</strong>" — Models cloned into Workspace at runtime, invisible until spawned"</li>
                            </ul>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Script in ServerScriptService
let economy = require(game.get_service("ServerStorage").EconomyModule);
economy.award_currency(player, 100);"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =============================================================
                    // SCRIPTING SERVICES SECTION
                    // =============================================================
                    <section id="scripting" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Scripting Services"
                        </h2>

                        <div id="scripting-server-script-service" class="subsection">
                            <h3>"ServerScriptService"</h3>
                            <p>
                                "ServerScriptService holds Script objects that run on the server
                                when the experience starts. Scripts here have full server
                                authority — they can access ServerStorage, manage datastores,
                                handle physics, and control game state. LocalScripts placed here
                                will NOT run."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Game Initialization"</strong>" — Startup scripts that configure the world and load data"</li>
                                <li><strong>"DataStore Management"</strong>" — Persistent player data (inventory, progress, currency)"</li>
                                <li><strong>"Physics Authority"</strong>" — Anti-cheat validation and damage calculation"</li>
                                <li><strong>"Event Handlers"</strong>" — Server-side listeners for RemoteEvents from clients"</li>
                            </ul>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// ServerScriptService/GameManager
let players = game.get_service("Players");

players.PlayerAdded.connect(|player| {
    let data = load_data(player);
    player.CharacterAdded.connect(|character| {
        character.Humanoid.MaxHealth = data.max_health;
        character.Humanoid.Health = data.max_health;
    });
});"#}</code></pre>
                            </div>
                            <div class="callout callout-tip">
                                <img src="/assets/icons/sparkles.svg" alt="Security" />
                                <div>
                                    <strong>"Security"</strong>
                                    <p>"Always validate client input in server scripts. Never trust
                                    values sent via RemoteEvents without checking. Keep game-critical
                                    logic server-side to prevent exploits."</p>
                                </div>
                            </div>
                        </div>

                        <div id="scripting-soul-service" class="subsection">
                            <h3>"SoulService"</h3>
                            <p>
                                "SoulService is the Eustress-native scripting service that manages
                                the Rune virtual machine. It controls script execution permissions,
                                AI-assisted code generation, and security sandboxing for scripts."
                            </p>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Property"</th>
                                            <th>"Type"</th>
                                            <th>"Default"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"EnableAI"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"true"</code></td>
                                            <td>"Enable AI-assisted code generation"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"AllowFileSystemAccess"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"false"</code></td>
                                            <td>"Allow scripts to read/write the local file system"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"AllowNetworkAccess"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"false"</code></td>
                                            <td>"Allow scripts to make HTTP requests"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"SandboxEnabled"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"true"</code></td>
                                            <td>"Run scripts in an isolated sandbox"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <p>"SoulService enforces a capability-based security model:"</p>
                            <ol class="numbered-list">
                                <li><strong>"Default: Sandboxed"</strong>" — Scripts run in isolation with no host access"</li>
                                <li><strong>"Opt-in Permissions"</strong>" — Enable file system or network access per-Space"</li>
                                <li><strong>"AI Guardrails"</strong>" — AI-generated code is reviewed before execution"</li>
                                <li><strong>"Audit Trail"</strong>" — All permission escalations are logged"</li>
                            </ol>
                        </div>
                    </section>

                    // =============================================================
                    // RENDERING SERVICES SECTION
                    // =============================================================
                    <section id="rendering" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Rendering Services"
                        </h2>

                        <div id="rendering-material-service" class="subsection">
                            <h3>"MaterialService"</h3>
                            <p>
                                "MaterialService loads and manages all material definitions used by
                                parts. It reads " <code>".mat.toml"</code> " preset files from the
                                assets directory and provides a MaterialRegistry of named materials
                                with base color textures, normal maps, roughness, and metallic
                                values."
                            </p>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Property"</th>
                                            <th>"Type"</th>
                                            <th>"Default"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"Use2022Materials"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"true"</code></td>
                                            <td>"Use modern PBR textures instead of legacy flat colors"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"AssetPath"</code></td>
                                            <td>"string"</td>
                                            <td><code>"materials/"</code></td>
                                            <td>"Directory path for material texture assets"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"material.mat.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[material]
name = "CustomBrick"
base_color_texture = "textures/brick_albedo.png"
normal_map_texture = "textures/brick_normal.png"
roughness = 0.8
metallic = 0.0"#}</code></pre>
                            </div>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"let part = workspace.FindFirstChild("Wall");
part.Material = "Brick";

// Or use a custom material:
part.MaterialVariant = "CustomBrick";"#}</code></pre>
                            </div>
                        </div>

                        <div id="rendering-sound-service" class="subsection">
                            <h3>"SoundService"</h3>
                            <p>
                                "SoundService controls the global audio environment. It manages the
                                distance attenuation model, Doppler effect, and rolloff settings.
                                Sound objects in the world reference these global settings for
                                spatialized 3D audio."
                            </p>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Property"</th>
                                            <th>"Type"</th>
                                            <th>"Default"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"AmbientReverb"</code></td>
                                            <td>"enum"</td>
                                            <td><code>"NoReverb"</code></td>
                                            <td>"Environment reverb preset (Cave, Hall, Room, etc.)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"DistanceFactor"</code></td>
                                            <td>"float"</td>
                                            <td><code>"3.33"</code></td>
                                            <td>"Scale factor mapping studs to audio distance"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"DopplerScale"</code></td>
                                            <td>"float"</td>
                                            <td><code>"1.0"</code></td>
                                            <td>"Doppler effect intensity (0 = disabled)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"RolloffScale"</code></td>
                                            <td>"float"</td>
                                            <td><code>"1.0"</code></td>
                                            <td>"How quickly sounds fade with distance"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"VolumetricAudio"</code></td>
                                            <td>"bool"</td>
                                            <td><code>"true"</code></td>
                                            <td>"Enable spatial 3D audio processing"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Reverb Preset"</th>
                                            <th>"Use Case"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr><td><code>"NoReverb"</code></td><td>"Outdoors, open spaces"</td></tr>
                                        <tr><td><code>"Cave"</code></td><td>"Underground, tunnels"</td></tr>
                                        <tr><td><code>"Hall"</code></td><td>"Large indoor spaces, churches"</td></tr>
                                        <tr><td><code>"Room"</code></td><td>"Standard indoor rooms"</td></tr>
                                        <tr><td><code>"Forest"</code></td><td>"Dense vegetation, muffled"</td></tr>
                                        <tr><td><code>"Underwater"</code></td><td>"Submerged, heavy filtering"</td></tr>
                                    </tbody>
                                </table>
                            </div>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"let sound_service = game.get_service("SoundService");

// Cave environment
sound_service.AmbientReverb = "Cave";
sound_service.RolloffScale = 1.5;

// Disable Doppler for a puzzle game
sound_service.DopplerScale = 0;"#}</code></pre>
                            </div>
                        </div>

                        <div id="rendering-adornment-service" class="subsection">
                            <h3>"AdornmentService"</h3>
                            <p>
                                "AdornmentService provides visual decoration objects that overlay
                                or surround parts. These include SelectionBox (wireframe outline),
                                Highlight (glow/outline effect), BillboardGui (world-space UI),
                                SurfaceGui (texture-space UI), and Beam (particle trail between
                                attachments)."
                            </p>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/cube.svg" alt="SelectionBox" />
                                    </div>
                                    <h4>"SelectionBox"</h4>
                                    <p>"Wireframe outline for selection indicators"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/sparkles.svg" alt="Highlight" />
                                    </div>
                                    <h4>"Highlight"</h4>
                                    <p>"Glow and outline effect for hover cues"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/template.svg" alt="BillboardGui" />
                                    </div>
                                    <h4>"BillboardGui"</h4>
                                    <p>"Camera-facing GUI for nametags and labels"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/monitor.svg" alt="SurfaceGui" />
                                    </div>
                                    <h4>"SurfaceGui"</h4>
                                    <p>"GUI mapped to a part face for in-world screens"</p>
                                </div>
                            </div>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Hover highlight
let highlight = Instance.new("Highlight");
highlight.FillColor = Color3.new(0, 0.5, 1);
highlight.FillTransparency = 0.5;
highlight.OutlineColor = Color3.new(0, 0.7, 1);
highlight.Parent = hovered_part;"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // Navigation footer
                    <nav class="docs-nav-footer">
                        <a href="/docs/ui" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"UI Systems"</span>
                            </div>
                        </a>
                        <a href="/docs/simulation" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Simulation"</span>
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
