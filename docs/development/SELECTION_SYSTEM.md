# Selection System - Drag-to-Select & Visual Feedback

## Overview

The Eustress Engine selection system provides **Roblox-style selection** with drag-to-select bounding boxes and cyan selection visualization. The system supports:
- **Click-to-select** individual BasePart instances
- **Drag-to-select** multiple instances with a bounding box
- **Cyan selection boxes** rendered around selected entities
- **Shift-click** for additive selection
- **UI focus detection** to prevent viewport input when interacting with panels

## Features

### 1. Drag-to-Select Bounding Box

**Visual Feedback:**
- Semi-transparent cyan rectangle while dragging
- Outline color: `Color::srgba(0.35, 0.75, 1.0, 0.8)` (bright cyan)
- Fill color: `Color::srgba(0.35, 0.75, 1.0, 0.15)` (semi-transparent cyan)
- Renders in 2D screen space using Bevy gizmos

**Behavior:**
- Click and drag on empty space to start box selection
- Drag threshold: 3 pixels (prevents accidental box selection)
- All unlocked BasePart instances with centers inside the box are selected
- Locked parts are excluded from selection
- Shift+drag for additive selection (keeps previous selection)

**Code Location:** `eustress/crates/engine/src/select_tool.rs`
- `BoxSelectionState` resource tracks drag state
- `handle_box_selection()` system handles selection logic
- `render_box_selection()` system renders the visual rectangle

### 2. Cyan Selection Boxes

**Visual Style:**
- Primary color: `Color::srgba(0.35, 0.75, 1.0, 1.0)` (bright cyan-blue)
- Edge highlight: `Color::srgba(0.5, 0.9, 1.0, 1.0)` (brighter cyan)
- Corner dots: `Color::srgba(0.9, 0.97, 1.0, 1.0)` (white with blue tint)
- Renders on top of all geometry (depth bias: -1.0)
- Line width: 2.0 pixels for better visibility

**Supported Shapes:**
- **Box** - Standard rectangular parts (most common)
- **Sphere** - Circular wireframe with latitude/longitude lines
- **Cylinder** - Wireframe with top/bottom circles and vertical edges
- **Wedge** - Triangular prism wireframe
- **CornerWedge** - Corner-cut wedge wireframe

**Code Location:** `eustress/crates/engine/src/selection_box.rs`
- `SelectionBox` component marks selected entities
- `draw_selection_boxes()` system renders wireframes
- `draw_billboard_gui_selection()` for BillboardGui entities

### 3. UI Focus Detection

**Purpose:**
Prevents viewport input (click, drag, box selection) when the mouse is over Slint UI panels (Explorer, Properties, Toolbox, etc.).

**Implementation:**
- `SlintUIFocus` resource tracks UI focus state
- `has_focus: bool` - true when mouse is over UI panels
- `last_ui_position: Option<Vec2>` - last known cursor position over UI

**Integration:**
All viewport input systems check `SlintUIFocus` before processing:
```rust
// Block input when Slint UI has focus (mouse is over UI panels)
if let Some(ui_focus) = ui_focus {
    if ui_focus.has_focus {
        return;
    }
}
```

**Code Locations:**
- Resource definition: `eustress/crates/engine/src/ui/mod.rs`
- Initialization: `eustress/crates/engine/src/ui/slint_ui.rs` (SlintUiPlugin)
- Usage: `eustress/crates/engine/src/select_tool.rs` (handle_select_drag, handle_box_selection)

## Selection Workflow

### Single Selection (Click)
1. User clicks on a BasePart instance
2. System raycasts from cursor to 3D world
3. Checks if ray intersects any unlocked BasePart
4. If hit, `SelectionBox` component is added to the entity
5. Cyan selection box renders around the entity

### Multi-Selection (Drag Box)
1. User clicks on empty space (no part under cursor)
2. `BoxSelectionState.pending` set to true
3. User drags mouse (exceeds 3px threshold)
4. `BoxSelectionState.active` set to true
5. Visual cyan rectangle renders in screen space
6. All unlocked BaseParts with centers inside box are selected
7. On mouse release, selection is finalized

### Additive Selection (Shift)
1. User holds Shift key
2. Previous selection is stored in `BoxSelectionState.previous_selection`
3. New selection is added to previous selection
4. Both old and new entities have `SelectionBox` component

## Architecture

### Resources

**`SelectToolState`**
- Tracks drag state for moving selected parts
- Stores initial positions/rotations for undo
- Group bounding box for multi-selection transforms
- Drag threshold: 5 pixels

**`BoxSelectionState`**
- `active: bool` - Box selection is being drawn
- `pending: bool` - Mouse down on empty space (threshold not exceeded)
- `start_pos: Vec2` - Screen space start position
- `current_pos: Vec2` - Screen space current position
- `additive: bool` - Shift held for additive mode
- `previous_selection: Vec<Entity>` - Entities selected before box started

