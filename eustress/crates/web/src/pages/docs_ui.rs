// =============================================================================
// Eustress Web - UI Systems Documentation Page
// =============================================================================
// Comprehensive UI documentation covering TOML-defined interfaces, Slint
// components, and Rune scripting API for dynamic UI behavior.
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
                TocSubsection { id: "overview-architecture", title: "Architecture" },
                TocSubsection { id: "overview-philosophy", title: "Design Philosophy" },
            ],
        },
        TocSection {
            id: "toml",
            title: "TOML UI Definitions",
            subsections: vec![
                TocSubsection { id: "toml-structure", title: "File Structure" },
                TocSubsection { id: "toml-elements", title: "UI Elements" },
                TocSubsection { id: "toml-properties", title: "Properties" },
                TocSubsection { id: "toml-layouts", title: "Layouts" },
            ],
        },
        TocSection {
            id: "slint",
            title: "Slint Components",
            subsections: vec![
                TocSubsection { id: "slint-basics", title: "Basics" },
                TocSubsection { id: "slint-styling", title: "Styling & Themes" },
                TocSubsection { id: "slint-callbacks", title: "Callbacks" },
                TocSubsection { id: "slint-bindings", title: "Data Bindings" },
            ],
        },
        TocSection {
            id: "rune",
            title: "Rune UI API",
            subsections: vec![
                TocSubsection { id: "rune-reading", title: "Reading UI State" },
                TocSubsection { id: "rune-writing", title: "Writing UI State" },
                TocSubsection { id: "rune-events", title: "Event Handling" },
                TocSubsection { id: "rune-animation", title: "Animations" },
            ],
        },
        TocSection {
            id: "screengui",
            title: "ScreenGui System",
            subsections: vec![
                TocSubsection { id: "screengui-structure", title: "Folder Structure" },
                TocSubsection { id: "screengui-elements", title: "Element Types" },
                TocSubsection { id: "screengui-hotreload", title: "Hot Reload" },
            ],
        },
        TocSection {
            id: "examples",
            title: "Examples",
            subsections: vec![
                TocSubsection { id: "examples-hud", title: "Battery HUD" },
                TocSubsection { id: "examples-dashboard", title: "Data Dashboard" },
                TocSubsection { id: "examples-menu", title: "Interactive Menu" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// UI Systems documentation page with floating TOC.
#[component]
pub fn DocsUiPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-ui"></div>
            </div>

            <div class="docs-layout">
                // Floating TOC Sidebar
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/template.svg" alt="UI" class="toc-icon" />
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

                // Main Content
                <main class="docs-content">
                    // Hero
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"UI Systems"</span>
                        </div>
                        <h1 class="docs-title">"UI Systems"</h1>
                        <p class="docs-subtitle">
                            "Build dynamic, data-driven user interfaces with TOML definitions, 
                            Slint components, and Rune scripting. File-system-first, hot-reloadable, 
                            and infinitely customizable."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "25 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Beginner to Advanced"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "v0.16.1"
                            </span>
                        </div>
                    </header>

                    // ─────────────────────────────────────────────────────
                    // Overview
                    // ─────────────────────────────────────────────────────
                    <section id="overview" class="docs-section">
                        <h2 class="section-anchor">"Overview"</h2>

                        <div id="overview-intro" class="docs-block">
                            <h3>"Introduction"</h3>
                            <p>
                                "Eustress Engine's UI system is built on three pillars: "
                                <strong>"TOML definitions"</strong>" for declarative layout, "
                                <strong>"Slint"</strong>" for native rendering, and "
                                <strong>"Rune scripts"</strong>" for dynamic behavior. This combination 
                                gives you the best of all worlds — human-readable configuration files, 
                                GPU-accelerated rendering, and powerful scripting."
                            </p>
                            <div class="docs-callout info">
                                <strong>"File-System-First:"</strong>
                                " Every UI element is defined in plain TOML files on disk. Edit with 
                                any text editor, version control with Git, and see changes instantly 
                                with hot-reload."
                            </div>
                        </div>

                        <div id="overview-architecture" class="docs-block">
                            <h3>"Architecture"</h3>
                            <pre class="code-block"><code>{"StarterGui/                    ← UI root folder
├── BatteryHUD/                 ← ScreenGui folder
│   ├── _instance.toml          ← ScreenGui metadata
│   ├── Panel.frame.toml        ← Frame element
│   ├── Title.textlabel.toml    ← TextLabel element
│   ├── VoltageLabel.textlabel.toml
│   ├── SOCBar.frame.toml
│   └── scripts/
│       └── battery_hud.rune    ← Dynamic behavior
├── MainMenu/
│   ├── _instance.toml
│   └── ..."}</code></pre>
                            <p>
                                "The engine scans "<code>"StarterGui/"</code>" on startup and hot-reloads 
                                any changes. Each subfolder becomes a ScreenGui, and each "<code>".toml"</code>" 
                                file becomes a UI element."
                            </p>
                        </div>

                        <div id="overview-philosophy" class="docs-block">
                            <h3>"Design Philosophy"</h3>
                            <ul class="docs-list">
                                <li><strong>"Declarative"</strong>" — Define what you want, not how to build it"</li>
                                <li><strong>"Composable"</strong>" — Nest elements to create complex layouts"</li>
                                <li><strong>"Reactive"</strong>" — Rune scripts respond to simulation data in real-time"</li>
                                <li><strong>"Portable"</strong>" — Same UI works on desktop, mobile, and VR"</li>
                                <li><strong>"Debuggable"</strong>" — Inspect any element, see its TOML source"</li>
                            </ul>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // TOML UI Definitions
                    // ─────────────────────────────────────────────────────
                    <section id="toml" class="docs-section">
                        <h2 class="section-anchor">"TOML UI Definitions"</h2>

                        <div id="toml-structure" class="docs-block">
                            <h3>"File Structure"</h3>
                            <p>
                                "UI elements are defined in TOML files with specific naming conventions. 
                                The file extension determines the element type:"
                            </p>
                            <ul class="docs-list">
                                <li><code>".frame.toml"</code>" — Container frame (like a div)"</li>
                                <li><code>".textlabel.toml"</code>" — Static or dynamic text"</li>
                                <li><code>".textbutton.toml"</code>" — Clickable button with text"</li>
                                <li><code>".imagebutton.toml"</code>" — Clickable button with image"</li>
                                <li><code>".imagelabel.toml"</code>" — Static image display"</li>
                                <li><code>".textbox.toml"</code>" — Text input field"</li>
                                <li><code>"_instance.toml"</code>" — ScreenGui container metadata"</li>
                            </ul>
                        </div>

                        <div id="toml-elements" class="docs-block">
                            <h3>"UI Elements"</h3>
                            <p>"Here's a complete TextLabel definition:"</p>
                            <pre class="code-block"><code>{"# VoltageLabel.textlabel.toml
[element]
class = \"TextLabel\"
name = \"VoltageLabel\"
parent = \"Panel\"

[position]
x = { scale = 0.05, offset = 0 }
y = { scale = 0.15, offset = 0 }

[size]
x = { scale = 0.9, offset = 0 }
y = { scale = 0.0, offset = 24 }

[style]
text = \"Voltage: 0.00 V\"
text_color = [0.9, 0.9, 0.9, 1.0]
text_size = 16
font = \"RobotoMono\"
text_x_alignment = \"Left\"
background_transparency = 1.0"}</code></pre>
                        </div>

                        <div id="toml-properties" class="docs-block">
                            <h3>"Properties"</h3>
                            <p>"Common properties available on all UI elements:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Property"</th>
                                        <th>"Type"</th>
                                        <th>"Description"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td><code>"position"</code></td>
                                        <td>"UDim2"</td>
                                        <td>"Position relative to parent (scale + offset)"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"size"</code></td>
                                        <td>"UDim2"</td>
                                        <td>"Size relative to parent (scale + offset)"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"anchor_point"</code></td>
                                        <td>"[f32; 2]"</td>
                                        <td>"Origin point for positioning (0-1)"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"visible"</code></td>
                                        <td>"bool"</td>
                                        <td>"Whether element is rendered"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"z_index"</code></td>
                                        <td>"i32"</td>
                                        <td>"Stacking order (higher = on top)"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"background_color"</code></td>
                                        <td>"[f32; 4]"</td>
                                        <td>"RGBA background color"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"background_transparency"</code></td>
                                        <td>"f32"</td>
                                        <td>"0 = opaque, 1 = transparent"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="toml-layouts" class="docs-block">
                            <h3>"Layouts"</h3>
                            <p>
                                "Use UIListLayout or UIGridLayout for automatic arrangement:"
                            </p>
                            <pre class="code-block"><code>{"# ButtonContainer.frame.toml
[element]
class = \"Frame\"
name = \"ButtonContainer\"

[layout]
type = \"UIListLayout\"
direction = \"Horizontal\"
padding = 8
horizontal_alignment = \"Center\""}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Slint Components
                    // ─────────────────────────────────────────────────────
                    <section id="slint" class="docs-section">
                        <h2 class="section-anchor">"Slint Components"</h2>

                        <div id="slint-basics" class="docs-block">
                            <h3>"Basics"</h3>
                            <p>
                                "Under the hood, Eustress uses Slint for GPU-accelerated UI rendering. 
                                While most users work with TOML files, you can also write custom Slint 
                                components for advanced use cases."
                            </p>
                            <pre class="code-block"><code>{"// custom_widget.slint
component CustomGauge inherits Rectangle {
    in property <float> value: 0.5;
    in property <string> label: \"Gauge\";
    
    background: #1a1a1a;
    border-radius: 8px;
    
    Text {
        text: label;
        color: #d4d4d4;
        font-size: 14px;
    }
    
    Rectangle {
        width: parent.width * value;
        height: 4px;
        background: #0078d4;
        border-radius: 2px;
    }
}"}</code></pre>
                        </div>

                        <div id="slint-styling" class="docs-block">
                            <h3>"Styling & Themes"</h3>
                            <p>
                                "Eustress provides a built-in theme system with CSS-like variables:"
                            </p>
                            <pre class="code-block"><code>{"// Access theme colors
background: Theme.panel-background;  // #1a1a1a
color: Theme.text-primary;           // #d4d4d4
border-color: Theme.accent;          // #0078d4"}</code></pre>
                        </div>

                        <div id="slint-callbacks" class="docs-block">
                            <h3>"Callbacks"</h3>
                            <p>
                                "Slint callbacks connect UI events to Rust/Rune handlers:"
                            </p>
                            <pre class="code-block"><code>{"// In Slint
callback on-click();
callback on-value-changed(float);

// In Rust (engine side)
ui.on_click(|| {
    info!(\"Button clicked!\");
});

// In Rune (script side)
pub fn on_button_click() {
    log(\"Button clicked from Rune!\");
}"}</code></pre>
                        </div>

                        <div id="slint-bindings" class="docs-block">
                            <h3>"Data Bindings"</h3>
                            <p>
                                "Bind UI properties to simulation data for real-time updates:"
                            </p>
                            <pre class="code-block"><code>{"// Two-way binding between Slint and ECS
in-out property <float> voltage <=> simulation.battery.voltage;
in-out property <float> soc <=> simulation.battery.soc;
in-out property <string> status <=> simulation.battery.status;"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Rune UI API
                    // ─────────────────────────────────────────────────────
                    <section id="rune" class="docs-section">
                        <h2 class="section-anchor">"Rune UI API"</h2>

                        <div id="rune-reading" class="docs-block">
                            <h3>"Reading UI State"</h3>
                            <p>
                                "Access UI element properties from Rune scripts:"
                            </p>
                            <pre class="code-block"><code>{"// Get text from a TextLabel
let voltage_text = ui.get_text(\"VoltageLabel\");

// Get numeric property
let bar_width = ui.get_property(\"SOCBar\", \"size\");

// Check visibility
let is_visible = ui.get_property(\"WarningPanel\", \"visible\");

// Get position
let pos = ui.get_property(\"Cursor\", \"position\");"}</code></pre>
                        </div>

                        <div id="rune-writing" class="docs-block">
                            <h3>"Writing UI State"</h3>
                            <p>
                                "Update UI elements dynamically based on simulation data:"
                            </p>
                            <pre class="code-block"><code>{"// Update text
ui.set_text(\"VoltageLabel\", \"Voltage: \" + voltage.round(2) + \" V\");

// Update size (for progress bars)
ui.set_property(\"SOCBarFill\", \"size\", [soc / 100.0, 1.0]);

// Update color based on state
if temperature > 50.0 {
    ui.set_property(\"TempLabel\", \"text_color\", [1.0, 0.3, 0.3, 1.0]);
} else {
    ui.set_property(\"TempLabel\", \"text_color\", [0.3, 1.0, 0.3, 1.0]);
}

// Show/hide elements
ui.set_property(\"WarningPanel\", \"visible\", dendrite_risk > 0.5);"}</code></pre>
                        </div>

                        <div id="rune-events" class="docs-block">
                            <h3>"Event Handling"</h3>
                            <p>
                                "Respond to user interactions:"
                            </p>
                            <pre class="code-block"><code>{"// Button click handlers
pub fn on_charge_click() {
    sim.set_mode(\"charging\");
    ui.set_text(\"StatusText\", \"CHARGING\");
    ui.set_property(\"StatusIndicator\", \"background_color\", [0.2, 0.8, 0.2, 1.0]);
}

pub fn on_stop_click() {
    sim.pause();
    ui.set_text(\"StatusText\", \"STOPPED\");
}

// Slider change handler
pub fn on_speed_changed(value) {
    sim.set_time_scale(value * 1000000.0);
    ui.set_text(\"SpeedLabel\", \"Speed: \" + value.round(1) + \"x\");
}"}</code></pre>
                        </div>

                        <div id="rune-animation" class="docs-block">
                            <h3>"Animations"</h3>
                            <p>
                                "Create smooth UI animations with tweening:"
                            </p>
                            <pre class="code-block"><code>{"// Animate a property over time
ui.tween(\"Panel\", \"position\", [0.5, 0.5], 0.3, \"ease_out\");

// Pulse effect for warnings
pub fn pulse_warning() {
    ui.tween(\"WarningIcon\", \"transparency\", 0.0, 0.2, \"linear\");
    ui.tween(\"WarningIcon\", \"transparency\", 0.5, 0.2, \"linear\");
}

// Slide in from side
ui.set_property(\"Notification\", \"position\", [1.2, 0.1]);
ui.tween(\"Notification\", \"position\", [0.9, 0.1], 0.4, \"ease_out_back\");"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // ScreenGui System
                    // ─────────────────────────────────────────────────────
                    <section id="screengui" class="docs-section">
                        <h2 class="section-anchor">"ScreenGui System"</h2>

                        <div id="screengui-structure" class="docs-block">
                            <h3>"Folder Structure"</h3>
                            <p>
                                "Each ScreenGui is a folder in "<code>"StarterGui/"</code>":"
                            </p>
                            <pre class="code-block"><code>{"StarterGui/
├── BatteryHUD/                 ← ScreenGui: always visible
│   ├── _instance.toml          ← display_order = 10
│   └── ...
├── MainMenu/                   ← ScreenGui: shown on startup
│   ├── _instance.toml          ← enabled = true, reset_on_spawn = false
│   └── ...
├── PauseMenu/                  ← ScreenGui: toggled by script
│   ├── _instance.toml          ← enabled = false (hidden by default)
│   └── ...
└── DebugOverlay/               ← ScreenGui: dev-only
    ├── _instance.toml          ← enabled = false
    └── ..."}</code></pre>
                        </div>

                        <div id="screengui-elements" class="docs-block">
                            <h3>"Element Types"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Element"</th>
                                        <th>"Extension"</th>
                                        <th>"Use Case"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"Frame"</td>
                                        <td><code>".frame.toml"</code></td>
                                        <td>"Container, panel, background"</td>
                                    </tr>
                                    <tr>
                                        <td>"TextLabel"</td>
                                        <td><code>".textlabel.toml"</code></td>
                                        <td>"Static or dynamic text display"</td>
                                    </tr>
                                    <tr>
                                        <td>"TextButton"</td>
                                        <td><code>".textbutton.toml"</code></td>
                                        <td>"Clickable button with text"</td>
                                    </tr>
                                    <tr>
                                        <td>"ImageLabel"</td>
                                        <td><code>".imagelabel.toml"</code></td>
                                        <td>"Icon, logo, decoration"</td>
                                    </tr>
                                    <tr>
                                        <td>"ImageButton"</td>
                                        <td><code>".imagebutton.toml"</code></td>
                                        <td>"Icon button"</td>
                                    </tr>
                                    <tr>
                                        <td>"TextBox"</td>
                                        <td><code>".textbox.toml"</code></td>
                                        <td>"Text input field"</td>
                                    </tr>
                                    <tr>
                                        <td>"ScrollingFrame"</td>
                                        <td><code>".scrollingframe.toml"</code></td>
                                        <td>"Scrollable container"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="screengui-hotreload" class="docs-block">
                            <h3>"Hot Reload"</h3>
                            <p>
                                "UI files hot-reload automatically when saved. The engine watches 
                                "<code>"StarterGui/"</code>" and updates the UI in real-time:"
                            </p>
                            <ol class="docs-list numbered">
                                <li>"Edit any "<code>".toml"</code>" file in your editor"</li>
                                <li>"Save the file (Ctrl+S)"</li>
                                <li>"UI updates instantly — no restart needed"</li>
                                <li>"Rune scripts also hot-reload on save"</li>
                            </ol>
                            <div class="docs-callout success">
                                <strong>"Pro Tip:"</strong>
                                " Use split-screen with your editor and Eustress Studio to see 
                                changes in real-time as you type."
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Examples
                    // ─────────────────────────────────────────────────────
                    <section id="examples" class="docs-section">
                        <h2 class="section-anchor">"Examples"</h2>

                        <div id="examples-hud" class="docs-block">
                            <h3>"Battery HUD"</h3>
                            <p>
                                "A complete example showing real-time battery simulation data:"
                            </p>
                            <pre class="code-block"><code>{"// battery_hud.rune
// Updates UI every tick with simulation data

pub fn on_tick() {
    // Get simulation values
    let voltage = ecs.get_sim(\"battery.voltage\");
    let soc = ecs.get_sim(\"battery.soc\") * 100.0;
    let temp = ecs.get_sim(\"battery.temperature_c\");
    let power = ecs.get_sim(\"battery.power\");
    
    // Update text labels
    ui.set_text(\"VoltageLabel\", \"Voltage: \" + voltage.round(2) + \" V\");
    ui.set_text(\"SOCLabel\", \"SOC: \" + soc.round(1) + \"%\");
    ui.set_text(\"TempLabel\", \"Temp: \" + temp.round(1) + \" °C\");
    ui.set_text(\"PowerLabel\", \"Power: \" + power.round(0) + \" W\");
    
    // Update SOC bar width
    ui.set_property(\"SOCBarFill\", \"size\", [soc / 100.0, 1.0]);
    
    // Color-code temperature
    if temp > 50.0 {
        ui.set_property(\"TempLabel\", \"text_color\", [1.0, 0.3, 0.3, 1.0]);
    } else if temp > 40.0 {
        ui.set_property(\"TempLabel\", \"text_color\", [1.0, 0.8, 0.3, 1.0]);
    } else {
        ui.set_property(\"TempLabel\", \"text_color\", [0.3, 1.0, 0.3, 1.0]);
    }
}"}</code></pre>
                        </div>

                        <div id="examples-dashboard" class="docs-block">
                            <h3>"Data Dashboard"</h3>
                            <p>
                                "Multi-panel dashboard with graphs and controls:"
                            </p>
                            <pre class="code-block"><code>{"// dashboard.rune
// Complex dashboard with multiple data sources

let history = [];
let max_history = 100;

pub fn on_tick() {
    // Record history for graphing
    let value = ecs.get_sim(\"sensor.value\");
    history.push(value);
    if history.len() > max_history {
        history.remove(0);
    }
    
    // Update graph (custom component)
    ui.set_property(\"Graph\", \"data\", history);
    
    // Update stats
    let min = history.min();
    let max = history.max();
    let avg = history.sum() / history.len();
    
    ui.set_text(\"MinLabel\", \"Min: \" + min.round(2));
    ui.set_text(\"MaxLabel\", \"Max: \" + max.round(2));
    ui.set_text(\"AvgLabel\", \"Avg: \" + avg.round(2));
}"}</code></pre>
                        </div>

                        <div id="examples-menu" class="docs-block">
                            <h3>"Interactive Menu"</h3>
                            <p>
                                "Pause menu with navigation and settings:"
                            </p>
                            <pre class="code-block"><code>{"// pause_menu.rune
// Toggle pause menu with Escape key

let menu_visible = false;

pub fn on_escape_pressed() {
    menu_visible = !menu_visible;
    ui.set_property(\"PauseMenu\", \"visible\", menu_visible);
    
    if menu_visible {
        sim.pause();
        ui.tween(\"PauseMenu\", \"transparency\", 0.0, 0.2, \"ease_out\");
    } else {
        sim.resume();
    }
}

pub fn on_resume_click() {
    menu_visible = false;
    ui.set_property(\"PauseMenu\", \"visible\", false);
    sim.resume();
}

pub fn on_settings_click() {
    ui.set_property(\"SettingsPanel\", \"visible\", true);
    ui.tween(\"SettingsPanel\", \"position\", [0.5, 0.5], 0.3, \"ease_out_back\");
}

pub fn on_quit_click() {
    // Save and exit
    sim.stop_recording();
    sim.export(\"recordings/session.json\");
    app.quit();
}"}</code></pre>
                        </div>
                    </section>

                    // Navigation footer
                    <nav class="docs-nav-footer">
                        <a href="/docs/building" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Building"</span>
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
