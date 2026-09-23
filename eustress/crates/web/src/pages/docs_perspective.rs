// =============================================================================
// Eustress Web - Perspective Documentation Page
// =============================================================================
// Perspective: 2D and 3D views, perspective and orthographic projection, and
// how the editor switches between them without converting the world.
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
                TocSubsection { id: "overview-one-world", title: "One World, Two Views" },
                TocSubsection { id: "overview-three-views", title: "The Three Views" },
            ],
        },
        TocSection {
            id: "projection",
            title: "Projection",
            subsections: vec![
                TocSubsection { id: "projection-compare", title: "Perspective vs Orthographic" },
                TocSubsection { id: "projection-enum", title: "An Enum, Not a Boolean" },
                TocSubsection { id: "projection-zoom", title: "One Zoom for Both" },
                TocSubsection { id: "projection-transition", title: "The Seamless Switch" },
            ],
        },
        TocSection {
            id: "two-d",
            title: "2D",
            subsections: vec![
                TocSubsection { id: "two-d-enter", title: "Entering 2D" },
                TocSubsection { id: "two-d-planes", title: "Working Planes" },
                TocSubsection { id: "two-d-navigate", title: "Navigating" },
                TocSubsection { id: "two-d-build", title: "Building in 2D" },
            ],
        },
        TocSection {
            id: "controls",
            title: "Controls",
            subsections: vec![
                TocSubsection { id: "controls-selector", title: "The Perspective Control" },
                TocSubsection { id: "controls-keys", title: "Shortcuts" },
                TocSubsection { id: "controls-properties", title: "Camera Properties" },
            ],
        },
        TocSection {
            id: "automation",
            title: "Agents & Files",
            subsections: vec![
                TocSubsection { id: "automation-actions", title: "Actions over MCP" },
                TocSubsection { id: "automation-memory", title: "Per-Space Memory" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-content", title: "Native 2D Content" },
                TocSubsection { id: "roadmap-runtime", title: "2D at Runtime" },
            ],
        },
    ]
}

/// Perspective vs orthographic, drawn as the rays each projection casts.
/// Two equal cubes sit at two depths: perspective shrinks the far one,
/// orthographic draws both the same size.
#[component]
fn ProjectionDiagram() -> impl IntoView {
    view! {
        <figure class="perspective-figure">
            <svg class="perspective-diagram" viewBox="0 0 640 230" role="img"
                aria-label="Perspective rays spread from one eye point, so a far cube looks smaller. Orthographic rays run parallel, so both cubes look the same size.">
                // Perspective
                <text x="20" y="24" class="pd-title">"Perspective"</text>
                <line x1="40" y1="125" x2="296" y2="42" class="pd-ray"></line>
                <line x1="40" y1="125" x2="296" y2="208" class="pd-ray"></line>
                <line x1="296" y1="42" x2="296" y2="208" class="pd-plane"></line>
                <line x1="78" y1="112.5" x2="78" y2="137.5" class="pd-near"></line>
                <circle cx="40" cy="125" r="5" class="pd-eye"></circle>
                <rect x="126" y="104" width="42" height="42" class="pd-cube"></rect>
                <rect x="226" y="104" width="42" height="42" class="pd-cube pd-cube-far"></rect>
                // Each cube's wedge of rays: the far one is visibly narrower.
                <line x1="40" y1="125" x2="126" y2="104" class="pd-sight pd-sight-near"></line>
                <line x1="40" y1="125" x2="126" y2="146" class="pd-sight pd-sight-near"></line>
                <line x1="40" y1="125" x2="226" y2="104" class="pd-sight pd-sight-far"></line>
                <line x1="40" y1="125" x2="226" y2="146" class="pd-sight pd-sight-far"></line>
                <text x="120" y="176" class="pd-note">"near: large"</text>
                <text x="214" y="176" class="pd-note">"far: small"</text>

                // Orthographic
                <text x="344" y="24" class="pd-title">"Orthographic"</text>
                <line x1="352" y1="60" x2="620" y2="60" class="pd-ray"></line>
                <line x1="352" y1="190" x2="620" y2="190" class="pd-ray"></line>
                <line x1="352" y1="60" x2="352" y2="190" class="pd-near"></line>
                <line x1="620" y1="60" x2="620" y2="190" class="pd-plane"></line>
                <rect x="430" y="104" width="42" height="42" class="pd-cube"></rect>
                <rect x="540" y="104" width="42" height="42" class="pd-cube pd-cube-far"></rect>
                <line x1="352" y1="104" x2="582" y2="104" class="pd-sight"></line>
                <line x1="352" y1="146" x2="582" y2="146" class="pd-sight"></line>
                <text x="416" y="176" class="pd-note">"same size"</text>
                <text x="526" y="176" class="pd-note">"same size"</text>
            </svg>
            <figcaption>
                "Perspective rays spread from one point, so distance shrinks things. Orthographic
                rays run parallel, so a part measures the same on screen at any depth."
            </figcaption>
        </figure>
    }
}

