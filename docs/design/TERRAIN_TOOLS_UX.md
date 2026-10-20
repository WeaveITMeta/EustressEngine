# Terrain Tools UX: Molding Ground That Feels Right

**Status:** ACTIVE design for the terrain molding tools (2026-09-25). It owns
how sculpting, painting, water and region editing feel in the Studio. The
terrain data model lives in `docs/development/TERRAIN_ARCHITECTURE.md`; the
visual rules every tool follows live in `docs/design/DRAFTING_UX.md` and
`docs/development/TOOLSET_UX.md`. A change to the Terrain tab, the terrain
tool bar, the brush cursor or the terrain hotkeys conforms to this document
or changes it first.

The benchmark is Roblox Studio's Terrain Editor. The bar is to match every
tool and setting it has, keep its shortcuts where a Roblox creator's hands
already know them, then beat it on feedback: the user always sees exactly
what the next stroke will touch, where it snaps, and how hard it pushes,
before pressing the mouse.

---

## 1. The finished session (narrative spec)

1. **Enter.** The user opens the Terrain tab and clicks **Sculpt** (or
   presses `T`, then `2`). The Studio switches to the terrain tools the way it
   switches to Move: the Select and Move buttons go quiet; part selection, the
   marquee and the transform handles stand down; the Sculpt button lights
   cyan. A glass **terrain tool bar** appears at the top left of the
   viewport: seven tool chips, each with its digit, then the settings Sculpt
   uses.

2. **Aim.** Over the ground, a ring lies on the surface, bending over every
   bump, with a soft disc inside it whose opacity is the push the brush
   applies at each point. A thinner ring marks where the push falls to half.
   A small readout trails the cursor: `Grow · 16 m · 40% · Y 23.4 m · Grass`.
   Nothing has changed yet.

3. **Stroke.** The user drags. The ground swells smoothly under the ring, at
   the same rate at 30 or 240 frames per second, with dabs laid a quarter of
   the brush apart so a fast drag leaves an even ridge. The cursor stays
   visible throughout. Releasing the mouse makes one undo entry, "Grow
   Terrain".

4. **Invert and smooth.** Holding `Ctrl`, the ring turns orange and the
   readout says `Erode`; the stroke cuts instead. Holding `Shift`, the ring
   turns cyan and says `Smooth`. Letting go restores Grow.

5. **Size and strength.** Holding `B` and dragging (or scrolling) sizes the
   brush, `Shift+B` sets the strength, exactly as in Roblox; `[` and `]` step
   the size too. The ring and readout follow at once, and the camera ignores
   the wheel while `B` is down.

6. **Build flat.** The user picks **Draw** (`1`), the cube shape, pivot
   Bottom, and presses `P`. A translucent plane appears at the height of the
   ground under the cursor (never at the world origin), gridded, with its
   height in the readout. The cube wireframe slides along that plane and
   every dab is clipped to it, so a drag builds a dead-flat plateau.
   `PageUp` raises the plane one grid step. `G` turns on snap: a local grid
   fades in around the cursor, with height contours, and the cube jumps from
   cell to cell.

7. **Mirror.** `M` puts a mirror plane through the cursor. A dimmed ghost of
   the brush follows on the far side, and every stroke lands on both sides in
   one undo entry.

8. **Paint.** `5` picks Paint. The material swatch opens a picker of real
   texture thumbnails. `I` over the ground samples the material under the
   cursor into the swatch.

9. **Leave.** `Esc` (or clicking the lit tool, `T`, or `Alt+Z`) returns to
   Select. The bar and the cursor meshes go, and part selection is live
   again. The brush settings are still there next session.

---

## 2. The Roblox baseline

Roblox Studio's Terrain Editor as documented and shipped (sources in
section 12):

- **Tabs and tools.** Create: Import, Generate, Clear. Edit: Select,
  Transform, Fill (the region tools) and Sea Level, then five brushes: Draw
  (Add or Subtract mode, the old Add and Subtract tools), Sculpt (Grow or
  Erode, the old Grow and Erode tools), Smooth, Paint (with a Replace mode,
  the old Replace tool) and Flatten.
- **Brush settings.** Shape (sphere, box, cylinder), size 1 to 64 studs,
  height for box and cylinder, strength 0.1 to 1 (Sculpt, Smooth, Flatten),
  pivot (bottom, center, top), snapping to the voxel grid, plane lock (Auto
  tilts with the camera; Manual has an editable plane), flatten mode (Erode to
  Flat, Grow to Flat, Flatten All) and a fixed flatten plane, Ignore Water,
  Ignore Parts, Auto Material, material picker tiles, Source and Target for
  Replace. Brush settings persist between sessions.
