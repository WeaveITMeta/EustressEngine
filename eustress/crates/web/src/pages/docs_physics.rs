// =============================================================================
// Eustress Web - Physics Documentation Page
// =============================================================================
// Physics: Avian rigid bodies at a fixed 60 Hz step, Edit versus Play, how
// parts become bodies and colliders, joints, queries, characters and damage.
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
                TocSubsection { id: "overview-avian", title: "Avian at a Fixed Step" },
                TocSubsection { id: "overview-units", title: "SI Units" },
                TocSubsection { id: "overview-repeatable", title: "Repeatable Runs" },
                TocSubsection { id: "overview-scale", title: "Large Scenes" },
            ],
        },
        TocSection {
            id: "play",
            title: "Edit and Play",
            subsections: vec![
                TocSubsection { id: "play-edit", title: "Edit Mode" },
                TocSubsection { id: "play-start", title: "Starting Play" },
                TocSubsection { id: "play-stop", title: "What Stop Restores" },
            ],
        },
        TocSection {
            id: "bodies",
            title: "Bodies and Colliders",
            subsections: vec![
                TocSubsection { id: "bodies-properties", title: "Physics Properties" },
                TocSubsection { id: "bodies-mass", title: "Mass, Friction, Bounce" },
                TocSubsection { id: "bodies-shapes", title: "Collider Shapes" },
                TocSubsection { id: "bodies-size", title: "Size, Applied Once" },
            ],
        },
        TocSection {
            id: "joints",
            title: "Joints",
            subsections: vec![
                TocSubsection { id: "joints-imported", title: "Imported Constraints" },
                TocSubsection { id: "joints-mates", title: "Mates" },
                TocSubsection { id: "joints-limits", title: "Not Wired Yet" },
            ],
        },
        TocSection {
            id: "queries",
            title: "Queries and Contacts",
            subsections: vec![
                TocSubsection { id: "queries-rune", title: "Raycasts in Rune" },
                TocSubsection { id: "queries-agents", title: "Raycasts for Agents" },
                TocSubsection { id: "queries-touch", title: "Touched Events" },
            ],
        },
        TocSection {
            id: "characters",
            title: "Characters",
            subsections: vec![
                TocSubsection { id: "characters-body", title: "The Character Body" },
                TocSubsection { id: "characters-ground", title: "Ground, Slopes, Steps" },
                TocSubsection { id: "characters-climb", title: "Climbing and Feet" },
            ],
        },
        TocSection {
            id: "damage",
            title: "Deformation and Fracture",
            subsections: vec![
                TocSubsection { id: "damage-enable", title: "Making a Part Destructible" },
                TocSubsection { id: "damage-dent", title: "From Impact to Dent" },
                TocSubsection { id: "damage-fracture", title: "Fracture" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-materials", title: "Material-Driven Contact" },
                TocSubsection { id: "roadmap-joints", title: "Joints and Movers" },
                TocSubsection { id: "roadmap-scripts", title: "Script Physics" },
            ],
        },
    ]
}

/// The Play cycle: Edit keeps the clock paused and every body static, Play
/// snapshots the world and simulates, Stop restores the snapshot.
#[component]
fn PlayCycleDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 200" role="img"
                aria-label="Edit mode keeps the physics clock paused and every body static. Play unpauses the clock and makes unanchored parts dynamic. Stop restores the snapshot taken at Play, pauses the clock and returns to Edit.">
                <defs>
                    <marker id="play-cycle-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                // Edit
                <rect x="20" y="40" width="170" height="76" rx="8" class="dg-box"></rect>
                <text x="105" y="68" class="dg-label" text-anchor="middle">"Edit"</text>
                <text x="105" y="88" class="dg-note" text-anchor="middle">"physics clock paused"</text>
                <text x="105" y="104" class="dg-note" text-anchor="middle">"every body static"</text>

                <line x1="190" y1="78" x2="238" y2="78" class="dg-line" marker-end="url(#play-cycle-arrow)"></line>
                <text x="214" y="66" class="dg-note" text-anchor="middle">"F5 / F7"</text>

                // Play
                <rect x="240" y="40" width="170" height="76" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="325" y="68" class="dg-label" text-anchor="middle">"Play"</text>
                <text x="325" y="88" class="dg-note" text-anchor="middle">"steps at 60 Hz"</text>
                <text x="325" y="104" class="dg-note" text-anchor="middle">"unanchored parts dynamic"</text>

                <line x1="410" y1="78" x2="458" y2="78" class="dg-line" marker-end="url(#play-cycle-arrow)"></line>
                <text x="434" y="66" class="dg-note" text-anchor="middle">"F8 / Esc"</text>

                // Stop
                <rect x="460" y="40" width="160" height="76" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="540" y="68" class="dg-label" text-anchor="middle">"Stop"</text>
                <text x="540" y="88" class="dg-note" text-anchor="middle">"restore the snapshot"</text>
                <text x="540" y="104" class="dg-note" text-anchor="middle">"pause the clock"</text>

                // Back to Edit
                <line x1="540" y1="116" x2="540" y2="160" class="dg-line dg-line-dashed"></line>
                <line x1="540" y1="160" x2="105" y2="160" class="dg-line dg-line-dashed"></line>
                <line x1="105" y1="160" x2="105" y2="120" class="dg-line dg-line-dashed" marker-end="url(#play-cycle-arrow)"></line>
                <text x="322" y="182" class="dg-note" text-anchor="middle">"back to Edit, as it was when Play started"</text>
            </svg>
            <figcaption>
                "Edit mode never simulates. Play takes a snapshot, unpauses the physics clock and
                makes unanchored parts dynamic; Stop restores the snapshot and pauses the clock again."
            </figcaption>
        </figure>
    }
}

