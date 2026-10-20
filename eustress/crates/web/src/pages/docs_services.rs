// =============================================================================
// Eustress Web - Services Documentation Page
// =============================================================================
// Services: the fixed top-level containers of every Space, how each one is
// stored as a folder with a _service.toml, and what each does in the engine.
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
                TocSubsection { id: "overview-what", title: "What a Service Is" },
                TocSubsection { id: "overview-list", title: "Every Service" },
            ],
        },
        TocSection {
            id: "disk",
            title: "Services on Disk",
            subsections: vec![
                TocSubsection { id: "disk-folder", title: "Folder and Service File" },
                TocSubsection { id: "disk-values", title: "Property Values" },
                TocSubsection { id: "disk-panel", title: "The Properties Panel" },
                TocSubsection { id: "disk-custom", title: "Your Own Services" },
            ],
        },
        TocSection {
            id: "world",
            title: "Workspace & Lighting",
            subsections: vec![
                TocSubsection { id: "world-workspace", title: "Workspace" },
                TocSubsection { id: "world-lighting", title: "Lighting" },
                TocSubsection { id: "world-lighting-props", title: "Lighting Properties" },
            ],
        },
        TocSection {
            id: "content",
            title: "Content Services",
            subsections: vec![
                TocSubsection { id: "content-materials", title: "MaterialService" },
                TocSubsection { id: "content-sound", title: "SoundService" },
                TocSubsection { id: "content-data", title: "Data, Experiments, Adornments" },
                TocSubsection { id: "content-website", title: "Website" },
            ],
        },
        TocSection {
            id: "player",
            title: "Player & Script Services",
            subsections: vec![
                TocSubsection { id: "player-starter", title: "Players and Starters" },
                TocSubsection { id: "player-storage", title: "Storage Services" },
                TocSubsection { id: "player-scripts", title: "Script Services" },
            ],
        },
        TocSection {
            id: "scripts",
            title: "Reaching Services",
            subsections: vec![
                TocSubsection { id: "scripts-luau", title: "From Luau" },
                TocSubsection { id: "scripts-rune", title: "From Rune" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-properties", title: "Every Row Applied" },
                TocSubsection { id: "roadmap-runtime", title: "Live Services" },
            ],
        },
    ]
}

/// How a service loads and saves: the folder's _service.toml becomes a
/// service object, Properties lays its rows out from a schema that ships with
/// the engine, and an edit is written back to the same file.
#[component]
fn ServiceFlowDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 220" role="img"
                aria-label="A service folder's _service.toml loads into a service object. The Properties panel shows the service's values using a row layout that ships with the engine, and an edit is saved back to the same file.">
                <defs>
                    <marker id="svc-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                // Save path from the panel back to the file.
                <path d="M 545 84 L 545 46 L 95 46 L 95 82" class="dg-line dg-line-dashed" fill="none" marker-end="url(#svc-arrow)"></path>
                <text x="320" y="36" class="dg-note" text-anchor="middle">"edit: written back to the file, signed when you are signed in"</text>

                // The file in the Space.
                <rect x="20" y="84" width="150" height="56" rx="8" class="dg-box"></rect>
                <text x="95" y="108" class="dg-label" text-anchor="middle">"_service.toml"</text>
                <text x="95" y="126" class="dg-note" text-anchor="middle">"in the Space folder"</text>
                <line x1="170" y1="112" x2="243" y2="112" class="dg-line" marker-end="url(#svc-arrow)"></line>
                <text x="207" y="104" class="dg-note" text-anchor="middle">"load"</text>

                // The live service.
                <rect x="245" y="84" width="150" height="56" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="320" y="108" class="dg-label" text-anchor="middle">"Service"</text>
                <text x="320" y="126" class="dg-note" text-anchor="middle">"values from [service]"</text>
                <line x1="395" y1="112" x2="468" y2="112" class="dg-line" marker-end="url(#svc-arrow)"></line>

                // The panel and the schema that lays it out.
                <rect x="470" y="84" width="150" height="56" rx="8" class="dg-box"></rect>
                <text x="545" y="108" class="dg-label" text-anchor="middle">"Properties"</text>
                <text x="545" y="126" class="dg-note" text-anchor="middle">"current values"</text>
                <rect x="470" y="168" width="150" height="40" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="545" y="193" class="dg-label" text-anchor="middle">"Row layout"</text>
                <line x1="545" y1="168" x2="545" y2="142" class="dg-line" marker-end="url(#svc-arrow)"></line>
                <text x="458" y="193" class="dg-note" text-anchor="end">"ships with the engine"</text>
            </svg>
            <figcaption>
                "A service lives in its folder's _service.toml. Opening the Space turns the file into
                a service object, Properties shows its values in a layout that ships with the
                engine, and an edit is saved back into the same file."
            </figcaption>
        </figure>
    }
}

