// =============================================================================
// Eustress Web - Universes Documentation Page
// =============================================================================
// Universes: how Universes and Spaces sit on disk, the WorldDb a Space runs
// from, git history, branching, the causal op-log and many engines at once.
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
                TocSubsection { id: "overview-terms", title: "Universe, Space, Instance" },
                TocSubsection { id: "overview-stores", title: "Files, Database, History" },
                TocSubsection { id: "overview-rehearse", title: "A Place to Rehearse" },
            ],
        },
        TocSection {
            id: "disk",
            title: "On Disk",
            subsections: vec![
                TocSubsection { id: "disk-workspace", title: "The Eustress Folder" },
                TocSubsection { id: "disk-universe", title: "Inside a Universe" },
                TocSubsection { id: "disk-space", title: "Inside a Space" },
                TocSubsection { id: "disk-instance", title: "An Instance File" },
            ],
        },
        TocSection {
            id: "worlddb",
            title: "The WorldDb",
            subsections: vec![
                TocSubsection { id: "worlddb-what", title: "One Database per Space" },
                TocSubsection { id: "worlddb-partitions", title: "Partitions" },
                TocSubsection { id: "worlddb-open", title: "Opening a Space" },
                TocSubsection { id: "worlddb-writes", title: "Where an Edit Lands" },
            ],
        },
        TocSection {
            id: "history",
            title: "History",
            subsections: vec![
                TocSubsection { id: "history-save", title: "Every Save Is a Commit" },
                TocSubsection { id: "history-coverage", title: "What Git Captures" },
                TocSubsection { id: "history-restore", title: "Going Back" },
            ],
        },
        TocSection {
            id: "branching",
            title: "Branching",
            subsections: vec![
                TocSubsection { id: "branching-git", title: "Git Branches" },
                TocSubsection { id: "branching-experiments", title: "Experiments" },
                TocSubsection { id: "branching-parallel", title: "Variants in Parallel" },
            ],
        },
        TocSection {
            id: "oplog",
            title: "The Op-Log",
            subsections: vec![
                TocSubsection { id: "oplog-records", title: "What It Records" },
                TocSubsection { id: "oplog-read", title: "Reading It" },
            ],
        },
        TocSection {
            id: "engines",
            title: "Engines & Tools",
            subsections: vec![
                TocSubsection { id: "engines-many", title: "Several Engines" },
                TocSubsection { id: "engines-inspect", title: "Inspect Without the Engine" },
                TocSubsection { id: "engines-repair", title: "Repair Tools" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-branches", title: "Database Branches" },
                TocSubsection { id: "roadmap-history", title: "Complete History" },
            ],
        },
    ]
}

/// The three places a Space keeps its world: the files in its folder, the
/// WorldDb the scene is built from, and the git history of the folder.
#[component]
fn StoresDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 300" role="img"
                aria-label="The Space folder's files are ingested into the world.fjalldb database, which holds the tree, entities and mutations partitions. Studio loads the scene from the database and writes edits back to it. Saves commit the Space folder to git history; the database folder is not committed.">
                <defs>
                    <marker id="stores-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                // Space folder (files)
                <rect x="20" y="40" width="160" height="110" rx="8" class="dg-box"></rect>
                <text x="100" y="78" class="dg-label" text-anchor="middle">"Space folder"</text>
                <text x="100" y="100" class="dg-note" text-anchor="middle">"TOML, scripts, services"</text>
                <text x="100" y="118" class="dg-note" text-anchor="middle">"edit in any editor"</text>

                // The WorldDb and three of its partitions
                <rect x="240" y="20" width="180" height="170" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="330" y="44" class="dg-label" text-anchor="middle">"world.fjalldb"</text>
                <rect x="256" y="58" width="148" height="34" rx="6" class="dg-box dg-box-muted"></rect>
                <text x="330" y="80" class="dg-note" text-anchor="middle">"tree: files by path"</text>
                <rect x="256" y="100" width="148" height="34" rx="6" class="dg-box dg-box-muted"></rect>
                <text x="330" y="122" class="dg-note" text-anchor="middle">"entities: binary parts"</text>
                <rect x="256" y="142" width="148" height="34" rx="6" class="dg-box dg-box-muted"></rect>
                <text x="330" y="164" class="dg-note" text-anchor="middle">"mutations: op-log"</text>
                <text x="330" y="210" class="dg-note" text-anchor="middle">"never committed to git"</text>

                // The scene Studio shows
                <rect x="480" y="40" width="140" height="110" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="550" y="78" class="dg-label" text-anchor="middle">"Scene"</text>
                <text x="550" y="100" class="dg-note" text-anchor="middle">"what Studio shows"</text>
                <text x="550" y="118" class="dg-note" text-anchor="middle">"and simulates"</text>

                // Git history of the folder
                <rect x="20" y="220" width="160" height="60" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="100" y="246" class="dg-label" text-anchor="middle">"git history"</text>
                <text x="100" y="266" class="dg-note" text-anchor="middle">"one commit per save"</text>

                // Flows
                <line x1="180" y1="75" x2="256" y2="75" class="dg-line" marker-end="url(#stores-arrow)"></line>
                <text x="218" y="66" class="dg-note" text-anchor="middle">"ingest"</text>
                <line x1="420" y1="80" x2="480" y2="80" class="dg-line-accent" marker-end="url(#stores-arrow)"></line>
                <text x="450" y="71" class="dg-note" text-anchor="middle">"load"</text>
                <line x1="480" y1="120" x2="420" y2="120" class="dg-line" marker-end="url(#stores-arrow)"></line>
                <text x="450" y="138" class="dg-note" text-anchor="middle">"edits"</text>
                <line x1="100" y1="150" x2="100" y2="220" class="dg-line" marker-end="url(#stores-arrow)"></line>
                <text x="108" y="190" class="dg-note" text-anchor="start">"Ctrl+S, autosave"</text>
            </svg>
            <figcaption>
                "Files are read into the database when a Space opens and whenever you save one
                in another editor. Studio builds the scene from the database and writes edits back
                to it (and, for some parts, to their files). Each save commits the folder to git;
                the database folder is excluded."
            </figcaption>
        </figure>
    }
}

