# Studio Audit: egui → Slint Transition Status

**Date:** February 20, 2026  
**Scope:** Full UX/UI audit of Eustress Engine Studio — what transitioned, what's stub-only, what's missing  
**Comparison:** Eustress vs Roblox Studio vs Unreal Editor — honest gap analysis

---

## Executive Summary

The Slint UI shell is **visually complete** — 32 `.slint` files define every panel, ribbon, toolbar, dialog, and overlay. However, many Rust-side systems are **stub modules** that compile but do nothing. The Slint→Rust callback wiring is extensive (~80+ callbacks), but several critical write-back paths are incomplete. The egui code is fully removed from runtime, but ~49 comment references and stub modules remain for API compatibility.

**Bottom line:** The Studio *looks* right but ~40% of the interactive UX features are non-functional or partially wired.

---

## Part 1: Transition Status by Panel

### Legend
- ✅ **Complete** — Slint UI + Rust backend fully wired, interactive
- 🔶 **Partial** — Slint UI exists, Rust backend partially wired (some features work)
- 🔴 **Stub** — Slint UI exists, Rust backend is an empty stub or TODO
- ⬛ **Missing** — Neither Slint UI nor Rust backend exists

---

### Core Layout & Navigation

| Feature | Status | Notes |
|---------|--------|-------|
| Window chrome / title bar | ✅ | Custom window icon via `WINIT_WINDOWS` thread-local |
| Ribbon toolbar (Home/Model/Test/View/Plugins) | ✅ | `ribbon.slint` + full callback wiring |
| Tool buttons (Select/Move/Scale/Rotate) | ✅ | Slint + `SlintAction::SelectTool` + keyboard shortcuts (Alt+Z/X/C/V) |
| Dock layout (left/right/bottom panels) | 🔶 | `dock_layout.slint` exists, layout presets wired, but **panel detach to OS window is TODO** |
| Center tab bar (Scene/Script/Web) | ✅ | Full drag-drop reorder, close, Ctrl+W/T/L shortcuts |
| Performance overlay (FPS/frame time/entities) | ✅ | `dock_layout.slint` PerformanceOverlay + live sync from `UIWorldSnapshot` |
| Exit confirmation (unsaved changes) | ✅ | `exit_confirmation.slint` + Alt+F4 / X button handling |

### Explorer Panel

| Feature | Status | Notes |
|---------|--------|-------|
| Entity tree hierarchy | ✅ | `explorer.slint` + `sync_explorer_to_slint` system |
| Expand/collapse nodes | ✅ | `ExplorerExpanded` resource + `SlintAction::ExpandEntity/CollapseEntity` |
| Select entity (click) | ✅ | `SlintAction::SelectEntity` → `ExplorerState.selected` |
| Rename entity (inline) | 🔴 | Callback wired (`on_rename_entity`), but **context menu "rename" is TODO** — no inline edit trigger |
| Drag-drop reparent | 🔶 | `SlintAction::ReparentEntity` wired and inserts `ChildOf`, but **no visual drag indicator in explorer tree** |
| Right-click context menu | 🔶 | `context_menu.slint` exists, actions dispatch, but **"Insert" submenu is TODO** |
| Search/filter entities | 🔴 | `explorer_search.rs` and `explorer_search_ui.rs` are **empty stubs** — `get_searchable_properties()` returns `vec![]` |
| Class-specific icons | ✅ | SVG icons migrated (53 UI icons + class icons), `class_icon_for_slint()` maps all ClassNames |
| Multi-select in tree | 🔴 | Explorer only tracks single `selected: Option<Entity>` — **no multi-select in tree** |
| Service nodes (Workspace, Lighting, etc.) | 🔶 | Service resources exist as stubs, tree shows them, but **service properties editing is stub** |

### Properties Panel