/// Physics documentation page.
#[component]
pub fn DocsPhysicsPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-physics"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/physics.svg" alt="Physics" class="toc-icon" />
                        <h2>"Physics"</h2>
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
                            <span class="current">"Physics"</span>
                        </div>
                        <h1 class="docs-title">"Physics"</h1>
                        <p class="docs-subtitle">
                            "Physics in Eustress is rigid-body simulation by Avian, stepped at a fixed 60 Hz
                            in SI units. Parts hold still while you edit; press Play and every unanchored part
                            falls and collides, and a destructible part can dent and crack. Press Stop and
                            the world returns to where it was."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "20 min read"
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

                        <div id="overview-avian" class="subsection">
                            <h3>"Avian at a Fixed Step"</h3>
                            <p>
                                "Eustress simulates rigid bodies with Avian 0.7, a physics engine built for Bevy,
                                the ECS that Eustress runs on. Avian does collision detection, contact solving,
                                joints, sleeping and spatial queries. Eustress decides which parts become bodies,
                                what shape their colliders take, when the physics clock runs, and what happens
                                when a part breaks."
                            </p>
                            <div class="stats-grid">
                                <div class="stat-card">
                                    <div class="stat-value">"60 Hz"</div>
                                    <div class="stat-label">"Fixed physics step"</div>
                                    <div class="stat-note">"independent of frame rate"</div>
                                </div>
                                <div class="stat-card">
                                    <div class="stat-value">"6"</div>
                                    <div class="stat-label">"Substeps per step"</div>
                                    <div class="stat-note">"Avian's default, pinned"</div>
                                </div>
                                <div class="stat-card">
                                    <div class="stat-value">"9.80665 m/s²"</div>
                                    <div class="stat-label">"Gravity along -Y"</div>
                                    <div class="stat-note">"standard gravity"</div>
                                </div>
                            </div>
                            <p>
                                "The substep count and solver settings are set explicitly to Avian's defaults, so
                                an Avian upgrade cannot quietly change trajectories. The frame time fed to the fixed
                                step is capped at 33 ms, so one slow frame costs about two catch-up steps instead
                                of a spiral of them. A body that stays at rest for 0.5 s falls asleep (Avian's
                                default) and costs no solver time until it is disturbed."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Studio and headless"</strong>
                                    <p>
                                        "The headless runtime runs the same Avian setup and the same Play activation.
                                        Joint binding, deformation, fracture and collider streaming are registered by
                                        Studio, so a headless run simulates bodies and colliders without them. See "
                                        <a href="/learn/cli">"CLI & Headless"</a>". Heat, electrochemistry, material
                                        laws and particle fluids come from the "<a href="/docs/realism">"Realism"</a>
                                        " libraries, and the simulation clock and recordings from "
                                        <a href="/docs/simulation">"Simulation"</a>"."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-units" class="subsection">
                            <h3>"SI Units"</h3>
                            <p>
                                "The world is meter-native: one unit of space is one meter. Everything on this
                                page is in SI units."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Quantity"</th><th>"Unit"</th><th>"Where you meet it"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Length"</td><td>"meter (m)"</td><td>"Size, Position, ray distances"</td></tr>
                                    <tr><td>"Mass"</td><td>"kilogram (kg)"</td><td>"Body mass, computed from volume and density"</td></tr>
                                    <tr><td>"Density"</td><td>"kg/m³"</td><td>"The density key under [properties.physics]"</td></tr>
                                    <tr><td>"Time"</td><td>"second (s)"</td><td>"The 1/60 s physics step"</td></tr>
                                    <tr><td>"Force, impulse"</td><td>"newton (N), N·s"</td><td>"Contacts and impacts"</td></tr>
                                    <tr><td>"Energy"</td><td>"joule (J)"</td><td>"Impact energy and fracture thresholds"</td></tr>
                                    <tr><td>"Stiffness, strength"</td><td>"pascal (Pa)"</td><td>"Young's modulus, yield strength"</td></tr>
                                    <tr><td>"Fracture toughness"</td><td>"Pa·√m"</td><td>"K_IC in the material presets"</td></tr>
                                    <tr><td>"Angle"</td><td>"radian"</td><td>"Joint angle limits"</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="overview-repeatable" class="subsection">
                            <h3>"Repeatable Runs"</h3>
                            <p>
                                "Same inputs, same world. The step, substeps and solver settings are pinned, and
                                randomness that shapes a simulation draws from one global seed ("
                                <code>"GlobalRngSeed"</code>", a fixed constant by default) instead
                                of system entropy. The repository's "<code>"same_seed_same_world"</code>" test
                                holds the engine to it: it drops 16 cubes onto a floor twice with the same seed,
                                steps each world 120 times and requires identical final poses."
                            </p>
                            <p>
                                "Agents can advance the live world one tick at a time. The "
                                <code>"sim_step"</code>" tool of the "<a href="/learn/mcp">"MCP server"</a>
                                " runs N fixed steps of 1/60 s (up to 10,000 per call) and leaves physics paused
                                afterward, so nothing moves between steps."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Same build, same kind of machine"</strong>
                                    <p>
                                        "Eustress builds Avian without its "<code>"enhanced-determinism"</code>
                                        " option, which makes math identical across CPU architectures. Identical
                                        results are expected from the same build on the same kind of machine."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-scale" class="subsection">
                            <h3>"Large Scenes"</h3>
                            <p>
                                "A still scene is cheap. While the physics clock is paused, Avian's per-step
                                collider upkeep runs only on steps where a collider moved, appeared or
                                disappeared, so a static Edit-mode Space skips it however many parts it holds."
                            </p>
                            <p>
                                "Very large Spaces streamed from the "<a href="/docs/universes">"WorldDb"</a>
                                " go further. Their anchored parts load with a collider description instead of a
                                collider, and during Play a real collider is created only for parts within 128 m
                                of physics activity (an awake dynamic body, the character or the main camera) and
                                removed again beyond 160 m, with at most 512 such changes per frame."
                            </p>
                            <p>
                                "To time Avian on your own hardware, the repository includes a headless benchmark.
                                It runs 600 steps at 60 Hz for piles of 1,000 to 25,000 falling cubes and for
                                10,000 and 100,000 static colliders with 100 dynamic bodies, and reports the mean
                                step time over all steps, the first 100 and the last 100."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Shell"</span>
                                </div>
                                <pre><code class="language-bash">{r#"cd eustress/benches/instance-capacity
cargo run --release --bin avian-physics-bench"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // EDIT AND PLAY
                    // =========================================================
                    <section id="play" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Edit and Play"
                        </h2>

                        <div id="play-edit" class="subsection">
                            <h3>"Edit Mode"</h3>
                            <p>
                                "Edit mode does not simulate. The physics clock is paused from the moment Studio
                                starts, and every part that collides is a static body, so nothing falls or drifts
                                while you build."
                            </p>
                            <p>
                                "Colliders still exist in Edit mode. Clicking and hovering parts in the viewport
                                and agent raycasts use the same Avian colliders that Play will simulate."
                            </p>
                            <PlayCycleDiagram />
                        </div>

                        <div id="play-start" class="subsection">
                            <h3>"Starting Play"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Key"</th><th>"Action"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"F5"</code></td><td>"Play with a character"</td></tr>
                                    <tr><td><code>"F7"</code></td><td>"Play Solo: free editor camera, no character"</td></tr>
                                    <tr><td><code>"F6"</code></td><td>"Pause or resume the physics clock"</td></tr>
                                    <tr><td><code>"F8"</code>" or "<code>"Esc"</code></td><td>"Stop"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "When Play starts, the physics clock unpauses and every unanchored part that has a
                                collider becomes a dynamic body. Anchored parts stay static, and a part with
                                CanCollide off has no collider, so it is never simulated. Play with a character
                                also spawns the avatar, at a SpawnLocation when the Space has one."
                            </p>
                            <p>
                                "Anchored is live during Play: unanchoring a part makes it dynamic at once, and
                                anchoring it makes it static again with its velocity zeroed. Pausing freezes the
                                physics clock without leaving Play. With the Roblox keymap, Play Solo moves to "
                                <code>"F8"</code>" and Stop to "<code>"Shift+F5"</code>"."
                            </p>
                        </div>

                        <div id="play-stop" class="subsection">
                            <h3>"What Stop Restores"</h3>
                            <p>
                                "Pressing Play takes a snapshot of the world, and Stop puts it back. The physics
                                clock pauses and the world returns to the state it had when you pressed Play:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"What"</th><th>"On Stop"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Parts"</td><td>"Position, rotation and scale; Anchored, CanCollide, Transparency and Color"</td></tr>
                                    <tr><td>"Deleted parts"</td><td>"Reloaded from the snapshot if a script or tool removed them"</td></tr>
                                    <tr><td>"Play-only objects"</td><td>"The character and its camera, fracture fragments and anything else spawned during Play are removed"</td></tr>
                                    <tr><td>"Damage"</td><td>"Dented meshes return to their authored shape and fractured parts reappear"</td></tr>
                                    <tr><td>"Humanoids"</td><td>"Health, maximum health, walk speed and jump power"</td></tr>
                                    <tr><td>"Simulation state"</td><td>"Battery and thermal state, and the Lighting service"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "That full restore runs for the Stop button, "<code>"F8"</code>" and "
                                <code>"Esc"</code>". A second check runs on every return to Edit mode, whatever
                                ended Play: if the snapshot has not been restored yet, it rewinds part poses,
                                Anchored, CanCollide and simulation state, so no part is left where physics threw it."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // BODIES AND COLLIDERS
                    // =========================================================
                    <section id="bodies" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Bodies and Colliders"
                        </h2>

                        <div id="bodies-properties" class="subsection">
                            <h3>"Physics Properties"</h3>
                            <p>
                                "A part's physics comes from a few properties. The Properties panel lists them
                                under Physics, and the part's file stores them in its "<code>"[properties]"</code>
                                " table."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Property"</th><th>"File key"</th><th>"Default"</th><th>"Effect"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"Anchored"</code></td><td><code>"anchored"</code></td><td>"false"</td><td>"Static in Play when on, dynamic when off"</td></tr>
                                    <tr><td><code>"CanCollide"</code></td><td><code>"can_collide"</code></td><td>"true"</td><td>"Off means no collider: never simulated, not hit by rays, and bodies pass through it"</td></tr>
                                    <tr><td><code>"Destructible"</code></td><td><code>"destructible"</code></td><td>"false"</td><td>"Impacts in Play can dent the part and crack it in two"</td></tr>
                                    <tr><td><code>"Locked"</code></td><td><code>"locked"</code></td><td>"false"</td><td>"Stops click selection in the viewport; no effect on physics"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Parts also carry CanTouch, CollisionGroup, Density and Mass values, but the
                                rigid-body solver does not read them yet. Density is used only when a part
                                fractures (see "<a href="#damage-fracture">"Fracture"</a>")."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"CanCollide is applied when a part loads"</strong>
                                    <p>
                                        "Whether a part gets a collider is decided when it loads. Toggling CanCollide
                                        on a part already in the world saves the new value, and Rune raycasts honor it
                                        at once, but the collider itself is added or removed only when the Space next
                                        loads."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="bodies-mass" class="subsection">
                            <h3>"Mass, Friction and Bounce"</h3>
                            <p>
                                "Avian derives each body's mass, center of mass and inertia from its collider's
                                volume and a density. A part without a physics table gets Avian's defaults: density
                                1.0 kg/m³, friction 0.5 and restitution 0. A 1 m cube therefore has a mass of 1 kg,
                                slides moderately and does not bounce."
                            </p>
                            <p>
                                "A part's Material sets how it looks and, for a destructible part, how it takes
                                damage. It does not change mass, friction or bounce in the solver. Those come from
                                a "<code>"[properties.physics]"</code>" table, which the Roblox importer writes for
                                every part with custom physical properties (Roblox friction fills both friction
                                coefficients, and elasticity becomes restitution). You can write one by hand:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[properties]
anchored = false
material = "Metal"

[properties.physics]
density = 7850.0          # kg/m³, drives mass and inertia
friction_static = 0.74    # resistance before sliding starts
friction_kinetic = 0.57   # resistance while sliding
restitution = 0.6         # bounce, 0 to 1"#}</code></pre>
                            </div>
                            <p>
                                "Each key is optional. A single friction value is used for both coefficients, and
                                a density must be positive to apply."
                            </p>
                        </div>

                        <div id="bodies-shapes" class="subsection">
                            <h3>"Collider Shapes"</h3>
                            <p>
                                "Colliders are simple shapes fitted to the part's Size, which keeps contacts fast
                                and stable:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Part shape"</th><th>"Collider"</th><th>"Fitted to"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Block"</td><td>"Box"</td><td>"Size on all three axes"</td></tr>
                                    <tr><td>"Wedge, CornerWedge"</td><td>"Box"</td><td>"The full bounding box, so a slope collides as its box"</td></tr>
                                    <tr><td>"Ball"</td><td>"Sphere"</td><td>"A radius of half Size X"</td></tr>
                                    <tr><td>"Cylinder, Cone"</td><td>"Cylinder"</td><td>"A radius of half Size X and a height of Size Y"</td></tr>
                                    <tr><td>"Custom mesh (GLB)"</td><td>"Box"</td><td>"The part's Size"</td></tr>
                                    <tr><td>"CAD part"</td><td>"Convex hull or convex pieces"</td><td>"The body's real shape"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "A convex "<a href="/docs/cad">"CAD"</a>" body gets a single hull, and any other
                                body is decomposed into convex pieces. A body over 20,000 triangles gets one hull
                                instead, and one whose surface is not closed falls back to a box. The character
                                collides as a capsule, and fracture fragments as convex hulls."
                            </p>
                        </div>

                        <div id="bodies-size" class="subsection">
                            <h3>"Size, Applied Once"</h3>
                            <p>
                                "A part renders a unit mesh stretched by its transform, so its Transform scale
                                equals its Size. Avian also multiplies every collider by the scale of the entity it
                                sits on. Eustress therefore builds the collider at Size divided by scale (a unit
                                shape for an ordinary part), and Avian's multiply brings it back to exactly Size. A
                                6 m plate collides as a 6 m plate."
                            </p>
                            <div class="equation-card">
                                <div class="equation">"collider = (Size ÷ scale) × scale = Size"</div>
                                <div class="equation-label">"Built in local units; Avian applies the scale once"</div>
                            </div>
                            <p>
                                "The same rule runs whenever a part loads and whenever you resize one with the
                                Scale tool or the Size property, so what collides always matches what you see. A
                                part whose scale is negative or not a finite number, such as a mirrored import,
                                gets no collider rather than an inverted one."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // JOINTS
                    // =========================================================
                    <section id="joints" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Joints"
                        </h2>

                        <div id="joints-imported" class="subsection">
                            <h3>"Imported Constraints"</h3>
                            <p>
                                "A joint connects two bodies and removes some of their relative freedom. Constraints
                                in an imported Roblox place become Avian joints when you press Play. The importer
                                stores each constraint's settings in a "<code>"[constraint]"</code>" table and its
                                two ends in a "<code>"[references]"</code>" table, as the UUIDs of the parts ("
                                <code>"Part0"</code>", "<code>"Part1"</code>") or attachments ("
                                <code>"Attachment0"</code>", "<code>"Attachment1"</code>") it connects. At Play,
                                Eustress looks those instances up, walks from each attachment to the part that owns
                                it, and inserts the joint once both ends exist."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Class"</th><th>"Ends"</th><th>"Avian joint"</th><th>"Reads"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"WeldConstraint"</td><td>"Parts"</td><td>"FixedJoint"</td><td>"enabled"</td></tr>
                                    <tr><td>"Motor6D"</td><td>"Parts"</td><td>"RevoluteJoint"</td><td>"enabled, lower_angle, upper_angle"</td></tr>
                                    <tr><td>"HingeConstraint"</td><td>"Attachments"</td><td>"RevoluteJoint"</td><td>"enabled, lower_angle, upper_angle"</td></tr>
                                    <tr><td>"TorsionSpringConstraint"</td><td>"Attachments"</td><td>"RevoluteJoint"</td><td>"enabled"</td></tr>
                                    <tr><td>"PrismaticConstraint"</td><td>"Attachments"</td><td>"PrismaticJoint"</td><td>"enabled"</td></tr>
                                    <tr><td>"CylindricalConstraint"</td><td>"Attachments"</td><td>"PrismaticJoint (slide only)"</td><td>"enabled"</td></tr>
                                    <tr><td>"BallSocketConstraint"</td><td>"Attachments"</td><td>"SphericalJoint"</td><td>"enabled"</td></tr>
                                    <tr><td>"UniversalConstraint"</td><td>"Attachments"</td><td>"SphericalJoint"</td><td>"enabled"</td></tr>
                                    <tr><td>"DistanceConstraint"</td><td>"Attachments"</td><td>"DistanceJoint, 0 to max_distance (5 m if absent)"</td><td>"enabled, max_distance"</td></tr>
                                    <tr><td>"RopeConstraint"</td><td>"Attachments"</td><td>"DistanceJoint, 0 to length (10 m if absent)"</td><td>"enabled, length"</td></tr>
                                    <tr><td>"RodConstraint"</td><td>"Attachments"</td><td>"DistanceJoint held at length (2 m if absent)"</td><td>"enabled, length"</td></tr>
                                    <tr><td>"SpringConstraint"</td><td>"Attachments"</td><td>"DistanceJoint held at rest_length (5 m if absent)"</td><td>"enabled, rest_length"</td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Imported joints pivot at part centers"</strong>
                                    <p>
                                        "Attachment offsets are not applied yet, so every imported joint binds at
                                        the two parts' centers: a weld pulls the centers together, and a hinge turns
                                        about the centers and each part's local Z axis (Avian's default). Angle
                                        limits apply only when both are present and are read in radians. Setting "
                                        <code>"enabled = false"</code>" binds the joint but tells Avian to skip it."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="joints-mates" class="subsection">
                            <h3>"Mates"</h3>
                            <p>
                                "To join two parts by hand, use the Constraints group on the Model tab. Hinge,
                                Slide and Ball each start a pick tool: click the first part, then the second, and
                                Eustress inserts an Avian joint between them."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Button"</th><th>"Avian joint"</th><th>"Free motion"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Hinge"</td><td>"RevoluteJoint"</td><td>"Rotation about the Y axis"</td></tr>
                                    <tr><td>"Slide"</td><td>"PrismaticJoint"</td><td>"Sliding along the X axis"</td></tr>
                                    <tr><td>"Ball"</td><td>"SphericalJoint"</td><td>"Rotation in every direction"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "A mate acts in Play, on whichever of its two parts is unanchored. It can be undone,
                                and it lasts for the current session: mates are not saved with the Space yet."
                            </p>
                        </div>

                        <div id="joints-limits" class="subsection">
                            <h3>"Not Wired Yet"</h3>
                            <p>"Several constraint classes exist without a working path to Avian today:"</p>
                            <ul class="docs-list">
                                <li><strong>"Weld and Motor buttons"</strong>" insert a WeldConstraint or Motor6D whose part fields are empty and which has no [references] table, so it binds nothing."</li>
                                <li><strong>"Movers"</strong>" (VectorForce, Torque, AlignPosition, AlignOrientation, LinearVelocity, AngularVelocity and the legacy BodyPosition, BodyVelocity, BodyGyro, BodyAngularVelocity, BodyForce and BodyThrust) have a runtime that applies forces in Play, but loading a Space does not give them their settings, so they exert nothing."</li>
                                <li><strong>"PlaneConstraint"</strong>" has no Avian equivalent and does nothing."</li>
                                <li><strong>"Seat and VehicleSeat"</strong>" are data only; nothing seats a character."</li>
                                <li><strong>"Motors and springs"</strong>": Motor6D and HingeConstraint joints have no motor drive, SpringConstraint is rigid at its rest length, and TorsionSpringConstraint has no spring."</li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // QUERIES AND CONTACTS
                    // =========================================================
                    <section id="queries" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Queries and Contacts"
                        </h2>

                        <div id="queries-rune" class="subsection">
                            <h3>"Raycasts in Rune"</h3>
                            <p>
                                "Rune scripts cast rays against the live colliders. "
                                <code>"workspace_raycast"</code>" returns the closest hit or "<code>"None"</code>
                                ", and "<code>"workspace_raycast_all"</code>" returns up to "<code>"max_hits"</code>
                                " hits sorted by distance. Each hit has "<code>"instance"</code>" (the part's name), "
                                <code>"entity_id"</code>", "<code>"position"</code>", "<code>"normal"</code>", "
                                <code>"distance"</code>" and "<code>"material"</code>"."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::{Vector3, workspace_raycast, set_sim_value};

pub fn on_update(dt) {
    let origin = Vector3::new(0.0, 50.0, 0.0);
    let down = Vector3::new(0.0, -1.0, 0.0);
    // The answer is last frame's cast from this same call site.
    if let Some(hit) = workspace_raycast(origin, down, None) {
        set_sim_value("probe.ground_height", hit.position.y);
    }
}"#}</code></pre>
                            </div>
                            <p>
                                "A script's raycast is answered after the frame, not during the call. The Nth
                                raycast a script makes in a frame returns the answer to its Nth raycast of the
                                previous frame, so a fixed pattern of casts per "<code>"on_update"</code>" reads
                                results one frame old. An answer whose origin has moved more than 2 m since it was
                                cast is discarded, and the call returns "<code>"None"</code>"."
                            </p>
                            <p>
                                "Pass "<code>"None"</code>" for the parameters. The default ray reaches 1,000 m and
                                skips parts whose CanCollide is off. The "<code>"RaycastParams"</code>" type has
                                no constructor registered for Rune yet, so filtered casts are not available from
                                scripts."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Luau cannot raycast yet"</strong>
                                    <p>
                                        "The Luau raycast module ("<code>"workspace:Raycast"</code>" and "
                                        <code>"RaycastParams"</code>") is not connected to the Luau VM, so Luau
                                        scripts have no spatial queries today. Use Rune for raycasts; see "
                                        <a href="/docs/scripting">"Scripting"</a>"."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="queries-agents" class="subsection">
                            <h3>"Raycasts for Agents"</h3>
                            <p>
                                "Over the MCP server, "<code>"scene_raycast"</code>" casts a ray against the
                                running engine's Avian colliders, in Edit mode or Play, and returns its hits
                                nearest first, each with an entity id, name, distance and hit point."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MCP"</span>
                                </div>
                                <pre><code class="language-json">{r#"{ "tool": "scene_raycast",
  "arguments": { "origin": [0, 50, 0], "direction": [0, -1, 0],
                 "max_distance": 200, "max_hits": 4 } }"#}</code></pre>
                            </div>
                            <p>
                                <code>"origin"</code>" defaults to "<code>"[0, 0, 0]"</code>" and "
                                <code>"direction"</code>" to straight down; "<code>"max_distance"</code>" defaults
                                to 1,000 m and "<code>"max_hits"</code>" to 8, at most 256. Use "
                                <code>"scene_raycast"</code>" rather than the older "<code>"raycast"</code>" tool,
                                which does not reach the live engine and returns an error."
                            </p>
                        </div>

                        <div id="queries-touch" class="subsection">
                            <h3>"Touched Events"</h3>
                            <p>
                                "When Play starts, every part is registered with the Luau runtime and asks Avian to
                                report its collisions. When two registered parts start touching, each part's "
                                <code>"Touched"</code>" signal fires with the other part as its argument, and "
                                <code>"TouchEnded"</code>" fires when they separate. CanTouch does not filter these
                                events yet."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // CHARACTERS
                    // =========================================================
                    <section id="characters" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Characters"
                        </h2>

                        <div id="characters-body" class="subsection">
                            <h3>"The Character Body"</h3>
                            <p>
                                "Play with a character spawns the avatar as a kinematic capsule: Avian collides it
                                with the world but never integrates it, and each frame the controller moves it with
                                Avian's move-and-slide query. Its rotation is locked, its friction and bounce are
                                zero, and its mass and capsule come from the avatar's height (1.45 m to 2.05 m)
                                and build. Studio and the Player share this controller."
                            </p>
                            <p>
                                "Two things follow from a kinematic body. Walking into an unanchored part does not
                                push it; the capsule stops or slides along it. And the character falls under its
                                own gravity of 9.80665 m/s², separate from the gravity that dynamic parts use."
                            </p>
                        </div>

                        <div id="characters-ground" class="subsection">
                            <h3>"Ground, Slopes and Steps"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Rule"</th><th>"Value"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Walkable slope"</td><td>"Up to 50°; anything steeper is a wall"</td></tr>
                                    <tr><td>"Step-up"</td><td>"Ledges up to 0.30 m are stepped, not jumped"</td></tr>
                                    <tr><td>"Ground probe"</td><td>"A sphere cast 0.35 m below the capsule, so edges and stair noses do not flicker between grounded and airborne"</td></tr>
                                    <tr><td>"Coyote time"</td><td>"A jump still works 0.12 s after leaving an edge"</td></tr>
                                    <tr><td>"Jump buffer"</td><td>"A jump pressed up to 0.10 s before landing still fires"</td></tr>
                                    <tr><td>"Speeds"</td><td>"Walk 1.45 m/s and run 3.9 m/s, scaled by leg length"</td></tr>
                                    <tr><td>"Jump"</td><td>"An apex of 0.55 m plus 0.35 m scaled by leg length, launched at √(2 g h)"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "A Humanoid's walk speed and jump power are saved and restored with the world, but
                                this controller takes its speeds from the avatar's body. If the capsule ends up
                                wedged between colliders, the controller searches nearby for free space and moves
                                it there."
                            </p>
                        </div>

                        <div id="characters-climb" class="subsection">
                            <h3>"Climbing and Feet"</h3>
                            <p>
                                "Every climbing move goes through one placement check: the capsule is tested
                                against the colliders at its target, and if it would not fit, it stops at the last
                                clear point on the way. That keeps a climbing character out of walls and out of the
                                ground."
                            </p>
                            <p>
                                "Foot placement casts a ray down from each foot, plants a foot that has nearly
                                stopped and holds it in place so it cannot slide, and tilts it by up to 35° to
                                follow a slope. It fades out above 1.4 times run speed, where the animation reads
                                better on its own."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // DEFORMATION AND FRACTURE
                    // =========================================================
                    <section id="damage" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "Deformation and Fracture"
                        </h2>

                        <div id="damage-enable" class="subsection">
                            <h3>"Making a Part Destructible"</h3>
                            <p>
                                "A destructible part can be dented by impacts and cracked into two bodies. Turn on
                                Destructible in the Physics section of Properties, or set it in the part's file:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[properties]
material = "Concrete"
destructible = true"#}</code></pre>
                            </div>
                            <p>
                                "Damage happens only in Play, in Studio. The part's Material name picks its
                                mechanical constants, matched regardless of case, spaces and underscores:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Material names"</th><th>"Preset"</th><th>"Young's modulus"</th><th>"Yield strength"</th><th>"Toughness K_IC"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Metal, CorrodedMetal, DiamondPlate, Steel"</td><td>"Steel"</td><td>"200 GPa"</td><td>"250 MPa"</td><td>"50 MPa·√m"</td></tr>
                                    <tr><td>"Aluminum, Foil"</td><td>"Aluminum"</td><td>"70 GPa"</td><td>"270 MPa"</td><td>"30 MPa·√m"</td></tr>
                                    <tr><td>"Concrete, Brick, Granite, Marble, Slate, Cobblestone"</td><td>"Concrete"</td><td>"30 GPa"</td><td>"30 MPa"</td><td>"1 MPa·√m"</td></tr>
                                    <tr><td>"Glass"</td><td>"Glass"</td><td>"70 GPa"</td><td>"45 MPa"</td><td>"0.7 MPa·√m"</td></tr>
                                    <tr><td>"Ice"</td><td>"Ice"</td><td>"9 GPa"</td><td>"1 MPa"</td><td>"0.1 MPa·√m"</td></tr>
                                    <tr><td>"Wood, WoodPlanks"</td><td>"Wood (Oak)"</td><td>"12 GPa"</td><td>"60 MPa"</td><td>"10 MPa·√m"</td></tr>
                                    <tr><td>"Rubber, Fabric, Grass"</td><td>"Rubber"</td><td>"10 MPa"</td><td>"15 MPa"</td><td>"5 MPa·√m"</td></tr>
                                    <tr><td>"Plastic, SmoothPlastic, Neon, Sand, Pebble"</td><td>"Plastic (ABS)"</td><td>"2.3 GPa"</td><td>"40 MPa"</td><td>"3 MPa·√m"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Any other material name uses a generic plastic-like fallback. The MCP tool "
                                <code>"query_material"</code>" returns these same constants for a material name."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A [material] table replaces the preset"</strong>
                                    <p>
                                        "Writing an explicit "<code>"[material]"</code>" table switches the name
                                        lookup off, and every field you leave out is read as zero. A zero modulus,
                                        yield strength or toughness then falls back to the generic plastic value,
                                        not to your material's preset, so write every constant you need."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="damage-dent" class="subsection">
                            <h3>"From Impact to Dent"</h3>
                            <p>
                                "On every physics step in Play, Eustress reads Avian's contact list for destructible
                                parts. A contact counts as an impact when the two bodies close at 0.5 m/s or
                                faster; slower contact is resting weight. The energy the collision must absorb is:"
                            </p>
                            <div class="equation-card">
                                <div class="equation">"E = ½ · m_eff · v²"</div>
                                <div class="equation-label">"m_eff = 1 / (1/m₁ + 1/m₂); an anchored part counts as immovable, with 1/m = 0"</div>
                            </div>
                            <p>
                                "Contact mechanics turns that energy into a depth. Below the material's yield-onset
                                energy E_y the dent is elastic (Hertz contact) and springs back, about 95 percent
                                within 0.6 s. Above it the dent is plastic, set by an indentation hardness H of three
                                times the yield strength, and it stays until Stop."
                            </p>
                            <div class="equation-card">
                                <div class="equation">"δ = (E / ((8/15) · E* · √R))^(2/5)"</div>
                                <div class="equation-label">"Elastic depth: E* is the two materials' combined stiffness, R the impactor radius"</div>
                            </div>
                            <div class="equation-card">
                                <div class="equation">"δ = √((E - E_y) / (π · R · H)),  H = 3σ_y"</div>
                                <div class="equation-label">"Plastic depth: σ_y is the yield strength"</div>
                            </div>
                            <p>
                                "The impactor's radius is taken as half its smallest dimension. The dent pushes
                                along the struck surface's normal, is capped at a quarter of the part's smallest
                                dimension and fades to nothing within 5 cm of an edge. A part takes at most one
                                impact every 0.25 s, so one collision leaves one dent. The mesh keeps its authored
                                vertices until it is struck, then is refined only where the crater lands, with edges
                                down to 1.5 cm and at most 8,000 triangles per part."
                            </p>
                            <p>
                                "Depths are physical, not exaggerated, so stiff and strong materials such as steel
                                barely mark under everyday hits. A dent changes the rendered mesh only; the part's
                                collider keeps its shape."
                            </p>
                        </div>

                        <div id="damage-fracture" class="subsection">
                            <h3>"Fracture"</h3>
                            <p>
                                "Before denting, the impact energy is compared with the energy needed to run a crack
                                through the part's smallest cross-section, the Griffith criterion:"
                            </p>
                            <div class="equation-card">
                                <div class="equation">"E > (K_IC² / E_Young) × A_min"</div>
                                <div class="equation-label">"K_IC fracture toughness, E_Young Young's modulus, A_min smallest cross-section area"</div>
                            </div>
                            <p>
                                "When the energy exceeds it, the part splits in two along a plane that contains the
                                impact direction, so a part struck from above cracks down through itself. Each half
                                becomes a dynamic body with a convex-hull collider and the part's Density (900
                                kg/m³ unless changed) and inherits the part's motion, and each half is pushed off
                                the crack plane at 0.35 m/s so the crack opens. The original part is hidden and its collider switched off, not
                                deleted; Stop brings it back and removes the fragments."
                            </p>
                            <p>
                                "Because toughness enters squared, materials separate sharply: a crack through
                                concrete costs about 33 J per square meter of cross-section, and through steel about
                                12,500 J. Fragments smaller than 5 cm are refused, at most four fractures run per
                                frame, and fragments are not destructible themselves, so each part splits once per
                                Play session."
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

                        <div id="roadmap-materials" class="subsection">
                            <h3>"Material-Driven Contact"</h3>
                            <p>
                                "Mass, friction and bounce will follow each part's Material and Density, so a steel
                                ball will outweigh a wooden one without a "<code>"[properties.physics]"</code>
                                " table. CanTouch and CollisionGroup will filter touches and contacts, CanCollide
                                will switch a collider on and off while the part is in the world, and Studio will
                                be able to show every collision shape in the viewport."
                            </p>
                        </div>

                        <div id="roadmap-joints" class="subsection">
                            <h3>"Joints and Movers"</h3>
                            <p>
                                "Joints will bind at their attachments' offsets and axes, motors and springs will
                                drive and stiffen them, and Weld and Motor from the ribbon will bind the parts you
                                choose. Movers will load their settings from disk so VectorForce and its family push
                                bodies, seats will seat characters, and mates will be saved with the Space."
                            </p>
                        </div>

                        <div id="roadmap-scripts" class="subsection">
                            <h3>"Script Physics"</h3>
                            <p>
                                "Luau scripts will get "<code>"workspace:Raycast"</code>" on the same bridge Rune
                                uses, and Rune scripts will build "<code>"RaycastParams"</code>" filters.
                                Destructible parts will gain a visual damage scale, so a scene can exaggerate dents
                                without changing the physics behind them."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Press Play to drop it. Press Stop to put it back."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/simulation" class="btn-secondary-steel">"Simulation Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/services" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Services"</span>
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
