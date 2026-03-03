// =============================================================================
// Eustress Web - Scripting Documentation Page (Industrial Design)
// =============================================================================
// Comprehensive Soul scripting documentation with floating TOC
// Covers: Soul Language, Rune scripting, ECS patterns, systems, events,
// resources, queries, hot reload, debugging, and API reference.
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
                TocSubsection { id: "overview-languages", title: "Language Options" },
                TocSubsection { id: "overview-philosophy", title: "Design Philosophy" },
            ],
        },
        TocSection {
            id: "soul",
            title: "Soul Language",
            subsections: vec![
                TocSubsection { id: "soul-basics", title: "Basics" },
                TocSubsection { id: "soul-syntax", title: "Syntax Rules" },
                TocSubsection { id: "soul-compilation", title: "Compilation Pipeline" },
                TocSubsection { id: "soul-examples", title: "Examples" },
            ],
        },
        TocSection {
            id: "ecs",
            title: "ECS Patterns",
            subsections: vec![
                TocSubsection { id: "ecs-entities", title: "Entities" },
                TocSubsection { id: "ecs-components", title: "Components" },
                TocSubsection { id: "ecs-systems", title: "Systems" },
                TocSubsection { id: "ecs-queries", title: "Queries" },
            ],
        },
        TocSection {
            id: "events",
            title: "Events and Messages",
            subsections: vec![
                TocSubsection { id: "events-messages", title: "Message System" },
                TocSubsection { id: "events-observers", title: "Observers" },
                TocSubsection { id: "events-custom", title: "Custom Events" },
            ],
        },
        TocSection {
            id: "resources",
            title: "Resources",
            subsections: vec![
                TocSubsection { id: "resources-global", title: "Global Resources" },
                TocSubsection { id: "resources-assets", title: "Asset Loading" },
                TocSubsection { id: "resources-state", title: "State Management" },
            ],
        },
        TocSection {
            id: "services",
            title: "Services",
            subsections: vec![
                TocSubsection { id: "services-workspace", title: "Workspace" },
                TocSubsection { id: "services-player", title: "PlayerService" },
                TocSubsection { id: "services-datastore", title: "DataStoreService" },
                TocSubsection { id: "services-teleport", title: "TeleportService" },
            ],
        },
        TocSection {
            id: "hotreload",
            title: "Hot Reload",
            subsections: vec![
                TocSubsection { id: "hotreload-setup", title: "Setup" },
                TocSubsection { id: "hotreload-workflow", title: "Workflow" },
                TocSubsection { id: "hotreload-limitations", title: "Limitations" },
            ],
        },
        TocSection {
            id: "debugging",
            title: "Debugging",
            subsections: vec![
                TocSubsection { id: "debugging-inspector", title: "ECS Inspector" },
                TocSubsection { id: "debugging-logging", title: "Logging" },
                TocSubsection { id: "debugging-profiling", title: "Performance Profiling" },
            ],
        },
        TocSection {
            id: "api",
            title: "API Reference",
            subsections: vec![
                TocSubsection { id: "api-prelude", title: "Prelude" },
                TocSubsection { id: "api-math", title: "Math Types" },
                TocSubsection { id: "api-input", title: "Input" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Scripting documentation page with floating TOC.
#[component]
pub fn DocsScriptingPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-scripting"></div>
            </div>

            <div class="docs-layout">
                // Floating TOC Sidebar
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/code.svg" alt="Scripting" class="toc-icon" />
                        <h2>"Scripting"</h2>
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
                            <span class="current">"Scripting"</span>
                        </div>
                        <h1 class="docs-title">"Scripting System"</h1>
                        <p class="docs-subtitle">
                            "Two ways to script your experiences: Soul Language for natural-language creation, 
                            and Rune for full programmatic control. Both compile to native Rust performance."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "30 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Beginner to Advanced"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "v0.16.1"
                            </span>
                        </div>
                    </header>

                    // ─────────────────────────────────────────────────────
                    // Overview
                    // ─────────────────────────────────────────────────────
                    <section id="overview" class="docs-section">
                        <h2 class="section-anchor">"Overview"</h2>

                        <div id="overview-intro" class="docs-block">
                            <h3>"Introduction"</h3>
                            <p>
                                "Eustress Engine provides two scripting approaches that work together seamlessly. 
                                Soul Language lets you describe behavior in plain English, which compiles into 
                                type-safe Rust code. For developers who want full control, Rune provides a 
                                dynamic scripting language with direct ECS access."
                            </p>
                            <div class="docs-callout info">
                                <strong>"Key Insight:"</strong>
                                " Both Soul and Rune scripts hot-reload instantly. Change your code, see results 
                                immediately in the running experience."
                            </div>
                        </div>

                        <div id="overview-languages" class="docs-block">
                            <h3>"Language Options"</h3>
                            <div class="comparison-cards">
                                <div class="lang-card soul">
                                    <h4>"Soul Language"</h4>
                                    <p>"Natural English scripting that compiles to Rust. Best for rapid prototyping, game designers, and beginners."</p>
                                    <ul>
                                        <li>"Write in plain English"</li>
                                        <li>"AI-assisted compilation"</li>
                                        <li>"Type-safe output"</li>
                                        <li>"Zero performance overhead"</li>
                                    </ul>
                                </div>
                                <div class="lang-card rune">
                                    <h4>"Rune"</h4>
                                    <p>"Dynamic scripting language with Rust-like syntax. Best for complex logic, experienced developers, and runtime flexibility."</p>
                                    <ul>
                                        <li>"Rust-like syntax"</li>
                                        <li>"Full ECS access"</li>
                                        <li>"Runtime hot-reload"</li>
                                        <li>"Pattern matching and closures"</li>
                                    </ul>
                                </div>
                            </div>
                        </div>

                        <div id="overview-philosophy" class="docs-block">
                            <h3>"Design Philosophy"</h3>
                            <p>
                                "Eustress scripting follows the principle of progressive disclosure. Start simple 
                                with Soul, graduate to Rune when you need more control, and drop down to native 
                                Rust plugins when you need maximum performance. All three interoperate seamlessly."
                            </p>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Soul Language
                    // ─────────────────────────────────────────────────────
                    <section id="soul" class="docs-section">
                        <h2 class="section-anchor">"Soul Language"</h2>

                        <div id="soul-basics" class="docs-block">
                            <h3>"Basics"</h3>
                            <p>
                                "Soul scripts are plain text files with the "<code>".soul"</code>" extension. Each file 
                                describes one or more behaviors in natural English. The Soul compiler analyzes your 
                                project context and generates idiomatic Rust/Bevy ECS code."
                            </p>
                            <pre class="code-block"><code>{"// player_movement.soul
When the player presses W, move them forward at 5 meters per second.
When the player presses Space and is on the ground, apply an upward
impulse of 8 units for jumping.
When the player is in the air, apply gravity at 35 units per second squared."}</code></pre>
                        </div>

                        <div id="soul-syntax" class="docs-block">
                            <h3>"Syntax Rules"</h3>
                            <p>"Soul has minimal syntax rules — it is designed to be as close to natural English as possible:"</p>
                            <ul class="docs-list">
                                <li><strong>"When"</strong>" — Starts a reactive behavior (maps to a system with run conditions)"</li>
                                <li><strong>"If"</strong>" — Adds a conditional branch within a behavior"</li>
                                <li><strong>"Every X seconds"</strong>" — Creates a timer-based system"</li>
                                <li><strong>"On [event]"</strong>" — Listens for a specific event (collision, input, message)"</li>
                                <li>"Lines starting with "<code>"//"</code>" are comments"</li>
                            </ul>
                        </div>

                        <div id="soul-compilation" class="docs-block">
                            <h3>"Compilation Pipeline"</h3>
                            <p>"Soul compilation is a multi-stage process:"</p>
                            <ol class="docs-list numbered">
                                <li><strong>"Parse"</strong>" — Natural language is tokenized and intent is extracted"</li>
                                <li><strong>"Resolve"</strong>" — References to entities, components, and assets are resolved against your project"</li>
                                <li><strong>"Generate"</strong>" — Idiomatic Rust ECS code is emitted with proper types"</li>
                                <li><strong>"Validate"</strong>" — Generated code is type-checked by the Rust compiler"</li>
                                <li><strong>"Hot Reload"</strong>" — Compiled code is injected into the running experience"</li>
                            </ol>
                        </div>

                        <div id="soul-examples" class="docs-block">
                            <h3>"Examples"</h3>
                            <p>"Here is a complete Soul script for a collectible coin system:"</p>
                            <pre class="code-block"><code>{"// collectibles.soul

When the player touches a Coin entity:
  - Play the \"coin_collect\" sound effect
  - Add 10 to the player's score
  - Spawn a sparkle particle effect at the coin's position
  - Destroy the coin

Every 30 seconds, spawn a new Coin at a random position
within the play area boundaries.

When the player's score reaches 100, display the
\"You Win!\" message and transition to the victory scene."}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // ECS Patterns
                    // ─────────────────────────────────────────────────────
                    <section id="ecs" class="docs-section">
                        <h2 class="section-anchor">"ECS Patterns"</h2>

                        <div id="ecs-entities" class="docs-block">
                            <h3>"Entities"</h3>
                            <p>
                                "Entities are lightweight identifiers (just a u64 index + generation). They have no 
                                data of their own — all state is stored in components attached to them."
                            </p>
                            <pre class="code-block"><code>{"// Spawn an entity with components
let player = commands.spawn((
    Transform::from_xyz(0.0, 5.0, 0.0),
    Player { health: 100.0 },
    RigidBody::Dynamic,
    Collider::capsule(0.5, 1.8),
    Name::new(\"Player\"),
)).id();"}</code></pre>
                        </div>

                        <div id="ecs-components" class="docs-block">
                            <h3>"Components"</h3>
                            <p>
                                "Components are plain Rust structs that derive "<code>"Component"</code>". They hold data 
                                and nothing else — no methods, no inheritance."
                            </p>
                            <pre class="code-block"><code>{"#[derive(Component, Reflect)]
struct Health {
    current: f32,
    maximum: f32,
}

#[derive(Component, Reflect)]
struct Speed(f32);

#[derive(Component, Reflect, Default)]
struct Player;"}</code></pre>
                        </div>

                        <div id="ecs-systems" class="docs-block">
                            <h3>"Systems"</h3>
                            <p>
                                "Systems are functions that operate on entities matching specific component queries. 
                                They run every frame (or on a fixed timestep) and are the primary way to implement game logic."
                            </p>
                            <pre class="code-block"><code>{"fn movement_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&mut Transform, &Speed), With<Player>>,
) {
    let delta = time.delta_secs();
    for (mut transform, speed) in query.iter_mut() {
        let mut direction = Vec3::ZERO;
        if keys.pressed(KeyCode::KeyW) { direction.z -= 1.0; }
        if keys.pressed(KeyCode::KeyS) { direction.z += 1.0; }
        if keys.pressed(KeyCode::KeyA) { direction.x -= 1.0; }
        if keys.pressed(KeyCode::KeyD) { direction.x += 1.0; }
        
        if direction != Vec3::ZERO {
            transform.translation += direction.normalize() * speed.0 * delta;
        }
    }
}"}</code></pre>
                        </div>

                        <div id="ecs-queries" class="docs-block">
                            <h3>"Queries"</h3>
                            <p>
                                "Queries filter entities by their component composition. Use "<code>"With"</code>" and 
                                "<code>"Without"</code>" for filtering, and "<code>"Changed"</code>" or "<code>"Added"</code>" for change detection."
                            </p>
                            <pre class="code-block"><code>{"// Query all enemies with health below 50%
fn low_health_warning(
    query: Query<(&Health, &Name), (With<Enemy>, Without<Dead>)>,
) {
    for (health, name) in query.iter() {
        if health.current < health.maximum * 0.5 {
            info!(\"{} is low on health!\", name);
        }
    }
}"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Events and Messages
                    // ─────────────────────────────────────────────────────
                    <section id="events" class="docs-section">
                        <h2 class="section-anchor">"Events and Messages"</h2>

                        <div id="events-messages" class="docs-block">
                            <h3>"Message System"</h3>
                            <p>
                                "Eustress uses Bevy's message system for inter-system communication. Messages are 
                                written by one system and read by others, enabling decoupled game logic."
                            </p>
                            <pre class="code-block"><code>{"#[derive(Message, Clone)]
struct DamageEvent {
    target: Entity,
    amount: f32,
    source: Entity,
}

// Writer system
fn attack_system(mut damage: MessageWriter<DamageEvent>) {
    damage.write(DamageEvent {
        target: enemy,
        amount: 25.0,
        source: player,
    });
}

// Reader system
fn health_system(
    mut events: MessageReader<DamageEvent>,
    mut query: Query<&mut Health>,
) {
    for event in events.read() {
        if let Ok(mut health) = query.get_mut(event.target) {
            health.current -= event.amount;
        }
    }
}"}</code></pre>
                        </div>

                        <div id="events-observers" class="docs-block">
                            <h3>"Observers"</h3>
                            <p>
                                "Observers react to lifecycle events like entity spawning, component insertion, 
                                or removal. They run immediately when triggered."
                            </p>
                            <pre class="code-block"><code>{"// Run when Health component is added to any entity
app.add_observer(on_health_added);

fn on_health_added(trigger: Trigger<OnAdd, Health>, query: Query<&Name>) {
    let entity = trigger.target();
    if let Ok(name) = query.get(entity) {
        info!(\"Health component added to {}\", name);
    }
}"}</code></pre>
                        </div>

                        <div id="events-custom" class="docs-block">
                            <h3>"Custom Events"</h3>
                            <p>
                                "Define custom events by deriving "<code>"Message"</code>" and register them with 
                                "<code>"app.add_message::<YourEvent>()"</code>". Events are buffered per-frame and 
                                automatically cleaned up."
                            </p>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Resources
                    // ─────────────────────────────────────────────────────
                    <section id="resources" class="docs-section">
                        <h2 class="section-anchor">"Resources"</h2>

                        <div id="resources-global" class="docs-block">
                            <h3>"Global Resources"</h3>
                            <p>
                                "Resources are singleton data accessible to all systems. Use them for global state 
                                like scores, configuration, or shared caches."
                            </p>
                            <pre class="code-block"><code>{"#[derive(Resource, Default)]
struct GameState {
    score: u32,
    level: u32,
    paused: bool,
}

// Insert in plugin
app.init_resource::<GameState>();

// Access in systems
fn score_display(state: Res<GameState>) {
    info!(\"Score: {} | Level: {}\", state.score, state.level);
}"}</code></pre>
                        </div>

                        <div id="resources-assets" class="docs-block">
                            <h3>"Asset Loading"</h3>
                            <p>
                                "Assets are loaded asynchronously through the "<code>"AssetServer"</code>". Handles 
                                are lightweight references that resolve when loading completes."
                            </p>
                            <pre class="code-block"><code>{"fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Load a GLTF scene
    let scene = asset_server.load(\"models/character.gltf#Scene0\");
    commands.spawn(SceneRoot(scene));
    
    // Load a texture
    let texture: Handle<Image> = asset_server.load(\"textures/grass.png\");
}"}</code></pre>
                        </div>

                        <div id="resources-state" class="docs-block">
                            <h3>"State Management"</h3>
                            <p>
                                "Use Bevy States for game-wide mode transitions like menus, gameplay, and loading screens. 
                                Systems can be configured to only run in specific states."
                            </p>
                            <pre class="code-block"><code>{"#[derive(States, Default, Debug, Clone, PartialEq, Eq, Hash)]
enum AppState {
    #[default]
    Loading,
    MainMenu,
    InGame,
    Paused,
}

// Only run during gameplay
app.add_systems(Update, gameplay_systems.run_if(in_state(AppState::InGame)));"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Services
                    // ─────────────────────────────────────────────────────
                    <section id="services" class="docs-section">
                        <h2 class="section-anchor">"Services"</h2>

                        <div id="services-workspace" class="docs-block">
                            <h3>"Workspace"</h3>
                            <p>
                                "The Workspace service is the root of the scene hierarchy. It manages the active 
                                scene, gravity, lighting environment, and global simulation settings."
                            </p>
                        </div>

                        <div id="services-player" class="docs-block">
                            <h3>"PlayerService"</h3>
                            <p>
                                "PlayerService manages connected players, their characters, spawn locations, and 
                                team assignments. It provides methods for respawning, teleporting, and querying 
                                player state."
                            </p>
                        </div>

                        <div id="services-datastore" class="docs-block">
                            <h3>"DataStoreService"</h3>
                            <p>
                                "DataStoreService provides persistent key-value storage for saving player progress, 
                                settings, and game state across sessions. Data is automatically replicated to the 
                                server for multiplayer."
                            </p>
                        </div>

                        <div id="services-teleport" class="docs-block">
                            <h3>"TeleportService"</h3>
                            <p>
                                "TeleportService handles moving players between experiences (places) and servers. 
                                It manages matchmaking, reserved servers, and cross-experience data transfer."
                            </p>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Hot Reload
                    // ─────────────────────────────────────────────────────
                    <section id="hotreload" class="docs-section">
                        <h2 class="section-anchor">"Hot Reload"</h2>

                        <div id="hotreload-setup" class="docs-block">
                            <h3>"Setup"</h3>
                            <p>
                                "Hot reload is enabled by default in the Eustress editor. Soul scripts and Rune 
                                scripts reload automatically when saved. For native Rust plugins, enable the 
                                "<code>"hot-reload"</code>" feature flag."
                            </p>
                        </div>

                        <div id="hotreload-workflow" class="docs-block">
                            <h3>"Workflow"</h3>
                            <ol class="docs-list numbered">
                                <li>"Edit your script in the integrated editor or any external editor"</li>
                                <li>"Save the file (Ctrl+S)"</li>
                                <li>"The file watcher detects the change and triggers recompilation"</li>
                                <li>"New systems replace old ones without restarting the experience"</li>
                                <li>"Entity state is preserved — only logic changes"</li>
                            </ol>
                        </div>

                        <div id="hotreload-limitations" class="docs-block">
                            <h3>"Limitations"</h3>
                            <div class="docs-callout warning">
                                <strong>"Note:"</strong>
                                " Hot reload cannot change component data layouts (adding/removing fields). 
                                If you change a component struct, a full restart is required. System logic 
                                changes always hot-reload safely."
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Debugging
                    // ─────────────────────────────────────────────────────
                    <section id="debugging" class="docs-section">
                        <h2 class="section-anchor">"Debugging"</h2>

                        <div id="debugging-inspector" class="docs-block">
                            <h3>"ECS Inspector"</h3>
                            <p>
                                "The built-in ECS Inspector lets you browse all entities, view their components, 
                                and edit values in real-time. Open it with "<code>"F12"</code>" or from the View menu."
                            </p>
                        </div>

                        <div id="debugging-logging" class="docs-block">
                            <h3>"Logging"</h3>
                            <p>
                                "Use the "<code>"tracing"</code>" crate macros for structured logging. Output appears 
                                in the Output panel with severity-based filtering."
                            </p>
                            <pre class="code-block"><code>{"use tracing::{info, warn, error, debug};

fn my_system() {
    info!(\"Player spawned at position\");
    warn!(\"Health is critically low\");
    error!(\"Failed to load asset\");
    debug!(\"Tick rate: {} Hz\", 120);
}"}</code></pre>
                        </div>

                        <div id="debugging-profiling" class="docs-block">
                            <h3>"Performance Profiling"</h3>
                            <p>
                                "Enable the "<code>"trace"</code>" feature for system-level profiling. Use Tracy or 
                                Chrome Tracing to visualize frame timings, system durations, and bottlenecks."
                            </p>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // API Reference
                    // ─────────────────────────────────────────────────────
                    <section id="api" class="docs-section">
                        <h2 class="section-anchor">"API Reference"</h2>

                        <div id="api-prelude" class="docs-block">
                            <h3>"Prelude"</h3>
                            <p>"Import the prelude to access all commonly used types:"</p>
                            <pre class="code-block"><code>{"use bevy::prelude::*;
use eustress_common::prelude::*;"}</code></pre>
                            <p>"This gives you access to: Transform, Vec3, Quat, Color, Entity, Commands, Query, Res, ResMut, Component, Resource, Plugin, App, and more."</p>
                        </div>

                        <div id="api-math" class="docs-block">
                            <h3>"Math Types"</h3>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"Vec2"</code>
                                    <span>"2D vector (x, y)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"Vec3"</code>
                                    <span>"3D vector (x, y, z)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"Vec4"</code>
                                    <span>"4D vector (x, y, z, w)"</span>
                                </div>
                                <div class="api-row">
                                    <code>"Quat"</code>
                                    <span>"Quaternion rotation"</span>
                                </div>
                                <div class="api-row">
                                    <code>"Mat4"</code>
                                    <span>"4x4 transformation matrix"</span>
                                </div>
                                <div class="api-row">
                                    <code>"Transform"</code>
                                    <span>"Position + Rotation + Scale"</span>
                                </div>
                            </div>
                        </div>

                        <div id="api-input" class="docs-block">
                            <h3>"Input"</h3>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"ButtonInput<KeyCode>"</code>
                                    <span>"Keyboard input — pressed(), just_pressed(), just_released()"</span>
                                </div>
                                <div class="api-row">
                                    <code>"ButtonInput<MouseButton>"</code>
                                    <span>"Mouse button input"</span>
                                </div>
                                <div class="api-row">
                                    <code>"AccumulatedMouseMotion"</code>
                                    <span>"Mouse movement delta per frame"</span>
                                </div>
                                <div class="api-row">
                                    <code>"Gamepads"</code>
                                    <span>"Connected gamepad state and axes"</span>
                                </div>
                            </div>
                        </div>
                    </section>

                    // Navigation footer
                    <div class="docs-nav-footer">
                        <a href="/docs/physics" class="nav-link next">
                            "Next: Physics System"
                            <img src="/assets/icons/arrow-right.svg" alt="Next" />
                        </a>
                    </div>
                </main>
            </div>

            <Footer />
        </div>
    }
}