| Feature | Status | Notes |
|---------|--------|-------|
| Display selected entity properties | ✅ | `sync_properties_to_slint` builds flat list with category headers from `PropertyAccess` trait |
| Edit Name | ✅ | `PropertyChanged("Name", val)` → updates `Instance.name` |
| Edit Position (X/Y/Z) | ✅ | `PropertyChanged("Position.X/Y/Z", val)` → updates `Transform.translation` |
| Edit Scale (X/Y/Z) | ✅ | `PropertyChanged("Scale.X/Y/Z", val)` → updates `Transform.scale` |
| Edit Transparency | ✅ | `PropertyChanged("Transparency", val)` → updates `BasePart.transparency` |
| Edit Anchored | ✅ | `PropertyChanged("Anchored", val)` → updates `BasePart.anchored` |
| Edit CanCollide | ✅ | `PropertyChanged("CanCollide", val)` → updates `BasePart.can_collide` |
| Edit Rotation | 🔴 | **Read-only display only** — `Rotation` shown as `"vec3"` but no write-back handler |
| Edit Color (color picker) | 🔴 | Color displayed as hex string `#rrggbb` — **no color picker widget, no write-back** |
| Edit Material (dropdown) | 🔴 | Material displayed as `"{:?}"` enum string — **no dropdown selector, no write-back** |
| Edit Size (BasePart.size) | 🔴 | **Not in PropertyChanged handler** — only Transform.scale is handled, not BasePart.size |
| Edit Locked | 🔴 | **Not in PropertyChanged handler** |
| Edit CastShadow | 🔴 | **Not in PropertyChanged handler** |
| Edit Reflectance | 🔴 | **Not in PropertyChanged handler** |
| Enum property dropdowns | 🔴 | All enum properties display as raw strings — **no dropdown/combobox widget** |
| Vector3 inline editor (X/Y/Z fields) | 🔴 | Vectors display as `"1.00, 2.00, 3.00"` string — **no per-axis inline fields** |
| Multi-select property editing | 🔴 | Properties panel only shows single entity — **no batch editing** |
| Undo integration for property edits | 🔴 | Property changes go directly to ECS — **no undo recording** |

### Output Console

| Feature | Status | Notes |
|---------|--------|-------|
| Display log messages | ✅ | `OutputConsole` resource + `sync_output_to_slint` |
| Log levels (Info/Warn/Error) | ✅ | `LogLevel` enum with color coding |
| Clear output | ✅ | Callback wired |
| Filter by level | 🔴 | `output.slint` has filter UI but **Rust-side filtering is stub** |
| Copy log text | 🔴 | **Not implemented** |
| Capture Bevy engine logs | 🔴 | `capture_bevy_logs()` is **empty stub** — only manual `out.info()` calls work |

### Script Editor

| Feature | Status | Notes |
|---------|--------|-------|
| Open script in tab | ✅ | `SlintAction::OpenScript` → center tab system |
| Syntax highlighting | 🔶 | `script_editor.slint` has basic text area — **no real syntax highlighting** |
| Build script | 🔶 | `SlintAction::BuildScript` logs message — **no actual Soul compilation trigger** |
| Auto-complete | 🔴 | **Not implemented** |
| Error markers | 🔴 | **Not implemented** |
| Find/Replace | 🔶 | `find_dialog.slint` exists — **Rust-side search logic is stub** |
| Multiple script tabs | ✅ | Center tab system supports multiple script tabs |

### Toolbox Panel

| Feature | Status | Notes |
|---------|--------|-------|
| Part insertion (Block/Ball/Cylinder/Wedge/Cone) | ✅ | `SlintAction::InsertPart` → `SpawnPartEvent` |
| Model insertion | ✅ | Direct `spawn_model()` call |
| Light insertion (Point/Spot/Surface/Directional) | ✅ | Direct spawn calls |
| Effect insertion (Particles/Beam/Fire/Smoke) | ✅ | Direct spawn calls |
| Constraint insertion (Weld/Hinge/Motor) | ✅ | Direct spawn calls |
| UI element insertion (ScreenGui/BillboardGui) | ✅ | Direct spawn calls |
| Folder/Script insertion | ✅ | Direct spawn calls |
| Drag from toolbox to viewport | 🔴 | **Not implemented** — click only, spawns at (0, 5, 0) |

### Terrain Editor

| Feature | Status | Notes |
|---------|--------|-------|
| Generate terrain | 🔶 | `SlintAction::GenerateTerrain` dispatches event — terrain plugin exists |
| Brush tools (Add/Subtract/Grow/Erode/Smooth/Flatten/Paint) | 🔶 | `SlintAction::SetTerrainBrush` wired — **actual brush application partially implemented** |
| Import heightmap | 🔴 | **TODO comment** — file picker opens but no loader |
| Export heightmap | 🔴 | **TODO comment** — file picker opens but no exporter |
| Terrain material painting | 🔴 | **Not implemented** |

