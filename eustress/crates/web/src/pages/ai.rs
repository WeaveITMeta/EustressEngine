// =============================================================================
// Eustress Web - AI Page (Industrial Design)
// =============================================================================
// AI-powered creation tools: Soul Language, asset generation, NPC behavior,
// terrain sculpting, and code assistance — all integrated into the engine.
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};
use crate::state::AppState;
use crate::api::ApiClient;

// -----------------------------------------------------------------------------
// Data Types
// -----------------------------------------------------------------------------

/// AI tool card data.
#[derive(Clone, Debug, PartialEq)]
struct AiTool {
    name: &'static str,
    description: &'static str,
    icon: &'static str,
    status: &'static str,
    features: Vec<&'static str>,
}

/// Soul Language example.
#[derive(Clone, Debug)]
struct SoulExample {
    prompt: &'static str,
    generated_code: &'static str,
    description: &'static str,
}

// -----------------------------------------------------------------------------
// Static Data
// -----------------------------------------------------------------------------

fn get_ai_tools() -> Vec<AiTool> {
    vec![
        AiTool {
            name: "Soul Language",
            description: "Write game logic in natural English. Soul compiles your intent into optimized Rust code that runs at native speed.",
            icon: "/assets/icons/sparkles.svg",
            status: "Available",
            features: vec![
                "Natural language to Rust compilation",
                "Context-aware code generation",
                "Type-safe output with error recovery",
                "Hot-reload compatible",
            ],
        },
        AiTool {
            name: "Asset Generator",
            description: "Generate 3D models, textures, and materials from text descriptions. Create entire asset libraries in minutes.",
            icon: "/assets/icons/cube.svg",
            status: "Beta",
            features: vec![
                "Text-to-3D model generation",
                "PBR texture synthesis",
                "Material graph creation",
                "Style-consistent batch generation",
            ],
        },
        AiTool {
            name: "NPC Behavior",
            description: "Design intelligent NPCs with goal-oriented action planning. Define personalities and let AI handle the rest.",
            icon: "/assets/icons/users.svg",
            status: "Available",
            features: vec![
                "Goal-oriented action planning (GOAP)",
                "Behavior tree generation",
                "Dialogue system integration",
                "Personality-driven responses",
            ],
        },
        AiTool {
            name: "Terrain Sculptor",
            description: "Describe landscapes in words and watch them materialize. AI-driven terrain generation with biome awareness.",
            icon: "/assets/icons/image.svg",
            status: "Beta",
            features: vec![
                "Text-to-terrain generation",
                "Biome-aware vegetation placement",
                "Erosion and weathering simulation",
                "Seamless LOD integration",
            ],
        },
        AiTool {
            name: "Code Assistant",
            description: "Integrated AI pair programmer that understands your project. Get suggestions, refactors, and bug fixes in real-time.",
            icon: "/assets/icons/code.svg",
            status: "Available",
            features: vec![
                "Project-aware completions",
                "ECS pattern suggestions",
                "Performance optimization hints",
                "Automatic test generation",
            ],
        },
        AiTool {
            name: "Audio Designer",
            description: "Generate sound effects, ambient audio, and adaptive music tracks from descriptions. AI-powered spatial audio.",
            icon: "/assets/icons/audio.svg",
            status: "Coming Soon",
            features: vec![
                "Text-to-sound effect generation",
                "Adaptive music composition",
                "Spatial audio placement",
                "Dynamic mixing based on gameplay",
            ],
        },
    ]
}

