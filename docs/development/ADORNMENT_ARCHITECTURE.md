# Adornment Architecture for Eustress Studio Tooling

## Overview

Adornments are **meta-entities** that provide visual feedback and interaction handles for editor tools. They are:
- **Hidden from Explorer** (meta flag prevents display in tree)
- **Parented to selected entities** during editing
- **Spawned/despawned dynamically** based on tool state and selection
- **Interactive** with mouse events for dragging/manipulation

## Roblox Adornment Class Hierarchy

```
Instance
└── GuiBase3d
    └── PVAdornment (has Adornee property)
        ├── HandleAdornment (abstract - CFrame, AlwaysOnTop, ZIndex, SizeRelativeOffset)
        │   ├── BoxHandleAdornment (Size: Vector3, Shading)
        │   ├── SphereHandleAdornment (Radius: float, Shading)
        │   ├── ConeHandleAdornment (Height, Radius, Shading)
        │   ├── CylinderHandleAdornment (Height, Radius, Shading)
        │   ├── ImageHandleAdornment (Image, Size)
        │   ├── LineHandleAdornment (Length, Thickness)
        │   ├── PyramidHandleAdornment (Size: Vector3, Shading)
        │   └── WireframeHandleAdornment (Scale: Vector3)
        ├── PartAdornment
        │   ├── SelectionBox (LineThickness, SurfaceColor3, SurfaceTransparency)
        │   └── SelectionSphere (SurfaceColor3, SurfaceTransparency)
        └── HandlesBase
            ├── ArcHandles (Axes, rotation arcs for X/Y/Z)
            └── Handles (Faces, Style - translation/resize arrows)
```

## Eustress Adornment Implementation

### Core Properties (All Adornments)

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `adornee` | Entity | None | The entity this adornment is attached to |
| `cframe` | Transform | Identity | Local offset from adornee |
| `always_on_top` | bool | true | Render in front of all 3D objects |
| `z_index` | i32 | 0 | Draw order (-1 to 10) |
| `size_relative_offset` | Vec3 | (0,0,0) | Offset as scale of adornee size |
| `color` | Color | White | Adornment color |
| `transparency` | f32 | 0.0 | 0 = opaque, 1 = invisible |
| `visible` | bool | true | Whether to render |
| `meta` | bool | true | **Hidden from Explorer** |

### Handle Adornment Types

#### BoxHandleAdornment
- **Use**: Scale tool corner/edge handles
- **Properties**: `size: Vec3`
- **Mesh**: Cube primitive

#### SphereHandleAdornment  
- **Use**: Scale tool corner handles (alternative), rotation pivot
- **Properties**: `radius: f32`
- **Mesh**: Sphere primitive

#### ConeHandleAdornment
- **Use**: Move tool axis arrows
- **Properties**: `height: f32`, `radius: f32`
- **Mesh**: Cone primitive pointing along local Y

#### CylinderHandleAdornment
- **Use**: Move tool axis shafts, rotation rings
- **Properties**: `height: f32`, `radius: f32`
- **Mesh**: Cylinder primitive

#### LineHandleAdornment
- **Use**: Axis lines, connection indicators
- **Properties**: `length: f32`, `thickness: f32`
- **Mesh**: Thin cylinder or line primitive

### Selection Adornments

#### SelectionBox
- **Use**: Highlight selected entities with wireframe box
- **Properties**: `line_thickness: f32`, `surface_color: Color`, `surface_transparency: f32`
- **Mesh**: Wireframe cube matching adornee bounds

#### SelectionSphere
- **Use**: Highlight spherical entities
- **Properties**: `surface_color: Color`, `surface_transparency: f32`
- **Mesh**: Wireframe sphere matching adornee bounds

### Handle Systems

#### ArcHandles (Rotate Tool)
- **Use**: 3 rotation arcs for X/Y/Z axes
- **Properties**: `axes: Axes` (which axes to show)
- **Mesh**: 3 torus segments (90° arcs) in red/green/blue
- **Events**: `MouseDrag(axis, angle_delta)`

#### Handles (Move/Scale Tool)
- **Use**: 6 directional arrows/cubes for translation/scaling
- **Properties**: `faces: Faces`, `style: HandleStyle`
- **Mesh**: 6 cones (move) or 6 cubes (scale) on ±X/Y/Z
- **Events**: `MouseDrag(face, distance_delta)`

## Tool → Adornment Mapping

### Select Tool (Base)
```
SelectionBox → attached to each selected entity
```

### Move Tool (extends Select)
```
SelectionBox → attached to each selected entity
Handles (Move style):
  - ConeHandleAdornment × 6 (axis arrows: ±X, ±Y, ±Z)
  - CylinderHandleAdornment × 3 (axis shafts)
  - BoxHandleAdornment × 3 (plane handles: XY, XZ, YZ)
```

### Scale Tool (extends Select)
```
SelectionBox → attached to each selected entity
Handles (Scale style):
  - BoxHandleAdornment × 8 (corner handles)
  - BoxHandleAdornment × 12 (edge handles)
  - BoxHandleAdornment × 6 (face handles)
```