/// Services documentation page.
#[component]
pub fn DocsServicesPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-services"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/settings.svg" alt="Services" class="toc-icon" />
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

                <main class="docs-content">
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Services"</span>
                        </div>
                        <h1 class="docs-title">"Services"</h1>
                        <p class="docs-subtitle">
                            "A service is one of the fixed, top-level containers every Space has: Workspace
                            holds the 3D world, Lighting the sun and sky, SoulService the scripts. Each
                            service is a folder in the Space with a _service.toml file that stores its
                            settings."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "14 min read"
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

                        <div id="overview-what" class="subsection">
                            <h3>"What a Service Is"</h3>
                            <p>
                                "A service is a container that exists once in every Space. On disk it is a
                                folder at the top of the Space holding a "<code>"_service.toml"</code>" file;
                                in Studio it is a top-level row in the Explorer. Everything you make lives
                                inside one: parts in Workspace, materials in MaterialService, scripts in
                                SoulService."
                            </p>
                            <p>
                                "When a Space opens, Eustress treats every top-level folder that holds a "
                                <code>"_service.toml"</code>" as a service. Sixteen standard services load
                                even when their folder is missing: Workspace, Lighting, Players, StarterGui,
                                StarterPack, StarterPlayer, ReplicatedStorage, ServerStorage,
                                ServerScriptService, SoulService, MaterialService, SoundService,
                                AdornmentService, DataService, Teams and Chat."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Services stay put"</strong>
                                    <p>
                                        "Delete skips any service in the selection, and Cut refuses one with a
                                        warning. Removing a service would orphan everything inside it."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-list" class="subsection">
                            <h3>"Every Service"</h3>
                            <p>
                                "The Explorer lists the standard services in a fixed order, then any other
                                service folders by name. Some rows open to show their contents; the rest are
                                listed as a single row for now."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Service"</th><th>"Holds"</th><th>"In the Explorer"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Workspace"</td><td>"The 3D world: parts, models, meshes, the camera"</td><td>"Contents"</td></tr>
                                    <tr><td>"Lighting"</td><td>"Sun, Moon, Sky, Atmosphere and the scene's light settings"</td><td>"Contents"</td></tr>
                                    <tr><td>"Players"</td><td>"Runtime only; empty while you edit"</td><td>"Row only"</td></tr>
                                    <tr><td>"StarterGui"</td><td>"ScreenGuis drawn over the view"</td><td>"Contents"</td></tr>
                                    <tr><td>"StarterPack"</td><td>"Tools every player starts with"</td><td>"Row only"</td></tr>
                                    <tr><td>"StarterPlayer"</td><td>"Default character and camera settings"</td><td>"Row only"</td></tr>
                                    <tr><td>"ReplicatedStorage"</td><td>"Objects shared by server and clients"</td><td>"Row only"</td></tr>
                                    <tr><td>"ServerStorage"</td><td>"Objects only the server uses"</td><td>"Row only"</td></tr>
                                    <tr><td>"ServerScriptService"</td><td>"Server-side scripts"</td><td>"Row only"</td></tr>
                                    <tr><td>"SoulService"</td><td>"Scripts, Workshop sessions, AI request logs"</td><td>"Contents"</td></tr>
                                    <tr><td>"MaterialService"</td><td>"Materials ("<code>".mat.toml"</code>") and their textures"</td><td>"Contents"</td></tr>
                                    <tr><td>"AdornmentService"</td><td>"Beams, billboards, particles, highlights"</td><td>"Contents"</td></tr>
                                    <tr><td>"SoundService"</td><td>"Sounds and audio files"</td><td>"Row only"</td></tr>
                                    <tr><td>"Teams"</td><td>"Team objects"</td><td>"Row only"</td></tr>
                                    <tr><td>"Chat"</td><td>"Chat settings"</td><td>"Row only"</td></tr>
                                    <tr><td>"DataService"</td><td>"Datasets, series, columns, runs, connectors"</td><td>"Contents"</td></tr>
                                    <tr><td>"ExperimentService"</td><td>"Designs, their parts and laws, and every run"</td><td>"Contents"</td></tr>
                                    <tr><td>"Website"</td><td>"Values a website reads"</td><td>"Contents"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "A Space created in Studio also gets Website and ExperimentService, plus
                                StarterPlayerScripts and StarterCharacterScripts folders that the Explorer
                                does not list yet. A service shown as a single row still loads the objects in
                                its folder; the Explorer just does not show them under it."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Large Spaces open lean"</strong>
                                    <p>
                                        "In a Space with more than 100,000 stored instances, only Workspace,
                                        Lighting and StarterGui load their contents when the Space opens. The
                                        other services open empty, which keeps a large import responsive."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // SERVICES ON DISK
                    // =========================================================
                    <section id="disk" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Services on Disk"
                        </h2>

                        <div id="disk-folder" class="subsection">
                            <h3>"Folder and Service File"</h3>
                            <p>
                                "Each service is a folder named after it, with a "<code>"_service.toml"</code>
                                " inside. The file's "<code>"[service]"</code>" table names the class and holds
                                the service's values; "<code>"[metadata]"</code>" records an id and when the
                                file was created and last changed. This is a Lighting file after its time of
                                day was set in Studio:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Lighting/_service.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[service]
