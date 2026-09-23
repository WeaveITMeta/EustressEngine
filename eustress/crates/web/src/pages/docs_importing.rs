// =============================================================================
// Eustress Web - Importing Documentation Page
// =============================================================================
// Importing: Roblox places and models, Gaussian splats, glTF models, images,
// video, tables of data and heightmaps, and what each becomes in a Space.
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
                TocSubsection { id: "overview-ways", title: "Ways In" },
                TocSubsection { id: "overview-formats", title: "Supported Formats" },
            ],
        },
        TocSection {
            id: "roblox",
            title: "Roblox Places",
            subsections: vec![
                TocSubsection { id: "roblox-run", title: "Running an Import" },
                TocSubsection { id: "roblox-classes", title: "Classes and Services" },
                TocSubsection { id: "roblox-parts", title: "Parts, Shapes and Units" },
                TocSubsection { id: "roblox-scripts", title: "Scripts, Values and Joints" },
            ],
        },
        TocSection {
            id: "roblox-assets",
            title: "Meshes, Unions, Terrain",
            subsections: vec![
                TocSubsection { id: "assets-download", title: "Asset Downloads" },
                TocSubsection { id: "assets-unions", title: "Unions" },
                TocSubsection { id: "assets-terrain", title: "Terrain" },
            ],
        },
        TocSection {
            id: "models",
            title: "3D Models",
            subsections: vec![
                TocSubsection { id: "models-gltf", title: "glTF and GLB" },
                TocSubsection { id: "models-mesh", title: "Custom Part Meshes" },
                TocSubsection { id: "models-other", title: "Other Formats" },
            ],
        },
        TocSection {
            id: "splats",
            title: "Gaussian Splats",
            subsections: vec![
                TocSubsection { id: "splats-import", title: "Importing a Splat" },
                TocSubsection { id: "splats-load", title: "What Happens on Load" },
                TocSubsection { id: "splats-mcp", title: "Over MCP" },
            ],
        },
        TocSection {
            id: "media",
            title: "Images and Video",
            subsections: vec![
                TocSubsection { id: "media-chooser", title: "The Radial Chooser" },
                TocSubsection { id: "media-surface", title: "Decals and Textures" },
                TocSubsection { id: "media-status", title: "What Displays Today" },
            ],
        },
        TocSection {
            id: "data",
            title: "Data and Heightmaps",
            subsections: vec![
                TocSubsection { id: "data-datasets", title: "Datasets" },
                TocSubsection { id: "data-heightmaps", title: "Heightmaps" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-roblox", title: "Roblox Import" },
                TocSubsection { id: "roadmap-formats", title: "More Formats" },
            ],
        },
    ]
}

/// The Roblox import pipeline: parse first, then build a brand-new Space,
/// write one instance folder per Roblox instance (downloading assets on the
/// way), and open the new Space.
#[component]
fn RobloxImportDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 200" role="img"
                aria-label="A Roblox file is parsed, a new Space is created, every instance is written as a folder while meshes, images and sounds are downloaded through a cache, and then Studio opens the new Space.">
                <defs>
                    <marker id="import-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                <rect x="10" y="30" width="100" height="52" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="60" y="61" class="dg-label" text-anchor="middle">".rbxl file"</text>
                <line x1="110" y1="56" x2="138" y2="56" class="dg-line" marker-end="url(#import-arrow)"></line>

                <rect x="138" y="30" width="100" height="52" rx="8" class="dg-box"></rect>
                <text x="188" y="61" class="dg-label" text-anchor="middle">"Parse"</text>
                <line x1="238" y1="56" x2="266" y2="56" class="dg-line" marker-end="url(#import-arrow)"></line>

                <rect x="266" y="30" width="110" height="52" rx="8" class="dg-box"></rect>
                <text x="321" y="61" class="dg-label" text-anchor="middle">"New Space"</text>
                <line x1="376" y1="56" x2="404" y2="56" class="dg-line" marker-end="url(#import-arrow)"></line>

                <rect x="404" y="30" width="110" height="52" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="459" y="54" class="dg-label" text-anchor="middle">"Write"</text>
                <text x="459" y="71" class="dg-note" text-anchor="middle">"one folder each"</text>
                <line x1="514" y1="56" x2="542" y2="56" class="dg-line" marker-end="url(#import-arrow)"></line>

                <rect x="542" y="30" width="90" height="52" rx="8" class="dg-box"></rect>
                <text x="587" y="61" class="dg-label" text-anchor="middle">"Open"</text>

                <line x1="459" y1="82" x2="459" y2="124" class="dg-line dg-line-dashed" marker-end="url(#import-arrow)"></line>
                <rect x="374" y="124" width="170" height="52" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="459" y="148" class="dg-label" text-anchor="middle">"Asset downloads"</text>
                <text x="459" y="165" class="dg-note" text-anchor="middle">"cached per Universe"</text>

                <text x="188" y="104" class="dg-note" text-anchor="middle">"a bad file stops here"</text>
                <text x="321" y="104" class="dg-note" text-anchor="middle">"named after the file"</text>
            </svg>
            <figcaption>
                "Every Roblox import builds a new Space. Parsing happens first, so a file that cannot be read
                leaves nothing behind, and the Space you had open is never written to."
            </figcaption>
        </figure>
    }
}

