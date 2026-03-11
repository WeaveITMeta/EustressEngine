# Slint UI System Architecture

This document defines how Slint powers the entire UI system in Eustress Engine — both the **Studio editor** and **user-created runtime UI** (ScreenGui, Frame, TextLabel, etc.).

---

## Table of Contents

1. [Overview](#overview)
2. [Two UI Domains](#two-ui-domains)
3. [Studio UI (Editor)](#studio-ui-editor)
4. [Runtime UI (User-Created)](#runtime-ui-user-created)
5. [TOML ↔ Slint Mapping](#toml--slint-mapping)
6. [Explorer Integration](#explorer-integration)
7. [Properties Panel Integration](#properties-panel-integration)
8. [Icon System](#icon-system)
9. [Live Editing Workflow](#live-editing-workflow)
10. [Implementation Architecture](#implementation-architecture)
11. [Implementation Checklist](#implementation-checklist)

---

## Overview

Slint is used for **all UI** in Eustress Engine:

| Domain | Purpose | Renderer |
|--------|---------|----------|
| **Studio UI** | Editor panels, ribbon, explorer, properties | Software renderer → Bevy texture overlay |
| **Runtime UI** | User-created game UI (ScreenGui, Frame, etc.) | Software renderer → Bevy texture overlay |

Both use the same Slint infrastructure but with different data sources:
- **Studio UI**: Hardcoded `.slint` files compiled into the engine
- **Runtime UI**: Dynamically generated from TOML instance files at runtime

---

## Two UI Domains

### Studio UI (Already Implemented)

```
eustress/crates/engine/ui/slint/
├── main.slint              ← StudioWindow root
├── ribbon.slint            ← Toolbar ribbon
├── explorer.slint          ← Entity tree
├── properties.slint        ← Property editor
├── toolbox.slint           ← Part insertion tools
├── theme.slint             ← Colors, fonts, spacing
└── ... (30+ components)
```

This is the **editor chrome** — panels, dialogs, menus. It's compiled into the engine binary.

### Runtime UI (To Be Implemented)

```
MyGame/StarterGui/
├── _service.toml
└── HUD/
    ├── _instance.toml      ← class_name = "ScreenGui"
    ├── HealthBar.frame.toml
    ├── Minimap.frame.toml
    └── Inventory/
        ├── _instance.toml  ← class_name = "Frame"
        ├── Slot1.imagebutton.toml
        └── Slot2.imagebutton.toml
```

This is **user-created game UI** — defined in TOML, rendered by Slint at runtime.

---

## Studio UI (Editor)

### Current Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Bevy Window                              │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    3D Viewport (Bevy)                     │  │
│  │                                                           │  │
│  │  ┌──────────────────────────────────────────────────────┐ │  │
│  │  │              Slint Overlay (Transparent)             │ │  │
│  │  │  ┌─────────┐ ┌────────────────────┐ ┌──────────────┐ │ │  │
│  │  │  │Explorer │ │     Viewport       │ │  Properties  │ │ │  │
│  │  │  │  Panel  │ │   (transparent)    │ │    Panel     │ │ │  │
│  │  │  └─────────┘ └────────────────────┘ └──────────────┘ │ │  │
│  │  │  ┌──────────────────────────────────────────────────┐│ │  │
│  │  │  │                  Output Console                   │ │  │  
│  │  │  └──────────────────────────────────────────────────┘│ │  │
│  │  └──────────────────────────────────────────────────────┘ │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### Key Components

| Component | File | Purpose |
|-----------|------|---------|
| `StudioWindow` | `main.slint` | Root window, layout manager |
| `ExplorerPanel` | `explorer.slint` | Entity tree with search |
| `PropertiesPanel` | `properties.slint` | Dynamic property editor |
| `Ribbon` | `ribbon.slint` | Toolbar with tabs |
| `Toolbox` | `toolbox.slint` | Part insertion grid |
| `OutputConsole` | `output.slint` | Log viewer |
| `CommandBar` | `command_bar.slint` | Ctrl+P command palette |
| `Theme` | `theme.slint` | Global colors/fonts |

### Data Flow

```
Bevy ECS World
      │
      ▼
Rust: sync_explorer_to_slint()
      │
      ▼
Slint: StudioWindow.tree-nodes = [TreeNode, ...]
      │
      ▼
Slint renders ExplorerPanel
      │
      ▼
User clicks node
      │
      ▼
Slint: on-select-node(id, type) callback
      │
      ▼
Rust: handle_explorer_selection() → update Bevy selection
```

---

## Runtime UI (User-Created)

### The Challenge

User-created UI is defined in TOML files, not `.slint` files. We need to:
1. Parse TOML → build Slint component tree dynamically
2. Render the UI in the game viewport
3. Support hot-reload when TOML changes
4. Handle input events and route to Soul scripts

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Game Window                              │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    3D World (Bevy)                        │  │
│  │                                                           │  │
│  │  ┌──────────────────────────────────────────────────────┐ │  │
│  │  │           Runtime UI Overlay (Slint)                 │ │  │
│  │  │  ┌────────────────────────────────────────────────┐  │ │  │
│  │  │  │  ScreenGui: HUD                                │  │ │  │
│  │  │  │  ┌─────────────┐  ┌──────────────────────────┐ │  │ │  │
│  │  │  │  │ HealthBar   │  │        Minimap           │ │  │ │  │
│  │  │  │  │ ██████░░░░  │  │     [map image]          │ │  │ │  │
│  │  │  │  └─────────────┘  └──────────────────────────┘ │  │ │  │
│  │  │  └────────────────────────────────────────────────┘  │ │  │
│  │  └──────────────────────────────────────────────────────┘ │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### Dynamic Slint Generation

Since Slint is compiled, we can't generate `.slint` files at runtime. Instead, we use **Slint's interpreter mode** or **pre-built generic components**:

#### Option A: Slint Interpreter (Recommended)

Slint has an interpreter that can load `.slint` files at runtime:

```rust
use slint_interpreter::{ComponentCompiler, ComponentInstance};

fn load_runtime_ui(toml_path: &Path) -> Result<ComponentInstance> {
    // Generate .slint source from TOML
    let slint_source = generate_slint_from_toml(toml_path)?;
    
    // Compile at runtime
    let compiler = ComponentCompiler::default();
    let definition = compiler.build_from_source(slint_source, PathBuf::new()).await?;
    
    // Instantiate
    let instance = definition.create()?;
    Ok(instance)
}
```

#### Option B: Pre-built Generic Components

Define generic Slint components that accept data:

```slint
// runtime_ui.slint
export component RuntimeFrame inherits Rectangle {
    in property <color> bg-color;
    in property <length> border-radius;
    in property <[RuntimeElement]> children;
    
    background: bg-color;
    border-radius: border-radius;
    
    for child in children: RuntimeElement {
        // Recursive rendering
    }
}

export component RuntimeTextLabel inherits Text {
    in property <string> content;
    in property <color> text-color;
    in property <length> font-size;
    
    text: content;
    color: text-color;
    font-size: font-size;
}
```

Then populate from Rust:

```rust
fn sync_runtime_ui(ui: &RuntimeUI, toml_data: &ScreenGuiData) {
    ui.set_children(toml_data.children.iter().map(|c| {
        RuntimeElement {
            element_type: c.class_name.clone(),
            position: c.position,
            size: c.size,
            // ...
        }
    }).collect());
}
```

### Recommended Approach: Hybrid

1. **Use Slint Interpreter** for complex user UI (ScreenGui trees)
2. **Use pre-built components** for common patterns (health bars, buttons)
3. **Cache compiled UI** in `.eustress/cache/ui/` for fast reload

---

## TOML ↔ Slint Mapping

### GUI Class → Slint Component Mapping

| TOML Class | Slint Component | Notes |
|------------|-----------------|-------|
| `ScreenGui` | `Window` or root `Rectangle` | Full-screen overlay |
| `BillboardGui` | `Rectangle` + 3D transform | World-space, camera-facing |
| `SurfaceGui` | `Rectangle` + 3D transform | World-space, surface-aligned |
| `Frame` | `Rectangle` | Container with background |
| `ScrollingFrame` | `ScrollView` | Scrollable container |
| `TextLabel` | `Text` | Static text |
| `TextButton` | `TouchArea` + `Text` | Clickable text |
| `TextBox` | `LineEdit` | Text input |
| `ImageLabel` | `Image` | Static image |
| `ImageButton` | `TouchArea` + `Image` | Clickable image |
| `ViewportFrame` | Custom (Bevy texture) | 3D viewport in UI |
| `VideoFrame` | Custom (video texture) | Video playback |

### Property Mapping

| TOML Property | Slint Property | Transform |
|---------------|----------------|-----------|
| `position` | `x`, `y` | `[x, y]` → `x: {x}px; y: {y}px;` |
| `size` | `width`, `height` | `[w, h]` → `width: {w}px; height: {h}px;` |
| `anchor_point` | Layout alignment | `[0.5, 0.5]` → centered |
| `background_color` | `background` | `[r, g, b, a]` → `rgba(r, g, b, a)` |
| `background_transparency` | `background` alpha | Multiply alpha |
| `border_size` | `border-width` | Direct |
| `border_color` | `border-color` | `[r, g, b, a]` → color |
| `corner_radius` | `border-radius` | Direct or `[tl, tr, br, bl]` |
| `visible` | `visible` | Direct bool |
| `z_index` | Render order | Sort children by z_index |

### Text Properties

| TOML Property | Slint Property |
|---------------|----------------|
| `text` | `text` |
| `text_color` | `color` |
| `font_size` | `font-size` |
| `font_family` | `font-family` |
| `font_weight` | `font-weight` |
| `text_x_alignment` | `horizontal-alignment` |
| `text_y_alignment` | `vertical-alignment` |
| `text_wrapped` | `wrap` |
| `text_truncate` | `overflow: elide` |

### Image Properties

| TOML Property | Slint Property |
|---------------|----------------|
| `image` | `source` (asset path) |
| `image_color` | Tint (multiply) |
| `scale_type` | `image-fit` |
| `slice_center` | 9-slice (custom) |

---

## Explorer Integration

### Displaying GUI Hierarchy in Explorer

The Explorer panel shows the full instance tree, including GUI:

```
StarterGui/                          [folder icon]
├── _service.toml
└── HUD/                             [screengui icon]
    ├── _instance.toml
    ├── HealthBar.frame.toml         [frame icon]
    ├── Minimap.frame.toml           [frame icon]
    └── Inventory/                   [frame icon]
        ├── _instance.toml
        ├── Slot1.imagebutton.toml   [imagebutton icon]
        └── Slot2.imagebutton.toml   [imagebutton icon]
```

### TreeNode for GUI Classes

```slint
// In explorer.slint, TreeNode already supports this:
export struct TreeNode {
    id: int,
    name: string,
    icon: image,           // Class-specific icon
    depth: int,
    expandable: bool,
    expanded: bool,
    selected: bool,
    visible: bool,
    node-type: string,     // "entity" | "file"
    class-name: string,    // "ScreenGui" | "Frame" | "TextLabel" | ...
    path: string,
    // ...
}
```

### Icon Assignment

```rust
fn get_icon_for_class(class_name: &str) -> &'static str {
    match class_name {
        "ScreenGui" => "icons/classes/screengui.svg",
        "BillboardGui" => "icons/classes/billboardgui.svg",
        "SurfaceGui" => "icons/classes/surfacegui.svg",
        "Frame" => "icons/classes/frame.svg",
        "ScrollingFrame" => "icons/classes/scrollingframe.svg",
        "TextLabel" => "icons/classes/textlabel.svg",
        "TextButton" => "icons/classes/textbutton.svg",
        "TextBox" => "icons/classes/textbox.svg",
        "ImageLabel" => "icons/classes/imagelabel.svg",
        "ImageButton" => "icons/classes/imagebutton.svg",
        "ViewportFrame" => "icons/classes/viewportframe.svg",
        "VideoFrame" => "icons/classes/videoframe.svg",
        // ... other classes
        _ => "icons/classes/instance.svg",
    }
}
```

---

## Properties Panel Integration

### Dynamic Property Loading

When a GUI instance is selected, the Properties panel loads its TOML and displays editable fields:

```rust
fn load_properties_for_gui(path: &Path) -> Vec<PropertyData> {
    let toml = fs::read_to_string(path)?;
    let data: toml::Value = toml::from_str(&toml)?;
    
    let mut properties = vec![];
    
    // [instance] section
    properties.push(PropertyData {
        name: "Name".into(),
        value: data["instance"]["name"].as_str().unwrap_or("").into(),
        property_type: "string".into(),
        category: "Instance".into(),
        editable: true,
        ..Default::default()
    });
    
    // [gui] section
    if let Some(gui) = data.get("gui") {
        properties.push(PropertyData {
            name: "Position".into(),
            value: format_vec2(gui.get("position")),
            property_type: "vec2".into(),
            category: "Layout".into(),
            editable: true,
            ..Default::default()
        });
        // ... more properties
    }
    
    properties
}
```

### Property Categories for GUI

| Category | Properties |
|----------|------------|
| **Instance** | Name, ClassName, Archivable, AI |
| **Layout** | Position, Size, AnchorPoint, ZIndex |
| **Appearance** | BackgroundColor, BackgroundTransparency, BorderSize, BorderColor, CornerRadius |
| **Text** (TextLabel/Button/Box) | Text, TextColor, FontSize, FontFamily, TextXAlignment, TextYAlignment |
| **Image** (ImageLabel/Button) | Image, ImageColor, ScaleType, SliceCenter |
| **Behavior** | Visible, Active, Selectable, Draggable |
| **Scrolling** (ScrollingFrame) | CanvasSize, ScrollBarThickness, ScrollingDirection |

### Write-Back on Edit

When user edits a property in the Properties panel:

```rust
fn on_property_changed(path: &Path, property_name: &str, new_value: &str) {
    // 1. Read current TOML
    let mut data: toml::Value = toml::from_str(&fs::read_to_string(path)?)?;
    
    // 2. Update the specific field
    set_nested_value(&mut data, property_name, new_value)?;
    
    // 3. Write back
    fs::write(path, toml::to_string_pretty(&data)?)?;
    
    // 4. File watcher triggers hot-reload
}
```

---

## Icon System

### Icon Directory Structure

```
eustress/crates/engine/assets/icons/
├── classes/
│   ├── part.svg
│   ├── model.svg
│   ├── folder.svg
│   ├── screengui.svg
│   ├── frame.svg
│   ├── textlabel.svg
│   ├── textbutton.svg
│   ├── textbox.svg
│   ├── imagelabel.svg
│   ├── imagebutton.svg
│   ├── scrollingframe.svg
│   ├── viewportframe.svg
│   ├── videoframe.svg
│   ├── billboardgui.svg
│   ├── surfacegui.svg
│   ├── pointlight.svg
│   ├── spotlight.svg
│   ├── camera.svg
│   ├── humanoid.svg
│   ├── sound.svg
│   ├── script.svg
│   └── ... (all classes)
├── tools/
│   ├── select.svg
│   ├── move.svg
│   ├── rotate.svg
│   ├── scale.svg
│   └── ...
├── actions/
│   ├── play.svg
│   ├── pause.svg
│   ├── stop.svg
│   ├── save.svg
│   └── ...
└── ui/
    ├── chevron-right.svg
    ├── chevron-down.svg
    ├── search.svg
    ├── close.svg
    └── ...
```

### Icon Loading in Slint

```slint
// In theme.slint
export global Icons {
    // Classes
    out property <image> part: @image-url("../assets/icons/classes/part.svg");
    out property <image> model: @image-url("../assets/icons/classes/model.svg");
    out property <image> screengui: @image-url("../assets/icons/classes/screengui.svg");
    out property <image> frame: @image-url("../assets/icons/classes/frame.svg");
    out property <image> textlabel: @image-url("../assets/icons/classes/textlabel.svg");
    // ... all icons
}
```

### Dynamic Icon Selection

```rust
fn get_class_icon(class_name: &str) -> slint::Image {
    match class_name {
        "Part" => Icons::get_part(),
        "Model" => Icons::get_model(),
        "ScreenGui" => Icons::get_screengui(),
        "Frame" => Icons::get_frame(),
        "TextLabel" => Icons::get_textlabel(),
        // ...
        _ => Icons::get_instance(),
    }
}
```

---

## Live Editing Workflow

### Studio → TOML → Runtime UI

```
┌─────────────────────────────────────────────────────────────────┐
│                         Studio Mode                             │
│                                                                 │
│  1. User creates ScreenGui in Explorer (right-click → Insert)   │
│     └─→ Creates StarterGui/MyHUD/_instance.toml                 │
│                                                                 │
│  2. User adds Frame child                                       │
│     └─→ Creates StarterGui/MyHUD/HealthBar.frame.toml           │
│                                                                 │
│  3. User edits properties in Properties panel                   │
│     └─→ Writes to TOML file                                     │
│                                                                 │
│  4. File watcher detects change                                 │
│     └─→ Triggers UI rebuild                                     │
│                                                                 │
│  5. Runtime UI re-renders with new properties                   │
│     └─→ User sees change immediately in viewport                │
└─────────────────────────────────────────────────────────────────┘
```

### Visual UI Editor (Future)

A drag-and-drop UI editor inside Studio:

```
┌─────────────────────────────────────────────────────────────────┐
│  UI Editor Tab                                                  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  ┌─────────┐                                              │  │
│  │  │ Toolbox │  ┌────────────────────────────────────────┐  │  │
│  │  │ ─────── │  │                                        │  │  │
│  │  │ [Frame] │  │         Canvas (ScreenGui)             │  │  │
│  │  │ [Text]  │  │                                        │  │  │
│  │  │ [Image] │  │    ┌──────────────────────┐            │  │  │
│  │  │ [Button]│  │    │   Selected Frame     │            │  │  │
│  │  │ [Input] │  │    │   (drag handles)     │            │  │  │
│  │  │ [Scroll]│  │    └──────────────────────┘            │  │  │
│  │  │         │  │                                        │  │  │
│  │  └─────────┘  └────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

This would be a Slint component that:
1. Renders the ScreenGui tree visually
2. Allows drag-and-drop positioning
3. Shows selection handles for resize
4. Writes changes back to TOML

---

## Implementation Architecture

### Module Structure

```
eustress/crates/engine/src/
├── ui/
│   ├── mod.rs
│   ├── slint_ui.rs           ← Studio UI (existing)
│   ├── slint_platform.rs     ← Bevy-Slint bridge (existing)
│   ├── runtime_ui.rs         ← NEW: Runtime UI manager
│   ├── toml_to_slint.rs      ← NEW: TOML → Slint converter
│   └── ui_events.rs          ← NEW: UI event routing to Soul
```

### Key Types

```rust
// runtime_ui.rs

/// Manages all user-created runtime UI
pub struct RuntimeUIManager {
    /// Active ScreenGuis (player-local)
    screen_guis: HashMap<Entity, ScreenGuiInstance>,
    /// Active BillboardGuis (world-space)
    billboard_guis: HashMap<Entity, BillboardGuiInstance>,
    /// Active SurfaceGuis (world-space)
    surface_guis: HashMap<Entity, SurfaceGuiInstance>,
    /// Slint interpreter for dynamic UI
    compiler: slint_interpreter::ComponentCompiler,
}

/// A loaded ScreenGui instance
pub struct ScreenGuiInstance {
    /// Path to the _instance.toml
    pub path: PathBuf,
    /// Compiled Slint component
    pub component: slint_interpreter::ComponentInstance,
    /// Child element map (name → element handle)
    pub elements: HashMap<String, ElementHandle>,
    /// Last modified time (for hot-reload)
    pub mtime: SystemTime,
}
```

### TOML → Slint Conversion

```rust
// toml_to_slint.rs

pub fn generate_slint_source(gui_path: &Path) -> Result<String> {
    let data = load_gui_tree(gui_path)?;
    
    let mut slint = String::new();
    slint.push_str("import { VerticalLayout, HorizontalLayout, Rectangle, Text, Image, TouchArea, LineEdit, ScrollView } from \"std-widgets.slint\";\n\n");
    
    slint.push_str("export component GeneratedUI inherits Rectangle {\n");
    generate_element(&mut slint, &data, 1)?;
    slint.push_str("}\n");
    
    Ok(slint)
}

fn generate_element(out: &mut String, element: &GuiElement, indent: usize) -> Result<()> {
    let pad = "    ".repeat(indent);
    
    match element.class_name.as_str() {
        "Frame" => {
            writeln!(out, "{pad}Rectangle {{")?;
            writeln!(out, "{pad}    x: {}px;", element.position[0])?;
            writeln!(out, "{pad}    y: {}px;", element.position[1])?;
            writeln!(out, "{pad}    width: {}px;", element.size[0])?;
            writeln!(out, "{pad}    height: {}px;", element.size[1])?;
            writeln!(out, "{pad}    background: {};", format_color(&element.background_color))?;
            for child in &element.children {
                generate_element(out, child, indent + 1)?;
            }
            writeln!(out, "{pad}}}")?;
        }
        "TextLabel" => {
            writeln!(out, "{pad}Text {{")?;
            writeln!(out, "{pad}    x: {}px;", element.position[0])?;
            writeln!(out, "{pad}    y: {}px;", element.position[1])?;
            writeln!(out, "{pad}    text: \"{}\";", escape_string(&element.text))?;
            writeln!(out, "{pad}    color: {};", format_color(&element.text_color))?;
            writeln!(out, "{pad}    font-size: {}px;", element.font_size)?;
            writeln!(out, "{pad}}}")?;
        }
        // ... other element types
        _ => {}
    }
    
    Ok(())
}
```

### Event Routing to Soul

```rust
// ui_events.rs

/// UI event that can be handled by Soul scripts
pub enum UIEvent {
    ButtonClicked { element_path: String },
    TextChanged { element_path: String, new_text: String },
    MouseEnter { element_path: String },
    MouseLeave { element_path: String },
    FocusGained { element_path: String },
    FocusLost { element_path: String },
}

/// System that routes Slint UI events to Soul scripts
pub fn route_ui_events(
    mut events: EventReader<UIEvent>,
    soul_vm: Res<SoulVM>,
    gui_scripts: Query<(&GuiScript, &Parent)>,
) {
    for event in events.read() {
        // Find the Soul script attached to this GUI element
        if let Some(script) = find_script_for_element(&event.element_path, &gui_scripts) {
            // Call the appropriate handler
            match event {
                UIEvent::ButtonClicked { .. } => {
                    soul_vm.call_method(script, "on_click", &[]);
                }
                UIEvent::TextChanged { new_text, .. } => {
                    soul_vm.call_method(script, "on_text_changed", &[new_text.clone()]);
                }
                // ...
            }
        }
    }
}
```

---

## Implementation Checklist

### Phase 1: Foundation

- [ ] Add `slint-interpreter` to Cargo.toml
- [ ] Create `runtime_ui.rs` module
- [ ] Create `toml_to_slint.rs` converter
- [ ] Implement basic Frame → Rectangle conversion
- [ ] Implement basic TextLabel → Text conversion
- [ ] Test: Load a simple ScreenGui from TOML and render it

### Phase 2: Full GUI Classes

- [ ] Implement all GUI class conversions:
  - [ ] Frame
  - [ ] ScrollingFrame
  - [ ] TextLabel
  - [ ] TextButton
  - [ ] TextBox
  - [ ] ImageLabel
  - [ ] ImageButton
  - [ ] ViewportFrame (Bevy texture integration)
  - [ ] VideoFrame (video texture integration)
- [ ] Implement property mapping for all properties
- [ ] Implement anchor point / layout system

### Phase 3: Explorer & Properties Integration

- [ ] Add GUI class icons to icon system
- [ ] Update Explorer to show GUI hierarchy correctly
- [ ] Update Properties panel to show GUI-specific properties
- [ ] Implement write-back for GUI property edits
- [ ] Test: Edit a TextLabel's text in Properties, see it update live

### Phase 4: Hot Reload

- [ ] Add file watcher for GUI TOML files
- [ ] Implement incremental UI rebuild on file change
- [ ] Cache compiled Slint components in `.eustress/cache/ui/`
- [ ] Test: Edit TOML in external editor, see UI update in Studio

### Phase 5: Event System

- [ ] Define UIEvent enum
- [ ] Wire Slint callbacks to UIEvent emission
- [ ] Implement Soul script event routing
- [ ] Test: Click a TextButton, trigger Soul script handler

### Phase 6: Visual UI Editor (Future)

- [ ] Create `ui_editor.slint` component
- [ ] Implement drag-and-drop element insertion
- [ ] Implement selection handles for resize
- [ ] Implement property editing inline
- [ ] Wire to TOML write-back

---

## Changelog

### v1.0 (2026-03-02)

- Initial specification
- Defined two UI domains (Studio vs Runtime)
- TOML ↔ Slint property mapping
- Explorer and Properties panel integration
- Icon system design
- Implementation architecture