/// Perspective documentation page.
#[component]
pub fn DocsPerspectivePage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-perspective"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/cube.svg" alt="Perspective" class="toc-icon" />
                        <h2>"Perspective"</h2>
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
                            <span class="current">"Perspective"</span>
                        </div>
                        <h1 class="docs-title">"Perspective"</h1>
                        <p class="docs-subtitle">
                            "One world, seen two ways. Work in the free 3D view or flatten it onto a 2D
                            plane, and switch between perspective and orthographic projection, without
                            converting a single part."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "8 min read"
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

                        <div id="overview-one-world" class="subsection">
                            <h3>"One World, Two Views"</h3>
                            <p>
                                "In Eustress, 2D is a way of looking at a Space, not a different kind of Space.
                                The same parts, the same physics, the same files on disk are there whether you
                                orbit them in 3D or lay them out flat in 2D. Nothing is exported, converted or
                                duplicated when you switch, so you can sketch a floor plan in 2D, tumble into
                                3D to check the heights, and drop straight back to the plan."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Two independent switches"</strong>
                                    <p>
                                        "The view has a dimension (3D or 2D) and a projection (perspective or
                                        orthographic). 3D can use either projection. 2D is always orthographic,
                                        because a flat working plane needs sizes that read true."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-three-views" class="subsection">
                            <h3>"The Three Views"</h3>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/cube.svg" alt="3D Perspective" />
                                    </div>
                                    <h4>"3D Perspective"</h4>
                                    <p>"The default. Orbit, look and fly; distance shrinks things the way your eye does."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/grid.svg" alt="3D Orthographic" />
                                    </div>
                                    <h4>"3D Orthographic"</h4>
                                    <p>"Still free to orbit, but parallel: elevations, plans and measured drawings."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/template.svg" alt="2D" />
                                    </div>
                                    <h4>"2D"</h4>
                                    <p>"Locked to one axis plane. Pan and zoom only; depth becomes layer order."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // PROJECTION
                    // =========================================================
                    <section id="projection" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Projection"
                        </h2>

                        <div id="projection-compare" class="subsection">
                            <h3>"Perspective vs Orthographic"</h3>
                            <p>
                                "A camera turns the 3D world into a flat image by casting rays. Perspective
                                casts them from a single point, like an eye or a lens, so the same part looks
                                smaller the further away it is. Orthographic casts them parallel, so a one-meter
                                part is the same size on screen at any depth."
                            </p>
                            <ProjectionDiagram />
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Use"</th><th>"Perspective"</th><th>"Orthographic"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Judging how a space feels"</td><td>"Yes"</td><td>"No"</td></tr>
                                    <tr><td>"Aligning parts along an axis"</td><td>"Hard"</td><td>"Easy"</td></tr>
                                    <tr><td>"Comparing sizes by eye"</td><td>"Misleading"</td><td>"Exact"</td></tr>
                                    <tr><td>"Plans, elevations, sections"</td><td>"No"</td><td>"Yes"</td></tr>
                                    <tr><td>"2D layouts and side views"</td><td>"No"</td><td>"Yes"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Parallel rays change the sky too. Every pixel looks the same way, so the sky
                                behind an orthographic view is one color, the sky straight ahead. There is no
                                aerial haze, sun disk or star field, so parts keep their true colors at any zoom."
                            </p>
                        </div>

                        <div id="projection-enum" class="subsection">
                            <h3>"An Enum, Not a Boolean"</h3>
                            <p>
                                "Some tools expose orthographic as an on/off flag on the camera. Eustress
                                runs on Bevy, where projection is a component holding one of two lenses,
                                and each lens carries its own settings: a field of view for perspective, a view
                                size for orthographic. A flag cannot hold both, and it leaves no room for a
                                third kind of projection later."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust (Bevy)"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// The camera's projection is an enum component.