/// Importing documentation page.
#[component]
pub fn DocsImportingPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-importing"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/download.svg" alt="Importing" class="toc-icon" />
                        <h2>"Importing"</h2>
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
                            <span class="current">"Importing"</span>
                        </div>
                        <h1 class="docs-title">"Importing"</h1>
                        <p class="docs-subtitle">
                            "Importing brings outside content into a Space: Roblox places and models, Gaussian
                            splat captures, glTF models, images, video, tables of data and heightmaps. Each
                            importer turns a file into ordinary instances that appear in the Explorer and save
                            with the Space."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "19 min read"
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

                        <div id="overview-ways" class="subsection">
                            <h3>"Ways In"</h3>
                            <p>
                                "An import copies a file into your Universe or Space and writes the instances that
                                use it. Most files come in through one button: "<strong>"Import"</strong>" in the
                                Home tab's File group. It opens a file picker listing every type it accepts and
                                sends each file to the right importer by its extension."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Way in"</th><th>"Takes"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Home tab, File group: "<strong>"Import"</strong></td><td>"Roblox places and models, Gaussian splats, images, videos"</td></tr>
                                    <tr><td>"File menu: "<strong>"Import Roblox Place..."</strong></td><td>"Roblox places and models only"</td></tr>
                                    <tr><td>"Data tab: "<strong>"Import"</strong></td><td>"CSV, JSON Lines and Parquet tables"</td></tr>
                                    <tr><td>"Terrain menu, Assets: "<strong>"Import Heightmap..."</strong></td><td>"Heightmap images and elevation grids"</td></tr>
                                    <tr><td>"Assets panel: "<strong>"Import"</strong></td><td>"Copies files into the Universe's "<code>".eustress/assets/meshes/"</code>" folder, without adding anything to the Space"</td></tr>
                                    <tr><td>"Copying a file into the Space's "<code>"Workspace"</code>" folder"</td><td>"glTF models ("<code>".glb"</code>", "<code>".gltf"</code>")"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Studio has no handler yet for files dropped onto its window from a file manager, so
                                use the Import buttons or copy the file into the Space folder."
                            </p>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/roblox.svg" alt="Roblox" />
                                    </div>
                                    <h4>"Roblox Places"</h4>
                                    <p>"Whole places into a new Space, with meshes, unions and terrain."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/sparkles.svg" alt="Splats" />
                                    </div>
                                    <h4>"Gaussian Splats"</h4>
                                    <p>"Photoreal captures that render beside parts and collide."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/image.svg" alt="Media" />
                                    </div>
                                    <h4>"Images and Video"</h4>
                                    <p>"A radial chooser picks the class a picture or clip becomes."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/cube.svg" alt="Models" />
                                    </div>
                                    <h4>"Models and Data"</h4>
                                    <p>"glTF files, datasets and heightmaps."</p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-formats" class="subsection">
                            <h3>"Supported Formats"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Files"</th><th>"Becomes in the Explorer"</th><th>"Stored at"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>".rbxl"</code>", "<code>".rbxlx"</code>", "<code>".rbxm"</code>", "<code>".rbxmx"</code></td><td>"A new Space holding the place's instances"</td><td><code>"<Universe>/Spaces/<file name>/"</code></td></tr>
                                    <tr><td><code>".ply"</code>" (3D Gaussian splats)"</td><td>"A "<code>"GaussianSplats"</code>" instance in Workspace"</td><td><code>"<Universe>/assets/splats/"</code></td></tr>
                                    <tr><td><code>".png"</code>", "<code>".jpg"</code>", "<code>".jpeg"</code>", "<code>".webp"</code>", "<code>".bmp"</code>", "<code>".gif"</code>", "<code>".tga"</code></td><td>"A Decal, Texture, ImageLabel or ImageButton, your choice"</td><td><code>"<Universe>/assets/images/"</code></td></tr>
                                    <tr><td><code>".mp4"</code>", "<code>".webm"</code>", "<code>".mov"</code>", "<code>".mkv"</code></td><td>"A VideoFrame or a 3D Video quad, your choice"</td><td><code>"<Universe>/assets/videos/"</code></td></tr>
                                    <tr><td><code>".glb"</code>", "<code>".gltf"</code></td><td>"A Part that renders the file's scene"</td><td>"Where you copy it in the Space"</td></tr>
                                    <tr><td><code>".csv"</code>", "<code>".json"</code>", "<code>".jsonl"</code>", "<code>".parquet"</code></td><td>"A Dataset"</td><td>"A folder beside the Dataset's instance file"</td></tr>
                                    <tr><td><code>".png"</code>", "<code>".r16"</code>", "<code>".raw"</code>", "<code>".hgt"</code>", "<code>".asc"</code>", "<code>".tif"</code>", "<code>".tiff"</code>" (heightmaps)"</td><td>"Terrain"</td><td><code>"Workspace/Terrain/"</code></td></tr>
                                    <tr><td><code>".stl"</code>", "<code>".step"</code>", "<code>".obj"</code>", mesh "<code>".ply"</code>", "<code>".fbx"</code>", USD"</td><td>"Nothing yet (see "<a href="#models-other">"Other Formats"</a>")"</td><td>"Not imported"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Copied files keep their names, with unsafe characters replaced and a suffix added
                                when the name is already taken, so importing the same file twice never overwrites
                                the first copy."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // ROBLOX PLACES
                    // =========================================================
                    <section id="roblox" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Roblox Places"
                        </h2>

                        <div id="roblox-run" class="subsection">
                            <h3>"Running an Import"</h3>
                            <p>
                                "The Roblox importer reads all four Roblox file formats: binary places ("
                                <code>".rbxl"</code>"), XML places ("<code>".rbxlx"</code>") and the binary and XML
                                model files ("<code>".rbxm"</code>", "<code>".rbxmx"</code>"). It checks the file's
                                own header rather than trusting the extension."
                            </p>
                            <ol class="numbered-list">
                                <li>"Open any Space in the Universe that should receive the import."</li>
                                <li>"Press "<strong>"Import"</strong>" on the Home tab, or choose "<strong>"Import Roblox Place..."</strong>" from the File menu, and pick the file."</li>
                                <li>"Studio creates a new Space named after the file under "<code>"<Universe>/Spaces/"</code>", adding "<em>" 2"</em>", "<em>" 3"</em>" and so on when the name is taken, and removes the starter parts a new Space normally gets."</li>
                                <li>"Every Roblox instance is written as a folder with its own "<code>"_instance.toml"</code>", downloading meshes, images and sounds as it goes."</li>
                                <li>"A notification reports how many entities were imported and how many warnings were raised, then Studio opens the new Space."</li>
                            </ol>
                            <RobloxImportDiagram />
                            <p>
                                "The import runs as one step before Studio responds again, so a large place with many
                                assets to download takes a while. The full report (class counts, unmapped classes and
                                properties, asset warnings, approximations, skipped services, unresolved references
                                and renamed items) goes to the engine log in "<code>".eustress_engine/logs"</code>"
                                under your home folder. A model file lands in a Space of its own in the same way."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"convert-to-eustress is a storage tool"</strong>
                                    <p>
                                        "The engine also builds a "<code>"convert-to-eustress"</code>" utility. It
                                        copies the TOML folders of existing Spaces into their databases, the same step
                                        Studio runs when it opens a Space. Roblox files always come in through Import;
                                        the "<a href="/docs/universes">"Universes"</a>" page covers the database."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="roblox-classes" class="subsection">
                            <h3>"Classes and Services"</h3>
                            <p>
                                "Each Roblox service lands in the Space folder of the same name: Workspace, Lighting,
                                Players, StarterGui, StarterPack, ReplicatedStorage, ServerScriptService,
                                ServerStorage, SoundService, Chat, Teams and MaterialService. StarterPlayer's two
                                script folders become StarterPlayerScripts and StarterCharacterScripts, and
                                ReplicatedFirst goes to "<code>"ReplicatedStorage/_replicated_first"</code>".
                                Runtime-only services such as RunService and TweenService have nothing saved in a
                                place file and are skipped. Services with no Eustress counterpart, such as
                                MarketplaceService, are kept under "<code>"_imported/<Service>/"</code>" for you to
                                sort out."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Roblox"</th><th>"Eustress"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Part, MeshPart, WedgePart, CornerWedgePart, TrussPart"</td><td>"Part"</td></tr>
                                    <tr><td>"UnionOperation"</td><td>"UnionOperation, drawn from the union's stored mesh"</td></tr>
                                    <tr><td>"NegateOperation, IntersectOperation"</td><td>"Part, drawn the same way"</td></tr>
                                    <tr><td>"Model, Folder, SpawnLocation, Seat, VehicleSeat, Camera"</td><td>"The same classes"</td></tr>
                                    <tr><td>"Script, LocalScript, ModuleScript"</td><td>"LuauScript, LuauLocalScript, LuauModuleScript"</td></tr>
                                    <tr><td>"Lights, Sky, Atmosphere, Clouds, Terrain"</td><td>"The same classes"</td></tr>
                                    <tr><td>"Constraints and movers (WeldConstraint, HingeConstraint, Motor6D, AlignPosition, LinearVelocity and more)"</td><td>"The same classes"</td></tr>
                                    <tr><td>"GUI (ScreenGui, BillboardGui, SurfaceGui, Frame, TextLabel, ImageLabel and more)"</td><td>"The same classes"</td></tr>
                                    <tr><td>"Effects (ParticleEmitter, Beam, Sound, Fire, Smoke, Trail, post-processing effects)"</td><td>"The same classes"</td></tr>
                                    <tr><td>"RemoteEvent, RemoteFunction, BindableEvent, BindableFunction"</td><td>"The same classes"</td></tr>
                                    <tr><td>"SpecialMesh, BlockMesh, CylinderMesh"</td><td>"Folded into the parent part's mesh"</td></tr>
                                    <tr><td>"The ten Value classes"</td><td>"Attributes on the parent"</td></tr>
                                    <tr><td>"Classes with no counterpart"</td><td>"Skipped with their children, listed in the report"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Instance names are kept for display. When a Roblox name cannot be a folder name
                                (it contains "<code>":"</code>", "<code>"?"</code>" or another reserved character,
                                ends in a dot, is longer than 96 characters, or is a Windows device name such as "
                                <code>"CON"</code>"), only the folder name changes. References between instances,
                                such as a weld's two parts or an ObjectValue's target, are resolved to the target's
                                UUID; the ones that cannot be resolved are listed in the report."
                            </p>
                        </div>

                        <div id="roblox-parts" class="subsection">
                            <h3>"Parts, Shapes and Units"</h3>
                            <ul class="docs-list">
                                <li><strong>"CFrame"</strong>" becomes position and rotation. "<code>"Orientation"</code>" is used only when a part has no CFrame."</li>
                                <li><strong>"Shape"</strong>" picks the built-in mesh: Ball, Cylinder, Wedge and CornerWedge get their own; Block keeps the default. Cylinders are turned 90 degrees in their own frame, because Roblox runs a cylinder along X and Eustress's cylinder mesh runs along Y."</li>
                                <li><strong>"Color and BrickColor"</strong>" become the part color. A BrickColor's palette number and the original color are also kept in the part's metadata."</li>
                                <li><strong>"Transparency"</strong>" becomes the color's alpha; "<strong>"Material"</strong>" maps to the preset of the same name; "<strong>"Anchored"</strong>", "<strong>"CanCollide"</strong>", "<strong>"Reflectance"</strong>", "<strong>"CastShadow"</strong>" and "<strong>"Locked"</strong>" carry over."</li>
                                <li><strong>"Other properties"</strong>" are kept in "<code>"[properties.extras]"</code>" so nothing is thrown away, even when Eustress does not use them yet."</li>
                            </ul>
                            <p>
                                "Roblox lengths are written unchanged, and each instance's "<code>"[metadata]"</code>
                                " sets "<code>"unit"</code>" to "<code>"ft"</code>". Eustress reads one Roblox unit as
                                one foot and converts to meters when the Space loads, so a part 4 units long is 1.22 m."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"Wedge classes import as blocks"</strong>
                                    <p>
                                        "Only a Part whose Shape is Wedge or CornerWedge gets a wedge mesh. The older
                                        WedgePart and CornerWedgePart classes, and TrussPart, have no Shape property,
                                        so they arrive as block-shaped Parts."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="roblox-scripts" class="subsection">
                            <h3>"Scripts, Values and Joints"</h3>
                            <p>
                                "A script's source is written to "<code>"script.luau"</code>" beside its "
                                <code>"_instance.toml"</code>". The ten Value classes (NumberValue, IntValue,
                                BoolValue, StringValue, ObjectValue, Color3Value, Vector3Value, CFrameValue,
                                BrickColorValue, BinaryStringValue) become typed attributes on their parent, with a "
                                <code>"_2"</code>" suffix when two share a name. RayValue, IntConstrainedValue and
                                DoubleConstrainedValue are dropped and recorded in the report. Scripts are rewritten
                                to match:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Luau"</span>
                                </div>
                                <pre><code class="language-lua">{r#"-- Before import                      -- After import
local speed = car.Speed.Value         local speed = car:GetAttribute("Speed")
car.Speed.Value = 40                  car:SetAttribute("Speed", 40)
car.Speed.Changed:Connect(onChange)   car:GetAttributeChangedSignal("Speed"):Connect(onChange)"#}</code></pre>
                            </div>
                            <p>
                                "A pattern the rewrite cannot handle safely, such as a Value object stored in a local
                                variable or created at run time, is left as it was and listed as a script warning.
                                The "<a href="/docs/scripting">"Scripting"</a>" page covers the Luau runtime."
                            </p>
                            <p>
                                "Legacy surface joints hold assemblies together, so they are mapped rather than
                                dropped: ManualWeld, Snap and Glue become Weld; Rotate becomes HingeConstraint; RotateP
                                becomes Motor; RotateV becomes VelocityMotor."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // MESHES, UNIONS, TERRAIN
                    // =========================================================
                    <section id="roblox-assets" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Meshes, Unions, Terrain"
                        </h2>

                        <div id="assets-download" class="subsection">
                            <h3>"Asset Downloads"</h3>
                            <p>
                                "Roblox parts refer to meshes, images and sounds by asset id. During a Studio import
                                each id is fetched from "<code>"assetdelivery.roblox.com"</code>" and saved in the new
                                Space's "<code>"assets"</code>" folder:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Meshes"</strong>": Roblox "<code>".mesh"</code>" files, versions 1.00 to 7.00, are decoded (the full-detail level only) to "<code>"assets/meshes/rbx-<id>.glb"</code>", and the part's "<code>"[asset] mesh"</code>" points at it."</li>
                                <li><strong>"Images"</strong>": PNG, JPEG, WebP, GIF, BMP and DDS go to "<code>"assets/textures/rbx-<id>.<ext>"</code>"."</li>
                                <li><strong>"Sounds"</strong>": OGG, WAV and MP3 go to "<code>"assets/sounds/"</code>"."</li>
                                <li><strong>"Anything else"</strong>", such as a packaged model, keeps a placeholder path under "<code>"assets/_unresolved/"</code>" and a warning in the report."</li>
                            </ul>
                            <p>
                                "Downloads are cached in "<code>"<Universe>/assets/.rbx_cache/"</code>", so a second
                                import of the same place fetches nothing it already has. An id that failed is marked
                                there too and skipped next time. Four environment variables, read when the import
                                starts, change where assets come from:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Variable"</th><th>"Effect"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"EUSTRESS_ROBLOX_ASSET_DIR"</code></td><td>"A local folder of assets named by id, tried before the network"</td></tr>
                                    <tr><td><code>"EUSTRESS_ROBLOX_NO_NETWORK=1"</code></td><td>"No downloads; assets come only from the local folder, or keep placeholders"</td></tr>
                                    <tr><td><code>"EUSTRESS_ROBLOSECURITY"</code></td><td>"A Roblox session cookie sent with each request, for assets that need a signed-in account"</td></tr>
                                    <tr><td><code>"EUSTRESS_ROBLOX_RETRY_ERRORS=1"</code></td><td>"Retry ids the cache has marked as failed"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Without a session cookie, many assets are refused with HTTP 401. After 32 refusals in
                                a row the importer stops asking for the rest of that import and keeps placeholders.
                                Refusals are never cached as failures, so the same place imports its assets once a
                                cookie is set."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"The session cookie is a password"</strong>
                                    <p>
                                        "Whoever holds a session cookie can act as that account. Set the variable only
                                        in the terminal that launches Studio, and remove it when the import is done."
                                    </p>
                                </div>
                            </div>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"Mesh textures are not applied"</strong>
                                    <p>
                                        "A part's "<code>"[asset]"</code>" section has a mesh slot but no texture slot.
                                        A SpecialMesh texture is dropped and recorded in the report, and a MeshPart's
                                        texture is downloaded but not used, so textured meshes arrive in their part
                                        color."
                                    </p>
                                </div>
                            </div>
                            <p>
                                "Folded SpecialMesh, BlockMesh and CylinderMesh children set the parent's mesh: Brick
                                is a block, Cylinder a cylinder, Sphere a ball, Wedge a wedge, FileMesh the downloaded
                                mesh. Head becomes a ball and the remaining types a block, each recorded as an
                                approximation. Their Scale and Offset change only how the part is drawn, never its
                                collider; a negative Scale (a mirrored mesh) draws mirrored but gets no collider."
                            </p>
                        </div>

                        <div id="assets-unions" class="subsection">
                            <h3>"Unions"</h3>
                            <p>
                                "A Roblox union (UnionOperation, NegateOperation or IntersectOperation) stores the
                                finished result of its boolean operation as a triangle mesh inside the place file.
                                The importer reads that stored mesh, preferring the modern "<code>"MeshData2"</code>
                                " field over the legacy "<code>"MeshData"</code>", decodes it (format versions 2, 4
                                and 5) and writes it as "<code>"csg.glb"</code>" in the union's folder. The union's "
                                <code>"[properties.extras]"</code>" records "<code>"csg_op"</code>" (union, negate or
                                intersect), plus "<code>"csg_triangles"</code>" when the mesh was found."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"Unions without a stored mesh become boxes"</strong>
                                    <p>
                                        "When a union's stored mesh is missing, empty, or only an unbaked marker,
                                        Eustress does not rebuild the union from its parts. The union imports as a box
                                        the size of its bounds, and the report lists a CSG box fallback. Because the box
                                        fills the union's whole bounds, it can cover parts that sat inside them."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="assets-terrain" class="subsection">
                            <h3>"Terrain"</h3>
                            <p>
                                "Roblox terrain is a voxel grid. The importer decodes it into 32 x 32 x 32 chunks and
                                writes each one, compressed, to "
                                <code>"Workspace/Terrain/voxel_chunks/chunk_<cx>_<cy>_<cz>.bin"</code>", with the
                                terrain's material colors in "<code>"Workspace/Terrain/_instance.toml"</code>".
                                Solid material and fill are kept for every voxel; water is read but not stored yet.
                                When the Space opens, the chunks are copied into its database."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"Imported terrain is stored, not yet drawn"</strong>
                                    <p>
                                        "The voxel terrain loader runs only for Spaces whose database is marked as fully
                                        converted, a mark the current engine does not set, and it draws only the top
                                        surface of each column. Imported voxel terrain is saved with the Space but does
                                        not appear in the viewport yet. See "<a href="/docs/building">"Building"</a>
                                        " for the terrain you can sculpt today."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // 3D MODELS
                    // =========================================================
                    <section id="models" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "3D Models"
                        </h2>

                        <div id="models-gltf" class="subsection">
                            <h3>"glTF and GLB"</h3>
                            <p>
                                "glTF is the format Eustress renders natively. Copy a "<code>".glb"</code>" or "
                                <code>".gltf"</code>" file into the Space's "<code>"Workspace"</code>" (or "
                                <code>"Lighting"</code>") folder while the Space is open in Studio. The file watcher
                                picks it up and adds a Part named after the file that renders the file's first scene,
                                materials included. Saving over the file reloads it in place. Put the file directly in
                                Workspace or in a plain folder: a file inside a part's folder is treated as that part's
                                own asset and is not added."
                            </p>
                            <p>
                                "Eustress reads glTF through Bevy's loader. It cannot decode Draco-compressed meshes,
                                so Draco files do not load and the log names them; export without Draco compression.
                                Its image decoder in this build reads PNG, HDR and KTX2, so embed textures as PNG."
                            </p>
                        </div>

                        <div id="models-mesh" class="subsection">
                            <h3>"Custom Part Meshes"</h3>
                            <p>
                                "Any part can wear a GLB mesh instead of a primitive. Point its "<code>"[asset]"</code>
                                " section at the file, relative to the part's own folder. The mesh must live inside the
                                Space folder, because meshes load through the Space's asset source. This is how the
                                Roblox importer attaches downloaded meshes."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Workspace/Chair/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[asset]
