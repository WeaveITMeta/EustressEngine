// =============================================================================
// Eustress Web - CAD Documentation Page
// =============================================================================
// CAD: parametric parts built from a feature tree, evaluated by the eustress-cad
// B-rep kernel, edited in Studio, assembled with mates and driven over MCP.
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
                TocSubsection { id: "overview-part", title: "A CAD Part" },
                TocSubsection { id: "overview-kernel", title: "The Kernel" },
            ],
        },
        TocSection {
            id: "tree",
            title: "Feature Tree",
            subsections: vec![
                TocSubsection { id: "tree-file", title: "features.toml" },
                TocSubsection { id: "tree-sketches", title: "Sketches" },
                TocSubsection { id: "tree-features", title: "Features" },
                TocSubsection { id: "tree-combine", title: "Combining Bodies" },
            ],
        },
        TocSection {
            id: "variables",
            title: "Variables",
            subsections: vec![
                TocSubsection { id: "variables-units", title: "Values Carry Units" },
                TocSubsection { id: "variables-expressions", title: "Expressions" },
                TocSubsection { id: "variables-regen", title: "Regeneration" },
            ],
        },
        TocSection {
            id: "studio",
            title: "In Studio",
            subsections: vec![
                TocSubsection { id: "studio-insert", title: "Inserting a Part" },
                TocSubsection { id: "studio-size", title: "Editing Dimensions" },
                TocSubsection { id: "studio-sketch", title: "The Sketch Panel" },
                TocSubsection { id: "studio-export", title: "Exporting GLB" },
            ],
        },
        TocSection {
            id: "assemblies",
            title: "Assemblies",
            subsections: vec![
                TocSubsection { id: "assemblies-mates", title: "Mates" },
                TocSubsection { id: "assemblies-motion", title: "Making Them Move" },
            ],
        },
        TocSection {
            id: "agents",
            title: "Agents",
            subsections: vec![
                TocSubsection { id: "agents-tools", title: "The cad_ Tools" },
                TocSubsection { id: "agents-loop", title: "Author, Then Verify" },
                TocSubsection { id: "agents-library", title: "Shared Parts" },
            ],
        },
        TocSection {
            id: "limits",
            title: "Kernel Limits",
            subsections: vec![
                TocSubsection { id: "limits-booleans", title: "Booleans Need Overlap" },
                TocSubsection { id: "limits-approx", title: "Approximate Features" },
                TocSubsection { id: "limits-missing", title: "Not Supported Yet" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-sketch", title: "Sketching in Studio" },
                TocSubsection { id: "roadmap-depth", title: "Deeper Modeling" },
            ],
        },
    ]
}

/// The regeneration pipeline: the feature tree on disk is solved, evaluated
/// into an exact B-rep solid, and only then tessellated into the triangles
/// the viewport, the collider and the GLB exporter consume.
#[component]
fn CadPipelineDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 210" role="img"
                aria-label="features.toml is read, its sketches are solved, its features are evaluated into a B-rep solid, and the solid is tessellated into a mesh that feeds both the viewport with its collider and the GLB exporter.">
                <defs>
                    <marker id="cad-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                // Row 1: the exact, parametric half.
                <rect x="10" y="24" width="130" height="48" rx="8" class="dg-box"></rect>
                <text x="75" y="53" class="dg-label" text-anchor="middle">"features.toml"</text>
                <line x1="140" y1="48" x2="170" y2="48" class="dg-line" marker-end="url(#cad-arrow)"></line>

                <rect x="170" y="24" width="130" height="48" rx="8" class="dg-box"></rect>
                <text x="235" y="53" class="dg-label" text-anchor="middle">"Solve sketches"</text>
                <line x1="300" y1="48" x2="330" y2="48" class="dg-line" marker-end="url(#cad-arrow)"></line>

                <rect x="330" y="24" width="140" height="48" rx="8" class="dg-box"></rect>
                <text x="400" y="53" class="dg-label" text-anchor="middle">"Evaluate features"</text>
                <line x1="470" y1="48" x2="500" y2="48" class="dg-line" marker-end="url(#cad-arrow)"></line>

                <rect x="500" y="24" width="130" height="48" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="565" y="53" class="dg-label" text-anchor="middle">"B-rep solid"</text>
                <text x="75" y="92" class="dg-note" text-anchor="middle">"variables, entries"</text>
                <text x="565" y="92" class="dg-note" text-anchor="middle">"exact faces and edges"</text>

                // Row 2: the triangle half.
                <line x1="565" y1="72" x2="565" y2="120" class="dg-line" marker-end="url(#cad-arrow)"></line>
                <rect x="500" y="120" width="130" height="56" rx="8" class="dg-box"></rect>
                <text x="565" y="146" class="dg-label" text-anchor="middle">"Tessellate"</text>
                <text x="565" y="164" class="dg-note" text-anchor="middle">"within 1 mm"</text>

                <line x1="500" y1="136" x2="420" y2="126" class="dg-line" marker-end="url(#cad-arrow)"></line>
                <line x1="500" y1="160" x2="420" y2="178" class="dg-line" marker-end="url(#cad-arrow)"></line>

                <rect x="210" y="104" width="210" height="40" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="315" y="129" class="dg-label" text-anchor="middle">"Viewport mesh + collider"</text>
                <rect x="210" y="160" width="210" height="40" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="315" y="185" class="dg-label" text-anchor="middle">"GLB with parameters"</text>
            </svg>
            <figcaption>
                "The tree is the source of truth. Everything visible, from the viewport mesh to the
                collider and the exported file, is derived from it again on every change."
            </figcaption>
        </figure>
    }
}