### Rotate Tool (extends Select)
```
SelectionBox → attached to each selected entity
ArcHandles:
  - Torus arc × 3 (X=red, Y=green, Z=blue)
  - SphereHandleAdornment × 1 (center pivot)
```

## Implementation Plan

### Phase 1: Core Adornment Components
1. Add `Adornment` marker component with `meta: bool` flag
2. Add `AdornmentAdornee` component linking to target entity
3. Modify Explorer sync to filter out entities with `meta = true`
4. Create base `HandleAdornment` component bundle

### Phase 2: Adornment TOML Definitions
Create `.toml` class definitions in `AdornmentService/`:
- `BoxHandleAdornment.toml`
- `SphereHandleAdornment.toml`
- `ConeHandleAdornment.toml`
- `CylinderHandleAdornment.toml`
- `LineHandleAdornment.toml`
- `SelectionBox.toml`
- `ArcHandles.toml`
- `Handles.toml`

### Phase 3: Tool Refactoring
1. Extract common selection logic into `SelectToolBase` trait
2. Implement `spawn_adornments()` and `despawn_adornments()` methods
3. Move tool inherits SelectToolBase + adds translation handles
4. Scale tool inherits SelectToolBase + adds scale handles
5. Rotate tool inherits SelectToolBase + adds arc handles

### Phase 4: Adornment Rendering
1. Create `AdornmentMaterial` with `always_on_top` support
2. Implement depth-test bypass for AlwaysOnTop adornments
3. Add hover highlighting (color change on MouseEnter)
4. Add drag interaction (MouseButton1Down → MouseDrag → MouseButton1Up)

## File Structure

```
crates/common/src/
  adornments/
    mod.rs              # Adornment component definitions
    handle.rs           # HandleAdornment base
    selection.rs        # SelectionBox, SelectionSphere
    arc_handles.rs      # ArcHandles for rotation
    handles.rs          # Handles for move/scale

crates/engine/src/
  tools/
    mod.rs              # Tool trait and base functionality
    select_tool.rs      # Base selection behavior
    move_tool.rs        # Translation with Handles
    scale_tool.rs       # Scaling with corner/edge handles
    rotate_tool.rs      # Rotation with ArcHandles
  adornment_renderer.rs # Specialized rendering for adornments
```

## Mouse Event Flow

```
1. Mouse moves over adornment mesh
2. Raycast detects adornment entity
3. Fire MouseEnter event → highlight adornment
4. Mouse button down → Fire MouseButton1Down
5. Mouse drag → Fire MouseDrag(axis, delta)
6. Mouse button up → Fire MouseButton1Up
7. Apply transform to adornee based on accumulated delta
8. Mouse leaves → Fire MouseLeave → unhighlight
```

## Axis Color Convention

| Axis | Color | RGB |
|------|-------|-----|
| X | Red | (1.0, 0.2, 0.2) |
| Y | Green | (0.2, 1.0, 0.2) |
| Z | Blue | (0.2, 0.2, 1.0) |
| XY Plane | Yellow | (1.0, 1.0, 0.2) |
| XZ Plane | Magenta | (1.0, 0.2, 1.0) |
| YZ Plane | Cyan | (0.2, 1.0, 1.0) |
| Center/All | White | (1.0, 1.0, 1.0) |

---

## Smart Grid System

The Smart Grid system provides intelligent alignment and snapping during object manipulation, similar to PowerPoint's smart guides but optimized for 3D spatial editing.

### Core Concepts

1. **Grid Sensors** - Corner/edge adornments that detect proximity to alignment opportunities
2. **Alignment Guides** - Visual lines showing when edges/centers align with other objects
3. **Snap Indicators** - Ghost previews showing where objects will snap to
4. **Spatial Index** - R-tree for fast nearest-neighbor queries during drag operations

### Smart Grid Adornments

#### GridSensor
Dynamic corner indicators that appear near the mouse cursor, showing the nearest grid intersection or alignment point.

```
Properties:
- sensor_radius: f32      # Detection radius in studs
- corner_size: f32        # Visual size of corner indicator
- active_corner: Corner   # Which corner is currently active (nearest to mouse)
- snap_distance: f32      # Distance threshold for snapping
```

#### AlignmentGuide
Red/green lines that appear when edges or centers align with other objects.

```
Properties:
- guide_type: GuideType   # Edge, Center, Corner
- axis: Axis              # X, Y, or Z
- color: Color            # Red for edge, Green for center
- start_point: Vec3       # Line start
- end_point: Vec3         # Line end
- thickness: f32          # Line thickness
```

#### SnapIndicator
Ghost preview showing the snapped position before releasing the mouse.

```
Properties:
- target_position: Vec3   # Where object will snap to
- snap_type: SnapType     # Grid, Edge, Center, Corner
- confidence: f32         # 0-1 strength of snap suggestion
```

### Alignment Types