class_name = "Lighting"
icon = "lighting"
description = "Controls global lighting, shadows, and atmospheric effects"
can_have_children = true
clock_time = 18.5
time_of_day = "18:30:00"

[metadata]
id = "lighting-service"
created = "2026-09-22T09:14:03.512004+00:00"
last_modified = "2026-09-22T09:20:41.087311+00:00""#}</code></pre>
                            </div>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Key"</th><th>"Meaning"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"class_name"</code></td><td>"The service's class. Keep it identical to the folder name."</td></tr>
                                    <tr><td><code>"icon"</code></td><td>"The Explorer icon, named after a file in the engine's icon set. Defaults to the class name in lowercase."</td></tr>
                                    <tr><td><code>"description"</code></td><td>"A line of text shown in Properties."</td></tr>
                                    <tr><td><code>"can_have_children"</code></td><td>"Whether objects can go inside. Defaults to true."</td></tr>
                                    <tr><td>"Any other key"</td><td>"A property value (see below)."</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "When you are signed in, each save also appends your name, public key and the
                                time to "<code>"[[metadata.modifications]]"</code>", and the first signed save
                                sets "<code>"[metadata.created_by]"</code>", so the file carries its own edit
                                history."
                            </p>
                        </div>

                        <div id="disk-values" class="subsection">
                            <h3>"Property Values"</h3>
                            <p>
                                "Values sit directly under "<code>"[service]"</code>". The loader accepts six
                                shapes and skips anything else:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Shape"</th><th>"Example"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"True or false"</td><td><code>"enabled = true"</code></td></tr>
                                    <tr><td>"Whole number"</td><td><code>"max_connections = 100"</code></td></tr>
                                    <tr><td>"Decimal"</td><td><code>"timeout_seconds = 30.0"</code></td></tr>
                                    <tr><td>"Text"</td><td><code>"api_endpoint = 'https://api.example.com'"</code></td></tr>
                                    <tr><td>"3 numbers (a vector)"</td><td><code>"spawn_offset = [0.0, 5.0, 0.0]"</code></td></tr>
                                    <tr><td>"4 numbers (an RGBA color, 0 to 1)"</td><td><code>"highlight_color = [1.0, 0.5, 0.0, 1.0]"</code></td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Properties shows a four-number color on a 0 to 255 scale, with the alpha as a
                                decimal."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A [properties] table is skipped"</strong>
                                    <p>
                                        "The service templates a new Space starts from keep their starting
                                        values in a separate "<code>"[properties]"</code>" table. The loader reads
                                        plain values and skips tables, so that table never reaches the service,
                                        and the first save from Studio rewrites the file without it. Until a value is saved under "<code>"[service]"</code>", the
                                        engine runs that setting on its own built-in value, while Properties
                                        shows the row's listed default."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="disk-panel" class="subsection">
                            <h3>"The Properties Panel"</h3>
                            <p>
                                "Select a service's row in the Explorer to see it in Properties. Eight
                                services ship with a row layout: Workspace, Lighting, Chat, StarterPlayer,
                                SoulService, ServerStorage, ExperimentService and Website. The layout names
                                each row, its type, its description and a default, and the panel fills in the
                                service's current value wherever one is set. Other services list their class,
                                description and file path, and whatever values their file holds."
                            </p>
                            <ServiceFlowDiagram />
                            <p>
                                "An edit writes the new value into the service and saves the whole file back.
                                Studio keeps an edit only for a value the file already holds under "
                                <code>"[service]"</code>"; Lighting's ClockTime and TimeOfDay are the exception
                                and are always kept. Lighting's time, latitude, brightness, fog distances and
                                exposure also change the scene the moment you edit them, saved or not."
                            </p>
                        </div>

                        <div id="disk-custom" class="subsection">
                            <h3>"Your Own Services"</h3>
                            <p>
                                "Any top-level folder with a "<code>"_service.toml"</code>" becomes a service
                                when the Space opens, with no code. Studio lists it after the standard
                                services, shows the objects inside it, and lists its values in Properties,
                                where you can edit them."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Tuning/_service.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[service]
