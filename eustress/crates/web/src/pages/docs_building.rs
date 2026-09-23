// =============================================================================
// Eustress Web - Building Documentation Page
// =============================================================================
// Building: parts and their properties, Models and Folders, booleans,
// materials, terrain, lighting, and what keeps a large Space fast.
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
                TocSubsection { id: "overview-world", title: "What a World Is Made Of" },
                TocSubsection { id: "overview-insert", title: "Adding Things" },
            ],
        },
        TocSection {
            id: "parts",
            title: "Parts",
            subsections: vec![
                TocSubsection { id: "parts-shapes", title: "Shapes and Meshes" },
                TocSubsection { id: "parts-properties", title: "Properties" },
                TocSubsection { id: "parts-units", title: "Meters" },
                TocSubsection { id: "parts-storage", title: "On Disk" },
            ],
        },
        TocSection {
            id: "grouping",
            title: "Grouping & Booleans",
            subsections: vec![
                TocSubsection { id: "grouping-models", title: "Models and Folders" },
                TocSubsection { id: "grouping-group", title: "Group and Ungroup" },
                TocSubsection { id: "grouping-csg", title: "Booleans" },
                TocSubsection { id: "grouping-limits", title: "Boolean Limits" },
            ],
        },
        TocSection {
            id: "materials",
            title: "Materials",
            subsections: vec![
                TocSubsection { id: "materials-library", title: "The Library" },
                TocSubsection { id: "materials-format", title: "Material Files" },
                TocSubsection { id: "materials-look", title: "Color, Tiling and Glow" },
            ],
        },
        TocSection {
            id: "terrain",
            title: "Terrain",
            subsections: vec![
                TocSubsection { id: "terrain-create", title: "Making Terrain" },
                TocSubsection { id: "terrain-sculpt", title: "Sculpting and Painting" },
                TocSubsection { id: "terrain-disk", title: "Terrain on Disk" },
                TocSubsection { id: "terrain-roads", title: "Roads" },
            ],
        },
        TocSection {
            id: "lighting",
            title: "Lighting",
            subsections: vec![
                TocSubsection { id: "lighting-lights", title: "Light Objects" },
                TocSubsection { id: "lighting-sun", title: "Sun and Time of Day" },
                TocSubsection { id: "lighting-sky", title: "Sky and Shadows" },
                TocSubsection { id: "lighting-more", title: "Labels and Particles" },
            ],
        },
        TocSection {
            id: "scale",
            title: "Large Builds",
            subsections: vec![
                TocSubsection { id: "scale-sharing", title: "Repetition Is Cheap" },
                TocSubsection { id: "scale-distance", title: "Render Distance and Lights" },
                TocSubsection { id: "scale-streaming", title: "Streaming Very Large Spaces" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-booleans", title: "Exact Booleans" },
                TocSubsection { id: "roadmap-effects", title: "Surface Effects" },
                TocSubsection { id: "roadmap-mesh", title: "Mesh Editing" },
                TocSubsection { id: "roadmap-terrain", title: "More Terrain Tools" },
            ],
        },
    ]
}

/// How a part gets its look: the part names a material, the material file
/// supplies factors and maps, the part's color and size tint and tile them,
/// and parts that end up looking the same share one GPU material.
#[component]
fn MaterialFlowDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 150" role="img"
                aria-label="A part names a material. The material file supplies factors and texture maps. The part's color tints them and its size sets the tiling. Parts that end up looking the same share one GPU material.">
                <defs>
                    <marker id="mat-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>
                <rect x="16" y="40" width="128" height="64" rx="8" class="dg-box"></rect>
                <text x="80" y="68" class="dg-label" text-anchor="middle">"Part"</text>
                <text x="80" y="88" class="dg-note" text-anchor="middle">"material, color, size"</text>
                <line x1="144" y1="72" x2="174" y2="72" class="dg-line" marker-end="url(#mat-arrow)"></line>

                <rect x="176" y="40" width="128" height="64" rx="8" class="dg-box"></rect>
                <text x="240" y="68" class="dg-label" text-anchor="middle">"Brick.mat.toml"</text>
                <text x="240" y="88" class="dg-note" text-anchor="middle">"factors and maps"</text>
                <line x1="304" y1="72" x2="334" y2="72" class="dg-line" marker-end="url(#mat-arrow)"></line>

                <rect x="336" y="40" width="128" height="64" rx="8" class="dg-box"></rect>
                <text x="400" y="68" class="dg-label" text-anchor="middle">"Tint and tile"</text>
                <text x="400" y="88" class="dg-note" text-anchor="middle">"color, 4 m repeat"</text>
                <line x1="464" y1="72" x2="494" y2="72" class="dg-line" marker-end="url(#mat-arrow)"></line>

                <rect x="496" y="40" width="128" height="64" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="560" y="68" class="dg-label" text-anchor="middle">"GPU material"</text>
                <text x="560" y="88" class="dg-note" text-anchor="middle">"shared by equal looks"</text>
            </svg>
            <figcaption>
                "The part names a material, the material file supplies factors and maps, and the
                part's color and size tint and tile them. Parts that come out looking the same share
                one GPU material and are drawn together."
            </figcaption>
        </figure>
    }
}