enum Projection {
    Perspective(PerspectiveProjection),   // fov, near, far
    Orthographic(OrthographicProjection), // view size, near, far
    Custom(CustomProjection),             // anything else
}

// Orthographic in meters: show 24 m of the world, top to bottom.
Projection::Orthographic(OrthographicProjection {
    scaling_mode: ScalingMode::FixedVertical { viewport_height: 24.0 },
    ..OrthographicProjection::default_3d()
})"#}</code></pre>
                            </div>
                            <p>
                                "The Camera object in the Explorer mirrors that shape. It has a "
                                <code>"Projection"</code>" property (Perspective or Orthographic) next to
                                "<code>"FieldOfView"</code>" for the perspective lens and "
                                <code>"OrthographicSize"</code>" (the visible height, in meters) for the
                                orthographic one, plus "<code>"ViewMode"</code>" for 3D or 2D."
                            </p>
                        </div>

                        <div id="projection-zoom" class="subsection">
                            <h3>"One Zoom for Both"</h3>
                            <p>
                                "Every camera orbits a pivot, the point in the middle of the screen. The
                                orthographic view height is derived from the distance to that pivot, so one
                                number sets the zoom in both projections:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Math"</span>
                                </div>
                                <pre><code class="language-text">{r#"view height = 2 x distance x tan(field of view / 2)

distance 20 m, field of view 70 deg  ->  view height 28.0 m"#}</code></pre>
                            </div>
                            <p>
                                "That height is exactly how tall the pivot plane looks in perspective. So the
                                thing you are looking at keeps its size when the projection changes, and
                                framing a selection with "<code>"F"</code>" fits it in either projection."
                            </p>
                        </div>

                        <div id="projection-transition" class="subsection">
                            <h3>"The Seamless Switch"</h3>
                            <p>
                                "Switching projection is animated as a dolly zoom. The field of view narrows
                                while the camera backs away, so the object at the center of the screen holds its
                                size while everything around it flattens. When perspective has flattened out,
                                true orthographic takes over. Going back plays the same move in reverse."
                            </p>
                            <div class="callout callout-tip">
                                <img src="/assets/icons/sparkles.svg" alt="Tip" />
                                <div>
                                    <strong>"It anchors on what you are looking at"</strong>
                                    <p>
                                        "Before going orthographic, the editor finds whatever sits under the center of
                                        the screen and anchors the zoom there. A wall two meters away keeps its size,
                                        even if the orbit point had drifted far behind it while you were flying."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // 2D
                    // =========================================================
                    <section id="two-d" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "2D"
                        </h2>

                        <div id="two-d-enter" class="subsection">
                            <h3>"Entering 2D"</h3>
                            <p>
                                "Press "<code>"Alt+2"</code>", or click "<strong>"2D"</strong>" in the
                                Perspective control at the top right of the viewport. The camera turns onto an
                                axis plane and flattens to orthographic in one motion. "<code>"Alt+3"</code>
                                " returns you to the 3D view and projection you left, centered on wherever the
                                2D view was panned to."
                            </p>
                            <p>
                                "If you are already looking along an axis (Top, say), 2D keeps that plane.
                                Otherwise it uses the plane you last worked in, and the XY plane the first
                                time. XY with +Y up and depth as draw order is the same convention Bevy's own
                                2D renderer uses, so 2D content lines up one to one."
                            </p>
                        </div>

                        <div id="two-d-planes" class="subsection">
                            <h3>"Working Planes"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Plane"</th><th>"View"</th><th>"Looks along"</th><th>"Typical use"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"XY"</td><td>"Front / Back"</td><td>"Z"</td><td>"Side views, side-scrollers, elevations"</td></tr>
                                    <tr><td>"XZ"</td><td>"Top / Bottom"</td><td>"Y"</td><td>"Floor plans, maps, top-down layouts"</td></tr>
                                    <tr><td>"YZ"</td><td>"Right / Left"</td><td>"X"</td><td>"End elevations, cross-sections"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Change plane from the Perspective menu, or with the view keys while in 2D:
                                "<code>"Num 2"</code>" for XY, "<code>"Num 8"</code>" for XZ, "
                                <code>"Num 6"</code>" for YZ. Hold "<code>"Ctrl"</code>" for the
                                opposite side."
                            </p>
                        </div>

                        <div id="two-d-navigate" class="subsection">
                            <h3>"Navigating"</h3>
                            <p>"2D has no rotation, so every gesture is a pan or a zoom:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Input"</th><th>"3D perspective"</th><th>"3D orthographic"</th><th>"2D"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Right-drag"</td><td>"Look around"</td><td>"Orbit the pivot"</td><td>"Pan"</td></tr>
                                    <tr><td>"Middle-drag"</td><td>"Pan"</td><td>"Pan"</td><td>"Pan"</td></tr>
                                    <tr><td>"Alt + left-drag"</td><td>"Orbit"</td><td>"Orbit"</td><td>"Pan"</td></tr>
                                    <tr><td>"Wheel"</td><td>"Fly toward cursor"</td><td>"Zoom about cursor"</td><td>"Zoom about cursor"</td></tr>
                                    <tr><td>"W / S"</td><td>"Fly forward / back"</td><td>"Zoom in / out"</td><td>"Pan up / down"</td></tr>
                                    <tr><td>"A / D"</td><td>"Strafe"</td><td>"Pan"</td><td>"Pan left / right"</td></tr>
                                    <tr><td>"Q / E"</td><td>"Down / up"</td><td>"Down / up"</td><td>"Zoom out / in"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Pans in orthographic and 2D are one to one: the point you grab stays under the
                                cursor. Keyboard speeds follow the zoom, so the view moves at the same pace across
                                a bolt or a city."
                            </p>
                        </div>

                        <div id="two-d-build" class="subsection">
                            <h3>"Building in 2D"</h3>
                            <ul class="docs-list">
                                <li><strong>"Dragging"</strong>" slides a part across the plane. Its depth, which is its layer, never changes, and grid snap works in the plane."</li>
                                <li><strong>"Inserting"</strong>" drops the new part at the center of the view, on the working plane."</li>
                                <li><strong>"The grid"</strong>" lies on the working plane, and its spacing follows the zoom in powers of ten. Parts in front of the plane cover it and its lines cross parts behind it, so you can tell which side of the plane a part is on. Seen from above, it lies on top of the floor."</li>
                                <li><strong>"Gizmos"</strong>" keep a constant size on screen, and the handle pointing straight at you stays out of the way of the ones you can drag."</li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // CONTROLS
                    // =========================================================
                    <section id="controls" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Controls"
                        </h2>

                        <div id="controls-selector" class="subsection">
                            <h3>"The Perspective Control"</h3>
                            <p>
                                "The Perspective control lives in the viewport's tab bar, beside the Space tab,
                                so it never covers the scene. The "<strong>"2D | 3D"</strong>" switch changes
                                dimension. The button beside it names what is on screen ("
                                <em>"User Perspective"</em>", "<em>"Top Orthographic"</em>", "
                                <em>"XY Plane"</em>") with the visible height when orthographic, and opens the
                                Perspective menu:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Projection"</strong>": Perspective or Orthographic, with a field-of-view slider for perspective"</li>
                                <li><strong>"View"</strong>": the six axis views. In 2D these pick the working plane"</li>
                                <li><strong>"Frame All"</strong>": fit the whole scene"</li>
                            </ul>
                            <p>"The same choices sit in the "<strong>"View"</strong>" menu under Perspective."</p>
                        </div>

                        <div id="controls-keys" class="subsection">
                            <h3>"Shortcuts"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Keys"</th><th>"Action"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"Alt+2"</code></td><td>"2D view"</td></tr>
                                    <tr><td><code>"Alt+3"</code></td><td>"3D view"</td></tr>
                                    <tr><td><code>"5"</code>" or "<code>"Num 5"</code></td><td>"Toggle perspective / orthographic (3D)"</td></tr>
                                    <tr><td><code>"Num 8"</code>", "<code>"Num 2"</code>", "<code>"Num 6"</code>", "<code>"Num 4"</code></td><td>"Top, Front, Right, Left (in 2D: the working plane)"</td></tr>
                                    <tr><td><code>"Ctrl"</code>" + the same"</td><td>"Bottom, Back, Left, Right"</td></tr>
                                    <tr><td><code>"Num ."</code></td><td>"Frame all"</td></tr>
                                    <tr><td><code>"F"</code></td><td>"Frame the selection"</td></tr>
                                </tbody>
                            </table>
                            <p>"All of them can be rebound in Keyboard Shortcuts, under Camera."</p>
                        </div>

                        <div id="controls-properties" class="subsection">
                            <h3>"Camera Properties"</h3>
                            <p>"Select the Camera in the Explorer to edit the view as properties:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Property"</th><th>"Values"</th><th>"Notes"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"ViewMode"</code></td><td>"3D, 2D"</td><td>"2D locks the view to an axis plane"</td></tr>
                                    <tr><td><code>"Projection"</code></td><td>"Perspective, Orthographic"</td><td>"Fixed to Orthographic in 2D"</td></tr>
                                    <tr><td><code>"OrthographicSize"</code></td><td>"meters"</td><td>"Visible height of the orthographic view"</td></tr>
                                    <tr><td><code>"FieldOfView"</code></td><td>"degrees"</td><td>"Vertical, for the perspective lens"</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // AGENTS & FILES
                    // =========================================================
                    <section id="automation" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Agents & Files"
                        </h2>

                        <div id="automation-actions" class="subsection">
                            <h3>"Actions over MCP"</h3>
                            <p>
                                "Every view change is an editor action, so an AI agent drives it the same way a
                                keypress does, through the "<code>"invoke_action"</code>" tool of the "
                                <a href="/learn/mcp">"MCP server"</a>":"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MCP"</span>
                                </div>
                                <pre><code class="language-json">{r#"{ "tool": "invoke_action", "arguments": { "action": "ViewMode2D" } }

// Also: ViewMode3D, ViewPerspectiveToggle,
//       ViewTop, ViewFront, ViewSideLeft, ViewSideRight"#}</code></pre>
                            </div>
                        </div>

                        <div id="automation-memory" class="subsection">
                            <h3>"Per-Space Memory"</h3>
                            <p>
                                "A Space remembers the view it was last worked in, so a 2D project opens in 2D.
                                The choice lives in a small file next to the Space's other editor state, and it
                                changes only when the mode does, never as you pan or zoom:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">".eustress/view.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"# The Perspective this Space opens in. Written by the editor.
mode = "2D"
projection = "Orthographic"
plane = "Front""#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // WHAT'S NEXT
                    // =========================================================
                    <section id="roadmap" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "What's Next"
                        </h2>

                        <div id="roadmap-content" class="subsection">
                            <h3>"Native 2D Content"</h3>
                            <p>
                                "Today, 2D work uses the same parts as 3D, and images are placed as flat quads
                                (the "<code>"Image"</code>" class), which is why they look right in both views.
                                Bevy also ships a dedicated 2D renderer ("<code>"Camera2d"</code>", "
                                <code>"Sprite"</code>", "<code>"Mesh2d"</code>") built for tens of thousands of
                                sprites. Because the 2D view already follows Bevy's 2D convention, that renderer
                                can stack on top of it as a second camera with a matching projection, and
                                sprites will register exactly against parts."
                            </p>
                        </div>

                        <div id="roadmap-runtime" class="subsection">
                            <h3>"2D at Runtime"</h3>
                            <p>
                                "2D is an editor view today. Next come a Play mode that follows the character
                                along the working plane, physics that keeps bodies on that plane, and the same
                                projection settings exposed to scripts, so a 2D experience plays in 2D."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Sketch it flat. Check it in 3D. Never convert a thing."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/building" class="btn-secondary-steel">"Building Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/building" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Building"</span>
                            </div>
                        </a>
                        <a href="/docs/cad" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"CAD"</span>
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