- **Shortcuts.** Hold `Ctrl` for the alternate mode (Draw, Sculpt); hold
  `Shift` for Smooth; `B` with a drag or scroll for size, `Ctrl+B` for
  height, `Shift+B` for strength; `Alt`+click for the material picker;
  `Ctrl` C, V, X, D and Delete in Select. No hotkeys switch tools, and none
  can be rebound.
- **Feedback.** A light 3D cursor of the brush shape, hidden while
  stroking; a plane for plane lock and Flatten; a wireframe of the selected
  region; progress bars on large operations. No numeric readout at the
  cursor.
- **What creators complain about.** The cursor is hard to see on snow and
  grey ground and disappears mid-stroke; the manual plane jumps to the world
  origin, far from the work; Draw without plane lock streaks lines across the
  map; brush lag and frame drops; Smooth and Flatten remove too much and leave
  jagged edges; resizing with `B` can break the cursor; material previews
  all look alike; no presets, no contour lines, no symmetry, no grid.

## 3. Where Eustress beats it

| Area | Roblox | Eustress |
|---|---|---|
| Brush footprint | A light shape cursor, hidden while stroking | A surface-hugging ring and a strength-shaded falloff disc that stay visible while stroking, plus the exact 3D volume for Draw |
| Strength | A slider value | Seen on the ground as disc opacity, with a half-strength ring |
| Readout | None | Tool, size, strength, height and material beside the cursor |
| Plane lock | Hard to see; the manual plane can jump to the origin | A gridded plane picked from the surface under the cursor with `P`, height shown, nudged with `PageUp`/`PageDown`, typed in the bar |
| Grid | Snap only | A local grid around the cursor that fades with distance, on the ground or the plane, at the snap step |
| Contours | None (requested) | Height contour lines in the local grid |
| Symmetry | None (requested) | Mirror across X, Z or both, with a ghost cursor, one undo entry |
| Stroke | Lags; streaks across the map | Distance-spaced dabs, time-based rate, a jump never strings dabs across the map, optional stroke smoothing |
| Tool switching | Clicks only | One key per tool (`1` to `7`), shown on the chips and ribbon buttons |
| Shortcuts | Fixed | The Roblox set, plus more, all rebindable in the shortcut editor |
| Materials | Previews look alike | Real texture thumbnails, and `I` samples the ground |
| Presets | None (requested) | Named brush presets, and settings that persist |
| Water | Brushes can erase it (Ignore Water) | Water is its own layer; no brush destroys it; Sea Level edits it |
| Undo | Per stroke | Per stroke, labelled per tool, in the History panel, mirror included |
| Scripting | Luau API | The same edits from Luau, Rune and MCP (`TerrainCommand`) |

---

## 4. Interaction model

### 4.1 Terrain tools are a tool, like Move

- Choosing any terrain tool (ribbon button, tool bar chip, its hotkey, or
  `T`, which brings back the last one) sets the Studio's current tool to
  **Terrain** and remembers which terrain tool is active. The Home tab's
  Select, Move, Scale and Rotate buttons show unselected.
- While the current tool is Terrain: click-select, marquee select, part
  drag and the move, scale and rotate handles do nothing; hover outlines on
  parts stay off.
- The camera keeps every gesture it has: right-drag look, middle-drag pan,
  wheel zoom, `Alt`+left-drag orbit, fly keys. A left press with `Alt` held
  never sculpts. The wheel sizes the brush instead of zooming only while `B`
  is held.
- Leaving: `Esc`, clicking the lit tool again, `T`, any of `Alt+Z`, `Alt+X`,
  `Alt+C`, `Alt+V`, or starting Play. `Esc` first cancels an open plane,
  region or sea level drag, then leaves.
- Terrain tools need terrain. With none in the Space, choosing a tool opens
  the Terrain panel's empty state (Flat, Generate World, Import) with the
  toast "Add terrain to sculpt it".
- During Play the terrain tools are off: the tool bar hides, the cursor
  meshes go and the hotkeys are inert.

### 4.2 A stroke

- A stroke starts only on a left press **inside the viewport** with no panel
  or text field holding focus. A drag that begins on the ribbon or a panel
  never sculpts, even after it enters the viewport.
