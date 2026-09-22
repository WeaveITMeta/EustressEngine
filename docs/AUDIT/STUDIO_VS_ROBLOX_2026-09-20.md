# Studio building experience: Eustress vs Roblox Studio (2026-09-20)

Goal: beat Roblox Studio on ease of use and functionality for building in 3D.
This is a comparison of what a builder touches every minute: selecting, tools,
right-click menus, the Explorer, inserting, files, lock/anchor, saving, history.

## Evidence

- Click timings come from the engine's own log of the user's editing session on
  the 2026-09-13 release binary (`~/.eustress_engine/logs/engine-90028.log`,
  Super Station, 2,019 visible Explorer nodes). Every viewport click is logged
  by `part_selection`, the Explorer push by `sync_unified_explorer_to_slint`,
  and stalls by `frame_diagnostics`.
- Live checks ran over the engine bridge against the 2026-09-19 debug binary
  (select, tool switch, editor state). Screen control was declined, so nothing
  below relies on driving the mouse.
- Menus, shortcuts and handlers were read from
  `eustress/crates/engine/ui/slint/*.slint` and `eustress/crates/engine/src`.
- Roblox behaviour is from the current Creator Docs (Explorer, Parts,
  Properties, playtest modes) plus long-standing Studio defaults.

## Scorecard

| Area | Roblox Studio | Eustress today | Verdict |
| --- | --- | --- | --- |
| Time to select | One click, no hitch, at 100K instances | One click selects in the same frame. The Explorer then rebuilds and re-pushes all 2,019 nodes; 3 of 5 clicks were followed by a 308 to 451 ms stall and the Explorer highlight landed 180 to 360 ms after the click | Behind |
| Hover feedback | Every part outlines on hover | No hover outline in edit mode. `selection_box.rs` has a hover material and a `Hovered` component, nothing inserts it; the only hover system (`interaction/click.rs`) is play-mode and needs a local character | Behind |
| Select semantics | Click = Model, Alt+click = part, Ctrl/Shift multi, box select, drag with Select moves | Same: Model first, Alt+click part, Ctrl/Shift add and remove, box select, Select-tool drag with surface snap and undo | Parity |
| Transform tools | Move/Scale/Rotate, increments on the toolbar, Shift toggles snap, Ctrl+L local, Ctrl+R/Ctrl+T 90 degrees, Collisions toggle | Same handles and chords (Ctrl+L, Ctrl+R, Ctrl+T); increments only in Settings or the 1/2/3 keys (1 m, 0.2 m, off); no collision-aware drag | Slightly behind |
| Insert | Part button with 5 shapes; Insert Object dialog (Ctrl+I) with search over every class; hover plus button on each Explorer row | Ribbon dropdown of about 40 classes, no search, no Ctrl+I, no per-row plus button; `InsertObjectEvent` has no producer; parts land camera-forward in mid-air (kept by design) | Behind |
| Viewport right-click | Cut, Copy, Paste Into, Duplicate, Delete, Rename, Group, Ungroup, Select Children, Zoom To, Insert Part, Insert Object | Insert Part Here, Paste Here (at the click point), Focus, Zoom to Selection, Duplicate, Copy, Cut, Toggle Anchor, Toggle Lock, Group, Ungroup, Copy Path, Delete. No Rename, Paste Into, Select Children, Insert Object | Mixed: better placement items, fewer edit items |
| Explorer right-click | Adjusted per object type (Open Script on scripts, Ungroup only on Models, Insert Object, Zoom To, Paste Into) | One list for every instance type (`main.slint` builds the same items for Part, Model, Script and Service, all enabled): Select Children/Descendants/Parent/Siblings/Invert, Cut, Copy, Paste, Duplicate, Delete, Group, Ungroup, Rename, Copy Path. No Insert, Zoom To, Paste Into, Anchor/Lock, Open Script | Behind |
| Explorer search | `is:Class`, `tag:Name`, `Prop = value`, ancestry with `.`, and/or, Ctrl+Shift+X focuses it | Name and label substring only, ancestors kept visible; no operators, no focus shortcut | Behind |
| Explorer navigation | Arrows, Home/End, F2, drag-drop reparent, expand on select | Arrows, F2, drag-drop reparent (disk and Fjall), expand-all/collapse-all, paging for huge trees | Parity |
| Properties | Filter (Ctrl+Shift+P), sections, Vector3 as one field or three, Color3 and BrickColor pickers, attributes with 15 types, tags | Filter box (no shortcut), Appearance, Metadata, Transform, Physics, Attributes order, vec3/rotation/udim2/slider/enum/material/colour editors, 7 colour wheels with RGB and hex, display units, attributes and tags. Multi-select: Position and Rotation apply as a group delta, the rest broadcast | Ahead |
| Lock and Anchor | Anchor button toggles the selection; Lock tool is click-to-toggle; Unlock All; Alt+A, Alt+L | Alt+A and Alt+L toggle the selection (or arm paint mode with nothing selected); the ribbon Anchor and Lock buttons ALWAYS arm paint mode even with a selection (`slint_ui.rs` "edit:anchor"); Unlock All present | Slightly behind (button semantics) |
| Save and autosave | Ctrl+S, asterisk in the title, prompt on close, autosave file every 5 min with recovery | Live edits persist continuously to WorldDb (`world_db_plugin.rs` mirrors Changed Transform/BasePart); Ctrl+S exports TOML, saves terrain and makes a git commit; autosave is a git commit every 5 min. `has_unsaved_changes` has zero writers, so the title asterisk and exit confirmation never fire | Different model, ahead on safety, unclear to a Roblox user |
| Undo and History | Ctrl+Z/Ctrl+Y, History window, click to revert to a point | Ctrl+Z, Ctrl+Y and Ctrl+Shift+Z, 33 undo action kinds, History panel with Revert to Here and Undo This Event (single event out of the middle) | Ahead |
| File menu | New, Open, Open Recent, Save, Save As, Publish, Game Settings, Studio Settings, Advanced, Close Place, Exit | New Universe, New Space, Open, Import Roblox Place, Save, Save As, Publish Universe, Publish Space, Settings, Exit. No Open Recent, no Close Space, Export lives in the Model tab | Slightly behind |
| Shortcuts | Fixed defaults, customisable via File > Advanced | 70 actions, primary plus alternates, rebind dialog with conflict refusal. Divergences: tools Alt+Z/X/C/V vs Shift+1..4, F8 stops (Roblox F8 runs, Shift+F5 stops), Ctrl+I inverts selection (Roblox: Insert Object), Ctrl+Shift+P publishes (Roblox: Properties filter) | Ahead on system, behind on muscle memory |
| Play controls | F5 Test, F8 Run, Shift+F5 Stop | F5 play with character, F6 pause, F7 play solo, F8 stop | Parity, different keys |

