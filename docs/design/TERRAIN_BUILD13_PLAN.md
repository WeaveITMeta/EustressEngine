# Terrain: build 13 feedback plan

McKale's build 13 notes on terrain, what the code does today (traced, with
file references), and the fix for each, sequenced per build.

## Status

The first round is written and waits for build 15. It covers W1, items 2
to 5, and the layer answer in item 6. What went in, where it differs from
the plan below:

- **W1.**
  - Paint with Water levels the footprint to one flat surface a step
    (`WATER_PAINT_RATE`, 0.1 m at full strength) above what stands at the
    brush centre, so a dab pools against the uphill side until W2 lets it
    run.
  - Draw Add with Water fills the brush shape, and Region Fill with Water is
    a FillWater. Region Replace refuses Water as its target.
  - Water is its own tile under the ground materials in the tool bar and in
    the Terrain panel. Scatter and MaterialFill no longer offer it. Spline
    beds and pads still list it, so older files that name it keep loading.
  - `[water]` sets the ocean's `enabled` and `sea_level` on load. Its colour
    and mode are left alone, since every file so far carries the template's.
- **Generated seas.**
  - Seabeds are Sand, Mud or Slate by depth, and the sea's water is written to
    `water.bin` over every pixel below sea level that reaches the map's edge.
  - Generated rivers keep their Water paint. They are painted channels, not
    splines with ribbons, and filling them with flat water now would step
    down every slope. W2's moving-water surface fills them.
- **Convert water paint** (Terrain panel):
  - Each painted body fills to its lowest unpainted rim, over seabed.
  - Cells above that level keep their paint.
  - It is one undo entry.
- **Radius.**
  - Scatter never streams past the Workspace RenderDistance.
  - Trees and custom meshes left on the Kind's distance (Radius 0) draw to
    the RenderDistance.
  - Grass, shrubs and rocks keep their short Kind distances rather than
    taking RenderDistance: grass to 1,000 m would be millions of blades.
  - Generated worlds' tree layers now write Radius 0. Worlds generated
    before keep their 600 m until regenerated or edited.
- **Layers (item 6).**
  - McKale's answer: layers stay dynamic instances, and the terrain follows
    them live. That is how layers already work: moving, editing or deleting
    one re-bakes its region, undoably.
  - "Regenerate layers from seed" stays in scope. "Flatten into terrain"
    waits.

## 1. Water is a layer over the ground, and it flows

### What happens today

- `TerrainMaterial::Water` is an ordinary opaque ground material (the ice
  texture tinted blue, `material_slots.rs:328`, `material.rs:209`), offered in
  every material picker (tool bar, Terrain panel, scatter and fill rules).
- Paint writes it into material cells (`editor.rs:1189`); Draw adds a solid
  lump of it (`editor.rs:1281`); Region Fill fills solid ground with it
  (`terrain_region.rs:828`). None of them adds water.
- Worldgen paints every seabed and river bed with it
  (`worldgen/materials.rs:336`) and writes `[water] enabled = false`; the
  loader never reads `[water]` anyway (`toml_loader.rs:142`). A generated sea
  is blue ground with no water on it. Only lakes get real water.
- Real water (Sea Level, the API's water fills, lakes, rivers, the ocean
  plane) uses one proper water shader: depth tint, foam, shoreline trim,
  drifting ripples (`water_surface.wgsl`). It has no refraction.
- No flow simulation exists. Sea Level fills every column under its level,
  sealed pits included.

### The fix

**W1: water is always water (next build).**

- "Water" in any picker means real water, never a ground colour:
  - Paint with Water raises the water over the footprint by the brush's
    strength per dab.
  - Draw Add with Water fills the brush shape with water.
  - Region Fill with Water is a FillWater.

  These are the same rules the scripting API already follows.
- The Water tile moves out of the ground-material list into its own "Water"
  slot beside it. Scatter and MaterialFill rules stop offering it.
- Worldgen changes:
  - Seabeds get a seabed material (sand, mud, gravel by depth), and river
    beds get gravel or mud.
  - The sea becomes real water: the columns below sea level that connect
    to the map edge are filled.
  - Rivers keep their ribbons.
- Settings: `_terrain.toml [water]` is read into `WaterConfig`.
- Existing Spaces with Water-painted ground: a one-time "Convert water paint"
  command in the Terrain panel. It turns those cells into the seabed material
  and puts water over them up to the level of their connected body. It is
  offered, never silent, since the level is a guess.

**W2: water flows and settles (the build after).**

- The simulation is the virtual-pipe shallow-water model over the per-column
  levels the terrain already stores (`TerrainVoxelWater`). This is the
  standard model for heightfield water. Per cell it tracks four outflow
  fluxes to its neighbours, driven by the water-surface height difference,
  and scales them so a cell never gives more than it holds. Water is
  conserved; the map edge is a sink.
- It runs where water changed (dirty tiles), budgeted per frame, in edit mode
  and in Play, and sleeps once every level moves less than a millimetre.
- Painted water runs downhill, pools in basins, and levels out. Sea Level
  fills only what connects to the rectangle's lowest point, not sealed pits.
- Undo: a water stroke snapshots the water it can reach before it starts, and
  commits one entry when the flow settles, or when the next edit starts.
