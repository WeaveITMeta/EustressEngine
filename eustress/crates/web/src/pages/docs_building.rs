// =============================================================================
// Eustress Web - Building Documentation Page
// =============================================================================
// Comprehensive building/construction documentation with floating TOC
// Covers: Parts & Primitives, Terrain, Model Import, Materials, Level Design,
//         CSG Operations, Performance
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
                TocSubsection { id: "overview-pipeline", title: "Asset Pipeline" },
                TocSubsection { id: "overview-filesystem", title: "File System Layout" },
            ],
        },
        TocSection {
            id: "parts",
            title: "Parts & Primitives",
            subsections: vec![
                TocSubsection { id: "parts-primitives", title: "Primitive Shapes" },
                TocSubsection { id: "parts-transform", title: "Transform & Properties" },
                TocSubsection { id: "parts-hierarchy", title: "Hierarchy & Grouping" },
            ],
        },
        TocSection {
            id: "terrain",
            title: "Terrain System",
            subsections: vec![
                TocSubsection { id: "terrain-heightmaps", title: "Heightmaps" },
                TocSubsection { id: "terrain-sculpting", title: "Sculpting Tools" },
                TocSubsection { id: "terrain-painting", title: "Texture Painting" },
                TocSubsection { id: "terrain-biomes", title: "Biome Layers" },
                TocSubsection { id: "terrain-lod", title: "LOD System" },
            ],
        },
        TocSection {
            id: "import",
            title: "Model Import",
            subsections: vec![
                TocSubsection { id: "import-pipeline", title: "Import Pipeline" },
                TocSubsection { id: "import-formats", title: "Supported Formats" },
                TocSubsection { id: "import-cas", title: "Content-Addressable Storage" },
            ],
        },
        TocSection {
            id: "materials",
            title: "Materials & Textures",
            subsections: vec![
                TocSubsection { id: "materials-pbr", title: "PBR Materials" },
                TocSubsection { id: "materials-toml", title: "Material Overrides" },
                TocSubsection { id: "materials-atlas", title: "Texture Atlases" },
            ],
        },
        TocSection {
            id: "level",
            title: "Level Design",
            subsections: vec![
                TocSubsection { id: "level-spatial", title: "Spatial Organization" },
                TocSubsection { id: "level-lighting", title: "Lighting Setup" },
                TocSubsection { id: "level-atmosphere", title: "Atmosphere & Skybox" },
            ],
        },
        TocSection {
            id: "csg",
            title: "CSG Operations",
            subsections: vec![
                TocSubsection { id: "csg-operations", title: "Boolean Operations" },
                TocSubsection { id: "csg-workflow", title: "Non-Destructive Workflow" },
                TocSubsection { id: "csg-examples", title: "Examples" },
            ],
        },
        TocSection {
            id: "performance",
            title: "Performance",
            subsections: vec![
                TocSubsection { id: "performance-lod", title: "LOD Levels" },
                TocSubsection { id: "performance-culling", title: "Occlusion Culling" },
                TocSubsection { id: "performance-instancing", title: "Instancing & Batching" },
                TocSubsection { id: "performance-streaming", title: "GPU Mesh Streaming" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Building documentation page with floating TOC.
#[component]
pub fn DocsBuildingPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-building"></div>
            </div>

            <div class="docs-layout">
                // Floating TOC Sidebar
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/cube.svg" alt="Building" class="toc-icon" />
                        <h2>"Building"</h2>
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
                            <span class="current">"Building"</span>
                        </div>
                        <h1 class="docs-title">"Building"</h1>
                        <p class="docs-subtitle">
                            "CAD-style 3D construction with file-system-first design. Import models,
                            sculpt terrain, place primitives, and design levels with no proprietary format lock-in."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "30 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Intermediate"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/calendar.svg" alt="Updated" />
                                "Updated Apr 2026"
                            </span>
                        </div>
                    </header>

                    // =========================================================
                    // OVERVIEW SECTION
                    // =========================================================
                    <section id="overview" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"01"</span>
                            "Overview"
                        </h2>

                        <div id="overview-intro" class="subsection">
                            <h3>"Introduction"</h3>
                            <p>
                                "Building in Eustress is file-system-first. Every asset, every scene graph node,
                                and every material definition lives as a readable file on disk. Import GLB/GLTF
                                models, use primitive parts, sculpt terrain — all without proprietary format
                                lock-in. Your project is a folder. Your assets are files. Version control just works."
                            </p>

                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/cube.svg" alt="Primitives" />
                                    </div>
                                    <h4>"Primitives"</h4>
                                    <p>"Cube, Sphere, Cylinder, Wedge, Plane with full collision"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/grid.svg" alt="Terrain" />
                                    </div>
                                    <h4>"Terrain"</h4>
                                    <p>"Heightmap sculpting, texture painting, biome layers"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/download.svg" alt="Import" />
                                    </div>
                                    <h4>"Model Import"</h4>
                                    <p>"GLB/GLTF pipeline with content-addressable storage"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/cube.svg" alt="Materials" />
                                    </div>
                                    <h4>"PBR Materials"</h4>
                                    <p>"Albedo, normal, metallic, roughness, AO maps"</p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-pipeline" class="subsection">
                            <h3>"Asset Pipeline"</h3>
                            <p>
                                "The build pipeline is designed to keep human-readable source files
                                separate from optimized runtime assets. Source files live in your project
                                directory; the engine compiles them into an efficient runtime format
                                on first load."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Pipeline"</span>
                                </div>
                                <pre><code class="language-text">{r#"Source Files          Build Step              Runtime
─────────────         ──────────              ───────
*.glb / *.gltf   ──▶  Asset Compiler   ──▶   .eustress/cache/
*.terrain        ──▶  Terrain Baker    ──▶   LOD meshes + collision
*.toml           ──▶  Material Parser  ──▶   GPU shader params
*.png / *.jpg    ──▶  Texture Packer   ──▶   Compressed atlas (BC7)"#}</code></pre>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Hot Reload"</strong>
                                    <p>"All source files are watched via "<code>"notify"</code>". Modify a
                                    texture, material, or model and see changes reflected in the viewport
                                    within milliseconds."</p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-filesystem" class="subsection">
                            <h3>"File System Layout"</h3>
                            <p>
                                "A typical Eustress project follows this directory structure. All paths
                                are relative to your project root:"
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Directory Structure"</span>
                                </div>
                                <pre><code class="language-text">{r#"my-project/
├── .eustress/
│   ├── assets/           # Imported models (content-addressed)
│   ├── cache/            # Compiled runtime assets
│   └── project.toml      # Project metadata
├── scenes/
│   ├── main.scene.toml   # Main scene graph
│   └── lobby.scene.toml  # Additional scenes
├── terrain/
│   ├── world.terrain     # Heightmap data
│   └── biomes.toml       # Biome layer definitions
├── materials/
│   ├── ground.toml       # Material definitions
│   └── metal.toml
├── textures/
│   ├── ground_albedo.png
│   ├── ground_normal.png
│   └── ground_roughness.png
└── models/
    ├── building.glb      # Source models
    └── props/
        ├── barrel.glb
        └── crate.glb"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // PARTS & PRIMITIVES SECTION
                    // =========================================================
                    <section id="parts" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Parts & Primitives"
                        </h2>

                        <div id="parts-primitives" class="subsection">
                            <h3>"Primitive Shapes"</h3>
                            <p>
                                "Eustress provides five fundamental primitive shapes. Each primitive is a
                                first-class ECS entity with Transform, Material, and Collision components
                                attached automatically."
                            </p>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Primitive"</th>
                                            <th>"Default Size"</th>
                                            <th>"Vertices"</th>
                                            <th>"Collision Shape"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"Cube"</code></td>
                                            <td>"1 x 1 x 1 m"</td>
                                            <td>"24"</td>
                                            <td>"Box collider"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Sphere"</code></td>
                                            <td>"r = 0.5 m"</td>
                                            <td>"482"</td>
                                            <td>"Sphere collider"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Cylinder"</code></td>
                                            <td>"r = 0.5 m, h = 1 m"</td>
                                            <td>"128"</td>
                                            <td>"Cylinder collider"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Wedge"</code></td>
                                            <td>"1 x 1 x 1 m"</td>
                                            <td>"18"</td>
                                            <td>"Convex hull"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Plane"</code></td>
                                            <td>"10 x 10 m"</td>
                                            <td>"4"</td>
                                            <td>"Halfspace"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::building::prelude::*;

// Spawn a cube with default properties
commands.spawn(PartBundle {
    part: Part::Cube,
    transform: Transform::from_xyz(0.0, 0.5, 0.0),
    material: MaterialHandle::default(),
    collision: CollisionShape::auto(),
    ..default()
});

// Spawn a sphere with custom size
commands.spawn(PartBundle {
    part: Part::Sphere { radius: 2.0 },
    transform: Transform::from_xyz(5.0, 2.0, 0.0),
    material: MaterialHandle::from_path("materials/metal.toml"),
    collision: CollisionShape::auto(),
    ..default()
});"#}</code></pre>
                            </div>
                        </div>

                        <div id="parts-transform" class="subsection">
                            <h3>"Transform & Properties"</h3>
                            <p>
                                "Every part carries three core property groups: Transform (position, rotation,
                                scale), Material (surface appearance), and Collision (physics interaction)."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Transform: position + rotation + scale
let transform = Transform {
    translation: Vec3::new(10.0, 0.0, -5.0),
    rotation: Quat::from_rotation_y(45.0_f32.to_radians()),
    scale: Vec3::new(2.0, 1.0, 3.0),
};

// Material: reference a .toml material definition
let material = MaterialHandle::from_path("materials/brick.toml");

// Collision: automatically derived from shape, or manual override
let collision = CollisionShape::ConvexHull {
    points: custom_hull_points,
    margin: 0.01,
};"#}</code></pre>
                            </div>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Property"</th>
                                            <th>"Type"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"translation"</code></td>
                                            <td><code>"Vec3"</code></td>
                                            <td>"World position in meters"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"rotation"</code></td>
                                            <td><code>"Quat"</code></td>
                                            <td>"Orientation quaternion"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"scale"</code></td>
                                            <td><code>"Vec3"</code></td>
                                            <td>"Non-uniform scale factors"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"anchored"</code></td>
                                            <td><code>"bool"</code></td>
                                            <td>"If true, part is static (no physics)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"can_collide"</code></td>
                                            <td><code>"bool"</code></td>
                                            <td>"Enable/disable collision detection"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"transparency"</code></td>
                                            <td><code>"f32"</code></td>
                                            <td>"0.0 (opaque) to 1.0 (invisible)"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>

                        <div id="parts-hierarchy" class="subsection">
                            <h3>"Hierarchy & Grouping"</h3>
                            <p>
                                "Parts can be organized into groups (called Models) that act as a single
                                unit. Groups preserve relative transforms and can be nested to any depth."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Create a model group
let model = commands.spawn((
    Model::new("Doorway"),
    Transform::from_xyz(0.0, 0.0, 0.0),
)).id();

// Spawn parts as children of the model
let frame = commands.spawn(PartBundle {
    part: Part::Cube,
    transform: Transform::from_scale(Vec3::new(0.2, 3.0, 0.2)),
    ..default()
}).id();

let lintel = commands.spawn(PartBundle {
    part: Part::Cube,
    transform: Transform {
        translation: Vec3::new(0.0, 3.0, 0.0),
        scale: Vec3::new(2.4, 0.2, 0.2),
        ..default()
    },
    ..default()
}).id();

// Attach children to model
commands.entity(model).add_children(&[frame, lintel]);"#}</code></pre>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Scene Graph"</strong>
                                    <p>"Models serialize to "<code>".scene.toml"</code>" files. Each child
                                    stores a local transform relative to its parent, so moving the root
                                    moves the entire group."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // TERRAIN SECTION
                    // =========================================================
                    <section id="terrain" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Terrain System"
                        </h2>

                        <div id="terrain-heightmaps" class="subsection">
                            <h3>"Heightmaps"</h3>
                            <p>
                                "Terrain in Eustress is stored as "<code>".terrain"</code>" files — compact
                                binary heightmaps with metadata headers. Each terrain chunk covers a
                                configurable world area and supports 16-bit height resolution."
                            </p>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Parameter"</th>
                                            <th>"Default"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"resolution"</code></td>
                                            <td>"257 x 257"</td>
                                            <td>"Height samples per chunk (must be 2^n + 1)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"chunk_size"</code></td>
                                            <td>"256 m"</td>
                                            <td>"World-space size of one chunk"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"height_range"</code></td>
                                            <td>"-500..2000 m"</td>
                                            <td>"Min/max elevation"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"precision"</code></td>
                                            <td>"16-bit"</td>
                                            <td>"Height value bit depth"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::terrain::prelude::*;

// Create a new terrain
let terrain = Terrain::new(TerrainConfig {
    resolution: 257,
    chunk_size: 256.0,
    height_range: -500.0..2000.0,
    ..default()
});

// Sample height at a world position
let height = terrain.height_at(Vec2::new(100.0, 50.0));

// Get the normal vector for lighting
let normal = terrain.normal_at(Vec2::new(100.0, 50.0));"#}</code></pre>
                            </div>
                        </div>

                        <div id="terrain-sculpting" class="subsection">
                            <h3>"Sculpting Tools"</h3>
                            <p>
                                "The terrain sculpting system provides brush-based editing tools for
                                shaping heightmaps in real-time. All operations are undoable and serialize
                                as operations in a history log."
                            </p>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Tool"</th>
                                            <th>"Hotkey"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"Raise"</code></td>
                                            <td>"B + LMB"</td>
                                            <td>"Raise terrain under brush radius"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Lower"</code></td>
                                            <td>"B + Shift+LMB"</td>
                                            <td>"Lower terrain under brush radius"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Flatten"</code></td>
                                            <td>"F"</td>
                                            <td>"Flatten terrain to sampled height"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Smooth"</code></td>
                                            <td>"S"</td>
                                            <td>"Gaussian smooth within brush radius"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Noise"</code></td>
                                            <td>"N"</td>
                                            <td>"Apply Perlin noise displacement"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Erode"</code></td>
                                            <td>"E"</td>
                                            <td>"Hydraulic erosion simulation"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Programmatic terrain sculpting
terrain.apply_brush(TerrainBrush {
    tool: BrushTool::Raise,
    position: Vec2::new(128.0, 128.0),
    radius: 20.0,
    strength: 0.5,
    falloff: BrushFalloff::Smooth,
});

// Apply hydraulic erosion over N iterations
terrain.erode(ErosionParams {
    iterations: 50_000,
    rain_rate: 0.01,
    sediment_capacity: 0.04,
    evaporation: 0.02,
    ..default()
});"#}</code></pre>
                            </div>
                        </div>

                        <div id="terrain-painting" class="subsection">
                            <h3>"Texture Painting"</h3>
                            <p>
                                "Terrain surfaces use a splatmap-based texture painting system. Up to
                                16 texture layers can be blended per chunk, with each layer carrying
                                full PBR material properties."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"TOML"</span>
                                </div>
                                <pre><code class="language-toml">{r#"# terrain/biomes.toml
[[layer]]
name = "grass"
albedo = "textures/grass_albedo.png"
normal = "textures/grass_normal.png"
roughness = 0.8
tiling = 4.0

[[layer]]
name = "rock"
albedo = "textures/rock_albedo.png"
normal = "textures/rock_normal.png"
roughness = 0.6
metallic = 0.1
tiling = 2.0

[[layer]]
name = "sand"
albedo = "textures/sand_albedo.png"
roughness = 0.9
tiling = 6.0"#}</code></pre>
                            </div>
                        </div>

                        <div id="terrain-biomes" class="subsection">
                            <h3>"Biome Layers"</h3>
                            <p>
                                "Biome layers allow automatic texture assignment based on altitude,
                                slope, moisture, and temperature. Define rules and the engine paints
                                the splatmap procedurally."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"TOML"</span>
                                </div>
                                <pre><code class="language-toml">{r#"# Biome rules for automatic texture painting
[[biome]]
name = "lowland_grass"
layer = "grass"
altitude = { min = 0, max = 500 }
slope = { min = 0.0, max = 30.0 }  # degrees

[[biome]]
name = "mountain_rock"
layer = "rock"
altitude = { min = 800, max = 2000 }
slope = { min = 25.0, max = 90.0 }

[[biome]]
name = "snow_cap"
layer = "snow"
altitude = { min = 1500, max = 2000 }
slope = { min = 0.0, max = 45.0 }"#}</code></pre>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Blend Zones"</strong>
                                    <p>"Biome transitions are automatically blended over configurable
                                    ranges. Set "<code>"blend_width"</code>" on each biome rule to control
                                    the transition sharpness."</p>
                                </div>
                            </div>
                        </div>

                        <div id="terrain-lod" class="subsection">
                            <h3>"LOD System"</h3>
                            <p>
                                "Terrain chunks use a quadtree-based LOD system. Nearby chunks render
                                at full resolution; distant chunks are progressively simplified. The
                                LOD system eliminates T-junction cracks with skirt geometry."
                            </p>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"LOD Level"</th>
                                            <th>"Distance"</th>
                                            <th>"Resolution"</th>
                                            <th>"Triangles/Chunk"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td>"LOD 0"</td>
                                            <td>"0 - 128 m"</td>
                                            <td>"257 x 257"</td>
                                            <td>"131,072"</td>
                                        </tr>
                                        <tr>
                                            <td>"LOD 1"</td>
                                            <td>"128 - 512 m"</td>
                                            <td>"129 x 129"</td>
                                            <td>"32,768"</td>
                                        </tr>
                                        <tr>
                                            <td>"LOD 2"</td>
                                            <td>"512 - 2048 m"</td>
                                            <td>"65 x 65"</td>
                                            <td>"8,192"</td>
                                        </tr>
                                        <tr>
                                            <td>"LOD 3"</td>
                                            <td>"2048+ m"</td>
                                            <td>"33 x 33"</td>
                                            <td>"2,048"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // MODEL IMPORT SECTION
                    // =========================================================
                    <section id="import" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Model Import"
                        </h2>

                        <div id="import-pipeline" class="subsection">
                            <h3>"Import Pipeline"</h3>
                            <p>
                                "Importing a model into Eustress is as simple as dropping a file into your
                                project's "<code>"models/"</code>" directory. The asset watcher detects the
                                new file, computes a SHA-256 hash, and stores a deduplicated copy in
                                content-addressable storage."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Pipeline Steps"</span>
                                </div>
                                <pre><code class="language-text">{r#"1. File detected in models/ directory
2. SHA-256 hash computed → e.g. a1b2c3d4e5f6...
3. File copied to .eustress/assets/a1/b2/a1b2c3d4e5f6...glb
4. Metadata extracted (vertices, materials, animations)
5. Collision meshes generated (convex decomposition)
6. LOD chain generated (meshopt simplification)
7. Asset manifest updated (.eustress/manifest.toml)"#}</code></pre>
                            </div>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::assets::prelude::*;

// Import a model programmatically
let handle = asset_server.import(ImportRequest {
    source: "models/building.glb",
    options: ImportOptions {
        generate_collision: true,
        generate_lods: true,
        lod_levels: 4,
        simplification_target: 0.5, // 50% reduction per level
        ..default()
    },
});

// Spawn the imported model into the scene
commands.spawn(SceneBundle {
    scene: handle,
    transform: Transform::from_xyz(0.0, 0.0, 0.0),
    ..default()
});"#}</code></pre>
                            </div>
                        </div>

                        <div id="import-formats" class="subsection">
                            <h3>"Supported Formats"</h3>
                            <p>
                                "Eustress uses GLB/GLTF as its primary interchange format. These formats
                                are open standards that preserve materials, textures, animations, and
                                scene hierarchy."
                            </p>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Format"</th>
                                            <th>"Extension"</th>
                                            <th>"Features Preserved"</th>
                                            <th>"Status"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td>"GLB (Binary)"</td>
                                            <td><code>".glb"</code></td>
                                            <td>"Meshes, materials, textures, animations, hierarchy"</td>
                                            <td>"Full support"</td>
                                        </tr>
                                        <tr>
                                            <td>"GLTF (JSON)"</td>
                                            <td><code>".gltf"</code></td>
                                            <td>"Same as GLB, separate texture files"</td>
                                            <td>"Full support"</td>
                                        </tr>
                                        <tr>
                                            <td>"OBJ"</td>
                                            <td><code>".obj"</code></td>
                                            <td>"Meshes, basic materials (via .mtl)"</td>
                                            <td>"Import only"</td>
                                        </tr>
                                        <tr>
                                            <td>"FBX"</td>
                                            <td><code>".fbx"</code></td>
                                            <td>"Meshes, materials, skeleton, animations"</td>
                                            <td>"Planned"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>

                            <div class="callout callout-warning">
                                <img src="/assets/icons/shield.svg" alt="Warning" />
                                <div>
                                    <strong>"Blender Users"</strong>
                                    <p>"Export as GLB with 'Include > Custom Properties' enabled. This
                                    preserves Eustress-specific metadata like collision flags and LOD
                                    group markers."</p>
                                </div>
                            </div>
                        </div>

                        <div id="import-cas" class="subsection">
                            <h3>"Content-Addressable Storage"</h3>
                            <p>
                                "All imported assets are stored using content-addressable storage (CAS).
                                The SHA-256 hash of each file becomes its identifier. This means:"
                            </p>

                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/check.svg" alt="Dedup" />
                                    </div>
                                    <h4>"Deduplication"</h4>
                                    <p>"Identical assets are stored once, regardless of filename"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/check.svg" alt="Integrity" />
                                    </div>
                                    <h4>"Integrity"</h4>
                                    <p>"Corruption is detected automatically via hash mismatch"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/check.svg" alt="Cache" />
                                    </div>
                                    <h4>"Cache-Friendly"</h4>
                                    <p>"CDN and local caches key on content hash, not filenames"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/check.svg" alt="Sync" />
                                    </div>
                                    <h4>"Sync-Ready"</h4>
                                    <p>"Only changed assets are transferred during multiplayer sync"</p>
                                </div>
                            </div>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::assets::cas::*;

// Resolve an asset by its content hash
let hash = ContentHash::from_hex("a1b2c3d4e5f6...");
let path = cas_store.resolve(hash)?;

// Check if an asset exists in the store
if cas_store.contains(&hash) {
    println!("Asset already imported");
}

// Get all assets with their metadata
for (hash, meta) in cas_store.iter() {
    println!("{}: {} vertices, {} materials",
        hash, meta.vertex_count, meta.material_count);
}"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // MATERIALS & TEXTURES SECTION
                    // =========================================================
                    <section id="materials" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Materials & Textures"
                        </h2>

                        <div id="materials-pbr" class="subsection">
                            <h3>"PBR Materials"</h3>
                            <p>
                                "Eustress uses a physically-based rendering (PBR) material model with
                                five texture channels. Materials are defined in TOML files and can
                                reference textures on disk."
                            </p>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Channel"</th>
                                            <th>"Format"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"albedo"</code></td>
                                            <td>"RGBA8 / sRGB"</td>
                                            <td>"Base color and opacity"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"normal"</code></td>
                                            <td>"RG16 / Linear"</td>
                                            <td>"Tangent-space normal map"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"metallic"</code></td>
                                            <td>"R8 / Linear"</td>
                                            <td>"Metalness (0 = dielectric, 1 = metal)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"roughness"</code></td>
                                            <td>"R8 / Linear"</td>
                                            <td>"Surface roughness (0 = mirror, 1 = matte)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"ao"</code></td>
                                            <td>"R8 / Linear"</td>
                                            <td>"Ambient occlusion (baked cavity shadows)"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"TOML"</span>
                                </div>
                                <pre><code class="language-toml">{r#"# materials/brick.toml
[material]
name = "Red Brick"
shader = "pbr_standard"

[textures]
albedo = "textures/brick_albedo.png"
normal = "textures/brick_normal.png"
roughness = "textures/brick_roughness.png"
ao = "textures/brick_ao.png"

[properties]
metallic = 0.0
roughness_scale = 1.0
normal_strength = 1.0
uv_scale = [2.0, 2.0]
emissive = [0.0, 0.0, 0.0]
alpha_cutoff = 0.5"#}</code></pre>
                            </div>
                        </div>

                        <div id="materials-toml" class="subsection">
                            <h3>"Material Overrides"</h3>
                            <p>
                                "Imported models come with embedded materials, but you can override any
                                property via TOML without modifying the source model. Overrides are
                                stored alongside the scene file."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"TOML"</span>
                                </div>
                                <pre><code class="language-toml">{r#"# scenes/main.overrides.toml
# Override materials on imported models

[[override]]
target = "building.glb::Wall"     # model::mesh_name
material = "materials/brick.toml"

[[override]]
target = "building.glb::Roof"
material = "materials/slate.toml"

[[override]]
target = "building.glb::Glass"
properties.transparency = 0.7
properties.roughness = 0.05
properties.metallic = 0.1"#}</code></pre>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Override Precedence"</strong>
                                    <p>"Overrides are applied in order: embedded model materials, then
                                    scene-level overrides, then entity-level component overrides. Later
                                    values win."</p>
                                </div>
                            </div>
                        </div>

                        <div id="materials-atlas" class="subsection">
                            <h3>"Texture Atlases"</h3>
                            <p>
                                "For performance, the engine automatically packs small textures into
                                atlas sheets during the build step. This reduces draw calls by allowing
                                multiple objects to share a single texture bind group."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"TOML"</span>
                                </div>
                                <pre><code class="language-toml">{r#"# .eustress/atlas.toml
[atlas]
max_size = 4096          # Maximum atlas dimension
padding = 2              # Pixel padding between entries
format = "BC7"           # GPU-compressed format
mip_levels = "auto"      # Generate full mip chain

# Textures smaller than this are atlas candidates
[atlas.threshold]
max_width = 512
max_height = 512"#}</code></pre>
                            </div>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Compression"</th>
                                            <th>"Quality"</th>
                                            <th>"Size (1024x1024)"</th>
                                            <th>"Use Case"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"BC7"</code></td>
                                            <td>"High"</td>
                                            <td>"1 MB"</td>
                                            <td>"Albedo, normal maps"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"BC5"</code></td>
                                            <td>"High"</td>
                                            <td>"0.5 MB"</td>
                                            <td>"Normal maps (RG only)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"BC4"</code></td>
                                            <td>"High"</td>
                                            <td>"0.25 MB"</td>
                                            <td>"Roughness, AO (single channel)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"RGBA8"</code></td>
                                            <td>"Lossless"</td>
                                            <td>"4 MB"</td>
                                            <td>"UI textures, masks"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // LEVEL DESIGN SECTION
                    // =========================================================
                    <section id="level" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Level Design"
                        </h2>

                        <div id="level-spatial" class="subsection">
                            <h3>"Spatial Organization"</h3>
                            <p>
                                "Eustress uses a right-handed Y-up coordinate system where 1 unit equals
                                1 meter. Levels are organized into spatial chunks for efficient streaming
                                and culling."
                            </p>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Axis"</th>
                                            <th>"Direction"</th>
                                            <th>"Convention"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"+X"</code></td>
                                            <td>"Right"</td>
                                            <td>"East"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"+Y"</code></td>
                                            <td>"Up"</td>
                                            <td>"Altitude"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"+Z"</code></td>
                                            <td>"Forward"</td>
                                            <td>"North"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"TOML"</span>
                                </div>
                                <pre><code class="language-toml">{r#"# scenes/main.scene.toml
[scene]
name = "Main World"
units = "meters"            # 1 unit = 1 meter
gravity = [0.0, -9.81, 0.0]

[scene.bounds]
min = [-2048.0, -500.0, -2048.0]
max = [2048.0, 2000.0, 2048.0]

[scene.streaming]
chunk_size = 256.0          # Spatial chunk size
load_radius = 1024.0        # Load chunks within this radius
unload_radius = 1536.0      # Unload beyond this radius"#}</code></pre>
                            </div>
                        </div>

                        <div id="level-lighting" class="subsection">
                            <h3>"Lighting Setup"</h3>
                            <p>
                                "Three light types are supported: directional (sun/moon), point (omni),
                                and spot (cone). Lights are ECS entities with configurable properties."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::lighting::prelude::*;

// Directional light (sun)
commands.spawn(DirectionalLightBundle {
    light: DirectionalLight {
        color: Color::rgb(1.0, 0.96, 0.88),
        illuminance: 100_000.0, // lux (bright sunlight)
        shadows_enabled: true,
        ..default()
    },
    transform: Transform::from_rotation(
        Quat::from_euler(EulerRot::XYZ, -45.0_f32.to_radians(), 30.0_f32.to_radians(), 0.0)
    ),
    ..default()
});

// Point light (lamp)
commands.spawn(PointLightBundle {
    light: PointLight {
        color: Color::rgb(1.0, 0.85, 0.6),
        intensity: 1600.0,    // lumens
        range: 20.0,          // meters
        radius: 0.1,          // source radius for soft shadows
        shadows_enabled: true,
        ..default()
    },
    transform: Transform::from_xyz(5.0, 3.0, 0.0),
    ..default()
});

// Spot light (flashlight)
commands.spawn(SpotLightBundle {
    light: SpotLight {
        color: Color::WHITE,
        intensity: 4000.0,
        range: 50.0,
        inner_angle: 15.0_f32.to_radians(),
        outer_angle: 35.0_f32.to_radians(),
        shadows_enabled: true,
        ..default()
    },
    transform: Transform::from_xyz(0.0, 2.0, 0.0)
        .looking_at(Vec3::new(10.0, 0.0, 0.0), Vec3::Y),
    ..default()
});"#}</code></pre>
                            </div>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Light Type"</th>
                                            <th>"Shadow Maps"</th>
                                            <th>"Max Count"</th>
                                            <th>"Cost"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td>"Directional"</td>
                                            <td>"Cascaded (4 cascades)"</td>
                                            <td>"1"</td>
                                            <td>"Low (global)"</td>
                                        </tr>
                                        <tr>
                                            <td>"Point"</td>
                                            <td>"Cubemap (6 faces)"</td>
                                            <td>"256"</td>
                                            <td>"Medium per light"</td>
                                        </tr>
                                        <tr>
                                            <td>"Spot"</td>
                                            <td>"Single (1 face)"</td>
                                            <td>"256"</td>
                                            <td>"Low per light"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>

                        <div id="level-atmosphere" class="subsection">
                            <h3>"Atmosphere & Skybox"</h3>
                            <p>
                                "Configure atmospheric scattering, volumetric fog, and skybox settings
                                to set the mood for your level."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"TOML"</span>
                                </div>
                                <pre><code class="language-toml">{r#"# scenes/main.atmosphere.toml
[atmosphere]
rayleigh_coefficient = [5.5e-6, 13.0e-6, 22.4e-6]
mie_coefficient = 21.0e-6
sun_intensity = 22.0
planet_radius = 6_371_000.0   # meters (Earth)
atmosphere_height = 100_000.0

[fog]
enabled = true
mode = "exponential"          # linear | exponential | volumetric
color = [0.7, 0.75, 0.8]
density = 0.002
start = 50.0                  # linear mode only
end = 500.0                   # linear mode only

[skybox]
mode = "procedural"           # procedural | cubemap | equirect
time_of_day = 10.5            # hours (24h format)
cloud_coverage = 0.4
cloud_speed = 0.01"#}</code></pre>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Day/Night Cycle"</strong>
                                    <p>"Set "<code>"time_of_day"</code>" to a dynamic value to enable a
                                    full day/night cycle. The atmosphere, fog color, and directional light
                                    direction all update automatically."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // CSG OPERATIONS SECTION
                    // =========================================================
                    <section id="csg" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "CSG Operations"
                        </h2>

                        <div id="csg-operations" class="subsection">
                            <h3>"Boolean Operations"</h3>
                            <p>
                                "Constructive Solid Geometry (CSG) allows you to combine primitive shapes
                                using boolean operations. Three operations are supported:"
                            </p>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Operation"</th>
                                            <th>"Symbol"</th>
                                            <th>"Result"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"Union"</code></td>
                                            <td>"A + B"</td>
                                            <td>"Combined volume of both shapes"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Subtract"</code></td>
                                            <td>"A - B"</td>
                                            <td>"A with B's volume removed"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Intersect"</code></td>
                                            <td>"A & B"</td>
                                            <td>"Only the overlapping volume"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::csg::prelude::*;

// Create a wall with a doorway using CSG subtract
let wall = CsgShape::cube(Vec3::new(10.0, 3.0, 0.3));
let doorway = CsgShape::cube(Vec3::new(1.2, 2.4, 0.5))
    .translate(Vec3::new(2.0, 1.2, 0.0));

let wall_with_door = wall.subtract(&doorway);

// Create a window using CSG subtract
let window = CsgShape::cube(Vec3::new(1.0, 1.0, 0.5))
    .translate(Vec3::new(-2.0, 1.8, 0.0));

let final_wall = wall_with_door.subtract(&window);

// Spawn the resulting mesh
commands.spawn(CsgBundle {
    mesh: final_wall.to_mesh(),
    material: MaterialHandle::from_path("materials/brick.toml"),
    ..default()
});"#}</code></pre>
                            </div>
                        </div>

                        <div id="csg-workflow" class="subsection">
                            <h3>"Non-Destructive Workflow"</h3>
                            <p>
                                "CSG operations in Eustress are non-destructive by default. The original
                                shapes and their operations are preserved in the scene graph. The final
                                mesh is recomputed whenever an operand changes."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"TOML"</span>
                                </div>
                                <pre><code class="language-toml">{r#"# scenes/main.scene.toml - CSG node
[[csg]]
name = "Wall with openings"
operation = "subtract"

[csg.base]
shape = "cube"
size = [10.0, 3.0, 0.3]
material = "materials/brick.toml"

[[csg.operands]]
shape = "cube"
size = [1.2, 2.4, 0.5]
position = [2.0, 1.2, 0.0]
operation = "subtract"

[[csg.operands]]
shape = "cube"
size = [1.0, 1.0, 0.5]
position = [-2.0, 1.8, 0.0]
operation = "subtract""#}</code></pre>
                            </div>

                            <div class="callout callout-warning">
                                <img src="/assets/icons/shield.svg" alt="Warning" />
                                <div>
                                    <strong>"Baking"</strong>
                                    <p>"For production builds, bake CSG operations into static meshes with "
                                    <code>"csg.bake()"</code>". This eliminates runtime recomputation and
                                    produces optimized collision geometry."</p>
                                </div>
                            </div>
                        </div>

                        <div id="csg-examples" class="subsection">
                            <h3>"Examples"</h3>
                            <p>
                                "Common CSG patterns for building architecture and props:"
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Arch: intersect a cylinder with a cube
let arch = CsgShape::cylinder(1.0, 0.4)
    .rotate(Quat::from_rotation_z(90.0_f32.to_radians()))
    .translate(Vec3::new(0.0, 2.0, 0.0));
let cut = CsgShape::cube(Vec3::new(2.0, 1.0, 0.4))
    .translate(Vec3::new(0.0, 2.0, 0.0));
let arch_shape = arch.intersect(&cut);

// Pipe: subtract inner cylinder from outer
let outer = CsgShape::cylinder(0.5, 10.0);
let inner = CsgShape::cylinder(0.45, 10.2);
let pipe = outer.subtract(&inner);

// L-shaped room: union two cubes
let room_a = CsgShape::cube(Vec3::new(8.0, 3.0, 6.0));
let room_b = CsgShape::cube(Vec3::new(4.0, 3.0, 10.0))
    .translate(Vec3::new(6.0, 0.0, 2.0));
let l_room = room_a.union(&room_b);"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // PERFORMANCE SECTION
                    // =========================================================
                    <section id="performance" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"08"</span>
                            "Performance"
                        </h2>

                        <div id="performance-lod" class="subsection">
                            <h3>"LOD Levels"</h3>
                            <p>
                                "Level of Detail (LOD) automatically reduces mesh complexity for distant
                                objects. LOD chains are generated at import time using meshopt
                                simplification."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::lod::prelude::*;

// Configure LOD for an entity
commands.spawn((
    MeshHandle(model),
    LodConfig {
        levels: vec![
            LodLevel { distance: 0.0,   quality: 1.0  },  // Full detail
            LodLevel { distance: 25.0,  quality: 0.5  },  // 50% triangles
            LodLevel { distance: 75.0,  quality: 0.25 },  // 25% triangles
            LodLevel { distance: 200.0, quality: 0.1  },  // 10% triangles
        ],
        crossfade: true,           // Dither fade between LODs
        crossfade_range: 5.0,      // Meters of fade overlap
    },
));"#}</code></pre>
                            </div>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"LOD Level"</th>
                                            <th>"Triangle %"</th>
                                            <th>"Typical Distance"</th>
                                            <th>"Visual Quality"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td>"LOD 0"</td>
                                            <td>"100%"</td>
                                            <td>"0 - 25 m"</td>
                                            <td>"Full detail, all features"</td>
                                        </tr>
                                        <tr>
                                            <td>"LOD 1"</td>
                                            <td>"50%"</td>
                                            <td>"25 - 75 m"</td>
                                            <td>"Simplified, no micro-detail"</td>
                                        </tr>
                                        <tr>
                                            <td>"LOD 2"</td>
                                            <td>"25%"</td>
                                            <td>"75 - 200 m"</td>
                                            <td>"Silhouette preserved"</td>
                                        </tr>
                                        <tr>
                                            <td>"LOD 3"</td>
                                            <td>"10%"</td>
                                            <td>"200+ m"</td>
                                            <td>"Billboard or impostor"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>

                        <div id="performance-culling" class="subsection">
                            <h3>"Occlusion Culling"</h3>
                            <p>
                                "The engine uses a hierarchical Z-buffer (HZB) occlusion culling system
                                that runs entirely on the GPU. Objects hidden behind other geometry are
                                skipped, reducing draw calls significantly in indoor and dense scenes."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::rendering::prelude::*;

// Enable occlusion culling on the camera
commands.spawn((
    Camera3dBundle::default(),
    OcclusionCulling {
        enabled: true,
        // Conservative: fewer false negatives, more draw calls
        // Aggressive: more false negatives, fewer draw calls
        mode: OcclusionMode::Conservative,
        // Minimum screen-space size to consider (pixels)
        min_screen_size: 4.0,
    },
));"#}</code></pre>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Occluder Hints"</strong>
                                    <p>"Mark large, opaque objects as occluders with the "
                                    <code>"Occluder"</code>" component. Walls, floors, and terrain chunks
                                    are automatically tagged. This helps the HZB pass prioritize the
                                    most effective blockers."</p>
                                </div>
                            </div>
                        </div>

                        <div id="performance-instancing" class="subsection">
                            <h3>"Instancing & Batching"</h3>
                            <p>
                                "Identical meshes sharing the same material are automatically instanced.
                                The engine batches draw calls to minimize GPU state changes. For scenes
                                with many identical objects (forests, debris), instancing can reduce draw
                                calls from thousands to single digits."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::instancing::prelude::*;

// Spawn 10,000 trees as instances — single draw call
let tree_mesh = asset_server.load("models/pine_tree.glb");
let tree_material = MaterialHandle::from_path("materials/tree.toml");

for i in 0..10_000 {
    let x = (i % 100) as f32 * 5.0;
    let z = (i / 100) as f32 * 5.0;
    let height = terrain.height_at(Vec2::new(x, z));

    commands.spawn(InstancedBundle {
        mesh: tree_mesh.clone(),
        material: tree_material.clone(),
        transform: Transform::from_xyz(x, height, z)
            .with_rotation(Quat::from_rotation_y(
                rand::random::<f32>() * std::f32::consts::TAU
            ))
            .with_scale(Vec3::splat(0.8 + rand::random::<f32>() * 0.4)),
        ..default()
    });
}"#}</code></pre>
                            </div>

                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Technique"</th>
                                            <th>"Savings"</th>
                                            <th>"Automatic"</th>
                                            <th>"Requirements"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td>"GPU Instancing"</td>
                                            <td>"~99% draw call reduction"</td>
                                            <td>"Yes"</td>
                                            <td>"Same mesh + material"</td>
                                        </tr>
                                        <tr>
                                            <td>"Draw Call Batching"</td>
                                            <td>"~60% state change reduction"</td>
                                            <td>"Yes"</td>
                                            <td>"Same material (different mesh ok)"</td>
                                        </tr>
                                        <tr>
                                            <td>"Texture Atlasing"</td>
                                            <td>"~80% bind group reduction"</td>
                                            <td>"Build step"</td>
                                            <td>"Textures under 512px"</td>
                                        </tr>
                                        <tr>
                                            <td>"Mesh Merging"</td>
                                            <td>"~90% for static geometry"</td>
                                            <td>"Opt-in"</td>
                                            <td>"Static, same material"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>

                        <div id="performance-streaming" class="subsection">
                            <h3>"GPU Mesh Streaming"</h3>
                            <p>
                                "For large worlds that exceed GPU memory, Eustress uses a virtual geometry
                                streaming system. Mesh data is loaded on demand based on camera proximity
                                and screen-space coverage."
                            </p>

                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::streaming::prelude::*;

// Configure mesh streaming
app.insert_resource(MeshStreamingConfig {
    // GPU memory budget for streamed meshes
    gpu_budget_mb: 512,
    // Maximum concurrent I/O requests
    max_pending_loads: 32,
    // Priority: screen-space size * (1 / distance)
    priority_mode: PriorityMode::ScreenCoverage,
    // Eviction policy when over budget
    eviction: EvictionPolicy::LeastRecentlyUsed,
    // Pre-fetch radius (meters ahead of camera)
    prefetch_distance: 100.0,
});"#}</code></pre>
                            </div>

                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/monitor.svg" alt="GPU" />
                                    </div>
                                    <h4>"512 MB Default Budget"</h4>
                                    <p>"Configurable GPU memory limit for streaming mesh data"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/play.svg" alt="Stream" />
                                    </div>
                                    <h4>"Async I/O"</h4>
                                    <p>"Non-blocking loads via io_uring (Linux) or IOCP (Windows)"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/grid.svg" alt="LOD" />
                                    </div>
                                    <h4>"LOD Integration"</h4>
                                    <p>"Coarse LODs load first, detail streams in progressively"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/trending.svg" alt="Metrics" />
                                    </div>
                                    <h4>"Budget Tracking"</h4>
                                    <p>"Real-time GPU memory usage exposed via diagnostics"</p>
                                </div>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Worlds of Any Size"</strong>
                                    <p>"With mesh streaming enabled, world size is limited only by disk
                                    space, not GPU memory. A 100 km\u{00B2} world with billions of triangles
                                    runs smoothly on 8 GB GPUs."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // Next/Prev Navigation
                    <nav class="docs-nav-footer">
                        <a href="/docs/physics" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Physics System"</span>
                            </div>
                        </a>
                        <a href="/docs/ui" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"UI System"</span>
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