class_name = "Tuning"
icon = "folder"
description = "Constants the Space's scripts read"
can_have_children = true
enabled = true
max_connections = 100
timeout_seconds = 30.0
spawn_offset = [0.0, 5.0, 0.0]
highlight_color = [1.0, 0.5, 0.0, 1.0]

[metadata]
id = "tuning-service""#}</code></pre>
                            </div>
                            <p>
                                "Properties shows these as Enabled, HighlightColor, MaxConnections,
                                SpawnOffset and TimeoutSeconds, sorted by name, followed by the file's path."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A class_name that differs from the folder name"</strong>
                                    <p>
                                        "Studio finds the service behind an Explorer row by the row's name, which
                                        is the folder name. If "<code>"class_name"</code>" does not match it, the
                                        row still appears, but Properties shows none of the service's values."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // WORKSPACE & LIGHTING
                    // =========================================================
                    <section id="world" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Workspace & Lighting"
                        </h2>

                        <div id="world-workspace" class="subsection">
                            <h3>"Workspace"</h3>
                            <p>
                                "Workspace is the 3D world. Every part, model and mesh in the viewport lives in
                                it, stored as a folder with an "<code>"_instance.toml"</code>", as a "
                                <code>".part.toml"</code>", "<code>".model.toml"</code>", "
                                <code>".glb.toml"</code>" or "<code>".instance.toml"</code>" file, or as a "
                                <code>".glb"</code>" or "<code>".gltf"</code>" model. Billboard UI attached to
                                parts loads here too. The Explorer lists the Camera first, then everything
                                else by name."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Setting"</th><th>"Value"</th><th>"Effect"</th></tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td><code>"RenderDistance"</code></td>
                                        <td>"1,000 m unless the file saves "<code>"render_distance"</code></td>
                                        <td>"Parts farther than this from the camera are not drawn. The distance is measured to each part's bounding sphere, so a large baseplate stays visible while any of it is in range. Clamped to 1 to 1,000,000 m."</td>
                                    </tr>
                                    <tr>
                                        <td>"Gravity"</td>
                                        <td>"9.80665 m/s² downward"</td>
                                        <td>"Standard gravity, set when the engine starts and applied to physics. The Gravity row in the Workspace panel is a separate stored value that physics does not read."</td>
                                    </tr>
                                </tbody>
                            </table>
                            <p>
                                "When "<code>"render_distance"</code>" is saved in the file, editing
                                RenderDistance re-applies the distance to every part at once. The other
                                Workspace rows (FallenPartsDestroyHeight, GlobalWind, AmbientColor,
                                OutdoorAmbient, Brightness, the three ColorCorrection values, SignalBehavior,
                                TouchesUseCollisionGroups and AllowThirdPartySales) are stored in the file
                                and not applied yet. Gravity and bodies are covered in "
                                <a href="/docs/physics">"Physics"</a>"."
                            </p>
                        </div>

                        <div id="world-lighting" class="subsection">
                            <h3>"Lighting"</h3>
                            <p>
                                "Lighting holds the sun, sky and overall light of the Space. A Space created
                                in Studio starts with four objects in its Lighting folder, each an "
                                <code>".instance.toml"</code>" file: Sun (class Star), Moon, Sky and
                                Atmosphere. The engine gives the Sun and Moon directional lights, places them
                                from the time of day and latitude, and drives the physically based sky from
                                the Atmosphere. Placing lights of your own is covered in "
                                <a href="/docs/building">"Building"</a>"."
                            </p>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/star.svg" alt="Sun" />
                                    </div>
                                    <h4>"Sun"</h4>
                                    <p>"Class Star. The main directional light, with cascaded shadows."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/eye.svg" alt="Moon" />
                                    </div>
                                    <h4>"Moon"</h4>
                                    <p>"A dimmer directional light, placed from the same time of day and latitude."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/sparkles.svg" alt="Sky" />
                                    </div>
                                    <h4>"Sky"</h4>
                                    <p>"The sky object. Its file holds the sky mode, star count and sky colors."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/web.svg" alt="Atmosphere" />
                                    </div>
                                    <h4>"Atmosphere"</h4>
                                    <p>"Scattering settings for the physically based sky."</p>
                                </div>
                            </div>
                            <p>
                                "Stop in Play mode puts Lighting back exactly as it was when you pressed
                                Play."
                            </p>
                        </div>

                        <div id="world-lighting-props" class="subsection">
                            <h3>"Lighting Properties"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Property"</th><th>"File key"</th><th>"Effect"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"ClockTime, TimeOfDay"</td><td><code>"clock_time"</code></td><td>"Time of day in hours; moves the sun. ClockTime reads HH:MM:SS; TimeOfDay is a slider that runs from 06:00 to 05:59."</td></tr>
                                    <tr><td>"GeographicLatitude"</td><td><code>"geographic_latitude"</code></td><td>"Latitude of the sun's arc, in degrees."</td></tr>
                                    <tr><td>"Brightness"</td><td><code>"brightness"</code></td><td>"Scales sunlight and the sky fill together. 2 is neutral; 4 doubles the light."</td></tr>
                                    <tr><td>"OutdoorAmbient"</td><td><code>"outdoor_ambient"</code></td><td>"Color of the sky fill that lights shadowed surfaces."</td></tr>
                                    <tr><td>"Ambient"</td><td><code>"ambient"</code></td><td>"Fill color, used only when OutdoorAmbient is black."</td></tr>
                                    <tr><td>"EnvironmentDiffuseScale"</td><td><code>"environment_diffuse_scale"</code></td><td>"Multiplies the sky fill."</td></tr>
                                    <tr><td>"EnvironmentSpecularScale"</td><td><code>"environment_specular_scale"</code></td><td>"Multiplies the sky's environment map, which drives reflections."</td></tr>
                                    <tr><td>"ExposureCompensation"</td><td><code>"exposure_compensation"</code></td><td>"Camera exposure in stops; +1 is twice as bright."</td></tr>
                                    <tr><td>"FogStart, FogEnd, FogColor"</td><td><code>"fog_start"</code>", "<code>"fog_end"</code>", "<code>"fog_color"</code></td><td>"Linear distance fog on the viewport camera, in meters."</td></tr>
                                </tbody>
                            </table>
                            <div class="equation-card">
                                <div class="equation">"camera EV100 = 13 - ExposureCompensation"</div>
                                <div class="equation-label">"A lower EV100 is a brighter image. The result is clamped to -5 to 25."</div>
                            </div>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"What applies at once"</strong>
                                    <p>
                                        "ClockTime, TimeOfDay, GeographicLatitude, Brightness, FogStart, FogEnd
                                        and ExposureCompensation change the scene as you edit them. The others
                                        in the table apply from the value saved under "<code>"[service]"</code>",
                                        when the Space loads or that value changes. GlobalShadows,
                                        ShadowSoftness, ColorShift_Top, ColorShift_Bottom and Technology are
                                        stored but not applied yet."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // CONTENT SERVICES
                    // =========================================================
                    <section id="content" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Content Services"
                        </h2>

                        <div id="content-materials" class="subsection">
                            <h3>"MaterialService"</h3>
                            <p>
                                "MaterialService holds the material definitions parts use: one "
                                <code>".mat.toml"</code>" file per material in the Space's MaterialService
                                folder, loaded into a MaterialRegistry of named materials. A definition sets
                                PBR factors and optional texture maps (base color, normal,
                                metallic-roughness, occlusion, emissive, depth). Texture paths resolve against
                                the Space first, then the engine's bundled library, and every map is
                                mipmapped as it loads. The bundled library repeats every 4 m with no visible
                                seam."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MaterialService/CustomBrick.mat.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[material]