/// Building documentation page.
#[component]
pub fn DocsBuildingPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-building"></div>
            </div>

            <div class="docs-layout">
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

                <main class="docs-content">
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Building"</span>
                        </div>
                        <h1 class="docs-title">"Building"</h1>
                        <p class="docs-subtitle">
                            "Building is making a Space's world out of parts: solids with a shape, a size
                            in meters, a color and a material, grouped into Models, cut with booleans, set
                            on terrain and lit by lights."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "19 min read"
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

                        <div id="overview-world" class="subsection">
                            <h3>"What a World Is Made Of"</h3>
                            <p>
                                "A Space's world is the contents of its Workspace: parts, the Models and
                                Folders that group them, terrain, and the lights and labels that sit among
                                them. The sun, sky and atmosphere belong to the Lighting service. Parts,
                                Models, Folders and lights are objects with a class, a name and
                                properties: the Explorer lists them, the Properties panel edits them, and
                                scripts and AI agents reach them the same way."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Class"</th><th>"What it is"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"Part"</code></td><td>"A solid: one of six built-in shapes or any glTF mesh, sized in meters"</td></tr>
                                    <tr><td><code>"SpawnLocation"</code></td><td>"A part where your character appears when Play starts"</td></tr>
                                    <tr><td><code>"Model"</code></td><td>"A group of parts that selects and moves as one"</td></tr>
                                    <tr><td><code>"Folder"</code></td><td>"A container for organizing anything"</td></tr>
                                    <tr><td><code>"PointLight"</code>", "<code>"SpotLight"</code>", "<code>"SurfaceLight"</code>", "<code>"DirectionalLight"</code></td><td>"Light sources"</td></tr>
                                    <tr><td><code>"BillboardGui"</code></td><td>"A label that floats in the world and faces the camera"</td></tr>
                                    <tr><td>"Terrain"</td><td>"A heightfield landscape, one per Space"</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="overview-insert" class="subsection">
                            <h3>"Adding Things"</h3>
                            <ul class="docs-list">
                                <li><strong>"Toolbox"</strong>": the Primitives row inserts the six shapes with one click."</li>
                                <li><strong>"Model tab"</strong>": the Part, Model and Folder buttons, and Point and Spot for lights."</li>
                                <li><strong>"Insert Object"</strong>" ("<code>"Ctrl+I"</code>"): every insertable class, grouped by category."</li>
                            </ul>
                            <p>
                                "A new part appears 10 m in front of the camera, gray, Plastic and
                                unanchored, and arrives selected. With a Model or Folder selected in the
                                Explorer, it goes inside that container instead. Inserts from the Model tab
                                and Insert Object copy the class's template, a small file shipped with
                                Studio, so every new object starts with a complete set of properties. The
                                panels and tools you use from here are covered in "
                                <a href="/docs/studio">"Studio"</a>"."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // PARTS
                    // =========================================================
                    <section id="parts" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Parts"
                        </h2>

                        <div id="parts-shapes" class="subsection">
                            <h3>"Shapes and Meshes"</h3>
                            <p>
                                "A part is a Part object: one mesh, scaled to a size in meters, with a color
                                and a material. The six built-in shapes are glTF meshes built to a 1 m cube,
                                so a part's size is simply the scale applied to its mesh. The shape also
                                decides the collider physics uses."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Shape"</th><th>"Mesh"</th><th>"Collider"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Block"</td><td><code>"parts/block.glb"</code></td><td>"Box"</td></tr>
                                    <tr><td>"Ball"</td><td><code>"parts/ball.glb"</code></td><td>"Sphere"</td></tr>
                                    <tr><td>"Cylinder"</td><td><code>"parts/cylinder.glb"</code></td><td>"Cylinder along the part's Y axis"</td></tr>
                                    <tr><td>"Wedge"</td><td><code>"parts/wedge.glb"</code></td><td>"Box"</td></tr>
                                    <tr><td>"Corner wedge"</td><td><code>"parts/corner_wedge.glb"</code></td><td>"Box"</td></tr>
                                    <tr><td>"Cone"</td><td><code>"parts/cone.glb"</code></td><td>"Cylinder"</td></tr>
                                    <tr><td>"Custom"</td><td>"any other "<code>".glb"</code></td><td>"Box"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Point a part at any other glTF file and it becomes a custom-mesh part, what
                                other tools call a MeshPart. Eustress has one Part class for both and reads a
                                MeshPart class name as Part. Bringing meshes and whole models in is covered in "
                                <a href="/docs/importing">"Importing"</a>"."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A mesh named like a shape loads the shape"</strong>
                                    <p>
                                        "Studio recognizes the built-in shapes by file name: a mesh whose file
                                        name contains block, ball, cylinder, wedge or cone is drawn with the
                                        built-in shape of that name, so "<code>"traffic_cone.glb"</code>" renders
                                        as the stock cone. Keep those words out of custom mesh names. A Part file
                                        with no mesh at all is a block, and so is a part whose custom mesh is
                                        missing on disk."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="parts-properties" class="subsection">
                            <h3>"Properties"</h3>
                            <p>
                                "The Properties panel shows a part's values under Transform, Appearance and
                                Physics, with Rotation in degrees and a BrickColor picker beside Color. In
                                the part's file they are these keys; the defaults are the Part template's:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Key"</th><th>"Default"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"position"</code></td><td>"0, 0, 0"</td><td>"Center of the part, in meters"</td></tr>
                                    <tr><td><code>"rotation"</code></td><td>"0, 0, 0, 1"</td><td>"Orientation as a quaternion (x, y, z, w)"</td></tr>
                                    <tr><td><code>"scale"</code></td><td>"4.0, 1.2, 2.0"</td><td>"The part's size in meters"</td></tr>
                                    <tr><td><code>"color"</code></td><td>"163, 162, 165"</td><td>"RGB, 0 to 255; tints the material"</td></tr>
                                    <tr><td><code>"material"</code></td><td><code>"Plastic"</code></td><td>"A material name (see Materials below)"</td></tr>
                                    <tr><td><code>"transparency"</code></td><td>"0.0"</td><td>"0 is opaque, 1 invisible; from 0.5 up the part casts no shadow"</td></tr>
                                    <tr><td><code>"reflectance"</code></td><td>"0.0"</td><td>"0 to 1; adds metalness and cuts roughness"</td></tr>
                                    <tr><td><code>"anchored"</code></td><td><code>"false"</code></td><td>"Anchored parts stay put in Play; others fall and collide"</td></tr>
                                    <tr><td><code>"can_collide"</code></td><td><code>"true"</code></td><td>"Off removes the part's collider"</td></tr>
                                    <tr><td><code>"cast_shadow"</code></td><td><code>"true"</code></td><td>"Off stops the part casting shadows"</td></tr>
                                    <tr><td><code>"locked"</code></td><td><code>"false"</code></td><td>"Locked parts can't be picked in the viewport"</td></tr>
                                    <tr><td><code>"destructible"</code></td><td><code>"false"</code></td><td>"Impacts dent the part and can crack it apart"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "In Studio every part with a collider is static. Entering Play turns
                                unanchored parts that have a collider into moving bodies; how they move,
                                dent and break is covered in "<a href="/docs/physics">"Physics"</a>"."
                            </p>
                        </div>

                        <div id="parts-units" class="subsection">
                            <h3>"Meters"</h3>
                            <p>
                                "The world is measured in meters: one unit is one meter in files, physics,
                                raycasts and gizmos alike. A file may declare another authoring unit, and
                                Studio converts its position and size to meters as it loads:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Workspace/Post/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[metadata]