- The brush centre is the cursor's hit on the terrain surface (caves and
  overhangs included), or on the locked plane when plane lock is on, then
  snapped when snap is on, then offset by the pivot.
- **Dabs are spaced by distance:** a dab lands each time the centre has
  moved a quarter of the brush radius along the path; the gap between two
  frames is filled with evenly spaced dabs, at most 16 per frame. A jump
  longer than that (the cursor leaving a near hill for ground far behind it)
  starts the path afresh at the new point, so no line of dabs is strung
  across the map.
- **Holding still keeps working at a fixed rate:** Sculpt, Smooth, Flatten
  and Paint dab 20 times a second while the button is held and the centre
  has not moved a quarter radius. Draw adds or carves a CSG shape: repeating
  a dab in place changes nothing, so holding still on a locked plane builds a
  clean flat top, and holding still without a plane builds toward the camera,
  since the surface the last dab made is the next hit (at most 8 dabs a
  second, faster with strength).
- **Stroke smoothing** (off by default; 0 to 100 percent in the advanced
  strip): the brush centre trails the cursor on a leash of that fraction of
  the brush radius, so hand jitter does not print into the ground. A thin
  line joins cursor and centre while it trails.
- Release closes the stroke: one undo entry labelled with what it did
  ("Grow Terrain", "Erode Terrain", "Draw Terrain", "Subtract Terrain",
  "Smooth Terrain", "Flatten Terrain", "Paint Terrain", "Replace Terrain
  Material"). Undo and redo are refused while a stroke is open ("Finish the
  terrain stroke first").

### 4.3 Modifiers

| Held | Effect |
|---|---|
| `Ctrl` | The alternate mode: Draw Add and Subtract swap, Sculpt Grow and Erode swap, Flatten's Erode and Grow modes swap (Flatten All stays), Sea Level Fill and Evaporate swap. Smooth, Paint and Region ignore it. |
| `Shift` | Smooth, from any brush, at the current size and strength. |
| `Alt` | Camera only. Terrain never reads a left press with `Alt` held. |

The cursor colour and readout change the moment a modifier goes down, so the
user sees the swap before the click. The modifiers held at the press fix the
tool for that stroke; pressing or releasing one mid-stroke takes effect at
the next press.

### 4.4 Tools

| Key | Tool | Modes | A stroke or apply does |
|---|---|---|---|
| `1` | **Draw** | Add, Subtract | Add unions the brush volume into the terrain, filled with the active material (or, with Auto material on, the material under the cursor at stroke start): builds walls, overhangs and bridges. Subtract carves it out, heightfield included: caves, tunnels, holes. |
| `2` | **Sculpt** | Grow, Erode | Raises or lowers the surface under the footprint, shaped by strength and hardness. |
| `3` | **Smooth** | | Relaxes heights toward their neighbours under the footprint, and rounds 3D edits (caves, overhangs) inside the brush volume. |
| `4` | **Flatten** | Erode to Flat, Grow to Flat, Flatten All | Levels toward a target height: the locked plane when plane lock is on, otherwise the surface height where the stroke started, fixed for the stroke. |
| `5` | **Paint** | Paint, Replace | Paint lays the active material into the material map. Replace paints the target material only where the ground's material is the source material. |
| `6` | **Sea Level** | Fill, Evaporate | Drag a rectangle on the ground, then drag the level plane up or down (or type it): Fill puts water in the air below the level, Evaporate removes water. One undo entry per apply. |
| `7` | **Region** | Select, Transform, Fill | Select drags a box on the ground (snap-aware) with face handles to resize; `Ctrl` C, X, V, D and Delete copy, cut, paste, duplicate and delete what it holds, with Paste showing a ghost until a click places it. Transform moves the box's contents with a drag (PageUp/PageDown raise them) and turns and scales them by the bar's Angle and Scale, Enter applies, Esc cancels. Fill fills the box with the active material, or replaces the source material with the target. |

### 4.5 Brush settings

| Setting | Values | Applies to | Notes |
|---|---|---|---|
| Size | 0.5 m to 256 m, in the display unit | every brush | The brush diameter. `B`+drag or scroll sets it; `[` `]` step it by a factor of 1.25. |
| Height | 0.5 m to 256 m | Draw with box or cylinder | Defaults to the size and follows it until set. `Ctrl+B`+drag or scroll. |
| Strength | 1 to 100 percent | Sculpt, Smooth, Flatten, Paint | `Shift+B`+drag or scroll; `Shift+[` `Shift+]` step it by 10 points. |
| Shape | Sphere, Box, Cylinder | every brush | The footprint is the shape's cross-section: a disc for sphere and cylinder, a square for box. |
| Hardness | Soft, Smooth, Hard | Sculpt, Smooth, Flatten, Paint | The falloff curve, drawn in the disc. |
| Pivot | Bottom, Center, Top | Draw | Where the volume sits on the hit: Bottom builds up from the ground, Top digs down. `,` and `.` cycle. |
| Plane lock | on/off plus height | Draw, Sculpt, Smooth, Paint | See 4.6. Flatten uses the same plane as its target. |
| Snap to grid | on/off plus step 0.25 m, 0.5 m, 1 m, 2 m, 4 m, 8 m | every brush, Sea Level, Region | See 4.7. |
| Contours | on/off | every brush | Height contour lines in the local grid (4.7). |
| Mirror | off, X, Z, X and Z | every brush | See 4.8. |
| Auto material | on/off | Draw Add | Fills with the material under the cursor at stroke start. (Sculpt moves the surface, which keeps its material.) |
| Material | material slot | Draw, Paint (target), Fill | The swatch opens the thumbnail picker. |
| Source material | material slot | Paint Replace, Region Fill replace | Picked in the picker or with `I`. |
| Stroke smoothing | 0 to 100 percent | every brush | Advanced strip. |
| Presets | named sets of all of the above | every brush | Advanced strip: save, choose, delete. |

Size is kept per brush family (the volume brushes Draw; the surface brushes
Sculpt, Smooth, Flatten, Paint) and strength per tool, so switching from a
wide Sculpt to a narrow Paint resizes neither. Every setting persists between
sessions in the Studio's user settings; presets persist the same way.

### 4.6 Plane lock

- `P` turns plane lock on at the surface height under the cursor (or at the
  last height when the cursor is off the terrain), and off again. The plane
  is never placed at the world origin.
- While it is on, the brush centre is the cursor ray's crossing of that
  horizontal plane, never the ground. The plane shows as a translucent disc
  three brush radii wide, gridded at the snap step, with a brighter rim; its
  height is in the readout and in the bar, where it can be typed.
- `Shift+P` re-picks the height from the surface under the cursor.
  `PageUp`/`PageDown` move it one grid step (`Shift` for ten).
- Draw clips to the plane: Add with pivot Bottom fills from the ground up to
  the plane and never above it; Subtract carves from the plane up. Sculpt
  Grow stops at the plane, Erode stops at it from above. Flatten levels to it.

### 4.7 Snap, the local grid and contours

- `G` toggles snap; `Shift+G` cycles the step (default 1 m).
- With snap on, the brush centre snaps to the grid in X and Z, and in Y for
  Draw (to the plane when it is locked), so 3D building lands on a lattice.
- A local grid is drawn around the cursor while snap is on: lines at the step
  lying on the ground (or on the locked plane), out to two and a half brush
  radii, fading with distance from the cursor so far lines never clutter the
  view; every tenth line is stronger.
- `C` toggles contours: iso-height lines through the same patch, one per
  step of height (every fifth stronger), so slopes and plateaus read at a
  glance. Contours work with snap off too, at the current step.

### 4.8 Mirror

- `M` toggles mirror on the X axis through the point under the cursor;
  `Shift+M` cycles X, Z, X and Z. The mirror planes draw as thin vertical
  lines across the terrain near the cursor.
- A ghost brush, at half the cursor's opacity, sits at every mirrored centre.
- Each dab also lands at the mirrored centres, in the same stroke and the
  same undo entry. Where a footprint overlaps its own mirror, the overlap is
  applied once.

---

## 5. Visual language

All cursor geometry is mesh-based: the Studio does not build Bevy's gizmo
renderer, so `Gizmos` lines never draw there. The meshes sit on the editor
overlay render layer the view grid uses, so the AI camera's captures never
include them; they are unlit and blended.

| Element | Geometry | Colour |
|---|---|---|
| Footprint ring | Line strip sampled on the surface every 4 degrees (every 1/24 of a side for the square), lifted 4 cm | Family colour, 90 percent while stroking, 70 percent hovering |
| Half-push ring | Same, at the radius where the falloff gives 0.5 | Family colour, 40 percent |
| Strength disc | Polar mesh (8 rings by 48 segments) on the surface, vertex alpha = strength times falloff there, capped at 35 percent | Family colour |
| Volume wireframe | The exact CSG shape at the pivot-adjusted centre (sphere: three great circles and a horizon ring; box: 12 edges; cylinder: two rings and four rails) | Family colour; a 25 percent always-on-top copy shows the part inside the ground |
| Centre mark | Small cross on the surface, a vertical tick to the volume centre | White, 60 percent |
| Locked plane | Disc of three brush radii, grid lines, rim | Cyan, 12 percent fill, 60 percent rim |
| Local grid | Line list on the ground or plane | White, alpha falling to 0 at the edge |
| Contours | Line list from marching squares over the grid patch | White, 30 percent (60 percent every fifth) |
| Mirror plane and ghost | Vertical line strip; ghost ring and wireframe | Family colour at half opacity |
| Smoothing leash | Line from cursor hit to brush centre | White, 40 percent |
| Region box and handles | 12 edges; face handle quads | `cat-parts`; handles cyan on hover |
| Sea level plane | Rectangle at the level | `text-accent`, 20 percent fill |

Tool colours follow DRAFTING_UX Law 3, one hue per group:

| Family | Tools and modes | Token |
|---|---|---|
| Build | Draw Add, Sculpt Grow | `cat-structure` |
| Cut | Draw Subtract, Sculpt Erode | `accent-orange` |
| Shape | Smooth, Flatten | `accent-cyan` |
| Surface | Paint, Replace | `cat-modify` |
| Water | Sea Level | `text-accent` |
| Region | Region | `cat-parts` |

Selection, armed state and focus are cyan (`#00bcd4`, `accent-cyan`): the lit
tool chip, the lit ribbon button, the active segment, the focused field.

### 5.1 The cursor readout

A glass pill in the `FloatingNumericInput` style (`panel-glass`,
`radius-lg`, top hairline), 18 px right of and below the cursor, flipping
sides near the viewport edge. One 11 px line:

`Grow · 16 m · 40% · Y 23.4 m · Grass`, then small icons for plane lock,
snap, contours and mirror when they are on. Lengths use the status-bar
display unit. The tool name shows the swapped or smoothing tool while `Ctrl`
or `Shift` is held. The pill hides while the cursor is off both the terrain
and the plane.

---

## 6. Hotkeys

Live only while the current tool is Terrain, the pointer is over the
viewport and no text field has focus (`T` also works from the Select tool).
They are entries in the keybinding table, in a terrain context: the shortcut
editor lists them and can rebind them, and in that context they take
precedence over global bindings on the same keys, so `1`, `2` and `3` do not
also change snap and `5` does not also toggle the perspective while
sculpting.

| Key | Action | Roblox |
|---|---|---|
| `1` to `7` | Draw, Sculpt, Smooth, Flatten, Paint, Sea Level, Region | none |
| `T` | Enter the terrain tools (last tool) or leave them; bare `T` only, never during a part drag | none |
| `Esc` | Cancel an open plane, region or sea level drag, else leave | Exit |
| `Ctrl` held | Alternate mode (4.3) | same |
| `Shift` held | Smooth (4.3) | same |
| `B`+left-drag or scroll | Size | same |
| `Ctrl+B`+left-drag or scroll | Height (box, cylinder) | same |
| `Shift+B`+left-drag or scroll | Strength | same |
| `[` / `]` | Size down / up (factor 1.25) | none |
| `Shift+[` / `Shift+]` | Strength down / up (10 points) | none |
| `,` / `.` | Pivot previous / next | none |
| `P` | Plane lock on at the surface under the cursor, or off | none |
| `Shift+P` | Re-pick the plane height from the surface | none |
| `PageUp` / `PageDown` | Plane up / down one grid step (`Shift`: ten); Sea Level's water level | none |
| `G` | Snap on / off | none |
| `Shift+G` | Next grid step | none |
| `C` | Contours on / off | none |
| `M` | Mirror on / off | none |
| `Shift+M` | Next mirror axis | none |
| `I` | Sample the material under the cursor (into Source in Replace) | `Alt`+click opens the picker |
| `Ctrl` C, X, V, D, Delete | Region: copy, cut, paste, duplicate, delete | same |
| `Enter` | Sea Level: apply the mode (`Ctrl`: the other one); Region Transform: apply | same |

`Alt`+click stays the camera's orbit in Eustress, so the material picker opens
from the swatch or with `I`. The ribbon buttons and chips show each tool's key
from the table, never a hard-coded label.

---

## 7. UI surfaces

### 7.1 Terrain tool bar (new, `terrain_tool_bar.slint`)

At the top left of the viewport, where the tool options bar sits (the two
never show together). Glass (`panel-glass`, `radius-lg`, 1 px `border-color`,
`shadow-float`, a top `border-highlight` hairline), two rows:

- **Row 1, tools:** a 28 px accent disc with the terrain icon, seven 34 px
  chips (icon, and the key in 9 px), the active tool's mode as a segmented
  control (Add | Subtract, Grow | Erode, and so on), then `×` to leave. The
  active chip has a cyan border, a 10 percent cyan fill and a cyan icon.
  Hovering a chip shows its name and key.
- **Row 2, settings for the active tool only:** Size and Strength as
  scrub-able `NumericField`s, each with a thin slider under it; Shape and
  Pivot as icon segmented controls; Plane (pill plus height field), Snap
  (pill plus step), Contours and Mirror (pills); the material swatch;
  Auto material, Source and Target, and the Sea Level and Region actions
  where they apply. `⋯` opens the advanced strip: hardness, stroke smoothing,
  presets.

Every control writes through to the brush at once, and the cursor follows in
the same frame.

### 7.2 Material picker

A popup under the swatch: a search field, then a grid of 56 px tiles, each
the material's albedo texture scaled down, its name under it, custom slots
tagged. The selected tile has a cyan border; hovering shows the full name.
A tile shows the slot's swatch colour until its thumbnail has loaded. The
Terrain panel's material list and this picker set the same material.

### 7.3 Ribbon Terrain tab

Groups, one hue each, named with nouns, six buttons at most:

| Group | Buttons |
|---|---|
| Create | Generate (menu: Small, Medium, Large, World), Flat, Import, Export, Clear |
| Brushes | Draw, Sculpt, Smooth, Flatten, Paint |
| Water | Sea Level, Ocean (on/off) |
| Region | Region |
| View | Show/Hide |

A tool button is lit (`selected`) while its tool is active; the separate
Edit toggle goes, because choosing a tool enters the tools. Right-click on a
tool button chooses it and opens the advanced strip. Each button shows its
key.

### 7.4 Terrain panel (`terrain_editor.slint`)

Keeps what belongs to the terrain rather than the brush: the empty state
(Flat, Generate World, Import), materials (add, choose), layers, properties,
import and export. The edit toggle becomes "Terrain tools (T)" and lights
cyan while they are on. It does not repeat the brush settings.

### 7.5 Icons

New 24 px Lucide-style icons (`fill="none"`, `stroke="currentColor"`, 1.5 px,
round caps and joins) under `assets/icons/terrain/`: one per tool, the three
shapes, the three pivots, plane lock, snap grid, contours and mirror.

---

## 8. Brush maths

- **Heightfield dabs iterate raster cells**, each at most once per dab,
  instead of sampling points: a cell never takes a dab twice, and chunk
  borders are not visited from both sides.
- **Sculpt moves metres:** a full-strength dab moves the centre by
  `0.04 × radius` metres, scaled by strength and falloff. At 20 dabs a second
  a 16 m brush at full strength moves the centre about 6 m a second. Heights
  are not quantised.
- **Smooth** blends each cell toward the mean of its 3 by 3 neighbourhood by
  `strength × falloff × 0.5` per dab, reading a copy of the footprint so the
  order of cells does not matter. The cap keeps a stroke from shaving ground
  away.
- **Flatten** blends toward the target by `strength × falloff × 0.5` per dab,
  limited by its mode (Erode to Flat never raises, Grow to Flat never
  lowers).
- **Paint** and **Replace** blend the material weight by `strength ×
  falloff`.
- **Draw** applies the CSG shape; the bricks it can write are recorded
  before it writes.
- **Mirror** unions the mirrored footprints before writing, so an overlap is
  applied once.

---

## 9. Starting point (what this design replaces)

Measured in the code on 2026-09-25:

- The brush cursor never drew in the Studio: it was `Gizmos` lines, and the
  Studio does not build the gizmo renderer.
- The default brush moved ground by `strength × voxel size × height scale`
  per frame (6.4 m per frame on the flat preset) and rounded heights to a
  tenth of the height band (12.8 m).
- Heightfield strokes applied once per frame, so their strength depended on
  the frame rate, and a cell under the brush could take a dab several times
  in one frame.
- Flatten re-read its target every frame, so it drifted with the ground.
- Part click-select, the marquee, the transform handles and `Alt`-orbit all
  stayed live while sculpting.
- `1`, `2` and `3` also changed snap and `5` also toggled the perspective
  while sculpting; `T` could only turn the editor off; nothing checked text
  focus.
- A drag that began on the ribbon started sculpting once it entered the
  viewport, and the brush ran during Play.
- One size, strength and falloff were shared by every tool; shape,
  precision and the 3D material had no control; Draw always filled Rock.
- Region and Fill refused with a notice; there was no Replace, Sea Level,
  plane lock, snap, pivot or mirror.
- Brush settings lived in a right-click popup on the ribbon and the edit
  toggle in the Terrain panel; the ribbon's brush tints broke the one hue per
  group rule.

---

## 10. Build plan

Each phase lands whole and compiles on its own.

| Phase | Scope | Files |
|---|---|---|
| 1. Brush core | `TerrainTool` (seven tools and their modes) mapped onto the brush; the settings of 4.5 with per-family size and per-tool strength; cell-based heightfield dabs in metres; distance spacing plus time rate; stroke-start flatten target; plane lock hit and clipping; snap; pivot; mirror; the `Ctrl` and `Shift` swaps; Auto material; Replace; cursor, grid and contour geometry as pure functions; unit tests | `common/src/terrain/editor.rs`, new `common/src/terrain/brush_cursor.rs`, `history.rs` labels |
| 2. Studio wiring | Terrain as a Studio tool; selection, marquee and handles stand down; a stroke starts only in the viewport; off in Play; cursor meshes replacing `Gizmos`; readout data; terrain-context hotkeys; `B`+wheel | `engine/src/terrain_plugin.rs`, new `engine/src/terrain_cursor.rs`; with their owners: one-line gates in `part_selection.rs` and `camera_controller.rs`, the terrain context in `keybindings.rs` |
| 3. Tool bar and ribbon | `terrain_tool_bar.slint`, the readout pill, the thumbnail picker, the ribbon Terrain tab, the Terrain panel changes, icons, Rust sync, settings persistence and presets | new `engine/ui/slint/terrain_tool_bar.slint`, `terrain_editor.slint`, `engine/src/ui/slint_ui.rs` terrain sync, `assets/icons/terrain/`; with UI: `ribbon.slint`, `main.slint` |
| 4. Sea Level and Region | Voxel water saved and undoable, then Fill and Evaporate over a rectangle; Region Select (box, handles, copy, cut, paste, duplicate, delete), Transform, Fill | `common/src/terrain/api.rs` (`FillWater`, `DrainWater`), `voxel_water.rs`, `disk.rs`, `history.rs`, new `engine/src/terrain_sea_level.rs` and `engine/src/terrain_region.rs`; with their owners: `undo.rs`, `ui/file_event_handler.rs`, one match arm in `play_datamodel/terrain_edits.rs` |

### 10.1 Phase 4 in detail

Both tools run through the terrain API (`TerrainCommand`, `api.rs`) with
`TerrainCommandOrigin::Tool`, so every apply is one undo entry and scripts can
do the same edit.

- **Water first.** Water is a per-column surface (`TerrainVoxelWater`, one
  level per raster cell). Every root gets that component on demand, not only
  imported ones. A disk terrain saves its levels to `Workspace/Terrain/water.bin`
  (`voxel_water::save_voxel_water`, read back by `hydrate_terrain_from_disk`;
  the file goes once the terrain holds no water, and a new terrain from
  Generate, Flat or Import clears it). An imported (migrated) Space builds its
  water from its voxel chunks on every open, and its terrain edits cannot be
  saved yet, water included. Every undo entry carries water tile deltas beside
  the height tiles, so a water change undoes and survives a reload.
- **Sea Level** (`engine/src/terrain_sea_level.rs`). A left drag on the
  ground draws the rectangle (snap-aware); a click without a drag clears it.
  The level plane then shows over it with a post at each corner and the
  shoreline the level would make, and a left drag inside the rectangle moves
  the level on a vertical plane facing the camera; `PageUp`/`PageDown` nudge it
  by the snap step, and the bar's Level field types it. The first rectangle
  starts at the ground where its drag began; later ones keep the last level.
  Enter or the bar's Fill applies `FillWater` up to the level; Evaporate
  (`Ctrl+Enter` from Fill) applies `DrainWater` down past any ground. Both are
  water-only commands, Roblox's `ReplaceMaterial` between Air and Water (which
  Roblox's own Sea Level tool calls): Fill wets only the columns whose ground
  stands below the level, and neither ever carves. The rectangle stays after an
  apply, `Esc` clears it, and a second `Esc` leaves the tools. An apply that
  changes nothing says why in a notice. Luau's
  `workspace.Terrain:ReplaceMaterial(region, res, Air, Water)` queues the same
  `FillWater` and `(Water, Air)` the same `DrainWater`, so a Roblox script
  that floods or drains land behaves as it does in Roblox; every other pair
  with Air or Water still raises.
- **Region Select** (`engine/src/terrain_region.rs`). A left drag on the
  ground draws the box's footprint, snapped to the grid step with snap on and
  to the volume lattice otherwise; its height spans the ground inside plus a
  margin (two lattice cells or a grid step). Face handles (squares at the face
  centres, cyan under the cursor) resize it along their axis on a plane that
  holds the axis and faces the camera; a face never crosses the one opposite.
  A click without a drag clears the box. `Ctrl+C` reads it (`read_voxels`,
  occupancy and material at the lattice spacing); `Ctrl+X` reads then empties
  it (`FillRegion` Air); `Ctrl+V` shows the copy as a ghost box under the
  cursor, sitting on the ground the way the copy sat, placed on click
  (`WriteVoxels` at the new corner) and selected; `Ctrl+D` copies it one box
  width along X and selects the copy; Delete empties it. The keys are
  terrain-context actions, so they never copy or delete a selected part while
  the terrain tools are on. The bar has the same five buttons, Paste enabled
  once there is a copy.
- **Region Transform.** A left drag on the target box slides it over the
  plane at its top (snapped); `PageUp`/`PageDown` raise and lower it; the bar
  types its Angle (degrees about Y, any value) and Scale (10 to 1000 percent),
  and Rotate adds a quarter turn. Enter or Apply reads the source, empties it
  and writes the target as one undo entry: a whole quarter turn at scale 1
  moves the voxels exactly; anything else resamples each target voxel from
  the source through the inverse transform (occupancy trilinear, material from
  the neighbour holding the most). Cancel or Esc puts the target back.
- **Region Fill.** Fill fills the box with the material swatch's material
  (`FillRegion`); Replace swaps the source swatch's material for it
  (`ReplaceMaterial`). Both swatches show in this mode. A custom material slot
  cannot fill yet (the API fills built-in materials), which a notice says.
- Limits: a copy or a transform's result holds at most the API's
  `MAX_SCRIPT_VOXELS`; a larger one is refused with a notice. Every edit is
  one labelled undo entry (Cut, Paste, Duplicate, Delete, Fill, Replace
  Terrain Material, Transform Terrain), water included.

## 11. Proof

Screenshots from a running Studio (a copy in `eustress/target/last-good/`) on
a copy of a terrain Space, never the Space itself: the ring and disc on a
slope, the box wireframe on a locked plane with the grid and contours, the
mirror ghost, the tool bar for each tool, the material picker, the readout,
the ribbon's lit state. Each defect in section 9 gets a before and after
where it can be seen.

## 12. Sources for the Roblox baseline

- Terrain Editor: https://create.roblox.com/docs/studio/terrain-editor
- Environmental terrain: https://create.roblox.com/docs/parts/terrain
- 2023 redesign release: https://devforum.roblox.com/t/2344311
- 2024 beta (settings persist): https://devforum.roblox.com/t/2841125
- 2026 Early Access (materials): https://devforum.roblox.com/t/4703282
- RDC 2026 terrain: https://devforum.roblox.com/t/4865617,
  https://devforum.roblox.com/t/4865880
- Creator complaints: manual plane at the origin
  https://devforum.roblox.com/t/2976390; brush lag
  https://devforum.roblox.com/t/3012674, https://devforum.roblox.com/t/4698265,
  https://devforum.roblox.com/t/4770268; Smooth and Flatten too aggressive
  https://devforum.roblox.com/t/4845973; cursor breaks after resizing
  https://devforum.roblox.com/t/3305254, https://devforum.roblox.com/t/3330267;
  no presets https://devforum.roblox.com/t/4525357; no contour lines
  https://devforum.roblox.com/t/3807726; no symmetry
  https://devforum.roblox.com/t/452273