fn get_soul_examples() -> Vec<SoulExample> {
    vec![
        SoulExample {
            prompt: "When the player touches a coin, play a collect sound, add 10 points to the score, and destroy the coin with a sparkle effect.",
            generated_code: r#"fn coin_collect_system(
    mut commands: Commands,
    mut collisions: MessageReader<CollisionStarted>,
    mut score: ResMut<Score>,
    audio: Res<AudioManager>,
    coins: Query<Entity, With<Coin>>,
    players: Query<Entity, With<Player>>,
) {
    for CollisionStarted(a, b) in collisions.read() {
        let (player, coin) = if players.contains(*a) && coins.contains(*b) {
            (*a, *b)
        } else if players.contains(*b) && coins.contains(*a) {
            (*b, *a)
        } else {
            continue;
        };
        
        score.points += 10;
        audio.play("collect_coin");
        commands.entity(coin).despawn();
        // Sparkle effect spawned at coin position
        commands.spawn(ParticleEffect::sparkle());
    }
}"#,
            description: "Soul understands game concepts like 'touch', 'collect', 'score', and 'destroy' and maps them to ECS patterns.",
        },
        SoulExample {
            prompt: "Make a day-night cycle that takes 10 minutes for a full day. The sun should move across the sky and the ambient light should change color.",
            generated_code: r#"fn day_night_system(
    time: Res<Time>,
    mut sun: Query<(&mut Transform, &mut DirectionalLight), With<Sun>>,
    mut ambient: ResMut<AmbientLight>,
) {
    let cycle_duration = 600.0; // 10 minutes
    let progress = (time.elapsed_secs() % cycle_duration) / cycle_duration;
    let angle = progress * std::f32::consts::TAU;
    
    if let Ok((mut transform, mut light)) = sun.single_mut() {
        transform.rotation = Quat::from_rotation_x(-angle);
        
        // Dawn/dusk warm, midday bright, night dark
        let intensity = (angle.sin()).max(0.0);
        light.illuminance = intensity * 100_000.0;
        light.color = Color::srgb(
            1.0,
            0.8 + 0.2 * intensity,
            0.6 + 0.4 * intensity,
        );
    }
    
    let ambient_intensity = 0.05 + 0.15 * (angle.sin()).max(0.0);
    ambient.brightness = ambient_intensity;
}"#,
            description: "Soul translates time-based concepts into smooth mathematical cycles with proper lighting calculations.",
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// AI tools and generation page — industrial design.
#[component]
pub fn AiPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let selected_tool = RwSignal::new(0usize);
    let tools = get_ai_tools();

    view! {
        <div class="page page-ai-industrial">
            <CentralNav active="ai".to_string() />

            // Background
            <div class="ai-bg">
                <div class="ai-grid-overlay"></div>
                <div class="ai-glow glow-1"></div>
                <div class="ai-glow glow-2"></div>
            </div>

            // ═══════════════════════════════════════════════════════════════
            // HERO SECTION
            // ═══════════════════════════════════════════════════════════════
            <section class="ai-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"AI TOOLS"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="ai-title">"Create at the "<span class="title-accent">"Speed of Thought"</span></h1>
                <p class="ai-tagline">
                    "Eustress integrates AI directly into every stage of creation."<br/>
                    "From natural-language scripting with Soul to procedural world generation,"<br/>
                    "our AI tools amplify your creativity without replacing it."
                </p>
                <div class="ai-stats-bar">
                    <div class="ai-stat">
                        <span class="stat-value">"6"</span>
                        <span class="stat-label">"AI Tools"</span>
                    </div>
                    <div class="stat-sep"></div>
                    <div class="ai-stat">
                        <span class="stat-value">"10x"</span>
                        <span class="stat-label">"Faster Iteration"</span>
                    </div>
                    <div class="stat-sep"></div>
                    <div class="ai-stat">
                        <span class="stat-value">"0"</span>
                        <span class="stat-label">"Lock-in"</span>
                    </div>
                </div>
            </section>

            // ═══════════════════════════════════════════════════════════════
            // API KEYS (for signed-in users)
            // ═══════════════════════════════════════════════════════════════
            <Show when=move || app_state.auth.get().is_authenticated()>
                <super::projects::ApiKeysSection />
            </Show>

            // ═══════════════════════════════════════════════════════════════
            // AI TOOL CARDS
            // ═══════════════════════════════════════════════════════════════
            <section class="ai-tools-section">
                <div class="section-header">
                    <span class="section-tag">"INTEGRATED TOOLS"</span>
                    <h2 class="section-title-epic">"AI That Understands Simulation"</h2>
                    <p class="section-desc">"Purpose-built AI tools trained on development patterns, not generic models."</p>
                </div>

                <div class="ai-tools-grid">
                    {tools.iter().enumerate().map(|(index, tool)| {
                        let is_selected = move || selected_tool.get() == index;
                        let status_class = match tool.status {
                            "Available" => "status-available",
                            "Beta" => "status-beta",
                            _ => "status-coming",
                        };
                        view! {
                            <div
                                class="ai-tool-card"
                                class:selected=is_selected
                                on:click=move |_| selected_tool.set(index)
                            >
                                <div class="tool-header">
                                    <img src={tool.icon} alt={tool.name} class="tool-icon" />
                                    <div class="tool-title-row">
                                        <h3 class="tool-name">{tool.name}</h3>
                                        <span class=format!("tool-status {}", status_class)>{tool.status}</span>
                                    </div>
                                </div>
                                <p class="tool-description">{tool.description}</p>
                                <ul class="tool-features">
                                    {tool.features.iter().map(|feature| {
                                        view! {
                                            <li class="tool-feature">
                                                <span class="feature-check">"+"</span>
                                                {*feature}
                                            </li>
                                        }
                                    }).collect::<Vec<_>>()}
                                </ul>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </section>

            // ═══════════════════════════════════════════════════════════════
            // HOW IT WORKS
            // ═══════════════════════════════════════════════════════════════
            <section class="ai-how-it-works">
                <div class="section-header">
                    <span class="section-tag">"ARCHITECTURE"</span>
                    <h2 class="section-title-epic">"How It Works"</h2>
                    <p class="section-desc">"AI runs locally where possible, with cloud fallback for heavy generation tasks."</p>
                </div>

                <div class="architecture-flow">
                    <div class="flow-step">
                        <div class="step-number">"1"</div>
                        <h4>"Describe"</h4>
                        <p>"Write your intent in Soul Language or use the generation panel in the editor."</p>
                    </div>
                    <div class="flow-arrow">"→"</div>
                    <div class="flow-step">
                        <div class="step-number">"2"</div>
                        <h4>"Analyze"</h4>
                        <p>"AI parses context from your project: ECS components, assets, existing code, and scene graph."</p>
                    </div>
                    <div class="flow-arrow">"→"</div>
                    <div class="flow-step">
                        <div class="step-number">"3"</div>
                        <h4>"Generate"</h4>
                        <p>"Type-safe code or assets are generated, validated against your project schema, and compiled."</p>
                    </div>
                    <div class="flow-arrow">"→"</div>
                    <div class="flow-step">
                        <div class="step-number">"4"</div>
                        <h4>"Hot Reload"</h4>
                        <p>"Changes are injected live into your running experience. See results instantly."</p>
                    </div>
                </div>
            </section>

            // ═══════════════════════════════════════════════════════════════
            // PRINCIPLES
            // ═══════════════════════════════════════════════════════════════
            <section class="ai-principles">
                <div class="section-header">
                    <span class="section-tag">"PHILOSOPHY"</span>
                    <h2 class="section-title-epic">"AI That Respects Creators"</h2>
                </div>

                <div class="principles-grid">
                    <div class="principle-card">
                        <h3>"Transparent"</h3>
                        <p>"Every AI generation shows its reasoning. You see what it does and why. No black boxes."</p>
                    </div>
                    <div class="principle-card">
                        <h3>"Editable"</h3>
                        <p>"All generated code and assets are fully editable. AI is a starting point, not a cage."</p>
                    </div>
                    <div class="principle-card">
                        <h3>"Opt-in"</h3>
                        <p>"Every AI feature is optional. Build entirely by hand if you prefer. Zero lock-in."</p>
                    </div>
                    <div class="principle-card">
                        <h3>"Private"</h3>
                        <p>"Your project data never leaves your machine for local models. Cloud features are explicit."</p>
                    </div>
                </div>
            </section>

            // ═══════════════════════════════════════════════════════════════
            // CTA
            // ═══════════════════════════════════════════════════════════════
            <section class="ai-cta">
                <div class="cta-bg">
                    <div class="cta-grid-overlay"></div>
                    <div class="cta-glow-orb"></div>
                </div>
                <div class="cta-container">
                    <h2 class="cta-headline">"Ready to Build with "<span class="cta-accent">"AI"</span>"?"</h2>
                    <p class="cta-subtext">"Download Eustress Engine and start creating with Soul Language today."</p>
                    <div class="cta-buttons">
                        <a href="/download" class="ai-cta-primary">"Download Engine →"</a>
                        <a href="/docs/scripting" class="ai-cta-secondary">"Soul Language Docs"</a>
                    </div>
                </div>
            </section>

            <Footer />
        </div>
    }
}