mesh = "../../assets/meshes/chair.glb"   # relative to Workspace/Chair/
scene = "Scene0""#}</code></pre>
                            </div>
                            <ul class="docs-list">
                                <li><strong>"What renders"</strong>": the first primitive of the file's first mesh, with the file's first material."</li>
                                <li><strong>"Size"</strong>": computed from the mesh bounds once it loads, times the part's scale."</li>
                                <li><strong>"Collider"</strong>": a box of the part's size when CanCollide is on."</li>
                                <li><strong>"Draco"</strong>": a Draco-compressed mesh falls back to the block primitive."</li>
                            </ul>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"Name custom meshes carefully"</strong>
                                    <p>
                                        "Eustress recognizes its built-in primitives by file name. A mesh whose file name
                                        contains "<code>"block"</code>", "<code>"ball"</code>", "<code>"cylinder"</code>
                                        ", "<code>"wedge"</code>" or "<code>"cone"</code>" is treated as that primitive and
                                        your file is not loaded: "<code>"cone_tower.glb"</code>" renders as the built-in
                                        cone. Rename the file to avoid those words."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="models-other" class="subsection">
                            <h3>"Other Formats"</h3>
                            <p>
                                "Studio watches the open Space for "<code>".stl"</code>", "<code>".step"</code>", "
                                <code>".stp"</code>", "<code>".obj"</code>", "<code>".ply"</code>" and "
                                <code>".fbx"</code>" files, meant to be converted to GLB beside the source. The
                                converters are not written yet, so each file found logs a failed conversion and
                                nothing is added. Convert these formats to GLB in another tool first."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"USD"</strong>": the USD modules are empty placeholders, and no USD file is read."</li>
                                <li><strong>"The Model tab's Import and Export buttons"</strong>" are not wired yet; pressing them prints a planned-feature notice in the Output."</li>
                                <li><strong>"The Assets panel's Import"</strong>" copies the chosen files into "<code>"<Universe>/.eustress/assets/meshes/"</code>" and refreshes the panel. It creates no instances."</li>
                            </ul>
                            <p>
                                "Parametric parts made in Eustress export to GLB from the "<a href="/docs/cad">"CAD"</a>
                                " tools."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // GAUSSIAN SPLATS
                    // =========================================================
                    <section id="splats" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Gaussian Splats"
                        </h2>

                        <div id="splats-import" class="subsection">
                            <h3>"Importing a Splat"</h3>
                            <p>
                                "A Gaussian splat is a photographic 3D capture stored as millions of small colored
                                blobs. Eustress renders standard 3D Gaussian Splatting "<code>".ply"</code>" files
                                beside ordinary parts. Press "<strong>"Import"</strong>" on the Home tab and pick the "
                                <code>".ply"</code>":"
                            </p>
                            <ol class="numbered-list">
                                <li>"The file is copied to "<code>"<Universe>/assets/splats/"</code>"."</li>
                                <li>"A "<code>"GaussianSplats"</code>" instance named after the file is created in Workspace, 2 m above the origin."</li>
                                <li>"Its "<code>"[gaussian_splats]"</code>" section records the file's path, relative to the Universe folder."</li>
                            </ol>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Workspace/bicycle/_instance.toml (excerpt)"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[transform]