| Type | Description | Visual |
|------|-------------|--------|
| **Edge-to-Edge** | Object edge aligns with another object's edge | Red line connecting edges |
| **Center-to-Center** | Object center aligns with another object's center | Green dashed line |
| **Corner-to-Corner** | Object corner aligns with another object's corner | Blue dot at intersection |
| **Edge-to-Center** | Object edge aligns with another object's center | Orange line |
| **Grid Snap** | Object snaps to world grid | Gray grid lines |
| **Surface Snap** | Object snaps to surface of another object | Cyan highlight on surface |

### Smart Grid Algorithm

```
1. On mouse move during drag:
   a. Get dragged object's 8 corners + 6 face centers + 1 center = 15 key points
   b. Query spatial index for nearby objects within snap_radius
   c. For each nearby object:
      - Calculate distances from all 15 key points to target's 15 key points
      - Find minimum distance pairs
      - If distance < snap_threshold, create AlignmentGuide
   d. Sort alignment opportunities by distance
   e. Apply strongest snap (or combine if multiple axes)
   f. Update SnapIndicator ghost preview

2. On mouse release:
   a. If snap active, move to snapped position
   b. Record undo state
   c. Despawn all alignment adornments
```

### Corner Detection for Grid Sensor

The GridSensor tracks which corner of the selection box is nearest to the mouse cursor:

```
Corners (looking down -Y):
  TopBackLeft -------- TopBackRight
       |                    |
       |    TopFrontLeft ---+--- TopFrontRight
       |         |          |         |
  BottomBackLeft-+----------+-BottomBackRight
                 |                    |
       BottomFrontLeft ---- BottomFrontRight
```

The active corner determines:
- Which corner shows the grid sensor adornment
- Which corner is used for edge alignment calculations
- The pivot point for corner-to-corner snapping

### Spatial Index Structure

```rust
struct SmartGridIndex {
    // R-tree for fast spatial queries
    rtree: RTree<SpatialEntry>,
    
    // Cache of object bounding boxes (updated on transform change)
    bounds_cache: HashMap<Entity, Aabb>,
    
    // Precomputed key points per entity (15 points each)
    key_points_cache: HashMap<Entity, KeyPoints>,
}

struct KeyPoints {
    corners: [Vec3; 8],      // 8 corners of AABB
    face_centers: [Vec3; 6], // 6 face centers
    center: Vec3,            // Object center
}

struct SpatialEntry {
    entity: Entity,
    aabb: Aabb,
}
```

### Configuration

```toml
[smart_grid]
enabled = true
snap_distance = 0.5          # Distance threshold for snapping (studs)
sensor_radius = 2.0          # Detection radius for grid sensor
show_alignment_guides = true # Show red/green alignment lines
show_snap_preview = true     # Show ghost preview
grid_size = 1.0              # World grid size (studs)
grid_subdivisions = 4        # Subdivisions for fine snapping
max_guides = 8               # Maximum simultaneous alignment guides
guide_color_edge = [1.0, 0.2, 0.2]    # Red
guide_color_center = [0.2, 1.0, 0.2]  # Green
guide_color_corner = [0.2, 0.2, 1.0]  # Blue
```

### Integration with Tools

#### Select Tool
- GridSensor appears at nearest corner when hovering over selected object
- Shows potential snap points on nearby objects

#### Move Tool
- AlignmentGuides appear during drag when edges/centers align
- SnapIndicator shows ghost preview of snapped position
- Holding Shift disables snapping for free movement
- Holding Ctrl enables grid-only snapping (ignores object alignment)

#### Scale Tool
- Edge alignment guides help scale to match other objects
- Corner snapping for precise sizing

#### Rotate Tool
- Angle snapping (15°, 45°, 90° increments)
- Alignment to other objects' orientations

### Performance Considerations

1. **Spatial Index Updates**: Only update R-tree when objects move (not every frame)
2. **Lazy Key Point Calculation**: Compute key points on demand, cache until transform changes
3. **Distance Culling**: Skip alignment checks for objects beyond snap_radius × 2
4. **Guide Limit**: Cap at max_guides to prevent visual clutter
5. **Frame Budget**: Limit alignment calculations to 1ms per frame, defer rest to next frame

### Visual Feedback States

| State | GridSensor | AlignmentGuide | SnapIndicator |
|-------|------------|----------------|---------------|
| Hovering (no selection) | Hidden | Hidden | Hidden |
| Selected (not dragging) | Shows at nearest corner | Hidden | Hidden |
| Dragging (no alignment) | Follows mouse | Hidden | Hidden |
| Dragging (alignment found) | At snap corner | Visible (red/green lines) | Ghost at snap position |
| Snap confirmed (release) | Flash animation | Fade out | Merge with object |

---

## References

- [Roblox HandleAdornment API](https://create.roblox.com/docs/reference/engine/classes/HandleAdornment)
- [Roblox ArcHandles API](https://create.roblox.com/docs/reference/engine/classes/ArcHandles)
- [Roblox SelectionBox API](https://create.roblox.com/docs/reference/engine/classes/SelectionBox)
- [Unreal Engine Transform Gizmo](https://docs.unrealengine.com/5.0/en-US/using-the-transform-gizmo-in-unreal-engine/)
- [PowerPoint Smart Guides](https://support.microsoft.com/en-us/office/use-smart-guides-to-align-objects-in-powerpoint)