### Transform Tools (3D Viewport)

| Feature | Status | Notes |
|---------|--------|-------|
| Select tool (click to select) | ✅ | `select_tool.rs` fully rewritten with math_utils |
| Box selection (drag rectangle) | ✅ | `handle_box_selection` system |
| Multi-select (Shift+click) | ✅ | Shift/Ctrl modifier support |
| Move tool (axis handles) | ✅ | Camera-distance-scaled arrow gizmos, axis-constrained drag |
| Scale tool (cube handles) | ✅ | Per-axis and symmetric scaling with Ctrl |
| Rotate tool (arc rings) | ✅ | Camera-scaled arc rings, angle snapping |
| Selection box outline | ✅ | `selection_box.rs` with hover highlight, corner dots |
| Surface snapping | ✅ | Physics spatial query via Avian3D |
| Grid snapping | ✅ | `EditorSettings` snap increments |
| Undo for transforms | ✅ | `UndoStack` records `TransformEntities` and `ScaleEntities` |
| Keyboard shortcuts (Alt+Z/X/C/V) | ✅ | `dispatch_keyboard_shortcuts` system in `keybindings.rs` |

### View Controls

| Feature | Status | Notes |
|---------|--------|-------|
| Wireframe toggle | ✅ | `SlintAction::ToggleWireframe` → `ViewSelectorState.wireframe` |
| Grid toggle | ✅ | `SlintAction::ToggleGrid` → `ViewSelectorState.grid` |
| Focus on selection (F key) | ✅ | `FocusSelection` keybinding dispatched |
| Camera numpad views (Top/Front/Side) | ✅ | Keybindings registered (Numpad 2/4/5/6/8) |
| View mode switching | 🔴 | `SlintAction::SetViewMode` handler is **empty comment** |
| Wireframe rendering | 🔴 | State toggles but **no actual wireframe render pass** |
| Grid rendering | 🔶 | Grid exists in default scene but **toggle doesn't hide/show it** |

### Play Mode

| Feature | Status | Notes |
|---------|--------|-------|
| Play Solo | ✅ | `SlintAction::PlaySolo` → `play_solo_requested` → full play mode system |
| Play with Character | ✅ | Character spawning system |
| Pause | ✅ | `pause_requested` flag |
| Stop | ✅ | `stop_requested` flag |
| Play Server mode | ✅ | In-process server + embedded client |

### Networking

| Feature | Status | Notes |
|---------|--------|-------|
| Start/Stop server | ✅ | `SlintAction::StartServer/StopServer` |
| Network panel | 🔶 | `network_panel.slint` exists — **limited live stats** |
| Forge Connect | 🔶 | `SlintAction::ConnectForge` wired — **actual Forge integration incomplete** |
| Synthetic clients | 🔶 | `SlintAction::SpawnSyntheticClients` wired — **stress test partially implemented** |

### Dialogs & Windows

| Feature | Status | Notes |
|---------|--------|-------|
| Command bar (Ctrl+K) | ✅ | `command_bar.slint` + `SlintAction::ExecuteCommand` |
| Settings window | 🔶 | `settings.slint` exists — **limited settings actually persist** |
| Keybindings window | 🔶 | `show_keybindings_window` flag exists — **no Slint UI for rebinding** |
| Publish dialog | 🔶 | `publish.slint` exists — **no actual publish backend** |
| Login dialog | 🔶 | `login.slint` exists — **auth flow partially implemented** |
| Asset manager | 🔴 | `asset_manager.slint` exists — **Rust-side is empty stub** |
| AI generation panel | 🔴 | `ai_generation.slint` exists — **Rust-side is empty stub** |
| Collaboration panel | 🔴 | `collaboration.slint` exists — **Rust-side is empty stub** |
| History panel (undo list) | 🔴 | `history_panel.slint` exists — **no sync from UndoStack to Slint** |
| Soul settings | 🔶 | `soul_settings.slint` exists — **partial wiring** |
| Data sources | 🔴 | `data_sources.slint` exists — **Rust-side is empty stub** |
| Sync domain modal | 🔴 | `sync_domain.slint` exists — **Rust-side is empty stub** |