- Save stores the settled levels (`water.bin` already exists).
- Moving water is drawn as a smooth heightfield surface of the per-column
  levels. Settled bodies keep today's flat, cheap surfaces.

**W3: refraction (later).** Screen-space refraction through the water
surface (Bevy's specular transmission on the water material, with the
camera's transmission steps), tuned against the depth tint.

Particle Simulations' DFSPH solver is the wrong scale for terrain water.
It is the right tool later for splashes where flowing water falls.

## 2. Workspace RenderDistance drives how far trees draw

- **Today:** `Workspace.RenderDistance` only sets `VisibilityRange` on Parts
  (`instance_loader.rs:2311`). Trees follow each scatter layer's own radius
  (`scatter.rs:1171`), up to 1,500 m for trees.
- **Fix:** scatter streams to `min(layer radius, RenderDistance)`, so lowering
  RenderDistance pulls trees in and raising it lets layers reach their radius.
  A layer with radius 0 uses RenderDistance. Scatter batches also get the same
  `VisibilityRange` fade as Parts. The Properties tooltip says it covers parts
  and scatter.

## 3. Trees never grow under water

- **Today:** scatter placement never looks at water (`scatter.rs:470`). Lake
  beds are grass, and the broadleaf layer has no floor at sea level.
- **Fix:** `place_tile` skips a candidate when water stands above the ground
  there, from three sources:
  - `TerrainVoxelWater`;
  - lakes (the flood masks, kept in `BuiltBody`);
  - the ocean plane, when it is on.

  Scatter batches go stale when water changes, so trees under new water go
  and trees on drained ground come back.

## 4. Clear clears everything

- **Today:** Clear empties `Workspace/Terrain`, but keeps the `Layers` folder
  on purpose. That folder holds the generated tree and lake layers and any
  roads, which re-bake onto the next terrain. Clear also keeps the Terrain
  instance, marking it source "none". Other survivors:
  - the ocean plane, whose sync returns before despawning it when no root
    exists (`water.rs:463`);
  - a selected road's gizmos.

  The ribbon's Clear also runs while Generate World is still running.
- **Fix:**
  - Clear removes every layer, to the Space's Trash so it can be recovered.
  - It despawns the ocean plane.
  - It refuses while a generation runs.
  - The Terrain instance stays in the Explorer, now empty, so terrain can be
    added again from it.

## 5. The road tool

Bugs found in code, and their fixes:

- **Next build:**
  - Esc (or right-click) finishes the road; it no longer leaves it armed to
    extend on the next Add Node.
  - A click that misses the terrain, or lands after a Clear, says so.
  - The surface ribbon gets a depth bias as well as its 5 cm lift, so it
    stops flickering against the ground and against far LODs.
  - The bed is carved 0.3 m below the ribbon, so cut ground no longer pokes
    through its edges.
- **The build after:**
  - The collider becomes one continuous strip mesh: no wedge gaps on bends,
    rebuilt once edits go quiet, not at 20 Hz.
  - New nodes drape on the ground.
  - The profile holds a maximum grade with smooth vertical curves. It is
    frozen at creation, so sculpting beside a road no longer bends all of it.
  - Ribbon edges get a short skirt down into the ground.

For the later city and road generation pass, the road's data moves to a
civil-design alignment:
- horizontal: tangents, circular curves and transition spirals;
- vertical: grades joined by parabolic vertical curves;
- cross-section: lanes, crown and superelevation.

The spline stays the editing handle. The generator and the tool then share
one road model. That pass waits for the chassis playground.

## 6. Objects generated into the terrain from a seed

- **Today:** "test object projections" matches nothing named that in code.
  The closest are the layers Generate World writes from the seed:
  - eight tree and grass scatter layers;
  - up to 32 lakes.

  Deleting them removes them for good, until the next Generate World.
  Nothing bakes layers into the ground: layers stay separate from the base
  terrain, and scatter and lakes are placed at runtime.
- **Proposed (to confirm with McKale which he meant):**
  - "Regenerate layers from seed" in the Terrain panel rebuilds the seed's
    tree, grass and lake layers without touching the ground
    (`replace_generated_layers` exists).
  - "Bake layers into terrain" applies stamps, pads, noise, fills and road
    cuts to the ground itself, then removes those layers.
  - "Convert scatter to instances" turns placed trees into real, editable
    instances.

## 7. History keeps current with terrain

- Terrain edits already push labelled undo entries through the path the panel
  watches: brush strokes, Sea Level, Region, MCP, Part to Terrain.
- The faults were the panel's. Bugs has staged fixes for build 14:
  - a rebuild when the tab opens;
  - a Space switch clears history;
  - undoing everything dims every row;
  - the list follows the current row;
  - no every-frame rebuild.
- Two `undo.rs` items went to Building Experience:
  - jumping to "before everything";
  - a revision counter.

## Sequencing

| Build | Terrain work |
|---|---|
| 15 | W1 water semantics and worldgen seas; RenderDistance for scatter; trees under water; Clear; road quick fixes |
| After | W2 flow simulation and moving-water surface (rivers become real water); road collider, grade and profile; Regenerate layers from seed |
| Later | W3 refraction; Flatten into terrain; civil road alignment with the city pass |