## Ranked gaps (what to fix to beat Roblox)

1. Selection stall. Every selection change takes the `needs_immediate_sync` path,
   rebuilds the whole tree and pushes a new `VecModel` of every visible node
   (`slint_ui.rs:19788`, `set_tree_nodes`). At 2,019 nodes that is a 300 to 450 ms
   frame. Update the `selected` bit on the affected rows and leave the model in
   place; rebuild only on structure changes. Target: no frame over 33 ms on select.
2. Hover outline in edit mode. Reuse the existing hover material and `Hovered`
   component: raycast under the cursor each frame in the Select tool (the same
   hit test `part_selection` uses on click) and insert/remove `Hovered`.
3. Anchor and Lock buttons: with a selection, toggle it (the chord already does);
   arm paint mode only when nothing is selected or from a dropdown arrow.
4. Insert Object: a searchable class picker on Ctrl+I and on a per-row plus
   button in the Explorer, inserting under the hovered node. The catalog
   (`insert_classes::build_catalog`) and the insert handlers exist; the dialog
   and the two entry points do not.
5. Explorer search operators: `is:Class`, `tag:Name`, `Prop = value`, plus a
   shortcut that focuses the box (Ctrl+Shift+X) and one for the Properties
   filter (Ctrl+Shift+P is taken by Publish Space; pick another or move Publish).
6. Context menus per type: Open Script on scripts, Ungroup only on containers,
   Insert Object / Insert Part on containers and services, Zoom To, Paste Into
   (new action, Ctrl+Shift+V), Anchor and Lock toggles in the Explorer menu,
   Rename and Select Children in the viewport menu.
7. Make Save legible: define what Ctrl+S means (snapshot), show the last
   snapshot time in the status bar, and either wire `has_unsaved_changes` from
   the op-log or delete the dead asterisk and exit-confirm code.
8. Snap increments on the ribbon (move and rotate fields with a toggle) and a
   Collisions toggle for drags.
9. A "Roblox" keymap preset using the existing alternates map: Shift+1..4 tools,
   Shift+F5 stop, F8 run, Ctrl+I insert.
10. File menu: Open Recent, Close Space, Export Selection.

## Status (2026-09-20, same day)

All ten gaps were implemented in source; none is verified in a running build
yet. What changed, and where to look if something misbehaves:

1. Selection stall: a selection-only change now patches the `selected` bit on
   the rows already in Slint (`patch_explorer_selection`, `slint_ui.rs`)
   instead of rebuilding and pushing the tree. The full rebuild still runs
   for expand/collapse, structure churn and a reveal that expands ancestors.
   Expected: no `STUTTER DETECTED` line after a click; the log prints
   "selection patched in place" at debug level.
2. Hover outline: `part_selection::hover_highlight_system` runs the same pick
   as the click (`pick_under_cursor`, now shared) on cursor move and marks
   `Hovered`; the existing `selection_box` hover wireframe draws it. Alt
   previews the part instead of its Model.
3. Ribbon Anchor and Lock act on the selection when there is one
   (`"edit:anchor"` / `"edit:lock"` in `slint_ui.rs`), paint mode otherwise.