---

## Part 2: Remaining egui References

**49 occurrences** across 12 files. All are **comments or dead code** — no runtime egui dependency remains.

| File | Count | Type |
|------|-------|------|
| `ui/mod.rs` | 4 | Comments: "egui has been completely removed", "don't depend on egui", stub headers |
| `spawn.rs` | 4 | Comments: "rendered via egui overlay system", "billboard_gui.rs render_billboard_gui_egui" |
| `commands/scene_management_commands.rs` | 9 | Comments: "called directly from egui UI" (stale) |
| `commands/part_commands.rs` | 9 | Comments: "called directly from egui UI" (stale) |
| `serialization/scene.rs` | 7 | Type names containing "Gui" (not egui-specific, just class names) |
| `notifications.rs` | 1 | Comment: "egui_notify removed" |
| `default_scene.rs` | 2 | Comments: "dark background like egui era", "same as egui era" |
| `studio_plugins/mod.rs` | 1 | Comment: "egui removed - using Slint UI" |
| `classes.rs` | 1 | SurfaceGui class definition (not egui-related) |
| `ui/slint_ui.rs` | 1 | BillboardGui/SurfaceGui/ScreenGui icon mapping |

**Action:** Clean up stale "egui" comments in `commands/`, `spawn.rs`, `default_scene.rs`. Low priority but improves code hygiene.

---

## Part 3: Stub Modules That Need Real Implementation

These modules in `ui/mod.rs` (lines 548-845) are **empty shells** that exist only so other code compiles:

| Stub Module | Lines | What It Should Do |
|-------------|-------|-------------------|
| `explorer` | 548-551 | Re-exports only — actual explorer is in Slint. **OK as-is.** |
| `context_menu` | 553-562 | `ContextMenuState` resource — **needs "Insert" submenu logic** |
| `service_properties` | 564-628 | 12 service resources with empty defaults — **needs property editing for Workspace/Lighting/etc.** |
| `docking` | 630-640 | Enum stubs — **needs panel detach-to-window support** |
| `notifications` (ui) | 642-647 | Empty plugin — **actual notifications are in separate `notifications.rs`** |
| `command_bar` | 649-662 | Stub `cache_rune_script` — **needs Rune script execution** |
| `script_editor` | 664-698 | `ScriptEditorState` with empty methods — **needs real script buffer management** |
| `icons` | 701-708 | Empty draw functions — **OK, SVG icons replaced these** |
| `class_icons` | 710-720 | Returns defaults — **needs real class color/category data** |
| `view_selector` | 726-733 | Empty functions — **needs wireframe/grid render mode switching** |
| `output` | 735-741 | Empty functions — **needs Bevy log capture** |
| `dynamic_properties` | 766-771 | Empty plugin — **needs dynamic property widget generation** |
| `selection_sync` | 773-778 | Empty plugin — **needs selection→properties sync** |
| `attributes_ui` | 781-787 | Empty render — **needs Attributes/Tags panel** |
| `history_panel` | 789-791 | Empty struct — **needs UndoStack→Slint sync** |
| `property_widgets` | 793-795 | Empty render — **needs color picker, enum dropdown, vec3 editor** |
| `ai_generation` | 812-818 | Empty plugin — **needs generative pipeline UI** |
| `soul_panel` | 803-810 | Empty plugin — **needs Soul script list/status UI** |
| `cef_browser` | 840-844 | Empty plugin — **replaced by wry WebView, can delete** |

---

## Part 4: Platform Comparison — Eustress vs Roblox Studio vs Unreal Editor

### Studio/Editor UX Feature Matrix

This is the **honest** comparison of what each editor actually ships today. Eustress should use this to identify the quality gap and prioritize accordingly.

