// =============================================================================
// Eustress Web - Studio Documentation Page
// =============================================================================
// Studio: the editor inside Eustress Engine. Its window and panels, the
// Explorer and its search language, Properties, tools, undo, shortcuts, Modes.
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
            id: "window",
            title: "The Window",
            subsections: vec![
                TocSubsection { id: "window-layout", title: "Layout" },
                TocSubsection { id: "window-panels", title: "Panels and Tabs" },
                TocSubsection { id: "window-presets", title: "Layout Presets" },
            ],
        },
        TocSection {
            id: "explorer",
            title: "Explorer",
            subsections: vec![
                TocSubsection { id: "explorer-tree", title: "The Tree" },
                TocSubsection { id: "explorer-reparent", title: "Reparenting" },
                TocSubsection { id: "explorer-menus", title: "Context Menus" },
                TocSubsection { id: "explorer-search", title: "Search" },
            ],
        },
        TocSection {
            id: "properties",
            title: "Properties and Insert",
            subsections: vec![
                TocSubsection { id: "properties-edit", title: "Editing Properties" },
                TocSubsection { id: "properties-insert", title: "Insert Object" },
                TocSubsection { id: "properties-catalog", title: "Where New Objects Go" },
            ],
        },
        TocSection {
            id: "tools",
            title: "Selection and Tools",
            subsections: vec![
                TocSubsection { id: "tools-select", title: "Selecting" },
                TocSubsection { id: "tools-transform", title: "Move, Scale, Rotate" },
                TocSubsection { id: "tools-snap", title: "Snapping" },
                TocSubsection { id: "tools-smart", title: "Smart Build Tools" },
            ],
        },
        TocSection {
            id: "history",
            title: "Undo and History",
            subsections: vec![
                TocSubsection { id: "history-undo", title: "Undo and Redo" },
                TocSubsection { id: "history-panel", title: "The History Panel" },
                TocSubsection { id: "history-timeline", title: "Timeline" },
            ],
        },
        TocSection {
            id: "keys",
            title: "Shortcuts and Saving",
            subsections: vec![
                TocSubsection { id: "keys-dialog", title: "Keyboard Shortcuts" },
                TocSubsection { id: "keys-defaults", title: "Default Keys" },
                TocSubsection { id: "keys-saving", title: "Saving" },
            ],
        },
        TocSection {
            id: "modes",
            title: "Modes",
            subsections: vec![
                TocSubsection { id: "modes-what", title: "What a Mode Is" },
                TocSubsection { id: "modes-custom", title: "Your Own Mode" },
                TocSubsection { id: "modes-mindspace", title: "MindSpace" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-panels", title: "Panels and Tabs" },
                TocSubsection { id: "roadmap-tools", title: "Tools" },
            ],
        },
    ]
}

/// The Studio window, drawn as its docked regions: the ribbon across the top,
/// Explorer left, the tabbed viewport center, Properties right, Output and the
/// command bar along the bottom.
#[component]
fn StudioLayoutDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 300" role="img"
                aria-label="The Studio window: the ribbon across the top, the Explorer panel on the left, the viewport with its tab bar in the center, the Properties panel on the right, and Output with the command bar along the bottom.">
                <rect x="20" y="12" width="600" height="46" rx="8" class="dg-box"></rect>
                <text x="320" y="33" class="dg-label" text-anchor="middle">"Ribbon"</text>
                <text x="320" y="50" class="dg-note" text-anchor="middle">"Modes and menus, Run and Play, ribbon tabs, tools"</text>

                <rect x="20" y="66" width="132" height="150" rx="8" class="dg-box"></rect>
                <text x="86" y="136" class="dg-label" text-anchor="middle">"Explorer"</text>
                <text x="86" y="154" class="dg-note" text-anchor="middle">"Assets, Universes,"</text>
                <text x="86" y="168" class="dg-note" text-anchor="middle">"Toolbox"</text>

                <rect x="160" y="66" width="320" height="22" rx="4" class="dg-box dg-box-muted"></rect>
                <text x="320" y="81" class="dg-note" text-anchor="middle">"Space tab, script and web tabs, Perspective control"</text>
                <rect x="160" y="94" width="320" height="122" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="320" y="152" class="dg-label" text-anchor="middle">"Viewport"</text>
                <text x="320" y="170" class="dg-note" text-anchor="middle">"layout presets at top left"</text>

                <rect x="488" y="66" width="132" height="150" rx="8" class="dg-box"></rect>
                <text x="554" y="136" class="dg-label" text-anchor="middle">"Properties"</text>
                <text x="554" y="154" class="dg-note" text-anchor="middle">"History, Soul,"</text>
                <text x="554" y="168" class="dg-note" text-anchor="middle">"Workshop"</text>

                <rect x="20" y="224" width="600" height="38" rx="8" class="dg-box"></rect>
                <text x="320" y="240" class="dg-label" text-anchor="middle">"Output"</text>
                <text x="320" y="255" class="dg-note" text-anchor="middle">"Timeline, Data Grid"</text>

                <rect x="20" y="268" width="600" height="22" rx="4" class="dg-box dg-box-violet"></rect>
                <text x="320" y="283" class="dg-note" text-anchor="middle">"Command bar: Luau or Rune"</text>
            </svg>
            <figcaption>
                "Studio's docked regions. The side panels and Output each hold several tabs; the names
                in small type are the other tabs in that region."
            </figcaption>
        </figure>
    }
}

