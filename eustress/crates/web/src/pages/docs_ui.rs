// =============================================================================
// Eustress Web - UI Systems Documentation Page
// =============================================================================
// UI Systems: the interface a player sees inside an experience. ScreenGui
// overlays and BillboardGui labels, their UDim2 layout, their TOML files, and
// how Rune scripts change them during Play.
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
                TocSubsection { id: "overview-what", title: "What In-Experience UI Is" },
                TocSubsection { id: "overview-classes", title: "The GUI Classes" },
            ],
        },
        TocSection {
            id: "layout",
            title: "Layout",
            subsections: vec![
                TocSubsection { id: "layout-udim2", title: "Scale and Offset" },
                TocSubsection { id: "layout-anchor", title: "Anchor Point" },
                TocSubsection { id: "layout-order", title: "Layering and Visibility" },
            ],
        },
        TocSection {
            id: "files",
            title: "GUI Files",
            subsections: vec![
                TocSubsection { id: "files-folders", title: "One Folder per Element" },
                TocSubsection { id: "files-format", title: "The TOML Format" },
                TocSubsection { id: "files-keys", title: "Keys Reference" },
            ],
        },
        TocSection {
            id: "screen",
            title: "Screen UI",
            subsections: vec![
                TocSubsection { id: "screen-build", title: "Building a HUD" },
                TocSubsection { id: "screen-drawing", title: "How It Draws" },
            ],
        },
        TocSection {
            id: "billboards",
            title: "Billboards",
            subsections: vec![
                TocSubsection { id: "billboards-attach", title: "Labels over Parts" },
                TocSubsection { id: "billboards-size", title: "Size in Meters" },
                TocSubsection { id: "billboards-legible", title: "Readable Labels" },
            ],
        },
        TocSection {
            id: "scripts",
            title: "Scripting UI",
            subsections: vec![
                TocSubsection { id: "scripts-rune", title: "Changing Elements from Rune" },
                TocSubsection { id: "scripts-stop", title: "Play and Stop" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-input", title: "Buttons and Text Input" },
                TocSubsection { id: "roadmap-layout", title: "Layouts and Surfaces" },
            ],
        },
    ]
}

/// How a UDim2 position, size and anchor point place a label in its parent.
#[component]
fn UDim2Diagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 240" role="img"
                aria-label="A 240 by 48 pixel label placed at half the parent's width and 24 pixels down, with anchor point one half on X, so it sits centered at the top of the parent.">
                <rect x="40" y="30" width="560" height="180" rx="6" class="dg-box dg-box-muted"></rect>
                <text x="52" y="198" class="dg-note">"parent (the viewport for a top-level element)"</text>
                <line x1="320" y1="30" x2="320" y2="210" class="dg-line-dashed"></line>
                <text x="326" y="200" class="dg-note">"0.5 of the width"</text>
                <rect x="200" y="54" width="240" height="48" rx="6" class="dg-box dg-box-accent"></rect>
                <text x="320" y="84" class="dg-label" text-anchor="middle">"Score: 0"</text>
                <circle cx="320" cy="54" r="4" class="dg-dot"></circle>
                <text x="452" y="62" class="dg-note">"position (0.5, 0, 0, 24)"</text>
                <text x="452" y="80" class="dg-note">"anchor_point (0.5, 0)"</text>
                <text x="452" y="98" class="dg-note">"size (0, 240, 0, 48)"</text>
                <line x1="330" y1="30" x2="330" y2="54" class="dg-line"></line>
                <text x="336" y="46" class="dg-note">"24 px"</text>
            </svg>
            <figcaption>
                "The position picks a point in the parent: half its width, 24 px down. The anchor point
                says which point of the label lands there, here the middle of its top edge, so the label
                stays centered at any window size."
            </figcaption>
        </figure>
    }
}