| Studio Feature | Eustress | Roblox Studio | Unreal Editor | Priority to Fix |
|----------------|----------|---------------|---------------|-----------------|
| **Properties Panel** | | | | |
| Inline property editing | 🔶 7 of ~50 props | ✅ All props | ✅ All props | **P0 — Critical** |
| Color picker widget | 🔴 Hex string only | ✅ Full HSV/RGB picker | ✅ Full picker + eyedropper | **P0** |
| Material selector dropdown | 🔴 Raw enum string | ✅ Visual material grid | ✅ Material browser | **P0** |
| Enum dropdowns | 🔴 Raw strings | ✅ Native dropdowns | ✅ Native dropdowns | **P0** |
| Vector3 per-axis fields | 🔴 Comma string | ✅ X/Y/Z drag fields | ✅ X/Y/Z drag fields | **P0** |
| Multi-select batch edit | 🔴 Single only | ✅ Batch editing | ✅ Batch editing | **P1** |
| Property search/filter | 🔴 None | ✅ Search bar | ✅ Search + categories | **P1** |
| Undo for property edits | 🔴 None | ✅ Full undo | ✅ Full undo | **P0** |
| **Explorer/Outliner** | | | | |
| Entity tree | ✅ Full hierarchy | ✅ Full hierarchy | ✅ World Outliner | — |
| Drag-drop reparent | 🔶 Works, no visual | ✅ Visual indicator | ✅ Visual indicator | **P1** |
| Multi-select in tree | 🔴 Single only | ✅ Shift/Ctrl select | ✅ Shift/Ctrl select | **P1** |
| Search/filter | 🔴 Stub | ✅ Name search | ✅ Advanced filters | **P1** |
| Inline rename (F2/double-click) | 🔴 TODO | ✅ Double-click | ✅ F2 rename | **P1** |
| **Viewport** | | | | |
| Transform gizmos | ✅ All 3 tools | ✅ All 3 tools | ✅ All 3 + universal | — |
| Selection outline | ✅ Gizmo-based | ✅ Blue outline | ✅ Orange outline | — |
| Grid rendering | 🔶 Static, no toggle | ✅ Toggleable | ✅ Toggleable + configurable | **P2** |
| Wireframe mode | 🔴 State only | ✅ Working | ✅ Multiple viz modes | **P2** |
| Snap to grid | ✅ Configurable | ✅ Configurable | ✅ Configurable | — |
| Surface snapping | ✅ Physics-based | ✅ Surface snap | ✅ Surface snap | — |
| Camera bookmarks | 🔴 None | 🔴 None | ✅ Camera bookmarks | **P3** |
| **Script Editor** | | | | |
| Syntax highlighting | 🔴 Plain text | ✅ Lua highlighting | ✅ C++ / Blueprint | **P1** |
| Auto-complete | 🔴 None | ✅ IntelliSense-like | ✅ Full IntelliSense | **P2** |
| Error markers | 🔴 None | ✅ Red underlines | ✅ Full diagnostics | **P2** |
| Breakpoints/debugging | 🔴 None | ✅ Breakpoints | ✅ Full debugger | **P3** |
| **Asset Management** | | | | |
| Asset browser | 🔴 Stub | ✅ Toolbox + Marketplace | ✅ Content Browser | **P1** |
| Import 3D models | 🔶 Code-only | ✅ Drag-drop .fbx/.obj | ✅ Full import pipeline | **P1** |
| Texture/material preview | 🔴 None | ✅ Thumbnails | ✅ Full preview | **P2** |
| **Undo/Redo** | | | | |
| Transform undo | ✅ Working | ✅ Working | ✅ Working | — |
| Property edit undo | 🔴 None | ✅ Working | ✅ Working | **P0** |
| History panel (visual list) | 🔴 Stub | 🔴 None | ✅ Full history | **P2** |
| **Collaboration** | | | | |
| Real-time co-editing | 🔴 Stub | ✅ Team Create | 🔴 None (Multi-User exists) | **P3** |
| **Output/Console** | | | | |
| Engine log capture | 🔴 Stub | ✅ Full output | ✅ Full output + categories | **P1** |
| Log filtering | 🔴 Stub | ✅ Level filter | ✅ Advanced filters | **P2** |
| Clickable error links | 🔴 None | ✅ Click to source | ✅ Click to source | **P3** |
| **Play Mode** | | | | |
| Play Solo | ✅ Working | ✅ Working | ✅ PIE | — |
| Play Server | ✅ Working | ✅ Working | ✅ Dedicated server | — |
| Character controller | ✅ Working | ✅ Working | ✅ Working | — |
| **Terrain** | | | | |
| Sculpt brushes | 🔶 Partial | ✅ Full suite | ✅ Full suite + erosion | **P2** |
| Paint materials | 🔴 None | ✅ Material painting | ✅ Layer painting | **P2** |
| Heightmap import/export | 🔴 TODO | ✅ Working | ✅ Working | **P2** |