class_name = "Part"
unit = "cm"                    # m, cm, mm, ft or in

[asset]
mesh = "parts/cylinder.glb"

[transform]
position = [0.0, 50.0, 0.0]    # 0.5 m up
scale = [10.0, 100.0, 10.0]    # 0.1 x 1.0 x 0.1 m"#}</code></pre>
                            </div>
                            <p>
                                "The unit badge in the status bar picks the unit the Properties panel
                                displays lengths in. It is a view setting: it never rewrites a file."
                            </p>
                        </div>

                        <div id="parts-storage" class="subsection">
                            <h3>"On Disk"</h3>
                            <p>
                                "An object that owns files is a folder in the Space: a part with its own
                                mesh, anything with children, scripts and GUI. The folder holds an "
                                <code>"_instance.toml"</code>" and the files it owns, and the folder name is
                                the object's name. You can write one by hand while Studio runs; the file
                                watcher spawns it."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Workspace/Crate/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[metadata]
class_name = "Part"
archivable = true

[asset]
mesh = "parts/block.glb"
scene = "Scene0"

[transform]
position = [0.0, 1.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [2.0, 2.0, 2.0]

[properties]
color = [150, 111, 51]
material = "WoodPlanks"
anchored = true
can_collide = true"#}</code></pre>
                            </div>
                            <p>
                                "Plain primitive parts do not need a folder. In a Space with its database
                                (the default), a shape inserted from the Toolbox at the top level of the
                                Workspace is stored as a compact record in the Space's "
                                <code>"world.fjalldb"</code>", which is how a Space holds very large numbers of
                                parts. Either way it loads as the same Part. How the database and the files
                                relate, and how history is kept, is covered in "
                                <a href="/docs/universes">"Universes"</a>"."
                            </p>
                        </div>
                    </section>
                    // =========================================================
                    // GROUPING & BOOLEANS
                    // =========================================================
                    <section id="grouping" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Grouping & Booleans"
                        </h2>

                        <div id="grouping-models" class="subsection">
                            <h3>"Models and Folders"</h3>
                            <p>
                                "A Model groups parts into one object: click any part of a Model in the
                                viewport and the whole Model is selected, while "<code>"Alt"</code>" + click
                                selects just that part. A Folder only organizes. It can hold anything, and
                                its contents still select one at a time. Neither has geometry of its own."
                            </p>
                            <p>
                                "Insert either one from the Model tab or Insert Object, keep it selected, and
                                insert parts: they go inside it. Objects saved as folders can also be dragged
                                onto it in the Explorer. The move is made on disk and in the Space's database
                                together, and one "<code>"Ctrl+Z"</code>" undoes it."
                            </p>
                        </div>

                        <div id="grouping-group" class="subsection">
                            <h3>"Group and Ungroup"</h3>
                            <p>
                                <code>"Ctrl+G"</code>" wraps two or more selected objects in a new Model
                                placed at the average of their positions. "<code>"Ctrl+U"</code>" dissolves
                                each selected Model or Folder and lifts its children one level up. Neither
                                moves anything in the world, and each is a single undo step however much is
                                selected. Both also sit in the Edit group of the Home tab."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Ctrl+G groups for this session only"</strong>
                                    <p>
                                        "The Model that "<code>"Ctrl+G"</code>" creates is not written to the
                                        Space, so it is gone the next time the Space opens. For a group you
                                        want to keep, insert a Model first and build inside it."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="grouping-csg" class="subsection">
                            <h3>"Booleans"</h3>
                            <p>
                                "The Boolean group on the Drafting tab turns two or more selected parts into
                                one new part, computed as an exact solid by the same kernel as "
                                <a href="/docs/cad">"CAD"</a>". Select the parts, then click:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Button"</th><th>"Result"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Union"</td><td>"One solid covering everything the parts cover"</td></tr>
                                    <tr><td>"Subtract"</td><td>"The first part you selected, minus every other part"</td></tr>
                                    <tr><td>"Intersect"</td><td>"Only the volume all the parts share"</td></tr>
                                    <tr><td>"Separate"</td><td>"Dissolves a selected Model, like Ungroup"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The result is a part named UnionResult, SubtractResult or IntersectResult,
                                anchored, with the color and material of the first part. Its mesh is saved
                                as "<code>"result.glb"</code>" in its folder, so it survives a reload. The
                                source parts are removed; those saved as folders go to the Space's trash,
                                and one "<code>"Ctrl+Z"</code>" brings them back and removes the result."
                            </p>
                        </div>

                        <div id="grouping-limits" class="subsection">
                            <h3>"Boolean Limits"</h3>
                            <ul class="docs-list">
                                <li><strong>"Shapes"</strong>": blocks and cylinders are cut exactly. Balls are cut as cubes, cones as straight cylinders and wedges as their bounding blocks, and the notice after the operation names any shape it approximated."</li>
                                <li><strong>"Collision"</strong>": the result collides as a box the size of its bounds."</li>
                                <li><strong>"Failures"</strong>": when the kernel cannot build a solid (coplanar faces, for example), the selection is grouped into a Model instead and the notice says why."</li>
                                <li><strong>"Baked"</strong>": the result is plain geometry. To change a cut, undo it, adjust the parts and run it again."</li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // MATERIALS
                    // =========================================================
                    <section id="materials" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Materials"
                        </h2>

                        <div id="materials-library" class="subsection">
                            <h3>"The Library"</h3>
                            <p>
                                "A material is a named surface: its color maps, bumpiness, roughness and
                                metalness. A part's "<code>"material"</code>" takes one of 22 built-in names:"
                            </p>
                            <p>
                                <code>"Plastic"</code>", "<code>"SmoothPlastic"</code>", "<code>"Wood"</code>", "
                                <code>"WoodPlanks"</code>", "<code>"Metal"</code>", "<code>"CorrodedMetal"</code>", "
                                <code>"DiamondPlate"</code>", "<code>"Foil"</code>", "<code>"Grass"</code>", "
                                <code>"Concrete"</code>", "<code>"Brick"</code>", "<code>"Granite"</code>", "
                                <code>"Marble"</code>", "<code>"Slate"</code>", "<code>"Sand"</code>", "
                                <code>"Fabric"</code>", "<code>"Glass"</code>", "<code>"Neon"</code>", "
                                <code>"Ice"</code>", "<code>"Gold"</code>", "<code>"Silver"</code>", "
                                <code>"Bronze"</code>"."
                            </p>
                            <p>
                                "Each has a definition file in the Space's MaterialService folder, and Studio
                                copies in any that are missing when a Space opens. Eighteen carry 2048 x 2048
                                texture maps drawn to repeat every 4 m and to wrap edge to edge, so a long
                                wall shows brick-sized bricks with no visible joins. Plastic, SmoothPlastic,
                                Glass and Neon are plain surfaces with no maps."
                            </p>
                        </div>

                        <div id="materials-format" class="subsection">
                            <h3>"Material Files"</h3>
                            <p>
                                "A material file has three tables: "<code>"[material]"</code>" names it and
                                picks a preset (the built-in material whose roughness, metalness and
                                reflectance fill any factor left out), "<code>"[pbr]"</code>" holds factors
                                that multiply the maps, and "<code>"[textures]"</code>" names the maps. This is
                                the library's Brick:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MaterialService/Brick.mat.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[material]
name = "Brick"
preset = "Brick"
description = "Red brick in running bond, 25 x 8.3 cm courses in recessed mortar"

[pbr]
base_color = [1.0, 1.0, 1.0, 1.0]
metallic = 0.0
roughness = 1.0
reflectance = 0.5

# metallic_roughness is packed glTF-style: R = occlusion, G = roughness, B = metallic.
[textures]
base_color = "materials/textures/brick_base_color.png"
normal = "materials/textures/brick_normal.png"
metallic_roughness = "materials/textures/brick_metallic_roughness.png"
occlusion = "materials/textures/brick_metallic_roughness.png""#}</code></pre>
                            </div>
                            <p>
                                "Map paths resolve next to the material file first and in the bundled
                                library second. The loader also reads "<code>"emissive"</code>" and "
                                <code>"depth"</code>" maps, and in "<code>"[pbr]"</code>": "<code>"alpha_mode"</code>
                                " (opaque, blend or mask, with "<code>"alpha_cutoff"</code>"), "
                                <code>"double_sided"</code>", "<code>"unlit"</code>", "<code>"emissive"</code>", and "
                                <code>"ior"</code>", "<code>"specular_transmission"</code>", "
                                <code>"diffuse_transmission"</code>" and "<code>"thickness"</code>" for
                                see-through materials."
                            </p>
                            <p>
                                "To add your own, drop a new "<code>".mat.toml"</code>" into MaterialService
                                (it loads while the Space is open) and set a part's material to its name:
                                the "<code>"name"</code>" in "<code>"[material]"</code>", or the file name when
                                that is empty. The MaterialService entry in "<a href="/docs/services">"Services"</a>
                                " covers the registry behind it."
                            </p>
                        </div>

                        <div id="materials-look" class="subsection">
                            <h3>"Color, Tiling and Glow"</h3>
                            <MaterialFlowDiagram />
                            <p>
                                "A part's color multiplies its material. The library maps are colored, so a
                                white part shows a map as drawn and a tinted part tints it. Tiling follows
                                the part's size: blocks, wedges and corner wedges get the texture laid flat on
                                each face with one repeat per 4 m of the broad faces, balls wrap it around,
                                and cylinders, cones and custom meshes use their own texture coordinates,
                                scaled by their two largest dimensions."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Reflectance"</strong>" adds to a material's metalness and cuts its roughness by up to half, so any material can be polished."</li>
                                <li><strong>"Transparency"</strong>" above 0 blends the part with what is behind it. Glass is smooth and highly reflective; give it some transparency to see through it."</li>
                                <li><strong>"Neon"</strong>" glows: it emits the part's color at twice its strength."</li>
                                <li><strong>"Maps"</strong>" load only when a part first uses the material, and each gets a full chain of smaller versions (mipmaps) so distant surfaces stay smooth. KTX2, DDS and Basis files bring their own."</li>
                            </ul>
                        </div>
                    </section>
                    // =========================================================
                    // TERRAIN
                    // =========================================================
                    <section id="terrain" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Terrain"
                        </h2>

                        <div id="terrain-create" class="subsection">
                            <h3>"Making Terrain"</h3>
                            <p>
                                "Terrain is a heightfield: one ground surface for the Space, stored as a grid
                                of heights and cut into square chunks that draw, collide and save on their
                                own. Making terrain replaces any terrain already there. The Terrain tab
                                offers:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Button"</th><th>"Result"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Small, Medium, Large"</td><td>"A generated world of 2 x 2, 3 x 3 or 4 x 4 regions, each 1,024 m square, built in the background: drainage and rivers, erosion, climate, biomes and ground materials"</td></tr>
                                    <tr><td>"Flat"</td><td>"A level 576 m baseplate at Y = 0 that you can dig 32 m into or raise 96 m above"</td></tr>
                                    <tr><td>"Import, Export"</td><td>"Build terrain from a heightmap image (PNG, R16 or RAW), or save the current terrain as a 16-bit grayscale PNG"</td></tr>
                                    <tr><td>"Water"</td><td>"Shows or hides a flat, see-through water plane at Y = 0 across the terrain; it has no collision"</td></tr>
                                    <tr><td>"Clear"</td><td>"Deletes the terrain and its files"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The Terrain panel (Terrain in the left panel's overflow menu) adds flat
                                plates of 320 m and 1.1 km, and Generate World, which takes your own seed:
                                the same seed and size always produce the same world. The ribbon's Small,
                                Medium and Large use seed 42."
                            </p>
                            <div class="callout callout-warning">
                                <img src="/assets/icons/shield.svg" alt="Warning" />
                                <div>
                                    <strong>"Clear, Generate and Import cannot be undone"</strong>
                                    <p>
                                        "Clear deletes "<code>"Workspace/Terrain"</code>" from disk straight away,
                                        and Generate, Flat and Import overwrite it. Commit the Space to git or
                                        copy the folder first if you might want the old terrain back."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="terrain-sculpt" class="subsection">
                            <h3>"Sculpting and Painting"</h3>
                            <p>
                                "Click Edit on the Terrain tab, or pick a brush, then drag across the ground.
                                While editing, "<code>"1"</code>" to "<code>"5"</code>" choose Raise, Lower,
                                Smooth, Flatten and Paint, and "<code>"["</code>" and "<code>"]"</code>" shrink
                                and grow the brush in 2 m steps between 1 and 50 m (it starts at 8 m). Each
                                stroke is one undo step, named Sculpt Terrain or Paint Terrain, and Save writes
                                both the heights and the paint."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Paint"</strong>" lays the grass layer; nothing in Studio picks another layer yet."</li>
                                <li><strong>"Region and Fill"</strong>" are not built; clicking them says so and keeps the current brush."</li>
                                <li><strong>"Look"</strong>": terrain is colored per vertex from four weights (grass, rock, dirt, snow) with slope and curvature shading, not textured."</li>
                                <li><strong>"Shape"</strong>": one height per point, so there are no caves or overhangs, and heights stay inside the band the terrain was made with."</li>
                            </ul>
                            <p>
                                "Every chunk has a heightfield collider built from its full-detail heights,
                                so falling parts and raycasts stop at the ground, and a chunk's collider is
                                rebuilt a moment after you stop editing it. Farther chunks draw with fewer
                                samples."
                            </p>
                        </div>

                        <div id="terrain-disk" class="subsection">
                            <h3>"Terrain on Disk"</h3>
                            <p>
                                "Terrain lives in "<code>"Workspace/Terrain"</code>": a settings file, one
                                16-bit height file per chunk under "<code>"chunks/"</code>", and one paint
                                image per chunk under "<code>"splatmap/"</code>" whose red, green, blue and alpha
                                channels hold the grass, rock, dirt and snow weights. The settings file the
                                Flat plate writes begins:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Workspace/Terrain/_terrain.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[terrain]
chunk_size = 64.0          # meters per chunk side
chunk_resolution = 64      # height samples per chunk side
height_scale = 128.0       # height band, meters
height_offset = -32.0      # world Y of the lowest height
seed = 0

[streaming]
view_distance = 256.0      # chunks cover -4 to +4 around the origin"#}</code></pre>
                            </div>
                            <p>
                                "A height sample's world Y is "<code>"height_offset"</code>" plus its 16-bit
                                value, scaled to "<code>"height_scale"</code>". Terrain imported with a Roblox
                                place arrives as voxels in the Space's database, and Studio shows its top
                                surface; see "<a href="/docs/importing">"Importing"</a>"."
                            </p>
                        </div>

                        <div id="terrain-roads" class="subsection">
                            <h3>"Roads"</h3>
                            <p>
                                "In Civil mode, the Plugins tab carries a Road Builder. Add Node places road
                                points on the terrain with each click (right-click or "<code>"Esc"</code>"
                                finishes), Apply to Terrain cuts and fills the ground along a smooth curve
                                through them and lays a road surface on top, and Remove Road deletes the road
                                and restores the ground. "<code>"Ctrl+Z"</code>" reverses the ground changes
                                either one makes."
                            </p>
                        </div>
                    </section>
                    // =========================================================
                    // LIGHTING
                    // =========================================================
                    <section id="lighting" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Lighting"
                        </h2>

                        <div id="lighting-lights" class="subsection">
                            <h3>"Light Objects"</h3>
                            <p>
                                "A light object adds a real light to the scene. A PointLight shines in every
                                direction, a SpotLight in a cone, and a DirectionalLight in parallel rays
                                across the whole Space, like the sun. A SurfaceLight currently shines as a
                                point light from where it sits. Changes in the Properties panel apply to the
                                light at once."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Property"</th><th>"Default"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"Brightness"</code></td><td>"1"</td><td>"A dial: each unit is 50,000 lumens for point, spot and surface lights, and 10,000 lux for a DirectionalLight"</td></tr>
                                    <tr><td><code>"Color"</code></td><td>"White"</td><td>"The light's color"</td></tr>
                                    <tr><td><code>"Range"</code></td><td>"60 m"</td><td>"How far a point, spot or surface light reaches"</td></tr>
                                    <tr><td><code>"Angle"</code></td><td>"45"</td><td>"SpotLight only: degrees from the center of the beam to its edge, so 45 makes a 90 degree cone, at full strength out to 85 percent of the angle"</td></tr>
                                    <tr><td><code>"Shadows"</code></td><td>"On"</td><td>"Whether the light casts shadows"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Studio budgets point and spot lights by distance to the camera, so a Space
                                can hold thousands; see "<a href="#scale-distance">"Render Distance and Lights"</a>"."
                            </p>
                        </div>

                        <div id="lighting-sun" class="subsection">
                            <h3>"Sun and Time of Day"</h3>
                            <p>
                                "The sun, moon, sky and atmosphere belong to the Lighting service, and a new
                                Space gets Sun, Moon, Sky and Atmosphere objects inside it. Select Lighting in
                                the Explorer to set the time and the overall light:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Property"</th><th>"Default"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"ClockTime"</code></td><td>"14:00:00"</td><td>"Time of day; the sun's height, color and strength follow it. The TimeOfDay slider scrubs it"</td></tr>
                                    <tr><td><code>"GeographicLatitude"</code></td><td>"41.73"</td><td>"The latitude the sun's path is worked out for"</td></tr>
                                    <tr><td><code>"Brightness"</code></td><td>"2"</td><td>"Scales sunlight and sky fill together: 2 is neutral, 4 doubles them"</td></tr>
                                    <tr><td><code>"OutdoorAmbient"</code></td><td>"Gray"</td><td>"Color of the sky light that fills shadows (Ambient stands in when it is black)"</td></tr>
                                    <tr><td><code>"ExposureCompensation"</code></td><td>"0"</td><td>"Camera exposure in stops: +1 is twice as bright"</td></tr>
                                    <tr><td><code>"FogStart"</code>", "<code>"FogEnd"</code>", "<code>"FogColor"</code></td><td>"0 m, 100,000 m, light gray"</td><td>"Fog that thickens linearly between the two distances"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "In Studio the sun stays where ClockTime puts it. The sun dims toward the
                                horizon and gives no light once it is 6 degrees below it. During Play the
                                simulation clock moves the time of day forward; the clock is covered in "
                                <a href="/docs/simulation">"Simulation"</a>"."
                            </p>
                        </div>

                        <div id="lighting-sky" class="subsection">
                            <h3>"Sky and Shadows"</h3>
                            <p>
                                "The sky is drawn by a physically based atmosphere model, which scatters
                                sunlight the way air does, so its color follows the sun through the day, and
                                the sun itself warms toward the horizon. At night a star field and a moon disc
                                showing its current phase appear. The sun casts shadows in four cascades out
                                to 1,000 m from the camera."
                            </p>
                        </div>

                        <div id="lighting-more" class="subsection">
                            <h3>"Labels and Particles"</h3>
                            <p>
                                "A BillboardGui is a text or image label that floats in the world and always
                                faces the camera; building one is covered in "<a href="/docs/ui">"UI Systems"</a>
                                ". The Particle Sim button on the Model tab inserts a physical particle
                                simulation (fluids, electrons, conduction), covered in "
                                <a href="/docs/realism">"Realism"</a>"."
                            </p>
                        </div>
                    </section>
                    // =========================================================
                    // LARGE BUILDS
                    // =========================================================
                    <section id="scale" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "Large Builds"
                        </h2>

                        <div id="scale-sharing" class="subsection">
                            <h3>"Repetition Is Cheap"</h3>
                            <p>
                                "Every part of the same shape shares one mesh, and every part with the same
                                look (material, color, transparency and reflectance, plus tiling on a textured
                                material) shares one GPU material, so identical parts are drawn together in
                                batches. A build that reuses a palette of colors and materials draws far
                                faster than one where every part is a slightly different shade. When a Space
                                is too big to load in a single frame, Studio also rounds colors to 16 levels
                                per channel so near-identical shades share a material."
                            </p>
                            <p>
                                "Shadows are the other big cost: the sun renders every shadow caster again
                                into each of its four shadow cascades, out to 1,000 m. Switch "
                                <code>"CastShadow"</code>" off on parts nobody will see a shadow from, such as
                                trim, interiors and anything under a roof."
                            </p>
                        </div>

                        <div id="scale-distance" class="subsection">
                            <h3>"Render Distance and Lights"</h3>
                            <p>
                                "The Workspace's "<code>"RenderDistance"</code>" property (Rendering, 5,000 m in
                                a new Space) is how far away parts are drawn. A part stops drawing once the
                                nearest point of its bounding sphere is beyond that distance, so a large
                                baseplate stays visible while you stand on it. Editing the value applies to
                                every part at once."
                            </p>
                            <p>
                                "Lights are budgeted by distance to the camera. The 64 nearest point and spot
                                lights shine; farther ones go dark beyond 350 m and relight within 250 m. The
                                32 nearest cast shadows and the rest do not, whatever their own shadow
                                setting says. The list is refreshed after 5 m of camera travel or every 30
                                frames."
                            </p>
                        </div>

                        <div id="scale-streaming" class="subsection">
                            <h3>"Streaming Very Large Spaces"</h3>
                            <p>
                                "When a Space's database holds more than 100,000 objects, Studio stops
                                loading everything at open. It loads the parts within 350 m of the camera and
                                unloads them past 500 m, saving edits first and never unloading the
                                selection. Everything farther away is drawn as stand-ins: one merged mesh per
                                256 m cell, built once in the background from the cell's shaped parts and
                                cut to about a tenth of their triangles, at most 2,000 per cell. A cell's
                                stand-in hides as the real parts around the camera take over."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Stand-ins"</strong>" can't be selected, cast no shadows, and leave out custom meshes and transparent parts."</li>
                                <li><strong>"They are built when the Space opens"</strong>" and not rebuilt, so the distant view shows a session's edits after the Space reopens."</li>
                                <li><strong>"Colliders"</strong>" in a streaming Space are created during Play only near moving bodies, the player and the camera (within 128 m), which keeps physics cost flat however big the map is."</li>
                            </ul>
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

                        <div id="roadmap-booleans" class="subsection">
                            <h3>"Exact Booleans for Every Shape"</h3>
                            <p>
                                "Booleans will cut balls, cones and wedges as their true shapes once the solid
                                kernel builds those shapes directly. Until then the notice after each
                                operation names every approximation it made, so a cut that comes out boxy is
                                never a mystery."
                            </p>
                        </div>

                        <div id="roadmap-effects" class="subsection">
                            <h3>"Surface Effects"</h3>
                            <p>
                                "Decals, tiled textures, beams between attachments and particle emitters will
                                draw in the viewport. Their classes already exist and can be inserted; the
                                renderers that draw them come next, together with light cookies, the
                                patterned textures a light can project."
                            </p>
                        </div>

                        <div id="roadmap-mesh" class="subsection">
                            <h3>"Mesh Editing"</h3>
                            <p>
                                "Custom meshes will get a face-editing mode in Studio. The half-edge editing
                                kernel it will use is already in the repository and extrudes and insets
                                faces; bevel and loop cut will follow before the mode ships."
                            </p>
                        </div>

                        <div id="roadmap-terrain" class="subsection">
                            <h3>"More Terrain Tools"</h3>
                            <p>
                                "The Region and Fill brushes already sit on the Terrain tab and will do real
                                work once they are built, and Paint will gain a choice of layer beyond grass,
                                using the rock, dirt and snow channels the paint maps already carry."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Parts, materials, terrain and light, measured in meters and saved with the Space."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/cad" class="btn-secondary-steel">"CAD Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/studio" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Studio"</span>
                            </div>
                        </a>
                        <a href="/docs/perspective" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Perspective"</span>
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