/// UI Systems documentation page.
#[component]
pub fn DocsUiPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-ui"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/template.svg" alt="UI Systems" class="toc-icon" />
                        <h2>"UI Systems"</h2>
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
                            <span class="current">"UI Systems"</span>
                        </div>
                        <h1 class="docs-title">"UI Systems"</h1>
                        <p class="docs-subtitle">
                            "In-experience UI is what a player sees on the screen and over the world: a
                            ScreenGui overlay of labels, frames and images, and BillboardGui labels that float
                            above parts. Every element is an object in the Space, sized with Roblox-style UDim2
                            values and stored as a TOML file you can read, edit and version."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "12 min read"
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
                            <h3>"What In-Experience UI Is"</h3>
                            <p>
                                "In-experience UI is the interface you build for the people using your Space: a
                                score in the corner, a status panel, a name floating over a machine. It is made of
                                GUI objects that live in the Space alongside parts. This page is about those
                                objects; the panels, ribbon and tabs of the editor itself are covered on the "
                                <a href="/docs/studio">"Studio"</a>" page."
                            </p>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/monitor.svg" alt="Screen UI" />
                                    </div>
                                    <h4>"Screen UI"</h4>
                                    <p>"A ScreenGui in StarterGui draws its elements over the viewport."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/tag.svg" alt="Billboards" />
                                    </div>
                                    <h4>"Billboards"</h4>
                                    <p>"A BillboardGui inside a part draws a camera-facing card above it."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/code.svg" alt="Scripts" />
                                    </div>
                                    <h4>"Scripted"</h4>
                                    <p>"Rune scripts change text, colors and visibility while you play."</p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-classes" class="subsection">
                            <h3>"The GUI Classes"</h3>
                            <p>
                                "The classes carry Roblox's names and properties, so imported places keep their
                                interface. Containers hold other elements; leaves draw content."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Class"</th><th>"Role"</th><th>"Drawn today"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"ScreenGui"</code></td><td>"Root of a screen overlay"</td><td>"Yes, from StarterGui"</td></tr>
                                    <tr><td><code>"BillboardGui"</code></td><td>"Camera-facing card in the world"</td><td>"Yes"</td></tr>
                                    <tr><td><code>"Frame"</code></td><td>"Box with a background, border and rounded corners"</td><td>"Yes"</td></tr>
                                    <tr><td><code>"ScrollingFrame"</code></td><td>"Frame that clips its contents"</td><td>"Drawn; no scrolling input yet"</td></tr>
                                    <tr><td><code>"TextLabel"</code></td><td>"Text"</td><td>"Yes"</td></tr>
                                    <tr><td><code>"TextButton"</code></td><td>"Text with a background, meant to be clicked"</td><td>"Drawn; clicks not delivered yet"</td></tr>
                                    <tr><td><code>"TextBox"</code></td><td>"Text field"</td><td>"Drawn; typing not wired yet"</td></tr>
                                    <tr><td><code>"ImageLabel"</code>", "<code>"ImageButton"</code></td><td>"Image"</td><td>"On screen; a placeholder on billboards"</td></tr>
                                    <tr><td><code>"ViewportFrame"</code></td><td>"A view into a 3D scene"</td><td>"A placeholder box labelled with the class"</td></tr>
                                    <tr><td><code>"SurfaceGui"</code></td><td>"UI on a face of a part"</td><td>"Stored, not drawn yet"</td></tr>
                                    <tr><td><code>"UIListLayout"</code>", "<code>"UIGridLayout"</code>", "<code>"UIPadding"</code>", "<code>"UICorner"</code>", "<code>"UIStroke"</code>" and the other UI modifiers"</td><td>"Layout and decoration settings"</td><td>"Stored, not applied yet"</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // LAYOUT
                    // =========================================================
                    <section id="layout" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Layout"
                        </h2>

                        <div id="layout-udim2" class="subsection">
                            <h3>"Scale and Offset"</h3>
                            <p>
                                "A UDim2 describes a position or size on two axes, each as a scale plus an offset.
                                The scale is a fraction of the parent's size, the offset is a fixed number of
                                pixels, and the two add up. A top-level element measures against the viewport;
                                an element inside a Frame or BillboardGui measures against that container."
                            </p>
                            <div class="equation-card">
                                <div class="equation">"pixels = scale x parent size + offset"</div>
                                <div class="equation-label">"computed separately for X and Y"</div>
                            </div>
                            <p>
                                "In a file a UDim2 is four numbers in the order X scale, X offset, Y scale, Y
                                offset:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"UDim2"</th><th>"As a size"</th><th>"As a position"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"[0, 200, 0, 50]"</code></td><td>"200 x 50 px, whatever the parent"</td><td>"200 px right, 50 px down"</td></tr>
                                    <tr><td><code>"[1, 0, 1, 0]"</code></td><td>"Fills the parent"</td><td>"The parent's bottom-right corner"</td></tr>
                                    <tr><td><code>"[0.5, 0, 0.5, 0]"</code></td><td>"Half the parent each way"</td><td>"The parent's center"</td></tr>
                                    <tr><td><code>"[1, -20, 0, 40]"</code></td><td>"Full width less 20 px, 40 px tall"</td><td>"20 px in from the right edge, 40 px down"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The table form "<code>"{ x = { scale = 1.0, offset = 0.0 }, y = { scale = 0.0, offset = 40.0 } }"</code>
                                " is read too. A two-number "<code>"[w, h]"</code>" value is rejected, so a file
                                using it fails to load instead of guessing."
                            </p>
                        </div>

                        <div id="layout-anchor" class="subsection">
                            <h3>"Anchor Point"</h3>
                            <p>
                                "The anchor point chooses which point of the element sits on its position, as
                                fractions of the element's own size. "<code>"[0, 0]"</code>" (the default) hangs
                                the element from its top-left corner, "<code>"[0.5, 0.5]"</code>" centers it on
                                the position, and "<code>"[1, 1]"</code>" puts its bottom-right corner there."
                            </p>
                            <UDim2Diagram />
                        </div>

                        <div id="layout-order" class="subsection">
                            <h3>"Layering and Visibility"</h3>
                            <ul class="docs-list">
                                <li><strong>"z_index"</strong>" orders elements: a higher value draws on top of a lower one."</li>
                                <li><strong>"visible = false"</strong>" hides that element. On a ScreenGui it hides everything inside; on a Frame it hides only the Frame, not the elements inside it."</li>
                                <li><strong>"Clipping"</strong>": a ScrollingFrame on a billboard cuts off anything inside it that extends past its edges."</li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // GUI FILES
                    // =========================================================
                    <section id="files" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "GUI Files"
                        </h2>

                        <div id="files-folders" class="subsection">
                            <h3>"One Folder per Element"</h3>
                            <p>
                                "Each GUI element is a folder with an "<code>"_instance.toml"</code>" inside it,
                                and nesting the folders nests the elements. The UI tab of the ribbon and the
                                Insert Object dialog ("<code>"Ctrl+I"</code>") create elements this way, inside
                                the ScreenGui, BillboardGui or Frame you have selected, or in "
                                <code>"StarterGui"</code>" when nothing is selected."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Space folder"</span>
                                </div>
                                <pre><code class="language-text">{r#"StarterGui/
  HUD/
    _instance.toml        class_name = "ScreenGui"
    Score/
      _instance.toml      class_name = "TextLabel"
    Panel/
      _instance.toml      class_name = "Frame"
      Status/
        _instance.toml    class_name = "TextLabel""#}</code></pre>
                            </div>
                            <p>
                                "Single files named by class also load, such as "<code>"Score.textlabel.toml"</code>
                                ", "<code>"Panel.frame.toml"</code>" or "<code>"Logo.imagelabel.toml"</code>
                                ", in "<code>"StarterGui"</code>" and in "<code>"Workspace"</code>"."
                            </p>
                        </div>

                        <div id="files-format" class="subsection">
                            <h3>"The TOML Format"</h3>
                            <p>
                                "A GUI file has up to four sections: "<code>"[metadata]"</code>" for the class, "
                                <code>"[instance]"</code>" for the name, "<code>"[gui]"</code>" for layout and
                                appearance, and "<code>"[text]"</code>" for anything that shows text. This label
                                sits centered at the top of the screen:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"StarterGui/HUD/Score/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[metadata]
class_name = "TextLabel"
archivable = true

[instance]
name = "Score"

[gui]
position = [0.5, 0.0, 0.0, 24.0]
size = [0.0, 240.0, 0.0, 48.0]
anchor_point = [0.5, 0.0]
background_color = [20, 24, 32]
background_transparency = 0.2
corner_radius = 8.0
z_index = 10

[text]
text = "Score: 0"
text_color = [255, 255, 255]
font_size = 28.0"#}</code></pre>
                            </div>
                            <p>
                                "Colors take three or four numbers. When every number is a whole number they are
                                read as 0 to 255, otherwise as 0.0 to 1.0, so "<code>"[255, 128, 0]"</code>" and "
                                <code>"[1.0, 0.5, 0.0]"</code>" are the same orange. Keys written in PascalCase,
                                such as "<code>"BackgroundColor"</code>", are read as their snake_case names."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A text element without [instance] name is called _instance"</strong>
                                    <p>
                                        "Text, button, box and image elements take their name from "
                                        <code>"[instance] name"</code>"; frames, ScreenGuis and BillboardGuis take
                                        the folder's name. Scripts find elements by name, so give every element a
                                        unique name and use the same name for its folder."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="files-keys" class="subsection">
                            <h3>"Keys Reference"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Key"</th><th>"Value"</th><th>"Default"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"[gui] position"</code></td><td>"UDim2"</td><td><code>"[0, 0, 0, 0]"</code></td></tr>
                                    <tr><td><code>"[gui] size"</code></td><td>"UDim2"</td><td><code>"[0, 100, 0, 30]"</code></td></tr>
                                    <tr><td><code>"[gui] anchor_point"</code></td><td>"Two fractions"</td><td><code>"[0, 0]"</code></td></tr>
                                    <tr><td><code>"[gui] background_color"</code></td><td>"Color"</td><td>"Dark gray, 80% opaque"</td></tr>
                                    <tr><td><code>"[gui] background_transparency"</code></td><td>"0 opaque to 1 invisible"</td><td>"Taken from the color's alpha"</td></tr>
                                    <tr><td><code>"[gui] border_size"</code>", "<code>"border_color"</code></td><td>"Pixels, color"</td><td>"0, gray"</td></tr>
                                    <tr><td><code>"[gui] corner_radius"</code></td><td>"Pixels"</td><td>"0"</td></tr>
                                    <tr><td><code>"[gui] visible"</code></td><td>"true or false"</td><td>"true"</td></tr>
                                    <tr><td><code>"[gui] z_index"</code></td><td>"Integer"</td><td>"0"</td></tr>
                                    <tr><td><code>"[text] text"</code></td><td>"String"</td><td>"Empty"</td></tr>
                                    <tr><td><code>"[text] text_color"</code>", "<code>"text_transparency"</code></td><td>"Color, 0 to 1"</td><td>"White, 0"</td></tr>
                                    <tr><td><code>"[text] font_size"</code></td><td>"Pixels"</td><td>"14"</td></tr>
                                    <tr><td><code>"[text] font"</code></td><td>"Font name, such as "<code>"GothamBold"</code></td><td>"Default font"</td></tr>
                                    <tr><td><code>"[text] text_x_alignment"</code>", "<code>"text_y_alignment"</code></td><td>"Left, Center, Right; Top, Center, Bottom"</td><td>"Left; Center"</td></tr>
                                    <tr><td><code>"[text] text_scaled"</code></td><td>"Fit the text to the box"</td><td>"false"</td></tr>
                                    <tr><td><code>"[text] text_stroke_color"</code>", "<code>"text_stroke_transparency"</code></td><td>"Outline color, 0 to 1"</td><td>"Black, 1 (no outline)"</td></tr>
                                    <tr><td><code>"[asset] path"</code></td><td>"Image file for an ImageLabel or ImageButton, relative to the folder holding the element's file"</td><td>"None"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "On billboards, a font name containing Bold draws bold and one containing Light or
                                Thin draws light. Changes to a BillboardGui's file apply while the Space is open;
                                the other GUI files are read when the Space opens. Billboard keys are listed in "
                                <a href="#billboards-legible">"Readable Labels"</a>"."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // SCREEN UI
                    // =========================================================
                    <section id="screen" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Screen UI"
                        </h2>

                        <div id="screen-build" class="subsection">
                            <h3>"Building a HUD"</h3>
                            <ol class="numbered-list">
                                <li>"On the ribbon's "<strong>"UI"</strong>" tab, click "<strong>"Screen"</strong>". A ScreenGui appears in "<code>"StarterGui"</code>"."</li>
                                <li>"Select it in the Explorer, then click "<strong>"Text"</strong>", "<strong>"Frame"</strong>", "<strong>"Image"</strong>" or "<strong>"Button"</strong>". Each new element is created inside the selected container."</li>
                                <li>"Style each element in its "<code>"_instance.toml"</code>" using the keys above."</li>
                            </ol>
                            <p>
                                "Screen UI draws in the viewport while you edit and while you play. A ScreenGui
                                belongs in "<code>"StarterGui"</code>", and an element draws on the screen only
                                when it sits inside one. Set "<code>"visible = false"</code>" under "
                                <code>"[gui]"</code>" in the ScreenGui's own file to hide the whole overlay."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A new ScreenGui shows its contents after the Space reopens"</strong>
                                    <p>
                                        "A ScreenGui created during the session starts drawing its elements the
                                        next time the Space is opened; until then they stay hidden. The files are
                                        written the moment you insert them, so close and reopen the Space once
                                        after adding a ScreenGui."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="screen-drawing" class="subsection">
                            <h3>"How It Draws"</h3>
                            <p>
                                "Screen elements are drawn by the same overlay that draws the editor, above the 3D
                                view, measured in the viewport's logical pixels. Billboards use their own
                                rasterizer, and the two support different subsets of the properties:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Feature"</th><th>"Screen overlay"</th><th>"Billboard card"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Background, border, rounded corners"</td><td>"Yes"</td><td>"Yes"</td></tr>
                                    <tr><td>"Text color and size"</td><td>"Yes, at least 8 px"</td><td>"Yes, at least 8 px"</td></tr>
                                    <tr><td><code>"text_scaled"</code></td><td>"No"</td><td>"Yes"</td></tr>
                                    <tr><td>"Font name and weight"</td><td>"No"</td><td>"Yes"</td></tr>
                                    <tr><td>"Text outline"</td><td>"No"</td><td>"Yes"</td></tr>
                                    <tr><td>"Horizontal alignment"</td><td>"Centered unless the value is written lower case, "<code>"left"</code>" or "<code>"right"</code></td><td>"Left, Center, Right"</td></tr>
                                    <tr><td>"Vertical alignment"</td><td>"Centered"</td><td>"Top, Center, Bottom"</td></tr>
                                    <tr><td>"Text too long for its box"</td><td>"Cut off with an ellipsis"</td><td>"Wrapped at words"</td></tr>
                                    <tr><td>"Images"</td><td>"Yes, fitted inside the box"</td><td>"Placeholder box"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "In Studio a click on a screen element also reaches the scene, so the part behind
                                a HUD element can still be selected."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // BILLBOARDS
                    // =========================================================
                    <section id="billboards" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Billboards"
                        </h2>

                        <div id="billboards-attach" class="subsection">
                            <h3>"Labels over Parts"</h3>
                            <p>
                                "A BillboardGui is a card in the world that turns to face the camera. It follows
                                whatever it is placed inside, so keep it in the folder of the part it labels.
                                To add one in Studio, select the part, open Insert Object ("<code>"Ctrl+I"</code>
                                "), choose "<strong>"BillboardGui"</strong>", then select the new BillboardGui and
                                add a "<strong>"Text"</strong>" element from the UI tab."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Workspace/Beacon/Label/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[metadata]
class_name = "BillboardGui"
archivable = true

[gui]
size = [4.0, 0.0, 1.0, 0.0]                 # 4 m x 1 m
units_offset_world_space = [0.0, 2.0, 0.0]  # 2 m above the part's center
always_on_top = true
max_distance = 150.0"#}</code></pre>
                            </div>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Workspace/Beacon/Label/Text/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[metadata]
class_name = "TextLabel"
archivable = true

[instance]
name = "BeaconText"

[gui]
size = [1.0, 0.0, 1.0, 0.0]     # fill the billboard
background_transparency = 1.0

[text]
text = "Beacon"
text_color = [255, 220, 120]
text_scaled = true
font = "GothamBold"
text_stroke_transparency = 0.0"#}</code></pre>
                            </div>
                            <p>
                                "Saving a BillboardGui's file while the Space is open resizes and moves the live
                                billboard. A BillboardGui placed directly in a Folder is not drawn, because a
                                folder has no position to follow."
                            </p>
                        </div>

                        <div id="billboards-size" class="subsection">
                            <h3>"Size in Meters"</h3>
                            <p>
                                "A billboard's size is a UDim2 whose scale is in meters: one meter is 50 pixels of
                                canvas, so a size resolves to scale x 50 + offset pixels, and the world quad is that
                                many pixels divided by 50 meters across. The default, 200 x 50 px, is a card 4 m
                                wide and 1 m tall. Elements inside the billboard lay out against that canvas."
                            </p>
                            <div class="stats-grid">
                                <div class="stat-card">
                                    <div class="stat-value">"50 px"</div>
                                    <div class="stat-label">"Canvas pixels per meter"</div>
                                </div>
                                <div class="stat-card">
                                    <div class="stat-value">"192 px"</div>
                                    <div class="stat-label">"Texture tile per billboard"</div>
                                    <div class="stat-note">"192 x 192, in one shared atlas"</div>
                                </div>
                                <div class="stat-card">
                                    <div class="stat-value">"300 m"</div>
                                    <div class="stat-label">"Drawing radius"</div>
                                    <div class="stat-note">"tiles freed beyond 360 m"</div>
                                </div>
                            </div>
                            <p>
                                "Every billboard is drawn on the CPU into a 192 x 192 px tile of a shared texture.
                                A billboard wider or taller than 3.84 m keeps its full size in the world, but its
                                content is drawn into the same tile and stretched, so large cards look softer than
                                small ones.
                                Billboards farther than 300 m from the camera are not drawn at all."
                            </p>
                        </div>

                        <div id="billboards-legible" class="subsection">
                            <h3>"Readable Labels"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"[gui] key"</th><th>"Effect"</th><th>"Default"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"size"</code></td><td>"Canvas size; scale is in meters"</td><td>"200 x 50 px"</td></tr>
                                    <tr><td><code>"units_offset"</code></td><td>"Offset in the parent's axes, meters; grows with the parent's scale"</td><td><code>"[0, 0, 0]"</code></td></tr>
                                    <tr><td><code>"units_offset_world_space"</code></td><td>"Offset along the world axes, meters, whatever the parent's rotation or scale"</td><td><code>"[0, 0, 0]"</code></td></tr>
                                    <tr><td><code>"always_on_top"</code></td><td>"Draw over all geometry instead of being hidden behind it"</td><td>"false"</td></tr>
                                    <tr><td><code>"z_index"</code></td><td>"Pull the card 0.5 m toward the camera per unit; negative pushes it away"</td><td>"0"</td></tr>
                                    <tr><td><code>"max_distance"</code>", "<code>"distance_upper_limit"</code></td><td>"Hide beyond this many meters; the smaller of the two applies"</td><td>"1000"</td></tr>
                                    <tr><td><code>"distance_lower_limit"</code></td><td>"Hide when the camera is closer than this"</td><td>"0 (off)"</td></tr>
                                    <tr><td><code>"enabled"</code></td><td>"Show or hide the billboard"</td><td>"true"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "With "<code>"text_scaled = true"</code>", a label on a billboard picks the largest
                                font that fits its box, searching from 1 px up to the box's height or 72 px,
                                whichever is larger. Otherwise it uses "<code>"font_size"</code>". An outline ("
                                <code>"text_stroke_transparency = 0.0"</code>") keeps text legible against busy
                                scenes, and a tight "<code>"max_distance"</code>" keeps distant labels from
                                crowding the view."
                            </p>
                            <div class="callout callout-tip">
                                <img src="/assets/icons/sparkles.svg" alt="Tip" />
                                <div>
                                    <strong>"Let the text fill the card"</strong>
                                    <p>
                                        "Size the TextLabel "<code>"[1, 0, 1, 0]"</code>" so it covers the whole
                                        billboard, and put the label at the part's center with "
                                        <code>"always_on_top = true"</code>", or lift it with a small "
                                        <code>"z_index"</code>" when it should still hide behind other parts."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // SCRIPTING UI
                    // =========================================================
                    <section id="scripts" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Scripting UI"
                        </h2>

                        <div id="scripts-rune" class="subsection">
                            <h3>"Changing Elements from Rune"</h3>
                            <p>
                                "Rune scripts change GUI elements by name while you play, for screen elements and
                                billboard contents alike. If two elements share a name, the change goes to one of
                                them."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Function"</th><th>"Changes"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"gui_set_text(name, text)"</code></td><td>"The text"</td></tr>
                                    <tr><td><code>"gui_set_visible(name, visible)"</code></td><td>"Whether the element is drawn"</td></tr>
                                    <tr><td><code>"gui_set_text_color(name, r, g, b, a)"</code></td><td>"Text color, 0 to 1 per channel"</td></tr>
                                    <tr><td><code>"gui_set_bg_color(name, r, g, b, a)"</code></td><td>"Background color"</td></tr>
                                    <tr><td><code>"gui_set_border_color(name, r, g, b, a)"</code></td><td>"Border color"</td></tr>
                                    <tr><td><code>"gui_set_font_size(name, size)"</code></td><td>"Font size in pixels"</td></tr>
                                </tbody>
                            </table>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"SoulService/Hud/Hud.rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::{get_sim_value, set_sim_value, gui_set_text, gui_set_text_color};

pub fn on_init() {
    set_sim_value("hud.time", 0.0);
}

pub fn on_update(dt) {
    let t = get_sim_value("hud.time") + dt;
    set_sim_value("hud.time", t);
    gui_set_text("Score", `Time: ${t}`);
    if t > 30.0 {
        gui_set_text_color("Score", 1.0, 0.3, 0.3, 1.0);
    }
}"#}</code></pre>
                            </div>
                            <p>
                                "The lifecycle of "<code>"on_init"</code>" and "<code>"on_update"</code>" is covered
                                on the "<a href="/docs/scripting">"Scripting"</a>" page."
                            </p>
                        </div>

                        <div id="scripts-stop" class="subsection">
                            <h3>"Play and Stop"</h3>
                            <p>
                                "When Play starts, Studio records how every GUI element looks. When you stop, it
                                puts every element back and drops any change a script queued in the last frame, so
                                a session never leaves its text or colors behind. Script changes affect only what
                                is drawn; the element's file is not touched."
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

                        <div id="roadmap-input" class="subsection">
                            <h3>"Buttons and Text Input"</h3>
                            <p>
                                "Clicking a ScreenGui TextButton during Play will call "
                                <code>"on_button_click(name)"</code>" in every Rune script that defines it, with the
                                button's name, so one handler can serve a whole menu. The dispatcher is already in
                                place; the button test that feeds it will be matched to the element type the
                                loader records. TextBox typing, ScrollingFrame scrolling and ImageButton hover and
                                pressed images will follow."
                            </p>
                        </div>

                        <div id="roadmap-layout" class="subsection">
                            <h3>"Layouts and Surfaces"</h3>
                            <p>
                                "UIListLayout, UIGridLayout, UIPadding, UICorner, UIStroke and the other modifiers
                                already load with their settings; a layout pass will apply them. SurfaceGui will
                                draw onto the face of its part, billboards will show real images and honor "
                                <code>"brightness"</code>" and "<code>"light_influence"</code>", and a billboard's "
                                <code>"adornee"</code>" will attach it to a part named anywhere in the Space."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Lay it out in a file. Drive it from a script. Read it anywhere in the world."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/scripting" class="btn-secondary-steel">"Scripting Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/importing" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Importing"</span>
                            </div>
                        </a>
                        <a href="/docs/audio" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Audio"</span>
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