---

## Part 5: Priority Action Items

### P0 — Ship Blockers (Properties Panel is the #1 gap)

1. **Properties write-back for ALL BasePart fields** — Rotation, Size, Color, Material, Locked, CastShadow, Reflectance, Massless, etc. Currently only 7 of ~50 properties can be edited.
2. **Color picker widget** — Slint needs an HSV/RGB color picker component. This is the single most-used property editor.
3. **Enum dropdown widget** — Material, PartType, and other enums need a ComboBox/dropdown in Slint.
4. **Vector3 per-axis editor** — Position/Rotation/Scale need individual X/Y/Z drag fields, not a comma-separated string.
5. **Undo recording for property edits** — Every `PropertyChanged` should push to `UndoStack` before applying.

### P1 — Core UX Gaps

6. **Explorer multi-select** — `ExplorerState.selected` needs to become `HashSet<Entity>`, with Shift/Ctrl support in tree.
7. **Explorer search/filter** — Wire `explorer_search.rs` to actually query entities by name/class/property.
8. **Explorer inline rename** — F2 or double-click should trigger inline text edit in the tree node.
9. **Bevy log capture** — Implement `capture_bevy_logs()` to pipe `tracing` output to `OutputConsole`.
10. **Asset browser** — Wire `asset_manager.slint` to scan asset directories and display thumbnails.
11. **Syntax highlighting** — Integrate a tokenizer for Soul/Rune scripts in the script editor.
12. **Explorer drag-drop visual** — Show insertion indicator line when dragging entities in the tree.

### P2 — Polish & Parity

13. **Wireframe render mode** — Actually switch Bevy's render pipeline when `ViewSelectorState.wireframe` is true.
14. **Grid toggle** — Show/hide the ground grid entity when `ViewSelectorState.grid` toggles.
15. **History panel** — Sync `UndoStack` entries to `history_panel.slint` for visual undo list.
16. **Output log filtering** — Filter `OutputConsole` entries by `LogLevel` in the Slint UI.
17. **Terrain material painting** — Implement multi-material terrain splatmap.
18. **Heightmap import/export** — Implement the TODO loaders.
19. **Property search** — Add search bar to properties panel to filter by property name.

### P3 — Competitive Advantage

20. **Script debugger** — Breakpoints, step-through, variable inspection for Soul scripts.
21. **Collaboration** — Wire `collaboration.slint` to real-time sync (CRDT or OT).
22. **Camera bookmarks** — Save/restore named camera positions.
23. **Clickable error links** — Output console errors link to script line numbers.
24. **Panel detach** — `SlintAction::DetachPanelToWindow` — pop panels into separate OS windows.

---

## Part 6: Updated Platform Comparison (for `home.rs`)

The current comparison table on the website has 10 rows. Below is a more honest and expanded version that accounts for **Studio/Editor quality** — the area where Roblox and Unreal currently lead.

### Current Scores (Honest Assessment)