/// Studio documentation page.
#[component]
pub fn DocsStudioPage() -> impl IntoView {
    let active_section = RwSignal::new("window".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-studio"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/monitor.svg" alt="Studio" class="toc-icon" />
                        <h2>"Studio"</h2>
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
                            <span class="current">"Studio"</span>
                        </div>
                        <h1 class="docs-title">"Studio"</h1>
                        <p class="docs-subtitle">
                            "Studio is the editor inside Eustress Engine: the window where you build a Space,
                            select and transform its objects, edit their properties, undo any step, and run
                            the result. This page is the tour of every panel, tool and shortcut in it."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "23 min read"
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
                    // THE WINDOW
                    // =========================================================
                    <section id="window" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"01"</span>
                            "The Window"
                        </h2>

                        <div id="window-layout" class="subsection">
                            <h3>"Layout"</h3>
                            <p>
                                "Studio is one window. The ribbon runs across the top, the 3D viewport fills the
                                middle, a panel sits on each side, and Output with the command bar runs along the
                                bottom. Drag the inner edge of a side panel to resize it (180 to 600 px on the left,
                                200 to 700 px on the right), and drag the top edge of Output to make it taller or
                                shorter."
                            </p>
                            <StudioLayoutDiagram />
                        </div>

                        <div id="window-panels" class="subsection">
                            <h3>"Panels and Tabs"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Region"</th><th>"Tabs"</th><th>"Toggle"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Left"</td><td>"Explorer, Assets, Universes, Toolbox, and Terrain under the chevron"</td><td><code>"Ctrl+1"</code>" (Explorer)"</td></tr>
                                    <tr><td>"Right"</td><td>"Properties, History, Soul, Workshop, and Problems under the chevron"</td><td><code>"Ctrl+2"</code>" (Properties)"</td></tr>
                                    <tr><td>"Bottom"</td><td>"Output, Timeline, Data Grid"</td><td><code>"Ctrl+3"</code>" (Output)"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The strip above the viewport works like a code editor's tab bar. The first tab is
                                the Space itself and is pinned there. Scripts, documents, images, web pages and
                                charts you open get tabs of their own beside it, which you can drag to reorder. The
                                globe button at the far left opens this Learn site in a web tab. Near the right end
                                sits the Perspective control, covered on the "<a href="/docs/perspective">"Perspective"</a>
                                " page."
                            </p>
                            <p>
                                "Output collects every message from Studio and from scripts. Its toolbar filters by
                                level (info, warnings, errors, debug) and by source (Rune, Luau), has a "
                                <em>"Filter..."</em>" box, a "<strong>"Clear"</strong>" button and an auto-scroll
                                switch. Soul lists the Space's scripts with their build status, Workshop is the AI
                                assistant, and Problems lists script diagnostics."
                            </p>
                        </div>

                        <div id="window-presets" class="subsection">
                            <h3>"Layout Presets"</h3>
                            <p>
                                "The selector at the top left of the viewport switches between five layouts in one
                                click. Each sets which panels show, how wide they are, and which tab is in front:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Preset"</th><th>"Shows"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Default"</td><td>"Explorer, Toolbox, Properties and Output"</td></tr>
                                    <tr><td>"Scripting"</td><td>"Explorer, Output and the Soul panel; no Properties"</td></tr>
                                    <tr><td>"Building"</td><td>"Explorer, Toolbox, Assets and Properties; no Output"</td></tr>
                                    <tr><td>"Minimal"</td><td>"Only the viewport"</td></tr>
                                    <tr><td>"Wide Panels"</td><td>"Every panel except Soul, with a 350 px left panel"</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // EXPLORER
                    // =========================================================
                    <section id="explorer" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Explorer"
                        </h2>

                        <div id="explorer-tree" class="subsection">
                            <h3>"The Tree"</h3>
                            <p>
                                "The Explorer is the tree of every instance in the Space. The top level is the
                                Space's services (Workspace, Lighting, SoulService and the rest); objects nest under
                                them the way their folders nest on disk. Click a row to select it, "
                                <code>"Ctrl"</code>"-click to add or remove a row, and "<code>"Shift"</code>"-click
                                to select a range."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Action"</th><th>"How"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Move through rows"</td><td><code>"Up"</code>" / "<code>"Down"</code></td></tr>
                                    <tr><td>"Collapse or expand the row"</td><td><code>"Left"</code>" / "<code>"Right"</code>", or click the arrow"</td></tr>
                                    <tr><td>"Rename"</td><td><code>"F2"</code>", type, "<code>"Enter"</code></td></tr>
                                    <tr><td>"Expand a row, or open a script"</td><td>"Double-click"</td></tr>
                                    <tr><td>"Insert an object inside a row"</td><td>"The plus button that appears at the right of the row on hover"</td></tr>
                                    <tr><td>"Clear the selection"</td><td>"Click the empty space below the tree"</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="explorer-reparent" class="subsection">
                            <h3>"Reparenting"</h3>
                            <p>
                                "Drag a row onto another row to make it a child of that row. A blue pill with the
                                row's name follows the cursor, and the target row lights up. You can drop onto a
                                service too, to move an object into Workspace or ReplicatedStorage. With several
                                rows selected, the whole selection moves."
                            </p>
                            <p>
                                "Because the Explorer mirrors the folders on disk, reparenting moves the object's
                                folder into its new parent's folder. The move is one undo step: "<code>"Ctrl+Z"</code>
                                " puts every moved object back where it came from."
                            </p>
                        </div>

                        <div id="explorer-menus" class="subsection">
                            <h3>"Context Menus"</h3>
                            <p>
                                "Right-click a row for a menu that fits what the row is. Right-clicking a row that is
                                not selected selects it first; right-clicking inside a multi-selection keeps the
                                selection, so the command applies to all of it."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Row"</th><th>"Menu highlights"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Service"</td><td>"Insert Object, Insert Part, Paste Into, Select Children, Select Descendants, Copy Path. Services cannot be cut, deleted or renamed."</td></tr>
                                    <tr><td>"Model or Folder"</td><td>"Insert Object, Insert Part, Paste Into, the selection commands, Zoom To, Group, Ungroup, Rename"</td></tr>
                                    <tr><td>"Part"</td><td>"Insert Object, Paste Into, Invert Selection, Toggle Anchor, Toggle Lock, Zoom To, Group, Rename"</td></tr>
                                    <tr><td>"Script"</td><td>"Open Script, Select Parent, Select Siblings, the clipboard commands, Rename"</td></tr>
                                    <tr><td>"Anything else"</td><td>"Insert Object, Paste Into, the selection and clipboard commands, Zoom To, Group, Rename"</td></tr>
                                </tbody>
                            </table>
                            <p>"Every menu also has Cut, Copy, Paste, Duplicate and Delete where they apply, and Copy Path to copy the object's path on disk."</p>
                        </div>

                        <div id="explorer-search" class="subsection">
                            <h3>"Search"</h3>
                            <p>
                                "The box at the top of the Explorer filters the tree as you type. Press "
                                <code>"Ctrl+Shift+X"</code>" to jump to it from anywhere. A search is a list of terms
                                separated by spaces, and a row must match every term; "<code>"or"</code>" splits the
                                query into alternatives. Matching ignores case. The parents of every match stay
                                visible, so a hit deep in the tree still shows where it lives."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Term"</th><th>"Matches"</th><th>"Example"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"plain text"</td><td>"The row's name, or the text of any TextLabel under it. A second try ignores spaces, so "<em>"the ledger"</em>" finds TheLedger."</td><td><code>"ledger"</code></td></tr>
                                    <tr><td>"a quoted phrase"</td><td>"Keeps words together as one term"</td><td><code>"\"big wall\""</code></td></tr>
                                    <tr><td><code>"is:"</code>"Class"</td><td>"An exact class, or a family of classes"</td><td><code>"is:Model"</code>", "<code>"is:BasePart"</code></td></tr>
                                    <tr><td><code>"tag:"</code>"Name"</td><td>"A CollectionService tag"</td><td><code>"tag:Door"</code></td></tr>
                                    <tr><td>"Property "<code>"="</code>" value"</td><td>"A property value; "<code>"=="</code>" works too, with or without spaces"</td><td><code>"Anchored = false"</code></td></tr>
                                    <tr><td><code>"-"</code>"term"</td><td>"Rows where the term does not match"</td><td><code>"-tag:Door"</code></td></tr>
                                    <tr><td><code>"or"</code></td><td>"Either side may match"</td><td><code>"is:Model or tag:Door"</code></td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The property terms are "<code>"Name"</code>" (part of the name), "
                                <code>"ClassName"</code>" (exact), and, on parts, "<code>"Anchored"</code>", "
                                <code>"Locked"</code>", "<code>"CanCollide"</code>", "<code>"CastShadow"</code>" (true,
                                false, yes, no, 1 or 0), "<code>"Transparency"</code>" and "<code>"Reflectance"</code>
                                " (numbers, to within 0.001) and "<code>"Material"</code>" ("<code>"Material = Neon"</code>
                                "). The families "<code>"is:"</code>" understands:"
                            </p>
                            <ul class="docs-list">
                                <li><code>"BasePart"</code>": Part, MeshPart, UnionOperation, WedgePart, CornerWedgePart, TrussPart, Seat, VehicleSeat, SpawnLocation, CadPart. "<code>"PVInstance"</code>" adds Model."</li>
                                <li><code>"GuiObject"</code>": the GUI leaves, from Frame and TextLabel to WebFrame. "<code>"Gui"</code>" means ScreenGui, BillboardGui and SurfaceGui."</li>
                                <li><code>"Script"</code>": SoulScript, Script, LocalScript and ModuleScript. "<code>"Light"</code>": the four light classes. "<code>"Constraint"</code>": the joint and constraint classes."</li>
                            </ul>
                            <p>
                                "So "<code>"is:Part Anchored = false or tag:Door"</code>" reads as: unanchored Parts,
                                or anything tagged Door."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Search sees the rows the tree has built"</strong>
                                    <p>
                                        "The filter runs over the rows of expanded branches. Objects inside a
                                        collapsed Model are not searched until you expand it; the text of a
                                        collapsed part's own labels is the exception. Expand the branch first when
                                        a search comes up empty."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // PROPERTIES AND INSERT
                    // =========================================================
                    <section id="properties" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Properties and Insert"
                        </h2>

                        <div id="properties-edit" class="subsection">
                            <h3>"Editing Properties"</h3>
                            <p>
                                "Properties edits the selected object. For a part the rows come in sections:
                                Metadata (Name, ClassName, the file's authored Unit), Transform (Position, Rotation,
                                and Scale, which is the part's size), Appearance (Color, BrickColor, Transparency,
                                Reflectance, CastShadow, Material), Physics (Anchored, CanCollide, Locked,
                                Destructible), then Attributes and Parameters. Tags are edited at the top of the
                                panel. Type a value and press "<code>"Enter"</code>"; number fields take plain
                                numbers."
                            </p>
                            <p>
                                "With several objects selected, Properties shows the values of one of them, the
                                primary selection, and an edit spreads like this:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"You edit"</th><th>"What happens to the selection"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Color, Transparency, Reflectance, Anchored, CanCollide, Locked"</td><td>"Every selected part takes the new value, recorded as one History entry"</td></tr>
                                    <tr><td>"Position"</td><td>"The primary goes to the typed position and every other part moves by the same offset, so the group keeps its layout"</td></tr>
                                    <tr><td>"Rotation"</td><td>"The primary takes the typed rotation and every other part turns by the same amount about its own origin"</td></tr>
                                    <tr><td>"Anything else, including Name, Material and Scale"</td><td>"Only the primary changes"</td></tr>
                                </tbody>
                            </table>
                            <ul class="docs-list">
                                <li><strong>"Color"</strong>": the swatch opens an RGB Color popup with R, G and B sliders from 0 to 255, a box per channel, and a Hex field for "<code>"#RRGGBB"</code>"."</li>
                                <li><strong>"BrickColor"</strong>": opens Color Wheels. Choose one of seven named wheels (Aether, Halo, Verdure, Stone, Char, Hex, Umbra), then one of its 127 named cells. The color goes to every selected part and the field shows the cell's name."</li>
                                <li><strong>"Display units"</strong>": the "<em>"Units"</em>" badge at the right of the menu bar shows Position and Scale in another unit and converts what you type back. Its menu lists Meters, Centimeters, Millimeters, Feet and Inches, plus Roblox's own unit for builders coming from there. The Space is always stored in meters, and the display returns to meters at the next launch."</li>
                                <li><strong>"Filter"</strong>": "<code>"Ctrl+Shift+E"</code>" jumps to the "<em>"Filter properties..."</em>" box, which keeps the rows whose name contains what you type. "<code>"Esc"</code>" clears it."</li>
                                <li><strong>"Attributes and tags"</strong>": the plus on the Attributes header opens Add Attribute (a name, one of 17 types, a value). Type a tag in the box at the top and press "<code>"Enter"</code>"."</li>
                            </ul>
                        </div>

                        <div id="properties-insert" class="subsection">
                            <h3>"Insert Object"</h3>
                            <p>
                                "The Insert Object dialog is a search box over every class you can create. Open it
                                with "<code>"Ctrl+I"</code>", with the plus button on an Explorer row, from "
                                <strong>"Insert > Insert Object..."</strong>", or from a row's context menu. Its
                                header names the destination, "<em>"into Workspace"</em>" or "<em>"into"</em>" the
                                selected object."
                            </p>
                            <p>
                                "Type any part of a class name, or of a category such as "<em>"Lighting"</em>" or "
                                <em>"GUI"</em>": the list narrows on every keystroke. "<code>"Up"</code>" and "
                                <code>"Down"</code>" move the highlight, "<code>"Enter"</code>" inserts, "
                                <code>"Esc"</code>" or a click outside closes. The new object is selected and can be
                                undone like any edit."
                            </p>
                        </div>

                        <div id="properties-catalog" class="subsection">
                            <h3>"Where New Objects Go"</h3>
                            <p>
                                "The list is built from the running engine: a class appears when it has both a
                                spawner and a template, so every row creates something. The "<strong>"Insert"</strong>
                                " menu shows the same catalog grouped by category, under a pinned "
                                <strong>"COMMON"</strong>" group: Part (Block), Sphere, Cylinder, Wedge, Model and
                                Folder."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Inside the selection"</strong>": with an object selected, the new object goes inside it."</li>
                                <li><strong>"The COMMON primitives"</strong>" (Part, Sphere, Cylinder, Wedge) otherwise go to Workspace, 10 m in front of the camera, sized from the part catalog: a Block is 4 by 1 by 2 m, a Sphere 2 m across."</li>
                                <li><strong>"Every other class"</strong>" starts from its template and otherwise goes to the service its category belongs to: lights to Lighting, audio to SoundService, GUI to StarterGui, scripts to SoulService, and everything else to Workspace."</li>
                                <li><strong>"Right-click in the viewport"</strong>" for "<em>"Insert Part Here"</em>", which drops a part at the point you clicked."</li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // SELECTION AND TOOLS
                    // =========================================================
                    <section id="tools" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Selection and Tools"
                        </h2>

                        <div id="tools-select" class="subsection">
                            <h3>"Selecting"</h3>
                            <p>
                                "Click a part in the viewport to select it; a selected part gets a blue outline.
                                With the Select tool, whatever a click would pick shows an amber outline as the
                                cursor passes over it. Clicking a part whose parent is a Model selects the Model;
                                hold "<code>"Alt"</code>" to pick the part itself. Locked parts cannot be picked."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Input"</th><th>"Result"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Click"</td><td>"Select just this"</td></tr>
                                    <tr><td><code>"Ctrl"</code>" + click"</td><td>"Add it, or remove it if it was selected"</td></tr>
                                    <tr><td><code>"Shift"</code>" + click"</td><td>"Add it"</td></tr>
                                    <tr><td><code>"Ctrl+Shift"</code>" + click"</td><td>"Remove it"</td></tr>
                                    <tr><td>"Click empty space"</td><td>"Clear the selection"</td></tr>
                                    <tr><td>"Drag from empty space"</td><td>"Box select: every unlocked part whose center lands in the box. Hold "<code>"Shift"</code>" to keep what was selected."</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The hierarchy commands grow or move a selection through the tree: "
                                <strong>"Select Children"</strong>" adds one level down, "<strong>"Select Descendants"</strong>
                                " every level, "<strong>"Select Siblings"</strong>" the rest of each parent's children, "
                                <strong>"Select Parent"</strong>" replaces the selection with the parents, and "
                                <strong>"Invert Selection"</strong>" swaps selected and unselected. They are on the
                                context menus and the keys below."
                            </p>
                            <p>
                                "A right click in the viewport that does not move the mouse opens the viewport menu:
                                Insert Part Here, Paste Here, Focus Selection, Select Children, Duplicate, Rename,
                                Copy, Cut, Toggle Anchor, Toggle Lock, Group, Ungroup, Copy Path and Delete. A right
                                drag looks around instead."
                            </p>
                        </div>

                        <div id="tools-transform" class="subsection">
                            <h3>"Move, Scale, Rotate"</h3>
                            <p>
                                "Pick a tool from the Tools group on the Home tab or with its key. In every tool you
                                can also drag a selected part's body: it slides across whatever is under the cursor
                                and lands flush on it, part or terrain."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"Handles"</th><th>"Drag"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Select ("<code>"Alt+Z"</code>")"</td><td>"None"</td><td>"The body only"</td></tr>
                                    <tr><td>"Move ("<code>"Alt+X"</code>")"</td><td>"Six arrows and three plane squares"</td><td>"An arrow moves along its axis, a square within its plane"</td></tr>
                                    <tr><td>"Scale ("<code>"Alt+C"</code>")"</td><td>"A cube on each face and one in the center"</td><td>"A face cube moves that side and the opposite side stays put; hold "<code>"Ctrl"</code>" to keep the center fixed; the center cube scales evenly"</td></tr>
                                    <tr><td>"Rotate ("<code>"Alt+V"</code>")"</td><td>"Three rings and a center sphere"</td><td>"A ring turns about its axis in 15 degree steps; "<code>"Shift"</code>" for 1 degree, "<code>"Ctrl"</code>" for none"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The handles follow world axes until you press "<code>"Ctrl+L"</code>" or choose "
                                <strong>"Local"</strong>" in the ribbon's Space group; then they follow the selected
                                object's own axes. "<code>"Esc"</code>" cancels a handle drag and puts everything back.
                                Handles always draw on top of the scene, so a handle buried inside the part it
                                controls is still visible and grabbable. Move handles keep one size on screen at any
                                distance, and Rotate rings never shrink below a minimum on-screen size."
                            </p>
                        </div>

                        <div id="tools-snap" class="subsection">
                            <h3>"Snapping"</h3>
                            <p>
                                "The Snap group on the Home tab holds the Snap switch, the "<strong>"Move"</strong>"
                                increment (default 1 m) and the "<strong>"Rotate"</strong>" increment (default 15
                                degrees). The keys "<code>"1"</code>", "<code>"2"</code>" and "<code>"3"</code>" set the
                                Move increment to 1 m or 0.2 m, or turn move snapping off."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Move handles"</strong>" move in whole increments from where the drag began. "<strong>"Scale"</strong>" rounds sizes to the increment, never below 0.1 m."</li>
                                <li><strong>"Body drags"</strong>" land flush on the surface under the cursor, snap to the increment in that surface's own axes, and pull onto a corner within 0.5 m."</li>
                                <li><strong>"Rotate"</strong>" always uses the Rotate increment; the Snap switch does not affect it."</li>
                                <li><strong>"Collisions"</strong>", the last button in the group and off by default, stops a dragged part at other parts instead of letting it pass through."</li>
                                <li><code>"-"</code>" lifts the selection one increment (1 m with snapping off) and "<code>"="</code>" settles it one increment down, or flush onto a surface within that distance. Hold either key to repeat."</li>
                                <li><code>"Ctrl+R"</code>" turns the selection 90 degrees about the vertical axis and "<code>"Ctrl+T"</code>" tilts it 90 degrees about Z."</li>
                            </ul>
                        </div>

                        <div id="tools-smart" class="subsection">
                            <h3>"Smart Build Tools"</h3>
                            <p>
                                "The Drafting tab holds tools that work from picks or from the whole selection.
                                While one is armed, the Tool Options bar at the top left of the viewport shows the
                                step you are on and the tool's options; "<code>"Esc"</code>" or its "
                                <strong>"×"</strong>" button cancels. When a tool commits, a toast offers "
                                <strong>"Undo"</strong>" for 5 seconds."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"Key"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Gap Fill"</td><td><code>"Ctrl+Alt+G"</code></td><td>"Pick two parts, then click once more: fills the gap between them with 2 or 4 anchored wedges ("<em>"Thickness"</em>" 0.2 m by default)"</td></tr>
                                    <tr><td>"Resize"</td><td><code>"Ctrl+Alt+A"</code></td><td>"Pick a source, then a target: the source's facing side stretches to meet the target (Outer Touch, Inner Touch or Rounded Join)"</td></tr>
                                    <tr><td>"Edge"</td><td><code>"Ctrl+Alt+E"</code></td><td>"Pick a source, then a target: moves the source's origin onto the target's origin"</td></tr>
                                    <tr><td>"Swap"</td><td><code>"Ctrl+Alt+P"</code></td><td>"Pick two parts: they trade positions, and rotations too with the advanced option"</td></tr>
                                    <tr><td>"Mirror"</td><td><code>"Ctrl+Alt+M"</code></td><td>"Choose the XY, XZ or YZ plane through the origin and press Apply: mirrored copies of the selection. "<em>"Linked"</em>" copies keep following their source."</td></tr>
                                    <tr><td>"Material Flip"</td><td><code>"Ctrl+Alt+F"</code></td><td>"Rotate or mirror the texture on every selected part, applied at once and not undoable"</td></tr>
                                    <tr><td>"Align X, Y, Z"</td><td>"Ribbon"</td><td>"Line up the centers of two or more parts on one axis"</td></tr>
                                    <tr><td>"Dist X, Y, Z"</td><td>"Ribbon"</td><td>"Space three or more parts evenly along an axis; the two ends stay put"</td></tr>
                                    <tr><td>"Linear, Radial, Grid"</td><td><code>"Ctrl+Alt+L"</code>", "<code>"Ctrl+Alt+R"</code>", "<code>"Ctrl+Alt+K"</code></td><td>"Copy the selection in a row, around an axis, or in a 3D grid of up to 1,000 copies per part; press Apply. Each array is one undo step."</td></tr>
                                    <tr><td>"Path"</td><td><code>"Ctrl+Alt+H"</code></td><td>"Click two or more points, then apply: copies spaced evenly along the path, optionally turned to follow it"</td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Arrays copy the selection at Apply"</strong>
                                    <p>
                                        "Clicks in the viewport still change the selection while a tool is armed, and
                                        the array tools read the selection only when you press Apply. Select what you
                                        want copied, arm the tool, and set its options in the panel without clicking
                                        in the viewport."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // UNDO AND HISTORY
                    // =========================================================
                    <section id="history" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Undo and History"
                        </h2>

                        <div id="history-undo" class="subsection">
                            <h3>"Undo and Redo"</h3>
                            <p>
                                "Every edit that changes the Space becomes one step on a single history: moves,
                                rotations and resizes, property edits, attributes and tags, inserts, deletes,
                                reparenting, grouping and CAD edits. "<code>"Ctrl+Z"</code>" undoes the newest step and "
                                <code>"Ctrl+Y"</code>" (or "<code>"Ctrl+Shift+Z"</code>") redoes it; the History group
                                on the Home tab has the same two buttons. Making a new edit after an undo discards
                                the steps you could have redone."
                            </p>
                            <p>
                                "Delete moves the object's folder into the Space's "<code>".eustress/trash"</code>"
                                folder, and undo moves it back. A few changes happen outside the history and cannot
                                be undone: objects a command-bar script creates, labels removed with the MindSpace "
                                <strong>"Remove"</strong>" button, Material Flip, and clearing terrain."
                            </p>
                        </div>

                        <div id="history-panel" class="subsection">
                            <h3>"The History Panel"</h3>
                            <p>
                                "The History tab in the right panel lists every step with an icon for its kind. A blue
                                bar marks the current step, and steps you have undone are dimmed. Click a row to jump
                                there: Studio undoes or redoes until that step is the newest one applied. Right-click
                                a row for two more commands:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Command"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><strong>"Undo This ... Change"</strong></td><td>"Reverses that one step and removes it from the list; every other step stays applied. This cannot be redone."</td></tr>
                                    <tr><td><strong>"Revert to Here"</strong></td><td>"Undoes every step after that row and keeps the row itself. The undone steps stay in the list, so Redo brings them back until you make a new edit."</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The same steps are published as they happen on the engine's in-process stream, one
                                topic per kind, so tools and agents can follow the edit history without reading the
                                panel:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"history.move"</span>
                                </div>
                                <pre><code class="language-json">{r#"{"seq":42,"kind":"move","topic":"history.move","description":"Move Part","label":null}"#}</code></pre>
                            </div>
                        </div>

                        <div id="history-timeline" class="subsection">
                            <h3>"Timeline"</h3>
                            <p>
                                "The Timeline shares the bottom region with Output: click "<strong>"Timeline"</strong>"
                                in the Output toolbar to switch to it, and "<strong>"Output"</strong>" to switch back.
                                It lays events out along time, in rows by tag. Today it records one kind of
                                event: each time a Smart Build or array tool commits, a yellow diamond keyframe
                                labeled with the tool's name. The session's last 2,048 events are kept in memory."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // SHORTCUTS AND SAVING
                    // =========================================================
                    <section id="keys" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Shortcuts and Saving"
                        </h2>

                        <div id="keys-dialog" class="subsection">
                            <h3>"Keyboard Shortcuts"</h3>
                            <p>
                                "Every shortcut can be rebound. Open "<strong>"File > Keyboard Shortcuts..."</strong>"
                                or the "<strong>"Keys"</strong>" button in the Home tab's Utils group. The dialog lists
                                the actions in groups (File, Edit, Tools, Smart Build Tools, Solid Modelling, Panels,
                                Camera, Snapping and Placement, Simulation, Network) with the chord each one uses."
                            </p>
                            <ol class="numbered-list">
                                <li>"Click a row. Its key cell changes to "<em>"Press a key…"</em>"."</li>
                                <li>"Press the new chord, with any of "<code>"Ctrl"</code>", "<code>"Alt"</code>" and "<code>"Shift"</code>". "<code>"Esc"</code>" cancels."</li>
                                <li>"If another action already owns that chord, the dialog refuses and names the owner, so two actions never share one key."</li>
                            </ol>
                            <p>
                                "The "<strong>"Preset"</strong>" row replaces the whole map in one click. "
                                <strong>"Eustress"</strong>" is the default map and the way back to it after hand
                                edits. "<strong>"Roblox Studio"</strong>" keeps it but moves the chords that differ:
                                the tools go to "<code>"Shift+1"</code>" through "<code>"Shift+4"</code>", "
                                <code>"F8"</code>" runs, "<code>"Shift+F5"</code>" stops, and "<code>"Ctrl+Shift+P"</code>
                                " filters Properties while Publish Space moves to "<code>"Ctrl+Alt+Shift+P"</code>". Once
                                you rebind a key by hand, the dialog shows "<em>"custom (edited by hand)"</em>"."
                            </p>
                            <p>
                                "A few actions also answer to a second chord that the dialog does not show: Redo to "
                                <code>"Ctrl+Shift+Z"</code>" and Select Descendants to "<code>"Ctrl+Shift+D"</code>".
                                Your map is saved to "<code>"~/.eustress_engine/keybindings.ron"</code>"; actions added
                                in later versions get their default keys without touching your changes."
                            </p>
                        </div>

                        <div id="keys-defaults" class="subsection">
                            <h3>"Default Keys"</h3>
                            <p>
                                "The Eustress keymap. Shortcuts never fire while you type in a text field."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Keys"</th><th>"Action"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"Alt+Z"</code>", "<code>"Alt+X"</code>", "<code>"Alt+C"</code>", "<code>"Alt+V"</code></td><td>"Select, Move, Scale, Rotate tool"</td></tr>
                                    <tr><td><code>"Ctrl+L"</code></td><td>"Switch the tools between world and local axes"</td></tr>
                                    <tr><td><code>"Ctrl+Z"</code>", "<code>"Ctrl+Y"</code>" or "<code>"Ctrl+Shift+Z"</code></td><td>"Undo, redo"</td></tr>
                                    <tr><td><code>"Ctrl+C"</code>", "<code>"Ctrl+X"</code>", "<code>"Ctrl+V"</code></td><td>"Copy, cut, paste"</td></tr>
                                    <tr><td><code>"Ctrl+Shift+V"</code></td><td>"Paste into the selected object"</td></tr>
                                    <tr><td><code>"Ctrl+D"</code>", "<code>"Delete"</code></td><td>"Duplicate, delete"</td></tr>
                                    <tr><td><code>"Ctrl+I"</code></td><td>"Insert Object"</td></tr>
                                    <tr><td><code>"Ctrl+G"</code>", "<code>"Ctrl+U"</code></td><td>"Group into a Model, ungroup"</td></tr>
                                    <tr><td><code>"Ctrl+A"</code>", "<code>"Ctrl+Shift+I"</code></td><td>"Select all, invert the selection"</td></tr>
                                    <tr><td><code>"Ctrl+Alt+V"</code>", "<code>"Ctrl+Alt+D"</code></td><td>"Select children, select descendants"</td></tr>
                                    <tr><td><code>"Ctrl+Shift+U"</code>", "<code>"Ctrl+Shift+A"</code></td><td>"Select parent, select siblings"</td></tr>
                                    <tr><td><code>"Alt+A"</code>", "<code>"Alt+L"</code>", "<code>"Alt+Shift+L"</code></td><td>"Anchor, lock, unlock the selection"</td></tr>
                                    <tr><td><code>"1"</code>", "<code>"2"</code>", "<code>"3"</code></td><td>"Snap 1 m, snap 0.2 m, snap off"</td></tr>
                                    <tr><td><code>"-"</code>", "<code>"="</code></td><td>"Lift one grid step, settle onto the surface below"</td></tr>
                                    <tr><td><code>"Ctrl+R"</code>", "<code>"Ctrl+T"</code></td><td>"Rotate 90 degrees about Y, tilt 90 degrees about Z"</td></tr>
                                    <tr><td><code>"F"</code></td><td>"Frame the selection"</td></tr>
                                    <tr><td><code>"Ctrl+1"</code>", "<code>"Ctrl+2"</code>", "<code>"Ctrl+3"</code></td><td>"Show or hide Explorer, Properties, Output"</td></tr>
                                    <tr><td><code>"Ctrl+Shift+X"</code>", "<code>"Ctrl+Shift+E"</code></td><td>"Search the Explorer, filter Properties"</td></tr>
                                    <tr><td><code>"Ctrl+N"</code>", "<code>"Ctrl+O"</code></td><td>"New Space, open a Space"</td></tr>
                                    <tr><td><code>"Ctrl+S"</code>", "<code>"Ctrl+Shift+S"</code></td><td>"Save, Save As"</td></tr>
                                    <tr><td><code>"F7"</code>", "<code>"F5"</code>", "<code>"F6"</code>", "<code>"F8"</code></td><td>"Run, Play, Pause, Stop ("<code>"Esc"</code>" also stops)"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The Smart Build keys are in the next section's table, and the view keys are on the "
                                <a href="/docs/perspective">"Perspective"</a>" page."
                            </p>
                        </div>

                        <div id="keys-saving" class="subsection">
                            <h3>"Saving"</h3>
                            <p>
                                "Studio records edits as you make them in the Space's database, "
                                <code>"world.fjalldb"</code>": a simple part (a built-in shape with no children) keeps
                                its edits there, and a part with children or a custom mesh also has its "
                                <code>"_instance.toml"</code>" rewritten. Saving means taking a snapshot. "
                                <code>"Ctrl+S"</code>" saves the scene the same database-first way, writes any terrain
                                to disk, and commits the Space folder's files to git as "<em>"manual save"</em>" plus
                                the time. An autosave commits every 5 minutes on its own. The database stays out of
                                git, so edits that live only there are not in that history; the "
                                <a href="/docs/universes#history-coverage">"Universes"</a>" page has the full table."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Unsaved"</strong>": the badge in the menu bar appears once a step has been recorded since the last snapshot, manual or automatic. Undoing that step does not clear it; the next snapshot does."</li>
                                <li><strong>"File menu"</strong>": shows "<em>"No snapshot yet"</em>", "<em>"Snapshot 14:32"</em>" or "<em>"Autosaved 14:37"</em>"."</li>
                                <li><strong>"Exit"</strong>": closing the window or pressing "<code>"Alt+F4"</code>" with unsaved edits opens "<em>"Edits since the last snapshot"</em>". "<strong>"Snapshot & Exit"</strong>" saves before closing, and "<strong>"Skip"</strong>" closes without a snapshot. Your edits are kept in the Space either way; the snapshot only adds a git restore point."</li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // MODES
                    // =========================================================
                    <section id="modes" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "Modes"
                        </h2>

                        <div id="modes-what" class="subsection">
                            <h3>"What a Mode Is"</h3>
                            <p>
                                "A Mode sets Studio up for a kind of work. It chooses which of the nine ribbon tabs
                                show, the default layout preset, and optionally an accent color, and it can add tabs
                                of its own and a menu of submodes. Pick one from the dropdown at the very left of the
                                menu bar, before File. A mode with submodes, such as Engineering with its
                                Mechanical, Electrical and other disciplines, expands into its submenu when you
                                click it."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Ribbon tab"</th><th>"What is on it"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Home"</td><td>"File, History, Clipboard, the four transform tools, Snap, World or Local space, Group, Lock and Anchor, Keys"</td></tr>
                                    <tr><td>"Model"</td><td>"Parts, structure, constraints, effects and lights to insert; see "<a href="/docs/building">"Building"</a></td></tr>
                                    <tr><td>"Drafting"</td><td>"Smart Build Tools, Align, Pattern and Boolean"</td></tr>
                                    <tr><td>"Data"</td><td>"Data Platform tools, starting with importing a CSV, JSON or Parquet file as a Dataset and charting it"</td></tr>
                                    <tr><td>"UI"</td><td>"GUI containers, labels, buttons and layouts; see "<a href="/docs/ui">"UI Systems"</a></td></tr>
                                    <tr><td>"Terrain"</td><td>"Generate, sculpt and paint terrain"</td></tr>
                                    <tr><td>"Test"</td><td>"Play and Pause"</td></tr>
                                    <tr><td>"MindSpace"</td><td>"Labels on parts; see below"</td></tr>
                                    <tr><td>"Plugins"</td><td>"Buttons added by Studio plugins; see "<a href="/docs/scripting">"Scripting"</a></td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Twelve modes are built in: Gaming, Student, Business, Legal, AI, Civil, Engineering,
                                Justice, Health, Military, Government and Public Sector. Engineering is the default
                                and shows all nine tabs. The tabs a mode adds for its own discipline are a map of
                                where Studio is going: the twelve manifests name 2,401 distinct tools, and about
                                thirty of them (the insert, CAD, CSG, Data and procurement actions) run code today.
                                The rest are labeled buttons that do nothing yet."
                            </p>
                        </div>

                        <div id="modes-custom" class="subsection">
                            <h3>"Your Own Mode"</h3>
                            <p>
                                "A mode is a TOML file and nothing else. Studio reads the built-in ones plus every "
                                <code>".toml"</code>" file in the "<code>"Eustress/Modes"</code>" folder of your local
                                app data ("<code>"%LOCALAPPDATA%\\Eustress\\Modes"</code>" on Windows), creating the
                                folder if it is missing. A file that does not parse is skipped with a warning; an
                                unknown tab id is dropped."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Eustress/Modes/survey.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[mode]
id = "survey"
name = "Survey"
icon = "bridge"          # a named icon from the built-in list

[ribbon]
# Any of: home, model, cad, data, ui, terrain, test, mindspace, plugins
tabs = ["home", "model", "terrain", "data"]

[layout]
preset = "Building"      # Default | Scripting | Building | Minimal | Wide Panels"#}</code></pre>
                            </div>
                            <p>
                                "A mode can also add "<code>"[[tabs]]"</code>" with sections of tool buttons and "
                                <code>"[[submodes]]"</code>", as the built-in manifests do. A tool button's id is sent
                                through the same dispatch as the ribbon's own buttons, so an id Studio does not know
                                makes a button that does nothing. A "<code>"required-role"</code>" hides a mode from
                                anyone who does not hold that role."
                            </p>
                        </div>

                        <div id="modes-mindspace" class="subsection">
                            <h3>"MindSpace"</h3>
                            <p>
                                "MindSpace uses parts as the nodes of a spatial mind map: each node carries a text
                                label, and a Beam can join two nodes. The MindSpace ribbon tab works on the selected
                                part:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Add"</strong>" attaches a BillboardGui with a TextLabel to the first selected part, sized to the part, and writes it into the part's folder."</li>
                                <li><strong>"Remove"</strong>" deletes every label on the selected part. The label folders are deleted outright, not moved to the trash."</li>
                                <li><code>"F2"</code>" with the cursor over the viewport opens the "<em>"Edit Text"</em>" dialog for the selected part's first label."</li>
                            </ul>
                            <p>
                                "Dragging a labeled part behaves differently from dragging a plain part. Instead of
                                sliding across surfaces, the node stays at a fixed distance from the camera and
                                follows it, so you can fly with "<code>"W A S D"</code>" while carrying it; the mouse
                                wheel changes that distance while you drag. Nodes joined to it by a Beam drift along
                                to keep their distance, like a force-directed graph."
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

                        <div id="roadmap-panels" class="subsection">
                            <h3>"Panels and Tabs"</h3>
                            <ul class="docs-list">
                                <li><strong>"Floating panels"</strong>": the small button at the right of each side panel's tab bar will move that panel into a window of its own. Today it opens an empty placeholder window."</li>
                                <li><strong>"Timeline"</strong>": markers will become clickable, and the watchpoint (orange dot) and breakpoint (red asterisk) markers the panel already draws will fill in as the simulation reports them."</li>
                                <li><strong>"Multi-selection"</strong>": Properties will show how many objects are selected and mark values that differ between them."</li>
                                <li><strong>"MindSpace"</strong>": Connect, Larger and Smaller, Import and Export, and the AI Tools group on the MindSpace tab will get working actions."</li>
                            </ul>
                        </div>

                        <div id="roadmap-tools" class="subsection">
                            <h3>"Tools"</h3>
                            <p>
                                "Several tools are written and waiting for a button or a key. The Measure tool will
                                report distance, angle, area, volume and mass. Selection sets will save and restore
                                named selections per Space. Lasso and paint selection, pivot modes for rotating a
                                group, and smart alignment guides during a Move drag will follow, along with the
                                discipline tools on each mode's own tabs."
                            </p>
                            <div class="future-cta">
                                <p><strong>"A history you can rewind, a Space you can read, and keys you choose."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/building" class="btn-secondary-steel">"Building Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/getting-started" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Getting Started"</span>
                            </div>
                        </a>
                        <a href="/docs/building" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Building"</span>
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