/// CAD documentation page.
#[component]
pub fn DocsCadPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-cad"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/grid.svg" alt="CAD" class="toc-icon" />
                        <h2>"CAD"</h2>
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
                            <span class="current">"CAD"</span>
                        </div>
                        <h1 class="docs-title">"CAD"</h1>
                        <p class="docs-subtitle">
                            "Parametric CAD in Eustress builds a solid part from an ordered feature tree of
                            sketches and features, evaluated by a pure-Rust B-rep kernel. Change a variable
                            and the part regenerates; export it to GLB with its parameters inside."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "18 min read"
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

                        <div id="overview-part" class="subsection">
                            <h3>"A CAD Part"</h3>
                            <p>
                                "A CAD part is an ordinary Part whose shape comes from a feature tree instead of
                                a primitive mesh. The tree lives in a "<code>"features.toml"</code>" file beside
                                the part's "<code>"_instance.toml"</code>", in the part's own folder under "
                                <code>"Workspace"</code>". The Explorer and Properties show it like any other
                                Part; only its mesh, collider and Size come from the tree."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Space folder"</span>
                                </div>
                                <pre><code class="language-text">{r#"Workspace/
  CadPlateHole/
    _instance.toml   # class_name = "Part": position, color, material
    features.toml    # the feature tree: variables, sketches, features"#}</code></pre>
                            </div>
                            <p>
                                "When a part loads with a "<code>"features.toml"</code>" beside it, the engine
                                attaches the CAD behavior. From then on it compares the file on disk with the tree
                                in memory every 500 ms, so an edit made in a text editor, restored from git or
                                written by an agent regenerates the part within half a second."
                            </p>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/list.svg" alt="Feature tree" />
                                    </div>
                                    <h4>"Feature Tree"</h4>
                                    <p>"Sketches and features in TOML, evaluated top to bottom."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/cube.svg" alt="B-rep" />
                                    </div>
                                    <h4>"B-rep Kernel"</h4>
                                    <p>"Exact solids; triangles only for display, physics and export."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/link.svg" alt="Mates" />
                                    </div>
                                    <h4>"Mates"</h4>
                                    <p>"Hinges, slides and ball joints that move in Play."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/sparkles.svg" alt="Agents" />
                                    </div>
                                    <h4>"18 Agent Tools"</h4>
                                    <p>"Author, inspect, validate and export over MCP."</p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-kernel" class="subsection">
                            <h3>"The Kernel"</h3>
                            <p>
                                "The kernel is the "<code>"eustress-cad"</code>" crate, built on the truck family
                                of pure-Rust boundary representation (B-rep) crates. B-rep means the part is held as
                                exact faces, edges and vertices rather than triangles: a drilled hole is a true
                                cylinder until the moment it is drawn."
                            </p>
                            <CadPipelineDiagram />
                            <p>
                                "Tessellation keeps the triangles within 1 mm of the true surface by default. A tree
                                can set its own tolerance in meters with "<code>"mesh_tolerance"</code>" under "
                                <code>"[metadata]"</code>"; smaller means smoother curves and more triangles."
                            </p>
                            <p>"The collider is chosen from the mesh, in descending order of accuracy:"</p>
                            <ul class="docs-list">
                                <li><strong>"Convex hull"</strong>" when the body is convex. A convex body's hull is the body, so this is exact."</li>
                                <li><strong>"Convex decomposition"</strong>" for everything else, so an L-bracket's notch and a plate's hole stay open."</li>
                                <li><strong>"Convex hull, approximate"</strong>" when the mesh has more than 20,000 triangles, so a slider drag never stalls on a decomposition."</li>
                                <li><strong>"Bounding box"</strong>" when the mesh is not closed or no hull can be built."</li>
                            </ul>
                            <p>
                                "The part's status line names the collider it received, next to how many features
                                evaluated. It is shown at the top of the Sketch panel."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // FEATURE TREE
                    // =========================================================
                    <section id="tree" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Feature Tree"
                        </h2>

                        <div id="tree-file" class="subsection">
                            <h3>"features.toml"</h3>
                            <p>
                                "A feature tree is a TOML document with a "<code>"[variables]"</code>" table, an
                                ordered list of "<code>"[[entry]]"</code>" tables and optional "
                                <code>"[metadata]"</code>". Each entry is either a sketch ("<code>"kind = sketch"</code>
                                ") or a feature ("<code>"kind = feature"</code>" with an "<code>"op"</code>"). The
                                kernel walks the entries top to bottom, so a feature can only use sketches and
                                features that come before it."
                            </p>
                            <p>"This is the Hole template that Studio inserts, with the plate's profile shortened:"</p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"features.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[variables]
length = "0.1 m"
width = "0.06 m"
height = "0.01 m"
hole_dia = "0.012 m"
hole_depth = "0.015 m"

[[entry]]
name = "BaseSketch"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "line"
p1 = [-0.05, -0.03]
p2 = [0.05, -0.03]

# ...three more lines close the rectangle. Horizontal and vertical
# constraints keep it square; linear dimensions tie it to length and width.

[[entry]]
name = "Base"
kind = "feature"
op = "extrude"
sketch = "BaseSketch"
depth = "height"
both_sides = true

[[entry]]
name = "HoleSketch"
kind = "sketch"
plane = "xy"

[[entry.entities]]
type = "point"
p = [0.0, 0.0]

[[entry]]
name = "Hole1"
kind = "feature"
op = "hole"
sketch_point = "HoleSketch/point-0"
diameter = "hole_dia"
depth = "hole_depth""#}</code></pre>
                            </div>
                            <p>
                                "An entry can also be suppressed: the kernel skips it but keeps its body in the file,
                                so it can be switched back on without losing anything."
                            </p>
                        </div>

                        <div id="tree-sketches" class="subsection">
                            <h3>"Sketches"</h3>
                            <p>
                                "A sketch is a named set of 2D entities, with coordinates in meters: "
                                <code>"line"</code>", "<code>"rectangle"</code>", "<code>"circle"</code>", "
                                <code>"arc"</code>", "<code>"point"</code>" and "<code>"construction"</code>" lines.
                                It also holds driving "<code>"dimensions"</code>" ("<code>"linear"</code>", "
                                <code>"radial"</code>", "<code>"angular"</code>") and geometric "
                                <code>"constraints"</code>" (coincident, concentric, collinear, parallel,
                                perpendicular, tangent, horizontal, vertical, equal length, equal radius, symmetric
                                and fix). Dimensions and constraints refer to entities by their position in the list,
                                starting at 0."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Profiles"</strong>": extrude, revolve and sweep take exactly one rectangle, exactly one circle, or a closed loop of lines. Arcs do not form profiles yet."</li>
                                <li><strong>"Points"</strong>": a hole drills at "<code>"Sketch/point-N"</code>", where N counts point entities only, starting at 0."</li>
                                <li><strong>"Dimension-driven outlines"</strong>": draw them as a loop of lines. A linear dimension on a rectangle drives only its width, which is why every template uses line loops."</li>
                                <li><strong>"Solving"</strong>": a damped Gauss-Newton solver runs on every sketch with constraints or dimensions before features use it, for up to 50 iterations. Line ends that start out touching stay welded while the solver moves them."</li>
                            </ul>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"Every sketch sits in the XY plane"</strong>
                                    <p>
                                        "The kernel builds each profile in the part's XY plane and extrudes along +Z.
                                        The "<code>"plane"</code>" a sketch names ("<code>"xy"</code>", "
                                        <code>"xz"</code>" or "<code>"yz"</code>") is stored in the file but does not
                                        move the profile yet. Eustress is Y-up and CAD parts are inserted unrotated, so
                                        extrusions point along the world Z axis: the Cylinder template lies on its side
                                        until you rotate the part."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="tree-features" class="subsection">
                            <h3>"Features"</h3>
                            <p>"Each feature has an "<code>"op"</code>". This is what each one does today:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"op"</th><th>"Status"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"extrude"</code></td><td>"Working"</td><td>"Sweeps a profile along +Z by "<code>"depth"</code>". "<code>"both_sides"</code>" or the "<code>"mid_plane"</code>" end condition centers it on the sketch; "<code>"through_all"</code>" passes through the whole body."</td></tr>
                                    <tr><td><code>"revolve"</code></td><td>"Working"</td><td>"Turns a profile about the world x, y or z axis by "<code>"angle"</code>"."</td></tr>
                                    <tr><td><code>"hole"</code></td><td>"Working"</td><td>"Drills at a sketch point, blind or through, with an optional counterbore. A countersink is cut as a straight 5 mm step."</td></tr>
                                    <tr><td><code>"pattern"</code></td><td>"Working"</td><td>"Linear or circular copies of earlier features. "<code>"count"</code>" includes the original; with "<code>"combine = subtract"</code>" it repeats a cut."</td></tr>
                                    <tr><td><code>"mirror"</code></td><td>"Working"</td><td>"Reflects the body, or named features, across the xy, xz or yz plane."</td></tr>
                                    <tr><td><code>"boolean"</code></td><td>"Working"</td><td>"Union, difference or intersection of the running body with an earlier feature's result."</td></tr>
                                    <tr><td><code>"split"</code></td><td>"Working"</td><td>"Cuts the body with the xy, xz or yz plane and keeps one side."</td></tr>
                                    <tr><td><code>"sweep"</code></td><td>"Working"</td><td>"Carries a profile along a path sketch of lines, one straight run per segment, joined into one solid."</td></tr>
                                    <tr><td><code>"fillet"</code></td><td>"Approximate"</td><td>"Rounds mesh creases after tessellation; the solid is unchanged."</td></tr>
                                    <tr><td><code>"chamfer"</code></td><td>"Approximate"</td><td>"The same crease softening, driven by "<code>"distance"</code>"."</td></tr>
                                    <tr><td><code>"shell"</code></td><td>"Approximate"</td><td>"Hollows the body with "<code>"wall_thickness"</code>", always open at the top (+Z)."</td></tr>
                                    <tr><td><code>"loft"</code></td><td>"Not implemented"</td><td>"Refused with an error."</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "A feature that asks for something the kernel cannot build fails with an error that
                                names the field, instead of quietly building something else. The full list is under "
                                <a href="#limits-missing">"Not Supported Yet"</a>"."
                            </p>
                        </div>

                        <div id="tree-combine" class="subsection">
                            <h3>"Combining Bodies"</h3>
                            <p>
                                "Extrude, revolve, sweep, mirror and pattern take a "<code>"combine"</code>" mode that
                                says how their result meets the running body: "<code>"new_body"</code>" replaces it, "
                                <code>"add"</code>" (the default) unions with it, "<code>"subtract"</code>" cuts it and "
                                <code>"intersect"</code>" keeps the overlap. A hole always subtracts."
                            </p>
                            <p>
                                "Every entry reports "<code>"ok"</code>", a message, and a "<code>"degraded"</code>"
                                flag. Degraded means the feature produced a body, but not the one you asked for: when
                                an "<code>"add"</code>" cannot union because the two bodies do not touch, the kernel
                                keeps the new body and drops the previous one. The status line says DEGRADED and names
                                the feature. Treat it as an error, because every later feature builds on the wrong body."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"A part is one solid"</strong>
                                    <p>
                                        "The kernel carries a single solid per part. A result made of separate pieces
                                        cannot be represented: mirroring a feature to a spot where the copy does not
                                        touch the original degrades, and a linear pattern of bosses that do not touch
                                        the body fails. Patterned cuts work, because each cut meets the body. Build
                                        separate pieces as separate parts."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // VARIABLES
                    // =========================================================
                    <section id="variables" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Variables"
                        </h2>

                        <div id="variables-units" class="subsection">
                            <h3>"Values Carry Units"</h3>
                            <p>
                                "Every length and angle in a tree is a string with a unit, such as "
                                <code>"50 mm"</code>", "<code>"0.1 m"</code>" or "<code>"90 deg"</code>". The kernel
                                converts to meters and radians internally and keeps the unit you wrote in the file."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Kind"</th><th>"Units accepted"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Length"</td><td><code>"m"</code>", "<code>"mm"</code>", "<code>"cm"</code>", "<code>"km"</code>", "<code>"in"</code>", "<code>"ft"</code>", "<code>"yd"</code></td></tr>
                                    <tr><td>"Angle"</td><td><code>"deg"</code>", "<code>"rad"</code></td></tr>
                                    <tr><td>"Mass"</td><td><code>"kg"</code>", "<code>"g"</code>", "<code>"lb"</code></td></tr>
                                    <tr><td>"Force"</td><td><code>"N"</code>", "<code>"lbf"</code></td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"A bare number is not a length"</strong>
                                    <p>
                                        "A length written as "<code>"10"</code>" with no unit is refused, and the error
                                        names the variable and the value it resolved to. There is no safe default
                                        between meters and millimeters, and a guess is a silent factor of 1,000 that
                                        still produces a perfectly valid solid. Angles are the exception: a bare number
                                        is read as degrees."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="variables-expressions" class="subsection">
                            <h3>"Expressions"</h3>
                            <p>
                                "Anywhere a value is expected, you can write a variable name or an arithmetic
                                expression over variables and quantities. The kernel tries a literal first, then a
                                variable, then an expression, so a value that worked before never changes meaning.
                                Variables may refer to other variables, up to 32 levels deep."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"features.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[variables]
length = "0.1 m"
wall = "3 mm"
pitch = "length/4"             # a length divided by a number: 25 mm
inner = "length - 2 * wall"    # 94 mm
rows = "3"
cols = "4"

# elsewhere, on a pattern feature:
# count = "rows * cols"         # 12 instances, recomputed on every change"#}</code></pre>
                            </div>
                            <p>"Arithmetic follows dimensional analysis, and a mismatch is an error that names both sides:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Expression"</th><th>"Result"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"length + length, length - length"</td><td>"length"</td></tr>
                                    <tr><td>"length * number, number * length, length / number"</td><td>"length"</td></tr>
                                    <tr><td>"length / length"</td><td>"number"</td></tr>
                                    <tr><td>"length + number"</td><td>"refused"</td></tr>
                                    <tr><td>"length * length"</td><td>"refused (the kernel has no area unit)"</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="variables-regen" class="subsection">
                            <h3>"Regeneration"</h3>
                            <p>
                                "Any change to the tree regenerates the part: the kernel re-evaluates every entry, the
                                mesh and collider are rebuilt, and Size is updated from the new bounds. Three things
                                change a tree:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Properties"</strong>": editing Size writes the variables behind each axis (see "<a href="#studio-size">"Editing Dimensions"</a>")."</li>
                                <li><strong>"Agents"</strong>": "<code>"cad_set_variable"</code>" and the other write tools rewrite "<code>"features.toml"</code>"."</li>
                                <li><strong>"The file itself"</strong>": any edit on disk is picked up by the 500 ms comparison."</li>
                            </ul>
                            <p>
                                "Each Studio edit to a CAD part (inserting it, a Size change, a new constraint, a
                                solve) is one labeled undo step. Undoing a tree edit writes the previous tree back to "
                                <code>"features.toml"</code>"; undoing an insert removes the part and moves its folder
                                to the Space's trash."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // IN STUDIO
                    // =========================================================
                    <section id="studio" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "In Studio"
                        </h2>

                        <div id="studio-insert" class="subsection">
                            <h3>"Inserting a Part"</h3>
                            <p>
                                "The Model tab's Parts group holds seven parametric templates next to the ordinary
                                inserts. Each one inserts a CAD part 8 m in front of the camera, saves its folder under "
                                <code>"Workspace"</code>" immediately, and selects it. The Model tab is part of the
                                default Engineering mode; "<a href="/docs/studio">"Studio"</a>" covers modes and the
                                tabs each one shows."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Button"</th><th>"Template"</th><th>"Default shape"</th><th>"Variables"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Plate"</td><td><code>"plate"</code></td><td>"100 x 60 x 10 mm"</td><td><code>"length"</code>", "<code>"width"</code>", "<code>"height"</code></td></tr>
                                    <tr><td>"Box"</td><td><code>"box"</code></td><td>"50 mm cube"</td><td><code>"size"</code></td></tr>
                                    <tr><td>"Cylinder"</td><td><code>"cylinder"</code></td><td>"40 mm across, 60 mm long"</td><td><code>"radius"</code>", "<code>"height"</code></td></tr>
                                    <tr><td>"Hole"</td><td><code>"plate_hole"</code></td><td>"The plate with a 12 mm through-hole"</td><td>"plate variables, "<code>"hole_dia"</code>", "<code>"hole_depth"</code></td></tr>
                                    <tr><td>"L-Bracket"</td><td><code>"l_bracket"</code></td><td>"A 60 x 50 mm L profile, 8 mm thick"</td><td><code>"thickness"</code></td></tr>
                                    <tr><td>"Frame"</td><td><code>"constrained_frame"</code></td><td>"Four skewed lines the solver squares, 10 mm deep"</td><td><code>"depth"</code></td></tr>
                                    <tr><td>"Shell"</td><td><code>"shelled_box"</code></td><td>"A 50 mm cube hollowed to a 4 mm wall"</td><td><code>"size"</code>", "<code>"wall"</code></td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The folder is named after the template ("<code>"CadPlate"</code>", "
                                <code>"CadBox"</code>" and so on) with a suffix when the name is taken. The same seven
                                templates are what agents create with "<code>"cad_create_part"</code>"."
                            </p>
                        </div>

                        <div id="studio-size" class="subsection">
                            <h3>"Editing Dimensions"</h3>
                            <p>
                                "A CAD part's Size in Properties is its computed extent. Typing a new Size writes the
                                variables that drive each axis, converted from the display unit to meters, and the
                                part regenerates. Which variable an axis drives depends on the names the tree uses:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Variables in the tree"</th><th>"X"</th><th>"Y"</th><th>"Z"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"length"</code>", "<code>"width"</code>", "<code>"height"</code></td><td><code>"length"</code></td><td><code>"width"</code></td><td><code>"height"</code></td></tr>
                                    <tr><td><code>"radius"</code>", "<code>"height"</code></td><td>"diameter"</td><td>"diameter"</td><td><code>"height"</code></td></tr>
                                    <tr><td><code>"size"</code></td><td><code>"size"</code></td><td><code>"size"</code></td><td><code>"size"</code></td></tr>
                                    <tr><td><code>"height"</code>", "<code>"thickness"</code>" or "<code>"depth"</code>" alone"</td><td>"none"</td><td>"none"</td><td>"that variable"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "When several axes drive the same variable, the last axis wins: on a Box the Z value
                                sets "<code>"size"</code>", and on a Cylinder the Y value sets the diameter. A part
                                with none of these names prints a warning in the Output instead. Variables that do
                                not map to an axis, such as "<code>"hole_dia"</code>" or "<code>"wall"</code>", are
                                changed in "<code>"features.toml"</code>" or with "<code>"cad_set_variable"</code>"."
                            </p>
                        </div>

                        <div id="studio-sketch" class="subsection">
                            <h3>"The Sketch Panel"</h3>
                            <p>
                                "Selecting a CAD part that has a sketch opens the Sketch panel on its first sketch.
                                Close it with "<strong>"x"</strong>" or "<code>"Esc"</code>"; double-click the part to
                                open it again. The panel shows the part's status line, every entity with its
                                coordinates in the display unit, and the sketch's constraints."
                            </p>
                            <ol class="numbered-list">
                                <li>"Click an entity row to pick it as A; click a second row to pick B."</li>
                                <li>"Press "<strong>"H"</strong>" or "<strong>"V"</strong>" to make A horizontal or vertical, or "<strong>"⊥"</strong>" (perpendicular) or "<strong>"⊙"</strong>" (coincident) to relate A and B."</li>
                                <li>"The constraint is added and the sketch solved at once; the result is written to "<code>"features.toml"</code>" and the part regenerates."</li>
                                <li>"Press "<strong>"Solve Sketch"</strong>" (or "<strong>"Solve"</strong>" on the Model tab) to solve every sketch in the part and write the solved coordinates back to the file."</li>
                            </ol>
                            <p>
                                "The solver reports one of four states: under-constrained, fully constrained,
                                over-constrained or failed, with the residual and the remaining degrees of freedom.
                                Insert the Frame template to see it work: its four lines start skewed, and the
                                horizontal and vertical constraints square them."
                            </p>
                        </div>

                        <div id="studio-export" class="subsection">
                            <h3>"Exporting GLB"</h3>
                            <p>
                                "Select a CAD part and press "<strong>"GLB"</strong>" in the Model tab's Parts group.
                                Studio evaluates the tree and writes "<code>"<Name>.glb"</code>" beside "
                                <code>"features.toml"</code>", then copies it into "<code>"Workspace/Assets/Cad/"</code>"
                                with a small TOML file listing its variables. The file holds positions, normals, UVs
                                and triangle indices in meters, and records how it was made in the node's "
                                <code>"extras"</code>":"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"GLB node extras (JSON)"</span>
                                </div>
                                <pre><code class="language-json">{r#"{
  "eustress": {
    "kind": "CadPart",
    "generator": "eustress-cad",
    "variables": { "height": "0.01 m", "length": "0.1 m", "width": "0.06 m" },
    "features": [{ "name": "Extrude1", "ok": true, "message": "ok" }]
  }
}"#}</code></pre>
                            </div>
                            <p>
                                "The "<code>"features"</code>" list has one row per tree entry (the sketch rows are
                                left out above). A program or model reading the GLB can see the parameters behind
                                the shape without the tree."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // ASSEMBLIES
                    // =========================================================
                    <section id="assemblies" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Assemblies"
                        </h2>

                        <div id="assemblies-mates" class="subsection">
                            <h3>"Mates"</h3>
                            <p>
                                "A mate joins two parts with an Avian physics joint, so an assembly moves the way the
                                mechanism would. Mates work on any two parts, CAD or not."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Mate"</th><th>"Avian joint"</th><th>"Behavior"</th><th>"Start it from"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Hinge"</td><td><code>"RevoluteJoint"</code></td><td>"Rotates about the Y axis"</td><td>"Model tab, Constraints: Hinge"</td></tr>
                                    <tr><td>"Slide"</td><td><code>"PrismaticJoint"</code></td><td>"Slides along the X axis"</td><td>"Model tab, Constraints: Slide"</td></tr>
                                    <tr><td>"Ball"</td><td><code>"SphericalJoint"</code></td><td>"Rotates freely about the anchors"</td><td>"Model tab, Constraints: Ball"</td></tr>
                                    <tr><td>"Weld"</td><td><code>"FixedJoint"</code></td><td>"Locks the two parts together"</td><td>"The Mate choice in the tool options"</td></tr>
                                    <tr><td>"Distance"</td><td><code>"DistanceJoint"</code></td><td>"Holds the anchors at their current distance, within 1%"</td><td>"The Mate choice in the tool options"</td></tr>
                                </tbody>
                            </table>
                            <ol class="numbered-list">
                                <li>"Press Hinge, Slide or Ball. The tool options bar shows a Mate choice with all five kinds."</li>
                                <li>"Click the first part. The point you click becomes its anchor."</li>
                                <li>"Click the second part. Its anchor is recorded and the mate commits."</li>
                            </ol>
                            <p>
                                "Press "<code>"Esc"</code>" to cancel before the second click. Creating a mate is an
                                undo step. The axes are fixed today: Y for Hinge and X for Slide."
                            </p>
                        </div>

                        <div id="assemblies-motion" class="subsection">
                            <h3>"Making Them Move"</h3>
                            <p>
                                "Studio runs physics only in Play, and a joint only moves parts that are not anchored.
                                When Play starts, every unanchored part with a collider becomes a dynamic body. CAD
                                parts are inserted anchored, so clear Anchored on the part that should move, keep its
                                partner anchored, and press Play to see the hinge swing or the slide travel. Stopping
                                Play restores every part to where it was. See "
                                <a href="/docs/physics">"Physics"</a>" for bodies and joints in general."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"Mates last for the session"</strong>
                                    <p>
                                        "A mate and its joint are not saved with the Space. After the Space is
                                        reopened the parts are still there, but the mates are gone and have to be
                                        added again."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // AGENTS
                    // =========================================================
                    <section id="agents" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Agents"
                        </h2>

                        <div id="agents-tools" class="subsection">
                            <h3>"The cad_ Tools"</h3>
                            <p>
                                "The "<a href="/learn/mcp">"MCP server"</a>" exposes 18 CAD tools. They read and
                                write "<code>"features.toml"</code>" files inside the open Space, and a running engine
                                picks each change up within half a second. Paths are relative to the Space; a path
                                containing "<code>".."</code>" or an absolute path is refused."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Job"</th><th>"Tools"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Create and change"</td><td><code>"cad_create_part"</code>", "<code>"cad_set_variable"</code>", "<code>"cad_add_feature"</code>", "<code>"cad_edit_feature"</code>", "<code>"cad_delete_feature"</code></td></tr>
                                    <tr><td>"Sketch"</td><td><code>"cad_create_sketch"</code>", "<code>"cad_add_sketch_entity"</code>", "<code>"cad_add_constraint"</code>", "<code>"cad_dimension"</code>", "<code>"cad_offset_sketch"</code>", "<code>"cad_solve_sketch"</code></td></tr>
                                    <tr><td>"Inspect"</td><td><code>"cad_describe_part"</code>", "<code>"cad_validate_part"</code>", "<code>"cad_measure"</code>", "<code>"cad_list_templates"</code></td></tr>
                                    <tr><td>"Export and share"</td><td><code>"cad_export_glb"</code>", "<code>"cad_publish_part"</code>", "<code>"cad_list_sources"</code></td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The sketch tools return the solver status and remaining degrees of freedom with every
                                edit, because a constraint's effect is global. "<code>"cad_offset_sketch"</code>" makes
                                a new sketch whose profile is an existing one moved in or out by a distance, which is
                                how a parametric wall is built: offset the outline inward by "<code>"-wall"</code>",
                                extrude it, and subtract it. "<code>"cad_delete_feature"</code>" refuses to delete an
                                entry that later entries reference unless asked to force it."
                            </p>
                        </div>

                        <div id="agents-loop" class="subsection">
                            <h3>"Author, Then Verify"</h3>
                            <p>
                                "CAD failures are often silent and partial: a boolean can succeed and still leave a
                                hole in the surface. The tools are built for a loop of one edit, then one read. The
                                feature and sketch tools return the re-evaluated state with every edit, including the
                                volume before and after, and flag an edit that changed the file but not the solid."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MCP"</span>
                                </div>
                                <pre><code class="language-json">{r#"{ "tool": "cad_create_part", "arguments": { "name": "Bracket", "template": "plate_hole" } }

{ "tool": "cad_add_feature", "arguments": {
    "path": "Workspace/Bracket", "op": "pattern", "name": "HoleRow",
    "pattern_kind": "linear", "features": ["Hole1"], "count": 3,
    "spacing": "20 mm", "direction": [1, 0, 0], "combine": "subtract" } }

{ "tool": "cad_validate_part", "arguments": { "path": "Workspace/Bracket" } }

{ "tool": "cad_measure", "arguments": { "path": "Workspace/Bracket", "density": "7850 kg/m^3" } }

{ "tool": "cad_export_glb", "arguments": { "path": "Workspace/Bracket" } }"#}</code></pre>
                            </div>
                            <p>
                                "The pattern repeats the hole's cutter, not the drilled plate: three holes at 20 mm
                                pitch, the original included. "<code>"cad_validate_part"</code>" returns pass or fail
                                for "<code>"parses"</code>", "<code>"evaluates"</code>", "<code>"all_features_ok"</code>",
                                "<code>"no_degraded_features"</code>", "<code>"non_empty_body"</code>", "
                                <code>"watertight"</code>", "<code>"manifold"</code>", "<code>"no_degenerate_triangles"</code>
                                " and "<code>"positive_volume"</code>". "<code>"cad_measure"</code>" returns volume,
                                surface area, center of mass and bounds, plus mass when given a density, and the exact
                                minimum distance to a second part with "<code>"against"</code>". "
                                <code>"cad_export_glb"</code>" writes "<code>"export.glb"</code>" beside the tree unless "
                                <code>"out"</code>" names another path."
                            </p>
                        </div>

                        <div id="agents-library" class="subsection">
                            <h3>"Shared Parts"</h3>
                            <p>
                                "A part can be published once and placed many times. "<code>"cad_publish_part"</code>
                                " copies a part's tree into the Universe's library at "
                                <code>".eustress/assets/cad/<id>/features.toml"</code>", refusing a tree that does not
                                evaluate. "<code>"cad_create_part"</code>" with "<code>"source"</code>" set to that id
                                makes a placement: a part whose "<code>"cad_source"</code>" attribute points at the
                                library entry."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"One owner"</strong>": editing the library file restates every placement. Placements in the open Space follow within half a second; placements in other Spaces read it when their Space opens."</li>
                                <li><strong>"Edits to a placement are refused"</strong>": Studio shows a warning instead of writing a tree the next sync would overwrite."</li>
                                <li><strong>"Ids"</strong>": one path segment of letters, digits, "<code>"_"</code>", "<code>"-"</code>" and "<code>"."</code>", up to 128 characters. "<code>"cad_list_sources"</code>" lists them with their variables."</li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // KERNEL LIMITS
                    // =========================================================
                    <section id="limits" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "Kernel Limits"
                        </h2>

                        <div id="limits-booleans" class="subsection">
                            <h3>"Booleans Need Overlap"</h3>
                            <p>
                                "Every subtract, union, intersect, hole, split and shell goes through the boolean
                                operations of truck-shapeops. They fail when two bodies share a face exactly or do not
                                touch at all, so the kernel pushes hole cuts and through-all cuts past the faces they
                                enter. When you author a cut yourself, make it protrude through the surface rather
                                than stop flush with it."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Pitfall" />
                                <div>
                                    <strong>"Why booleans are rescaled"</strong>
                                    <p>
                                        "The boolean library has a scale floor: the same shapes that combine cleanly at
                                        unit size fail at centimeter size, where real parts live. The kernel works around
                                        it by scaling both bodies toward unit size before each operation, trying six
                                        tolerances in turn, and scaling the result back. It also guards each attempt, so
                                        a failure inside the library becomes an error on the feature instead of a crash.
                                        A test in the crate fails on the day the library fixes the floor, so the
                                        workaround can be removed."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="limits-approx" class="subsection">
                            <h3>"Approximate Features"</h3>
                            <ul class="docs-list">
                                <li><strong>"Fillet and chamfer"</strong>" soften the mesh after tessellation. They apply to every crease in the part at the largest radius any fillet or chamfer in the tree asks for; the edge list is not used to pick edges. The solid is untouched, so "<code>"cad_measure"</code>" reports the volume without them."</li>
                                <li><strong>"Shell"</strong>" is always open at the top: the inner cut exits through +Z, and "<code>"open_faces"</code>" is not read."</li>
                                <li><strong>"Countersink"</strong>" is cut as a straight 5 mm step at "<code>"countersink_diameter"</code>", not a cone."</li>
                                <li><strong>"Split"</strong>" keeps one side of the plane; the other side is discarded."</li>
                            </ul>
                        </div>

                        <div id="limits-missing" class="subsection">
                            <h3>"Not Supported Yet"</h3>
                            <p>"Each of these fails with an error that says so, rather than building something else:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Request"</th><th>"Today"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"loft"</code></td><td>"Refused: needs a multi-profile solver the truck crates do not provide."</td></tr>
                                    <tr><td>"Extrude end conditions "<code>"to_plane"</code>", "<code>"to_surface"</code>", "<code>"up_to_next"</code></td><td>"Refused: the feature has no field naming the target."</td></tr>
                                    <tr><td>"A non-zero "<code>"draft_angle"</code></td><td>"Refused."</td></tr>
                                    <tr><td>"Patterns of kind "<code>"path"</code>" or "<code>"sketch"</code></td><td>"Refused."</td></tr>
                                    <tr><td>"Hole "<code>"tap_class"</code>" or "<code>"countersink_angle"</code></td><td>"Refused. Set "<code>"diameter"</code>" to the tap-drill size instead."</td></tr>
                                    <tr><td>"Chamfer "<code>"distance2"</code>" or "<code>"angle"</code>", fillet with "<code>"propagate_tangent = false"</code></td><td>"Refused."</td></tr>
                                    <tr><td>"Sketches on a feature's face"</td><td>"Refused by the sketch tools; evaluation has no face references."</td></tr>
                                    <tr><td>"Tangent, symmetric constraints"</td><td>"Tangent solves only between a line and a circle; symmetric only keeps a segment's midpoint on its axis."</td></tr>
                                    <tr><td>"STEP files"</td><td>"Not read or written. The truck STEP crate is a dependency with no caller yet."</td></tr>
                                </tbody>
                            </table>
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

                        <div id="roadmap-sketch" class="subsection">
                            <h3>"Sketching in Studio"</h3>
                            <p>
                                "Today the Sketch panel adds constraints to an existing sketch, and new sketches come
                                from templates or from agents. The CAD platform plan's sketch phase will add a Studio
                                canvas where you draw entities, apply constraints and drag them with a live solve, a
                                feature-tree panel for reordering and suppressing entries, and face references so a
                                sketch can sit on the face of an earlier feature. Variables that do not map to Size
                                will be editable directly in Properties."
                            </p>
                        </div>

                        <div id="roadmap-depth" class="subsection">
                            <h3>"Deeper Modeling"</h3>
                            <p>
                                "Next come procedural models whose attributes regenerate their children, with built-in
                                generators for gears, stairs, railings, trusses and pipe runs, and Luau or Rune
                                generator scripts. After that: loft and path sweeps once face and path references
                                land, shells with real offset surfaces, true B-rep fillets and chamfers as the truck
                                crates gain them, STEP export, a bill of materials from the assembly graph, and stress
                                analysis that extends the realism crate's deformation model onto the CAD mesh."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Sketch it, dimension it, change one number."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/learn/mcp" class="btn-secondary-steel">"MCP Server Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/perspective" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Perspective"</span>
                            </div>
                        </a>
                        <a href="/docs/importing" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Importing"</span>
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