| Category | Eustress | Roblox | Unity | Unreal |
|----------|----------|--------|-------|--------|
| Learning Curve | ✅ Super Easy | ✅ Easy | ~ Medium | ✗ Hard |
| Runtime Performance | ✅ Native Rust | ~ Lua VM | ~ C# + Mono | ✅ C++ |
| Memory Safety | ✅ Guaranteed | ✅ Sandboxed | ~ GC Pauses | ✗ Manual |
| Web Export | ✅ Native WASM | ✗ None | ~ WebGL | ✗ Limited |
| Multiplayer | ✅ Built-in | ✅ Built-in | ~ Paid Add-on | ✅ Built-in |
| Hot Reload | ✅ Instant | ✅ Fast | ~ Slow | ~ C++ Rebuild |
| Pricing | ✅ Revenue Share | ~ Revenue Share | ✗ Per Seat | ~ 5% Royalty |
| Max Instances | ✅ 10M+ | ~ 100K | ~ 500K | ✅ 1M+ |
| **Studio UX Quality** | **~ Partial** | **✅ Polished** | **✅ Polished** | **✅ AAA** |
| **Properties Editing** | **✗ 7/50 props** | **✅ All props** | **✅ All props** | **✅ All props** |
| **Script Editor** | **✗ Plain text** | **✅ Full IDE** | **✅ Full IDE** | **✅ Full IDE** |
| **Asset Pipeline** | **~ Code-only** | **✅ Drag-drop** | **✅ Content Browser** | **✅ Content Browser** |
| Pro Workflows | ✅ Full Suite | ✗ Basic | ~ Plugins | ✅ Built-in |
| Data Formats | ✅ Mesh, PCD, CAD | ✗ Mesh Only | ~ Mesh, PCD | ✅ Mesh, PCD |
| XR/VR Support | ✅ OpenXR native | ✗ None | ~ Plugin | ✅ Built-in |
| AI Integration | ✅ Soul Language | ✗ None | ~ Third-party | ~ Third-party |

### Honest Overall Scores

| Engine | Score | Rationale |
|--------|-------|-----------|
| **Eustress** | **7.5/10** | Best-in-class runtime (Rust, WASM, ECS, networking), but Studio UX is 40% stub. Properties panel and script editor are the biggest gaps. |
| **Roblox** | **7.0/10** | Polished Studio UX, but limited to Lua, no web export, 100K instance cap, walled garden. |
| **Unity** | **7.0/10** | Mature editor, huge ecosystem, but GC pauses, per-seat pricing, slow iteration. |
| **Unreal** | **8.5/10** | AAA editor quality, C++ performance, massive toolset. But steep learning curve, 5% royalty, no web export. |

**Key insight:** Eustress's runtime/architecture is already ahead of Roblox and competitive with Unreal. The gap is **entirely in Studio UX polish**. Fixing the P0 items (properties panel, color picker, undo) would move Eustress to **8.5/10** and close the gap with Unreal.

---

## Part 7: Recommended Comparison Table Update for `home.rs`

The current `home.rs` table claims **9.5/10** for Eustress. This is aspirational but not honest given the Studio UX gaps. Two options:

**Option A (Honest):** Drop to 7.5/10 now, raise as P0 items ship.  
**Option B (Split score):** Show separate "Runtime" and "Editor" scores:
- Eustress Runtime: 9.5/10 | Eustress Editor: 6.0/10  
- This is more transparent and motivating.

**Recommended:** Add a "Studio/Editor" row to the comparison table and keep the overall score honest. Users who try the Studio and find broken properties will lose trust if the website claims 9.5/10.

---

## Part 8: Clean-Up Tasks

| Task | Effort | Impact |
|------|--------|--------|
| Remove stale "egui" comments from `commands/`, `spawn.rs`, `default_scene.rs` | 30 min | Code hygiene |
| Delete `cef_browser` stub (replaced by wry WebView) | 5 min | Dead code removal |
| Consolidate duplicate stub functions (`capture_bevy_logs`, `push_to_log_buffer`, etc.) | 15 min | Reduce confusion |
| Remove `StudioUiPlugin` legacy wrapper (line 903-914) | 5 min | Dead code removal |
| Update `MIGRATION_PLAN.md` — all egui code examples are stale | 1 hr | Doc accuracy |
| Update `IMPLEMENTATION_STATUS.md` — Phase 2/3 status is outdated | 30 min | Doc accuracy |

---

## Conclusion

The Slint migration is **structurally complete** — the UI framework swap is done, callbacks are wired, and the visual shell is professional. The remaining work is **depth, not breadth**: making every panel actually functional rather than adding new panels. The Properties Panel is the single highest-impact fix — it's the most-used panel in any editor and currently only handles 7 of ~50 properties.

**If you fix one thing:** Properties panel write-back + color picker + enum dropdowns.  
**If you fix five things:** Add undo for properties, explorer multi-select, Bevy log capture, syntax highlighting, asset browser.  
**If you fix everything P0+P1:** Eustress Studio matches Roblox Studio quality and exceeds it in runtime capabilities.
