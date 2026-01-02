# Eustress Engine Tools

## Overview
Eustress Engine includes four primary tools for entity manipulation:

- Select (Alt+Z)
- Move (Alt+X)
- Resize (Alt+C)
- Rotate (Alt+V)

The active tool can be changed via the toolbar buttons or keyboard shortcuts.

## Keyboard Shortcuts
- Alt+Z: Select
- Alt+X: Move
- Alt+C: Resize
- Alt+V: Rotate

## Planned Gizmos (Arc Handles)
Gizmos visualize and constrain interactions:
- Axis handles (X/Y/Z) for move/rotate
- Plane handles (XY/YZ/ZX) for planar move
- Scale handles (X/Y/Z + Uniform)

Components (Rust ECS):
- `GizmoRoot` (attached to selected)
- `GizmoAxis { axis: X|Y|Z }`
- `GizmoPlane { plane: XY|YZ|ZX }`
- `GizmoScaleHandle { axis: X|Y|Z|Uniform }`
- `GizmoState { tool, space, snap }`

Systems:
- Spawn/Update gizmos on selection/tool changes
- Hit test against handles (raycast)
- Begin drag (lock constraint)
- Drag update (apply translate/rotate/scale, snapping)
- End drag

## Selection and Transforms (Native)
Tauri commands (native desktop) expose:
- `set_tool`, `get_tool`
- `select_entity`, `get_selection`
- `translate_selected`, `rotate_selected`, `scale_selected`
- `get_entity_transform`, `update_entity`

The web/WASM mode uses stubbed data until WASM-side bindings are implemented.

## Explorer and Services
The Explorer shows:
- Services (Rendering, Physics, Audio, Input, Asset, Scripting)
- Workspace (entities)

On desktop, services are provided via `list_services`.

## Notes
- Right-click is disabled globally in Engine.
- FPS overlay shows performance (green > 30, yellow 15–30, red < 15, clamped to 120).
- Default skybox loads from `assets/sky/default_skybox.ktx2` with a 10am sun.