4. Insert Object: `insert_object_dialog.slint`, opened by Ctrl+I
   (`Action::InsertObject`), the Explorer row plus button (`TreeItem` in
   `theme.slint`, `insert-into`), the ribbon "Insert Object..." item and
   the context menus. Rust filters the catalog per keystroke.
5. Explorer search language: `ui/explorer_query.rs` (`is:`, `tag:`,
   `Prop = value`, `or`, `-term`, quotes) with tests. Ctrl+Shift+X focuses
   the Explorer search, Ctrl+Shift+E the Properties filter.
6. Context menus per type: `ctx-items-*` in `main.slint`, chosen from the
   row's class; new ids `open-script`, `insert-object`, `insert-part`,
   `paste-into`, `zoom-to`. `Action::PasteInto` (Ctrl+Shift+V) pastes into
   the primary selection's folder (`clipboard.rs`). The viewport menu gained
   Rename and Select Children.
7. Save legibility: `has_unsaved_changes` now means "edits since the last
   snapshot" (undo sequence vs `saved_undo_sequence`); the File menu shows
   "Snapshot 12:03" / "Autosaved 12:08"; the exit dialog explains that edits
   are already stored and offers Snapshot & Exit.
8. Ribbon Snap group (toggle, Move and Rotate increments) and a Collisions
   toggle; `move_tool::enforce_drag_collisions` stops a drag at the first
   overlap when it is on.
9. Keymap presets: `KeymapPreset::{Eustress, Roblox}` with a preset row in
   the Keyboard Shortcuts dialog; rebinding by hand marks the map "custom".
   Default chords also moved: Ctrl+I inserts (Invert is Ctrl+Shift+I),
   Ctrl+Shift+V pastes into.
10. File menu: Recent (last eight Spaces, `EditorSettings.recent_spaces`),
    Export Selection, Close Space (snapshot, then the Universe browser).

## Drag and placement fixes (2026-09-21)

Reported: drags collided with the dragged object itself and climbed ("ghosting
iteration"), box select grabbed anything the rectangle brushed, multi-part
drags threw some parts off by an unknown distance, paste lost the chosen
colour, stacking and surface alignment misbehaved, and the Rotate tool drag
ghosted too. Causes found in code, and what changed:

1. Self-collision. The Move tool excluded only the selection's DIRECT
   children from its surface raycast and from face-contact / guide
   candidates, so a Model (or a MeshPart whose mesh is a child node) landed
   on its own grandchildren every frame. The Select tool also accepted any
   mesh as a surface, gizmo handles and selection wireframes included, so a
   part could snap onto its own handle. Now every drag builds a moving set
   (selection plus all descendants, `math_utils::moving_set`) and excludes it
   from surfaces, face contacts, geometry snaps and smart guides; only
   entities with a BasePart or Instance are surfaces; terrain and mesh
   colliders still count through a physics pass (`find_drag_surface`).
2. Double drive. The Select tool re-implemented the Scale and Rotate handle
   hit tests with different bounds (parts only, the tools include children),
   so a ring drag could also start a body drag and two systems wrote the
   transform each frame. The Select tool now stands down whenever a tool has
   engaged (`dragged_axis`), and no longer duplicates their hit tests.
3. Mixed frames. Drag maths mixed a parented entity's LOCAL translation with
   world-space targets; anything inside a Model (every Ctrl+G result) jumped
   by its parent's position. Select, Move, Rotate and Scale now snapshot
   world poses (`initial_world`), compute in world space and write back
   through each entity's parent (`math_utils::world_to_local_pose`). Undo
   still records local poses.
4. Box select tests the projected CENTER of each part against the rectangle.
5. Paste rewrote only the copied folder's position; colour, rotation, scale
   and the other BasePart properties came from whatever was last on disk.
   `patch_root_toml_with_live_state` now writes the values captured at copy.
6. Placement rests the WHOLE selection on the surface
   (`group_support_distance`), not just the leader, so a taller companion no
   longer sinks; face-frame grid snapping and edge snapping are unchanged.

## Already ahead of Roblox

- Group delta for multi-select Position and Rotation (Roblox collapses every
  selected part onto one value).
- History panel with Undo This Event and Revert to Here; redo on Ctrl+Shift+Z too.
- Continuous persistence to WorldDb plus git snapshots on save and autosave; a
  crash loses nothing.
- Insert Part Here and Paste Here at the click point in the viewport menu.
- Seven colour wheels with named swatches, RGB and hex entry; display units
  (m, cm, mm, ft, in, studs) applied at read and write.
- Hierarchy selection commands (children, descendants, parent, siblings, invert)
  and Copy Path on every node.
- Rebindable shortcuts with alternates and conflict refusal; drag-drop reparent
  that survives reload; Explorer paging for six-figure trees.
- Everything above is driveable over the engine bridge, so an agent can build
  alongside the user.