/// Universes documentation page.
#[component]
pub fn DocsUniversesPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-universes"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/star.svg" alt="Universes" class="toc-icon" />
                        <h2>"Universes"</h2>
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
                            <span class="current">"Universes"</span>
                        </div>
                        <h1 class="docs-title">"Universes"</h1>
                        <p class="docs-subtitle">
                            "A Universe is a folder of Spaces that share assets, recordings and tools. A
                            Space is one world: a folder of readable files, the database Studio runs it
                            from, and a git history that grows with every save."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "15 min read"
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

                        <div id="overview-terms" class="subsection">
                            <h3>"Universe, Space, Instance"</h3>
                            <p>
                                "A Space is one world: a scene with its parts, scripts, lighting and data,
                                kept in one folder. A Universe is a folder of related Spaces plus the
                                assets, recordings and experiment results they share. Everything inside a
                                Space is an instance: a part, a folder, a model, a script, a light."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Level"</th><th>"What it is"</th><th>"Where it lives"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Eustress folder"</td><td>"Holds every Universe"</td><td><code>"Documents/Eustress/"</code></td></tr>
                                    <tr><td>"Universe"</td><td>"Related Spaces and what they share"</td><td><code>"Universe1/"</code>", a folder with a "<code>"Spaces/"</code>" folder"</td></tr>
                                    <tr><td>"Space"</td><td>"One world, its database and its history"</td><td><code>"Universe1/Spaces/Space1/"</code></td></tr>
                                    <tr><td>"Instance"</td><td>"One object in the world"</td><td>"A folder with an "<code>"_instance.toml"</code>", or a record in the database"</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="overview-stores" class="subsection">
                            <h3>"Files, Database, History"</h3>
                            <p>"A Space keeps its world in three places, and each has one job:"</p>
                            <ul class="docs-list">
                                <li><strong>"Files"</strong>" are the readable form: service settings, scripts and "<code>"_instance.toml"</code>" documents you can open in any editor."</li>
                                <li><strong>"The WorldDb"</strong>" ("<code>"world.fjalldb/"</code>") is the form Studio runs from. Files are copied into it, and the scene is built from it."</li>
                                <li><strong>"Git history"</strong>" is a commit of the Space folder each time you save."</li>
                            </ul>
                            <StoresDiagram />
                            <p>
                                "The three are not copies of one another. Some edits exist only in the
                                database, and the database folder is never committed, so it is worth
                                knowing where each change goes. The rest of this page traces it."
                            </p>
                        </div>

                        <div id="overview-rehearse" class="subsection">
                            <h3>"A Place to Rehearse"</h3>
                            <p>
                                "The idea behind Universes is that a world should be as safe to change as a
                                codebase: branch it, try the change, measure the result, then keep it or
                                throw it away. Today that loop runs on git branches of a Space's files,
                                experiment runs that save their results, and extra engines that run
                                variants side by side. Branching the database itself exists in the storage
                                library and is not yet wired into Studio; "<a href="#roadmap">"What's Next"</a>
                                " covers it."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // ON DISK
                    // =========================================================
                    <section id="disk" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "On Disk"
                        </h2>

                        <div id="disk-workspace" class="subsection">
                            <h3>"The Eustress Folder"</h3>
                            <p>
                                "Every Universe lives in one Eustress folder, "<code>"Documents/Eustress"</code>
                                ". On Windows that is the local "<code>"%USERPROFILE%\\Documents"</code>" folder,
                                not a OneDrive-redirected copy, because OneDrive rewrites file metadata and
                                fights the file watcher. Set "<code>"EUSTRESS_WORKSPACE"</code>" to use another
                                folder. When the folder is empty, Studio creates "<code>"Universe1"</code>" with
                                one Space, "<code>"Space1"</code>"."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Eustress folder"</span>
                                </div>
                                <pre><code class="language-text">{r#"Documents/Eustress/
  .eustress/
    engine.port                     port of the last engine to start
    instances/<pid>.json            one record per running engine
  Universe1/                        a Universe
    .eustress/                      shared by the Universe's Spaces
    Spaces/
      Space1/                       a Space
      Space2/
  Universe2/"#}</code></pre>
                            </div>
                            <p>
                                "Studio remembers the last Space you opened ("<code>"last_space_path"</code>" in "
                                <code>"~/.eustress_engine/settings.json"</code>") and opens it again on the next
                                launch."
                            </p>
                        </div>

                        <div id="disk-universe" class="subsection">
                            <h3>"Inside a Universe"</h3>
                            <p>
                                "A Universe is any folder in the Eustress folder that holds a "
                                <code>"Spaces/"</code>" folder. Create one with "<strong>"File > New Universe..."</strong>
                                " ("<code>"Ctrl+Shift+N"</code>"). Its "<code>".eustress/"</code>" folder holds what
                                its Spaces share, so every Space in it sees the same default meshes, recordings
                                and experiment results:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Universe1/.eustress/"</span>
                                </div>
                                <pre><code class="language-text">{r#"engine.port                       bridge port of the engine working in this Universe
lsp.port                          Rune language server port
assets/parts/                     default part meshes: block.glb, ball.glb, ...
assets/meshes/                    shared meshes
knowledge/recordings/<Space>/     simulation recordings, one JSON file per run
experiments/                      results saved by run_experiment
telemetry.jsonl                   live watchpoint values, one line per second
runtime-snapshot.json             play state and sim values, 4 times a second
sim-commands.jsonl                commands queued by the MCP sim tools"#}</code></pre>
                            </div>
                        </div>

                        <div id="disk-space" class="subsection">
                            <h3>"Inside a Space"</h3>
                            <p>
                                "A Space is a folder under "<code>"Spaces/"</code>". "<strong>"File > New Space"</strong>
                                " ("<code>"Ctrl+N"</code>") creates one with 19 service folders, a Baseplate and a
                                Welcome Cube:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Space1/"</span>
                                </div>
                                <pre><code class="language-text">{r#"Workspace/                        parts, models and folders
  _service.toml
  Baseplate/_instance.toml
  WelcomeCube/_instance.toml
Lighting/                         Sun, Moon, Sky, Atmosphere
SoulService/  MaterialService/  DataService/  ...   19 services in all
src/
space.toml                        name, author, version
simulation.toml                   simulation clock settings
header.bin                        world id, engine and schema versions
world.fjalldb/                    the WorldDb, created on first open
.git/                             history, created on first save
.gitignore
.eustress/
  project.toml  settings.toml  sync.toml  publish.toml  ...
  view.toml                       the 2D or 3D view the Space opens in
  local/                          user-local state, never committed
  trash/                          deleted instances, recoverable
  exports/instances/              database contents exported as TOML
  last_reconcile                  when files were last read into the database"#}</code></pre>
                            </div>
                            <p>
                                "Deleting a file-backed instance in the Explorer moves its folder into "
                                <code>".eustress/trash/"</code>" instead of erasing it. "
                                <a href="/docs/perspective">"Perspective"</a>" explains "<code>"view.toml"</code>
                                ", and "<a href="/docs/simulation">"Simulation"</a>" explains the clock settings
                                and recordings."
                            </p>
                        </div>

                        <div id="disk-instance" class="subsection">
                            <h3>"An Instance File"</h3>
                            <p>
                                "On disk, an instance is a folder named after it that holds an "
                                <code>"_instance.toml"</code>". This is the Welcome Cube every new Space starts
                                with, a 4 m cube resting on the Baseplate:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Workspace/WelcomeCube/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[metadata]
class_name = "Part"
archivable = true
created = "2026-09-22T17:04:11.482019300+00:00"
last_modified = "2026-09-22T17:04:11.482019300+00:00"

[asset]
mesh = "parts/block.glb"
scene = "Scene0"

[transform]
position = [0.0, 2.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [4.0, 4.0, 4.0]

[properties]
color = [0.388, 0.706, 1.0, 1.0]
transparency = 0.0
reflectance = 0.2
anchored = true
can_collide = true
locked = false"#}</code></pre>
                            </div>
                            <p>
                                "Lengths are meters. A file can name a different authoring unit with "
                                <code>"unit"</code>" under "<code>"[metadata]"</code>" (for example "<code>"cm"</code>
                                " or "<code>"ft"</code>"), and the loader converts its position and scale to
                                meters once, as it loads."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // THE WORLDDB
                    // =========================================================
                    <section id="worlddb" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "The WorldDb"
                        </h2>

                        <div id="worlddb-what" class="subsection">
                            <h3>"One Database per Space"</h3>
                            <p>
                                "Each Space has one database, "<code>"world.fjalldb/"</code>", opened when the
                                Space opens. It is a Fjall store: a log-structured merge tree that appends
                                writes to a journal and compacts them in the background, so a Space can grow
                                past the point where one file per part stays practical. Eustress builds Fjall from
                                its own copy in the repository, wrapped by the "<code>"eustress-fjall"</code>" crate,
                                and everything reaches it through the "<code>"WorldDb"</code>" trait in "
                                <code>"eustress-worlddb"</code>"."
                            </p>
                            <p>
                                "Beside it, "<code>"header.bin"</code>" identifies the world. It starts with the
                                bytes "<code>"EUSWORLD"</code>" and records a world id, the version of the engine
                                that last wrote it and the schema version of the data inside."
                            </p>
                            <div class="callout callout-warning">
                                <img src="/assets/icons/shield.svg" alt="Warning" />
                                <div>
                                    <strong>"world.fjalldb is not a cache"</strong>
                                    <p>
                                        "Some edits exist only in the database, and "<code>"world.fjalldb/"</code>
                                        " is excluded from git. Back it up with the rest of the Space, and never
                                        delete it to force a reload: Studio would rebuild it from the files and
                                        every edit that lived only in the database would be gone. The "
                                        <code>".gitignore"</code>" Studio writes says the same thing."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="worlddb-partitions" class="subsection">
                            <h3>"Partitions"</h3>
                            <p>"Inside, the data is split into partitions, independent key spaces that compact separately:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Partition"</th><th>"What it holds"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"tree"</code></td><td>"Every file of the Space, keyed by its path relative to the Space folder, plus "<code>"#bin"</code>" records for simple instances"</td></tr>
                                    <tr><td><code>"entities"</code></td><td>"Binary instance cores, keyed by the spatial cell they sit in"</td></tr>
                                    <tr><td><code>"entities_uuid"</code></td><td>"The same cores, keyed by instance UUID"</td></tr>
                                    <tr><td><code>"path_to_uuid"</code>", "<code>"uuid_to_path"</code></td><td>"Lookups between file paths and UUIDs"</td></tr>
                                    <tr><td><code>"class_index"</code></td><td>"Which instances belong to each class"</td></tr>
                                    <tr><td><code>"mutations"</code></td><td>"The causal op-log"</td></tr>
                                    <tr><td><code>"datasets"</code>", "<code>"timeseries"</code></td><td>"Data Platform datasets and recorded series"</td></tr>
                                    <tr><td><code>"voxels"</code></td><td>"Voxel terrain chunks"</td></tr>
                                    <tr><td><code>"datastore"</code>", "<code>"datastore_ord"</code></td><td>"Laid out for DataStore values; scripts do not write here yet"</td></tr>
                                    <tr><td><code>"meta"</code></td><td>"The commit counter, the op-log sequence and the schema version"</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="worlddb-open" class="subsection">
                            <h3>"Opening a Space"</h3>
                            <p>"Opening a Space runs the same steps every time:"</p>
                            <ol class="numbered-list">
                                <li>"Studio opens "<code>"world.fjalldb/"</code>" and writes "<code>"header.bin"</code>" if the Space has none."</li>
                                <li>"On the first open the database is empty, so Studio copies every file of the Space into "<code>"tree"</code>", keyed by relative path. It skips "<code>".eustress/"</code>", "<code>".git/"</code>", "<code>"world.fjalldb/"</code>" and other dot folders, and leaves the files where they are."</li>
                                <li>"On every later open it reconciles: each "<code>".toml"</code>", "<code>".rune"</code>", "<code>".luau"</code>", "<code>".soul"</code>" or "<code>".md"</code>" file changed since the time stored in "<code>".eustress/last_reconcile"</code>" is copied in, and a "<code>".toml"</code>" deleted from disk is removed from "<code>"tree"</code>" as long as its folder still exists. This runs on a background thread while the scene loads, and the changes are applied to the scene as they arrive."</li>
                                <li>"The scene is built from the database, not from the files: services, folders and instances from "<code>"tree"</code>", binary parts from "<code>"entities"</code>". A Space with more than 100,000 binary parts streams them around the camera instead of loading them all."</li>
                                <li>"While the Space is open, the file watcher copies every file you save in another editor into "<code>"tree"</code>" and applies it to the live scene."</li>
                            </ol>
                            <p>
                                "If the database cannot be opened at all, Studio reads the files directly, so a
                                Space always opens. "<a href="/docs/building">"Building"</a>" covers streaming
                                large Spaces."
                            </p>
                            <div class="callout callout-tip">
                                <img src="/assets/icons/sparkles.svg" alt="Tip" />
                                <div>
                                    <strong>"Very large Spaces"</strong>
                                    <p>
                                        "The reconcile checks every file in the Space folder. On a Space with
                                        hundreds of thousands of files, set "<code>"EUSTRESS_SKIP_DISK_SCANS=1"</code>
                                        " to skip it. Files edited while Studio was closed are then ignored until
                                        you clear the variable and reopen the Space."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="worlddb-writes" class="subsection">
                            <h3>"Where an Edit Lands"</h3>
                            <p>"Where a change is stored depends on how you made it:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Change"</th><th>"Stored as"</th><th>"In git"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"A part from "<strong>"Toolbox > Primitives"</strong>" with no Folder or Model selected, in Edit mode"</td><td>"A binary core in "<code>"entities"</code>", with no file"</td><td>"No"</td></tr>
                                    <tr><td>"A part from the "<strong>"Insert"</strong>" menu or "<strong>"Insert Object..."</strong>", or from "<strong>"Primitives"</strong>" with a Folder or Model selected"</td><td>"A new folder with an "<code>"_instance.toml"</code></td><td>"Yes"</td></tr>
                                    <tr><td>"A Part an agent creates over MCP while the engine runs"</td><td>"A binary core in "<code>"entities"</code>", with no file"</td><td>"No"</td></tr>
                                    <tr><td>"Save a simple part: no children, a built-in shape"</td><td>"A "<code>"#bin"</code>" record in "<code>"tree"</code>"; the file keeps its earlier values"</td><td>"No"</td></tr>
                                    <tr><td>"Save a part that has children or a custom mesh"</td><td>"Its "<code>"_instance.toml"</code>", rewritten on disk"</td><td>"Yes"</td></tr>
                                    <tr><td>"Edit a file in another editor"</td><td>"The file, copied into "<code>"tree"</code>" by the watcher or the next open"</td><td>"Yes"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "A binary core is a compact record of one part that sits at the top level of the
                                Workspace. Its path, such as "<code>"Workspace/__bin_Part_<id>/_instance.toml"</code>
                                ", exists only in the database. Parts with children, custom meshes, scripts and GUI
                                elements always keep a real folder, because a single record cannot hold their
                                children or a relative mesh path."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // HISTORY
                    // =========================================================
                    <section id="history" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "History"
                        </h2>

                        <div id="history-save" class="subsection">
                            <h3>"Every Save Is a Commit"</h3>
                            <p>
                                <code>"Ctrl+S"</code>" ("<strong>"File > Save Space"</strong>") writes the scene,
                                then runs "<code>"git add -A"</code>" and commits in the Space folder with the
                                message "<code>"manual save <timestamp>"</code>". Git runs on a background thread,
                                so the editor never waits on it, and the first save runs "<code>"git init"</code>
                                ". When nothing changed since the last commit, no commit is made. The File menu
                                shows the time of the last snapshot."
                            </p>
                            <p>
                                "Autosave does the same on a timer, every 300 seconds by default, with the
                                message "<code>"autosave <timestamp>"</code>". A successful autosave shows the
                                toast "<em>"Auto-saved (git)"</em>"; a failed one shows an error instead of
                                failing quietly. Commits carry your Eustress user name when you are signed in.
                                Otherwise they use the repository's own identity, which Studio sets to "
                                <em>"Eustress Engine Autosave"</em>" when it creates the repository."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"~/.eustress_engine/settings.json"</span>
                                </div>
                                <pre><code class="language-json">{r#"{
  "auto_save_enabled": true,
  "auto_save_interval": 300.0
}"#}</code></pre>
                            </div>
                            <p>
                                "The interval is in seconds. Change these two keys, with Studio closed, to turn
                                autosave off or change its pace."
                            </p>
                        </div>

                        <div id="history-coverage" class="subsection">
                            <h3>"What Git Captures"</h3>
                            <p>
                                "A commit holds the files in the Space folder, which is not the whole Space.
                                Autosave adds "<code>"world.fjalldb/"</code>", "<code>".eustress/trash/"</code>" and "
                                <code>"*.bak-*"</code>" to the Space's "<code>".gitignore"</code>", and a new Space
                                already ignores "<code>".eustress/local/"</code>"."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Part of the Space"</th><th>"In git"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Services, scripts and GUI definitions"</td><td>"Yes, as files"</td></tr>
                                    <tr><td>"Parts from the Insert menu or Insert Object, and parts with children or custom meshes"</td><td>"Yes, as files"</td></tr>
                                    <tr><td>"Binary cores: parts from Toolbox > Primitives at the top level, and Parts agents create over MCP"</td><td>"No, database only"</td></tr>
                                    <tr><td>"Saved changes to simple parts ("<code>"#bin"</code>" records)"</td><td>"No, database only"</td></tr>
                                    <tr><td><code>"world.fjalldb/"</code></td><td>"Never"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "To put binary parts into history, export them first. The MCP tool "
                                <code>"export_instances_toml"</code>" writes every binary core as an ordinary "
                                <code>"_instance.toml"</code>" under "<code>".eustress/exports/instances/"</code>
                                ", and the next save commits that folder. The loader and the file watcher both
                                skip "<code>".eustress/"</code>", so an export is never loaded back as a second
                                copy of the scene."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MCP"</span>
                                </div>
                                <pre><code class="language-json">{r#"{ "tool": "export_instances_toml", "arguments": { "layout": "single_file" } }

// layout "folders" (default): one loadable <Name>_<id>/_instance.toml per core
// layout "single_file": one instances.toml, easier to read and diff
// limit: 5000 by default, 200000 at most; class and region narrow the export"#}</code></pre>
                            </div>
                        </div>

                        <div id="history-restore" class="subsection">
                            <h3>"Going Back"</h3>
                            <p>
                                "History is plain git, so any git client reads it. To bring back an earlier
                                version of a file, check it out of an older commit:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Bash"</span>
                                </div>
                                <pre><code class="language-bash">{r#"cd ~/Documents/Eustress/Universe1/Spaces/Space1
git log --oneline
git checkout <commit> -- Workspace/WelcomeCube/_instance.toml"#}</code></pre>
                            </div>
                            <p>
                                "Studio treats the restored file like any other edit made outside the editor:
                                the file watcher applies it while the Space is open, and the reconcile reads it
                                in on the next open. A checkout restores files only. Binary cores and "
                                <code>"#bin"</code>" records are in no commit, so they keep their current state."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // BRANCHING
                    // =========================================================
                    <section id="branching" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Branching"
                        </h2>

                        <div id="branching-git" class="subsection">
                            <h3>"Git Branches"</h3>
                            <p>
                                "A branch of a Space is a git branch of its folder. You can use any git client, or
                                let an agent use the git tools of the "<a href="/learn/mcp">"MCP server"</a>"
                                and of Studio's Workshop panel. They run in the Space's own repository, the
                                nearest "<code>".git"</code>" at or above the Space folder."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"git_branch"</code></td><td><code>"list"</code>", "<code>"create"</code>", "<code>"switch"</code>", "<code>"delete"</code>" or "<code>"merge"</code>" (merges with "<code>"--no-ff"</code>")"</td></tr>
                                    <tr><td><code>"git_commit"</code></td><td>"Stages all changes, or the files you list, and commits"</td></tr>
                                    <tr><td><code>"git_status"</code>", "<code>"git_diff"</code>", "<code>"git_log"</code></td><td>"Read the working tree and recent history"</td></tr>
                                    <tr><td><code>"feedback_diff"</code></td><td>"Diffs two branches, commits or tags, optionally as a summary"</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="branching-experiments" class="subsection">
                            <h3>"Experiments"</h3>
                            <p>
                                <code>"run_experiment"</code>" turns a what-if into a recorded run. It sets the
                                simulation values you give it, runs the simulation for "<code>"duration_s"</code>
                                " simulated seconds, waits for the run to finish (300 s of wall time at most, by
                                default), summarizes the telemetry and saves the result as JSON in the
                                Universe's "<code>".eustress/experiments/"</code>" folder. With "
                                <code>"create_branch"</code>" it first creates and switches to a git branch named "
                                <code>"exp/<name>-<timestamp>"</code>"."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MCP"</span>
                                </div>
                                <pre><code class="language-json">{r#"{
  "tool": "run_experiment",
  "arguments": {
    "name": "high_voltage_4v3",
    "sim_values": { "cell_voltage": 4.3 },
    "duration_s": 60,
    "time_scale": 100,
    "create_branch": true
  }
}"#}</code></pre>
                            </div>
                            <p>
                                <code>"list_experiments"</code>" lists saved results, newest first, and "
                                <code>"compare_runs"</code>" shows the change in every metric between two of them
                                ("<code>"latest"</code>" and "<code>"latest-1"</code>" work as names). Merge the
                                winning branch with "<code>"git_branch"</code>" and delete the rest. "
                                <a href="/docs/simulation">"Simulation"</a>" covers the clock, time compression
                                and watchpoints these runs use."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A git branch does not branch the database"</strong>
                                    <p>
                                        "Switching branches changes the files. State that lives only in "
                                        <code>"world.fjalldb/"</code>" stays as it is on every branch. The "
                                        <code>"sim_values"</code>" are applied to the running simulation and are
                                        not files either, so "<code>"git diff"</code>" does not show them; the
                                        experiment JSON records them under "<code>"config"</code>". Put a design
                                        change you want to compare in a file, and it travels with the branch."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="branching-parallel" class="subsection">
                            <h3>"Variants in Parallel"</h3>
                            <p>
                                "To run variants at the same time, give each one its own Space and its own
                                engine. "<code>"eustress open <space> --headless"</code>" starts a windowless
                                engine on a Space and prints its process id and bridge port; run each variant's
                                experiment against that engine, then compare the saved results. "
                                <a href="/learn/cli">"CLI & Headless"</a>" covers the commands, and the next
                                sections cover how the engines find each other."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // THE OP-LOG
                    // =========================================================
                    <section id="oplog" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "The Op-Log"
                        </h2>

                        <div id="oplog-records" class="subsection">
                            <h3>"What It Records"</h3>
                            <p>
                                "Git records states; the causal op-log records the events between them. It lives
                                in the "<code>"mutations"</code>" partition as an append-only list, one record per
                                create or delete, numbered by a sequence the database assigns and that survives
                                restarts. Every record carries:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Field"</th><th>"Meaning"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"seq"</code></td><td>"Position in the log"</td></tr>
                                    <tr><td><code>"tx_id"</code></td><td>"Transaction the change belongs to"</td></tr>
                                    <tr><td><code>"ts_nanos"</code></td><td>"Wall-clock time in nanoseconds"</td></tr>
                                    <tr><td><code>"actor"</code></td><td>"Who caused it: "<code>"User"</code>", "<code>"Script:<name>"</code>", "<code>"Mcp:<tool>"</code>", "<code>"Importer"</code>", "<code>"FileWatcher"</code>" or "<code>"System"</code></td></tr>
                                    <tr><td><code>"op"</code></td><td><code>"Create"</code>", "<code>"Update"</code>" or "<code>"Delete"</code></td></tr>
                                    <tr><td><code>"class"</code>", "<code>"uuid"</code>", "<code>"rel_path"</code></td><td>"Which instance, and its path in the Space"</td></tr>
                                    <tr><td><code>"has_before"</code>", "<code>"has_after"</code></td><td>"Whether the record holds the instance before and after the change"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Today the log captures binary cores: created from Toolbox > Primitives or through
                                the bridge, deleted in the editor, through the bridge or by undo, and recorded as "
                                <code>"System"</code>". It also captures new "<code>"_instance.toml"</code>" files that
                                appear while the Space is open, recorded as "<code>"FileWatcher"</code>". Property
                                edits, moves and new GUI elements are not
                                recorded yet, and no record carries a before-image, so the log can say what was
                                created and deleted but cannot yet rewind it."
                            </p>
                        </div>

                        <div id="oplog-read" class="subsection">
                            <h3>"Reading It"</h3>
                            <p>
                                "Read the tail of the log from a running engine with the "
                                <code>"oplog_tail"</code>" MCP tool, the "<code>"oplog.tail"</code>" bridge method, or
                                the CLI. The default is the last 50 records, the maximum 1,000, oldest first:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Bash"</span>
                                </div>
                                <pre><code class="language-bash">{r#"eustress bridge --universe ~/Documents/Eustress/Universe1 oplog --limit 20"#}</code></pre>
                            </div>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Response shape"</span>
                                </div>
                                <pre><code class="language-json">{r#"{
  "count": 1,
  "mutations": [
    {
      "seq": 42,
      "tx_id": 7,
      "ts_nanos": 1790000000000000000,
      "actor": "System",
      "op": "Create",
      "class": "Part",
      "uuid": "9f2c4e1ab7d34c0e8a61f0b2c3d4e5f6",
      "rel_path": "Workspace/__bin_Part_00000000a1b2c3d4/_instance.toml",
      "has_before": false,
      "has_after": true,
      "parent_tx": null,
      "reason": null
    }
  ]
}"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // ENGINES & TOOLS
                    // =========================================================
                    <section id="engines" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "Engines & Tools"
                        </h2>

                        <div id="engines-many" class="subsection">
                            <h3>"Several Engines"</h3>
                            <p>
                                "Every running engine, windowed or headless, opens a bridge on "
                                <code>"127.0.0.1"</code>" and writes its port to "<code>"engine.port"</code>" in its
                                Universe's "<code>".eustress/"</code>" folder, with a copy in the Eustress folder.
                                That file has one slot, so with two engines in one Universe it names only one of
                                them. Each engine therefore also writes a record of its own, keyed by process id:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Documents/Eustress/.eustress/instances/18244.json"</span>
                                </div>
                                <pre><code class="language-json">{r#"{
  "pid": 18244,
  "port": 53120,
  "kind": "headless",
  "space": "C:\\Users\\me\\Documents\\Eustress\\Universe1\\Spaces\\VariantA",
  "universe": "C:\\Users\\me\\Documents\\Eustress\\Universe1",
  "started_at": "2026-09-22T18:02:41.512934100+00:00"
}"#}</code></pre>
                            </div>
                            <p>
                                <code>"eustress instances"</code>" lists these records and removes any whose port no
                                longer answers, "<code>"eustress bridge --pid <pid> ..."</code>" drives one engine by
                                its record, and "<code>"eustress close"</code>" shuts engines down cleanly by pid, by
                                Space or all at once."
                            </p>
                            <div class="callout callout-warning">
                                <img src="/assets/icons/shield.svg" alt="Warning" />
                                <div>
                                    <strong>"One engine per Space"</strong>
                                    <p>
                                        "Fjall keeps no lock between processes, and nothing stops two engines from
                                        opening the same Space, so nothing coordinates their writes to its
                                        database. Give every engine its own Space, and close the engine on a Space
                                        before an offline tool writes to its database."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="engines-inspect" class="subsection">
                            <h3>"Inspect Without the Engine"</h3>
                            <p>
                                <code>"eustress-space"</code>" reads a Space's database without linking the engine.
                                It takes a Space folder or its "<code>"world.fjalldb/"</code>" and works on the
                                binary instance cores:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Bash"</span>
                                </div>
                                <pre><code class="language-bash">{r#"eustress-space open   <space>                 # world id, engine version, core count, bounds, classes
eustress-space verify <space>                 # validates every core; exits non-zero if any fail
eustress-space export <space> [--out <dir>]   # one readable .instance.toml per core"#}</code></pre>
                            </div>
                            <p>
                                "Export writes to "<code>"export_toml/"</code>" inside the given path unless you pass "
                                <code>"--out"</code>". The tool is built from its crate in the repository: "
                                <code>"cargo run -p eustress-space -- open <space>"</code>"."
                            </p>
                        </div>

                        <div id="engines-repair" class="subsection">
                            <h3>"Repair Tools"</h3>
                            <p>"Two more command-line tools repair a Space's database while no engine has it open:"</p>
                            <ul class="docs-list">
                                <li><code>"purge_tree_path"</code>" removes every record matching "<code>"--match"</code>" from every partition: the "<code>"tree"</code>" key and its "<code>"#bin"</code>" twin, the identity lookups and both cores. It is a dry run unless you pass "<code>"--apply"</code>", and with it every value is first copied to "<code>".eustress/trash/db-purge-<unix-seconds>/"</code>"."</li>
                                <li><code>"reseed-space-subtree"</code>" copies one folder's "<code>".toml"</code>" files from disk into "<code>"tree"</code>" and drops their "<code>"#bin"</code>" records, for when the database holds an older version of files you trust."</li>
                            </ul>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Bash"</span>
                                </div>
                                <pre><code class="language-bash">{r#"cargo run -p eustress-worlddb --bin purge_tree_path -- --space <space> --match Aureole
cargo run -p eustress-worlddb --bin purge_tree_path -- --space <space> --match Aureole --apply
cargo run -p eustress-engine --bin reseed-space-subtree -- --space <space> --subdir Workspace"#}</code></pre>
                            </div>
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

                        <div id="roadmap-branches" class="subsection">
                            <h3>"Database Branches"</h3>
                            <p>
                                "The storage library already contains the branch that git cannot provide. A "
                                <code>"BranchHandle"</code>" forks any database in constant time: writes go to an
                                in-memory overlay, reads fall through to the parent, "<code>"commit"</code>" replays
                                the overlay into the parent and "<code>"discard"</code>" drops it. Branches nest, and "
                                <code>"batch_rollout"</code>" runs many of them forward in parallel with no
                                rendering and returns a digest of each. Both are tested in "
                                <code>"eustress-worlddb"</code>", and nothing in Studio, the CLI or the MCP server
                                calls them yet. Wiring them in will let a what-if fork the whole Space, database
                                included, and throw the losers away at no cost to the original."
                            </p>
                        </div>

                        <div id="roadmap-history" class="subsection">
                            <h3>"Complete History"</h3>
                            <p>
                                "Two gaps separate history from the whole Space. The op-log will record property
                                edits and new GUI elements, attribute each change to the agent or person that made
                                it, and keep before-images, which lets the replay and rewind functions the storage
                                library already has step a Space back. And a versioned export of the database will
                                let a commit carry the parts that live only in "<code>"world.fjalldb/"</code>"."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Branch it. Run it. Keep what works."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/simulation" class="btn-secondary-steel">"Simulation Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/realism" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Realism"</span>
                            </div>
                        </a>
                        <a href="/docs/networking" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Networking"</span>
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