**`SlintUIFocus`**
- `has_focus: bool` - UI has focus (block viewport input)
- `last_ui_position: Option<Vec2>` - Last cursor position over UI

### Components

**`SelectionBox`**
- Marker component for selected entities
- Automatically added/removed by `SelectionSyncManager`
- Triggers cyan wireframe rendering

**`HoverHighlight`**
- Marker component for hovered (but not selected) entities
- Renders yellow-orange outline
- Used for preview before selection

### Systems

**`handle_box_selection()`**
- Detects mouse down on empty space
- Tracks drag distance and activates box selection
- Projects entity positions to screen space
- Selects all entities within the 2D bounding box
- Handles additive mode with Shift key

**`render_box_selection()`**
- Converts screen space to NDC (Normalized Device Coordinates)
- Draws filled rectangle with semi-transparent cyan
- Draws outline with solid cyan lines
- Only renders when `BoxSelectionState.active` is true

**`draw_selection_boxes()`**
- Queries all entities with `SelectionBox` component
- Determines part type (Box, Sphere, Cylinder, etc.)
- Draws appropriate wireframe geometry
- Renders corner dots for visual feedback
- Handles hierarchical selections (Models with children)

**`sync_selection_boxes()`**
- Synchronizes `SelectionManager` state with Bevy ECS
- Adds `SelectionBox` to newly selected entities
- Removes `SelectionBox` from deselected entities
- Excludes abstract celestial services (Atmosphere, Sun, Moon, Sky)

## Configuration

### Constants (select_tool.rs)

```rust
/// Drag threshold in pixels - must move this far to start dragging
const DRAG_THRESHOLD: f32 = 5.0;

/// Box selection threshold - must drag this far to start box select
const BOX_SELECT_THRESHOLD: f32 = 3.0;
```

### Colors (selection_box.rs)

```rust
/// Primary selection outline color (Roblox-style bright cyan-blue)
const SELECTION_COLOR: Color = Color::srgba(0.35, 0.75, 1.0, 1.0);

/// Brighter highlight for the selection outline edges
const SELECTION_EDGE_COLOR: Color = Color::srgba(0.5, 0.9, 1.0, 1.0);

/// Corner dot color (white with slight blue tint)
const CORNER_DOT_COLOR: Color = Color::srgba(0.9, 0.97, 1.0, 1.0);

/// Hover highlight color (yellow-orange, semi-transparent)
const HOVER_COLOR: Color = Color::srgba(1.0, 0.85, 0.2, 0.75);
```

## Tool Integration

The selection system works across all transformation tools:

**Select Tool** (`Tool::Select`)
- Full drag-to-select functionality
- Box selection enabled
- Click-to-select individual parts

**Move Tool** (`Tool::Move`)
- Box selection enabled
- Drag-to-move selected parts
- Gizmo handles for axis-constrained movement

**Scale Tool** (`Tool::Scale`)
- Box selection enabled
- Per-entity scale handles
- Uniform and non-uniform scaling

**Rotate Tool** (`Tool::Rotate`)
- Box selection enabled
- Group rotation around bounding box center
- Ring gizmos for X/Y/Z rotation

## Locked Parts

**Behavior:**
- Locked parts (`BasePart.locked = true`) are excluded from selection
- Clicking on a locked part starts box selection (treated as empty space)
- Locked parts are skipped during box selection iteration
- Prevents accidental modification of locked geometry

**Code:**
```rust
// Skip locked parts - clicking on them should start box selection
if let Some(bp) = basepart {
    if bp.locked {
        return false;
    }
}
```

## Performance Considerations

**Screen Space Projection:**
- Box selection projects entity centers to screen space
- Uses `Camera::world_to_viewport()` for accurate 2D positions
- Only checks entities within the dragged rectangle
- Efficient for large scenes with many parts

**Gizmo Rendering:**
- Selection boxes use Bevy's gizmo system
- Depth bias ensures rendering on top of geometry
- Wireframes are procedurally generated per frame
- No mesh assets required

**UI Focus Detection:**
- Minimal overhead (single bool check)
- Prevents unnecessary raycasts when mouse is over UI
- Improves responsiveness of UI interactions

## Future Enhancements

- **Lasso selection** - Free-form polygon selection
- **Invert selection** - Select all except current selection
- **Select by type** - Select all parts of a specific type
- **Select by material** - Select all parts with a specific material
- **Selection groups** - Save and restore named selections
- **Selection history** - Undo/redo selection changes
- **Outline shader** - GPU-based outline rendering for better performance
- **Hover preview** - Show selection box on hover before clicking

## References

- **Select Tool:** `eustress/crates/engine/src/select_tool.rs`
- **Selection Box Rendering:** `eustress/crates/engine/src/selection_box.rs`
- **Selection Sync:** `eustress/crates/engine/src/selection_sync.rs`
- **UI Focus:** `eustress/crates/engine/src/ui/mod.rs`
- **Slint UI Plugin:** `eustress/crates/engine/src/ui/slint_ui.rs`