name = "CustomBrick"
preset = "Brick"

# Factors multiply the maps.
[pbr]
base_color = [1.0, 1.0, 1.0, 1.0]
roughness = 1.0
metallic = 0.0

# metallic_roughness is packed glTF-style:
# R = occlusion, G = roughness, B = metallic.
[textures]
base_color = "textures/brick_base_color.png"
normal = "textures/brick_normal.png"
metallic_roughness = "textures/brick_orm.png"
occlusion = "textures/brick_orm.png""#}</code></pre>
                            </div>
                            <p>
                                "A part uses a material through its Material property, by name. Materials and
                                textures in depth are covered in "<a href="/docs/building">"Building"</a>"."
                            </p>
                        </div>

                        <div id="content-sound" class="subsection">
                            <h3>"SoundService"</h3>
                            <p>
                                "SoundService is where audio belongs. The Insert menu's Audio group puts a new
                                Sound here when nothing is selected, and the Space loader recognizes audio
                                files ("<code>".ogg"</code>", "<code>".mp3"</code>", "<code>".wav"</code>",
                                "<code>".flac"</code>") in this folder only. Eustress Engine does not play
                                audio yet; "<a href="/docs/audio">"Audio"</a>" covers what is stored today and
                                what comes next."
                            </p>
                            <p>
                                "Its template lists AmbientReverb (NoReverb), DistanceFactor (3.33),
                                DopplerScale (1), RolloffScale (1), VolumeScale (0.5) and
                                RespectFilteringEnabled (false). No system reads these values yet."
                            </p>
                        </div>

                        <div id="content-data" class="subsection">
                            <h3>"Data, Experiments, Adornments"</h3>
                            <ul class="docs-list">
                                <li>
                                    <strong>"DataService"</strong>
                                    " holds Data Platform objects: Dataset, Series, Column, Run and Connector.
                                    Connect on the Data tab creates a REST Connector here, with its endpoint,
                                    poll interval in seconds and format to fill in through Properties. A new
                                    Connector starts switched off. Once enabled, a Connector whose source is
                                    SAM.gov or Grants.gov polls it on its interval, never more often than
                                    every 60 seconds; other source types are not polled yet."
                                </li>
                                <li>
                                    <strong>"ExperimentService"</strong>
                                    " is the place for designs, the parts they are made of, the laws wiring
                                    those parts, and every run over them. Its Properties rows (Designs, Runs,
                                    Envelopes, Conflicts, FreeParameters, Unfalsifiable) are read-only counts
                                    that nothing computes yet."
                                </li>
                                <li>
                                    <strong>"AdornmentService"</strong>
                                    " holds beams, billboards, particles and highlights. Instance files placed
                                    loose in its folder load as objects."
                                </li>
                            </ul>
                        </div>

                        <div id="content-website" class="subsection">
                            <h3>"Website"</h3>
                            <p>
                                "Website holds the values a website reads. Publishing bakes them into a single
                                manifest at "<code>"universes/{id}/website-manifest.json"</code>" for a site to
                                fetch. Its Properties panel carries the manifest's Namespace and
                                SchemaVersion and the rows for its access key. The whole workflow is in "
                                <a href="/docs/website">"Website Service"</a>"."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // PLAYER & SCRIPT SERVICES
                    // =========================================================
                    <section id="player" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Player & Script Services"
                        </h2>

                        <div id="player-starter" class="subsection">
                            <h3>"Players and Starters"</h3>
                            <ul class="docs-list">
                                <li>
                                    <strong>"StarterGui"</strong>
                                    " holds the Space's ScreenGuis. It loads as a full-window UI root layered
                                    above the 3D view; how and when its ScreenGuis draw is covered in "
                                    <a href="/docs/ui">"UI Systems"</a>"."
                                </li>
                                <li>
                                    <strong>"StarterPlayer"</strong>
                                    " lists default character and camera settings in Properties, such as
                                    CharacterWalkSpeed (16), CharacterJumpHeight (7.2), CameraMode (Classic)
                                    and CameraMaxZoomDistance (128). The Play character does not read them
                                    yet."
                                </li>
                                <li>
                                    <strong>"Players"</strong>
                                    " is a runtime-only service: it has no contents while you edit, and the
                                    Explorer shows it as a single row."
                                </li>
                                <li>
                                    <strong>"StarterPack"</strong>" and "<strong>"Teams"</strong>
                                    " are containers, for the tools every player starts with and for Team
                                    objects."
                                </li>
                                <li>
                                    <strong>"Chat"</strong>
                                    " lists BubbleChatEnabled, LoadDefaultChat and FilteringEnabled in
                                    Properties, all on. No system reads them yet."
                                </li>
                            </ul>
                        </div>

                        <div id="player-storage" class="subsection">
                            <h3>"Storage Services"</h3>
                            <p>
                                "ReplicatedStorage and ServerStorage hold folders and objects that are not
                                part of the 3D world. Their names follow the Roblox convention: shared with
                                every client, or kept on the server. Eustress has no network transport yet,
                                so nothing replicates; both behave as plain containers, and Luau scripts
                                ported from Roblox still find them through "<code>"game:GetService"</code>".
                                See "<a href="/docs/networking">"Networking"</a>"."
                            </p>
                        </div>

                        <div id="player-scripts" class="subsection">
                            <h3>"Script Services"</h3>
                            <p>
                                "SoulService is the home for scripts. The Insert menu's Scripting group puts
                                new scripts here when nothing is selected, the Workshop saves its sessions in "
                                <code>"SoulService/Workshop"</code>", and AI requests are logged in "
                                <code>"SoulService/Logs"</code>"."
                            </p>
                            <p>
                                "Script files ("<code>".rune"</code>", "<code>".soul"</code>", "
                                <code>".md"</code>", "<code>".lua"</code>", "<code>".luau"</code>") load as
                                script objects in any service folder, ServerScriptService included; "
                                <a href="/docs/scripting">"Scripting"</a>" covers how they run. The
                                SoulService panel's EnableAI, SandboxEnabled, AllowFileSystemAccess and
                                AllowNetworkAccess rows are not read by any system yet."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // REACHING SERVICES
                    // =========================================================
                    <section id="scripts" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Reaching Services"
                        </h2>

                        <div id="scripts-luau" class="subsection">
                            <h3>"From Luau"</h3>
                            <p>
                                "In Luau, "<code>"game:GetService(name)"</code>" returns the table registered
                                under that name: Players, ReplicatedStorage, ServerStorage,
                                ServerScriptService, StarterGui, StarterPlayer, StarterPack, Lighting or
                                CollectionService. Workspace is the "<code>"workspace"</code>" global. Any
                                other name raises "<em>"Service 'Name' not found"</em>"; RunService,
                                TweenService, UserInputService, HttpService, DataStoreService, SoundService,
                                MarketplaceService and SimulationService are globals instead."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Luau"</span>
                                </div>
                                <pre><code class="language-lua">{r#"local Players = game:GetService("Players")
local Lighting = game:GetService("Lighting")

print(Players.LocalPlayer.Name)  -- LocalPlayer
print(Lighting.ClockTime)        -- 14
print(workspace.Gravity)         -- 9.80665

-- SoundService is a global, not a member of game.
local ok = pcall(function()
    return game:GetService("SoundService")
end)
print(ok)                        -- false"#}</code></pre>
                            </div>
                            <p>
                                "These tables stand in for the services today. Lighting carries fixed starting
                                values (ClockTime 14, Brightness 2), and assigning to them does not change the
                                scene."
                            </p>
                        </div>

                        <div id="scripts-rune" class="subsection">
                            <h3>"From Rune"</h3>
                            <p>
                                "Rune has no service objects. A service's features are functions in the "
                                <code>"eustress"</code>" module, named after the service: "
                                <code>"tween_service_create"</code>", "<code>"datastore_service_get"</code>", "
                                <code>"run_service_is_studio"</code>", "<code>"collection_add_tag"</code>", "
                                <code>"workspace_raycast"</code>" and "<code>"workspace_get_gravity"</code>". A
                                script file imports each one by name:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::log_info;
use eustress::run_service_is_studio;

pub fn on_init() {
    if run_service_is_studio() {
        log_info("Running in Eustress Engine");
    }
}"#}</code></pre>
                            </div>
                            <p>
                                "The command bar adds "<code>"use eustress::*;"</code>" to a one-line snippet
                                for you; a script file declares its imports. Which of these functions act on
                                the live world is covered in "<a href="/docs/scripting">"Scripting"</a>"."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // WHAT'S NEXT
                    // =========================================================
                    <section id="roadmap" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "What's Next"
                        </h2>

                        <div id="roadmap-properties" class="subsection">
                            <h3>"Every Row Applied"</h3>
                            <p>
                                "The rows that are stored today will reach the systems they name: shadows and
                                color shift for Lighting, fallen-part cleanup and wind for Workspace,
                                character and camera defaults for StarterPlayer, and the audio settings on
                                SoundService. New Spaces will start with their values under "
                                <code>"[service]"</code>", so the value a row shows is the value that runs."
                            </p>
                        </div>

                        <div id="roadmap-runtime" class="subsection">
                            <h3>"Live Services"</h3>
                            <p>
                                "The Luau service tables will become live views of the Space's services, the
                                Explorer will open every service to show its contents, and Players will fill
                                with real players once networking lands."
                            </p>
                            <div class="future-cta">
                                <p><strong>"One folder per service. Everything else lives inside."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/scripting" class="btn-secondary-steel">"Scripting Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/scripting" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Scripting"</span>
                            </div>
                        </a>
                        <a href="/docs/physics" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Physics"</span>
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