position = [0.0, 2.0, 0.0]
rotation = [1.0, 0.0, 0.0, 0.0]   # 180 degrees about X

[gaussian_splats]
path = "assets/splats/bicycle.ply"
cull_floaters = true              # both default to true when absent
ppisp = true"#}</code></pre>
                            </div>
                            <p>
                                "The rotation stands a typical capture upright: captures trained from photographs
                                usually store Y pointing down, and a half turn about X makes them Y-up like the rest
                                of Eustress. A capture made another way may need a turn with the Rotate tool; the
                                cloud file itself is never modified."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Why absolute paths are allowed"</strong>
                                    <p>
                                        "Imported splats and images live in the Universe's "<code>"assets"</code>"
                                        folder, above the Space folder, so the engine loads them by absolute path. Bevy
                                        refuses absolute asset paths by default, which made imports silently show
                                        nothing. Eustress allows them, because it only loads local files you picked."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="splats-load" class="subsection">
                            <h3>"What Happens on Load"</h3>
                            <ul class="docs-list">
                                <li><strong>"Floater removal"</strong>": before the cloud reaches the GPU, blobs that float alone in empty space are removed, judged by how much solid material surrounds them rather than by their opacity, so faint but dense detail such as foliage stays. Near-invisible blobs below 0.005 opacity go too. The environment variables "<code>"EUSTRESS_SPLAT_CULL_GRID"</code>", "<code>"EUSTRESS_SPLAT_CULL_MIN_MASS"</code>", "<code>"EUSTRESS_SPLAT_CULL_DUST"</code>", "<code>"EUSTRESS_SPLAT_CULL_MAX_REMOVE"</code>" and "<code>"EUSTRESS_SPLAT_CULL_VOXEL"</code>" tune it."</li>
                                <li><strong>"Collider"</strong>": the solid part of the cloud is fitted with boxes about 1/48 of its largest extent, between 5 cm and 2 m each, and becomes a static collider, so the capture is solid to physics."</li>
                                <li><strong>"Properties"</strong>": Appearance shows "<strong>"Path"</strong>" (read-only), "<strong>"CullFloaters"</strong>" and "<strong>"PPISP"</strong>". The toggles are saved to the "<code>"[gaussian_splats]"</code>" section."</li>
                                <li><strong>"Reopening"</strong>": when a Space opens, Eustress scans Workspace for splat instances and loads their clouds again."</li>
                            </ul>
                            <p>
                                "PPISP is a photometric correction for multi-camera captures. Its crate implements only
                                the exposure step so far, as a CPU reference, and the "<code>"ppisp"</code>" flag is
                                stored but does not change the cloud yet."
                            </p>
                        </div>

                        <div id="splats-mcp" class="subsection">
                            <h3>"Over MCP"</h3>
                            <p>
                                "Agents insert a working splat with the "<code>"insert_gaussian_splats"</code>" tool of
                                the "<a href="/learn/mcp">"MCP server"</a>". It takes a "<code>".ply"</code>" path
                                (absolute, or relative to the Universe), copies it into "<code>"assets/splats/"</code>
                                " unless it is already there, and writes the same instance the Import button writes.
                                Creating a "<code>"GaussianSplats"</code>" with "<code>"create_entity"</code>" gives an
                                empty instance, because that tool has no cloud path."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MCP"</span>
                                </div>
                                <pre><code class="language-json">{r#"{ "tool": "insert_gaussian_splats", "arguments": {
    "path": "C:/Captures/bicycle.ply",
    "name": "Bicycle",
    "position": [0, 2, 0],
    "cull_floaters": true } }"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // IMAGES AND VIDEO
                    // =========================================================
                    <section id="media" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Images and Video"
                        </h2>

                        <div id="media-chooser" class="subsection">
                            <h3>"The Radial Chooser"</h3>
                            <p>
                                "A picture or clip can become several different classes, so Eustress asks. Import an
                                image or video with the Home tab's "<strong>"Import"</strong>" button, or double-click
                                one that is already in the Assets panel. The file is copied to "
                                <code>"<Universe>/assets/images/"</code>" or "<code>"assets/videos/"</code>" and a
                                radial menu opens in the middle of the window:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Menu"</th><th>"Choice"</th><th>"Creates"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Apply Image As"</td><td>"Decal"</td><td>"A Decal on the face of a part you click next"</td></tr>
                                    <tr><td>"Apply Image As"</td><td>"Texture"</td><td>"A Texture tiled across the face of a part you click next"</td></tr>
                                    <tr><td>"Apply Image As"</td><td>"Image Label, Image Button"</td><td>"A GUI element in StarterGui, with the image in "<code>"[image] image"</code></td></tr>
                                    <tr><td>"Apply Video As"</td><td>"Video Frame"</td><td>"A GUI element in StarterGui, with the clip in "<code>"[video] video"</code></td></tr>
                                    <tr><td>"Apply Video As"</td><td>"3D Video"</td><td>"A video quad in Workspace, 6 x 3.375 m by default"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Click outside the menu or on its center to cancel. The copied file stays in the assets
                                folder either way, so you can apply it later from the Assets panel. The "
                                <a href="/docs/ui">"UI Systems"</a>" page covers the GUI classes."
                            </p>
                        </div>

                        <div id="media-surface" class="subsection">
                            <h3>"Decals and Textures"</h3>
                            <p>
                                "Choosing Decal or Texture starts surface placement. A preview follows the surface under
                                the cursor: green over a part you can use, red over a locked part or anything that is not
                                a part."
                            </p>
                            <ol class="numbered-list">
                                <li>"Move over the face that should carry the image."</li>
                                <li>"Click. The face is worked out from the surface you hit, and a Decal or Texture is added as a child of the part, in the part's folder, with "<code>"texture"</code>" and "<code>"face"</code>" set."</li>
                                <li>"Press "<code>"Esc"</code>" to cancel instead."</li>
                            </ol>
                            <p>
                                "The part must be saved to disk first; a part that exists only in memory is refused with
                                a message. A Texture repeats its image across the face and follows the part when it is
                                resized."
                            </p>
                        </div>

                        <div id="media-status" class="subsection">
                            <h3>"What Displays Today"</h3>
                            <p>
                                "The chooser and placement always create and save the instance. Whether the picture or
                                clip then shows depends on the class:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Class"</th><th>"Shows today"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"3D Video"</td><td>"Yes, for MP4 files with H.264 video, decoded in-process and looped. Other codecs play a moving test pattern."</td></tr>
                                    <tr><td>"Decal, Texture"</td><td>"Not yet. They load the stored path from the engine's own asset folder instead of the Universe folder, so the image is not found."</td></tr>
                                    <tr><td>"Image Label, Image Button"</td><td>"Not yet. The GUI loader resolves the path against the element's own folder, so the image is not found."</td></tr>
                                    <tr><td>"Video Frame"</td><td>"Not yet. Video in screen-space GUI needs a compositor that is not written."</td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"Only PNG decodes on 3D surfaces"</strong>
                                    <p>
                                        "The 3D renderer in this build decodes PNG (plus HDR and KTX2). JPEG, WebP, BMP,
                                        GIF and TGA files import and save, but cannot be drawn on 3D surfaces. Convert
                                        pictures to PNG before importing them."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // DATA AND HEIGHTMAPS
                    // =========================================================
                    <section id="data" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "Data and Heightmaps"
                        </h2>

                        <div id="data-datasets" class="subsection">
                            <h3>"Datasets"</h3>
                            <p>
                                "The Data tab's "<strong>"Import"</strong>" reads a table and creates a Dataset
                                instance. Pick a "<code>".csv"</code>", "<code>".json"</code>", "<code>".jsonl"</code>
                                " or "<code>".parquet"</code>" file:"
                            </p>
                            <ol class="numbered-list">
                                <li>"A folder named after the file is created in the selected folder, or in Workspace when nothing file-backed is selected."</li>
                                <li>"The file is copied into that folder, beside a Dataset "<code>"_instance.toml"</code>"."</li>
                                <li>"Its attributes record the source file name, the row count and each column's name and type."</li>
                                <li>"The Output confirms the import with the row and column counts."</li>
                            </ol>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"JSON means JSON Lines"</strong>
                                    <p>
                                        "Both "<code>".json"</code>" and "<code>".jsonl"</code>" files are read as JSON
                                        Lines: one JSON object per line. Write a JSON array out as one object per line
                                        before importing it. When a file cannot be read, the Output says so and nothing
                                        is created."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="data-heightmaps" class="subsection">
                            <h3>"Heightmaps"</h3>
                            <p>
                                "The Terrain menu's "<strong>"Import Heightmap..."</strong>" turns an elevation file
                                into terrain for the open Space: PNG heightmaps, raw 16-bit "<code>".r16"</code>" and "
                                <code>".raw"</code>" grids, SRTM "<code>".hgt"</code>" tiles, ASCII "<code>".asc"</code>
                                " grids and GeoTIFF "<code>".tif"</code>" files. The heights are written as terrain
                                chunks under "<code>"Workspace/Terrain"</code>", the same files Save writes, and the
                                terrain is rebuilt from them. "<a href="/docs/building">"Building"</a>" covers terrain
                                editing."
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

                        <div id="roadmap-roblox" class="subsection">
                            <h3>"Roblox Import"</h3>
                            <p>
                                "The import specification adds an options dialog before the import (choose the target
                                Space, and switch scripts, terrain, union meshes and asset downloads on or off) and a
                                report dialog after it, with one-click follow-ups to download missing assets and to
                                save the report as JSON. Roblox files will also be accepted when dropped on the
                                viewport. Unions without a stored mesh will be rebuilt from their parts with the CAD
                                kernel's booleans instead of arriving as boxes."
                            </p>
                        </div>

                        <div id="roadmap-formats" class="subsection">
                            <h3>"More Formats"</h3>
                            <p>
                                "The mesh watcher's converters are designed and will land one format at a time: STL and
                                PLY meshes written straight to GLB, OBJ through a parser, STEP through the truck STEP
                                reader and the CAD tessellator, and FBX through an external FBX2glTF converter. For
                                splats, the plan adds compact SOG and SPZ formats so large captures load as a fraction of
                                their PLY size, and uses PPISP to separate a capture's lighting from its surfaces, so
                                engine lights can relight it."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Bring in what you already have, then keep building."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/building" class="btn-secondary-steel">"Building Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/cad" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"CAD"</span>
                            </div>
                        </a>
                        <a href="/docs/ui" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"UI Systems"</span>
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
