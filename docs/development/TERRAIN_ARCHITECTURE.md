# Terrain Architecture — Eustress Engine

## Table of Contents

1. Design Decisions
2. Rendering Strategy (Hybrid Heightmap + Voxel)
3. Infinite Streaming
4. Data Model (File-System-First)
5. Material System (Fully Custom PBR)
6. Water System (Hybrid Simulated + Static Voxel)
7. Toolbar / Panel Design (Non-Redundant)
8. Phase Plan
9. Existing Code Inventory

---

## 1. Design Decisions

| Decision | Verdict | Rationale |
|----------|---------|-----------|
| Voxel vs Heightmap | **Hybrid** (heightmap primary, voxel overlays) | Heightmap for 90% of world (efficiency, LOD, SRTM import). Voxel Volume Component carves/adds caves, overhangs, tunnels. |
| Panel placement | **Left dock, right of Assets tab** | Non-redundant with ribbon toolbar. Panel = context-sensitive settings. Ribbon = action triggers. |
| Priority features | **Phase 0 → Import → Sculpt** | Proving data model + rendering with heightmap import is the killer differentiator over Roblox. |
| Material system | **Fully Custom PBR** | No preset limit. Material slots in `_terrain.toml` and `materials/*.mat.toml` (256 slots: built-ins 0-22, custom 23-254, 255 for no material), a per-cell material map naming two slots plus a blend weight, and a texture-array terrain shader that blends the four strongest slots per pixel. |
| Streaming | **Infinite, Minecraft-style** | Chunks stream in/out based on camera distance. No fixed world boundary. |
| Water | **Hybrid** | Simulated (realism crate hydro) when in motion, voxel when static, clever blending for splashes. |
| Git friendliness | **Tiled chunk files** | `chunks/x0_z0.r16` — editing one corner changes one small file, not a 50MB monolith. |
| Erosion | **Rune-scriptable** | Hydraulic/thermal erosion loops in Rune, live editor preview. Strongest technical differentiator. |
| Foliage | **Height-pinned** | Scatter is placed again from the surface data whenever the ground under it changes. Sculpt up = trees move up automatically. |

---

## 2. Rendering Strategy — Hybrid Heightmap + Voxel

### Primary: Chunked Heightmap
- Each chunk is a 2D grid of height values → fast mesh generation
- LOD via vertex decimation (existing: `resolution_for_lod()`)
- Standard heightmap-to-mesh shader handles 90% of the world
- SRTM / GeoTIFF / R16 / PNG import maps directly to height cache

### Secondary: Voxel Volume Component
- `VoxelOverlay` component attached to specific chunk entities
- Stores a sparse 3D voxel grid (only non-empty voxels stored)
- Operations: **Carve** (subtract from heightmap), **Add** (overhang/cave ceiling)
- Mesh generation: Marching Cubes or Surface Nets for smooth voxel surfaces
- Blended at chunk boundaries with heightmap mesh via shared vertex normals
- Only allocated when the user explicitly uses cave/overhang tools

### Non-destructive Layers: Base and Bake
- The root's `TerrainData` is the editable base. Brushes, Part to Terrain, heightmap import and undo write it, and Save writes it.
- Layers (`terrain::layers::LayerDesc`: splines for roads, paths, rivers, canyons and embankments, analytic stamps, flatten pads, noise, material fills) apply over the base in `order`, ties broken by id, into a `TerrainBaked` on the same root: heights first, then materials over the finished heights, so a material fill's slope and height rules see the final ground.
- Meshers, colliders, the surface material, picking raycasts and gameplay material queries read `surface_data(base, baked)`: the bake while the root has layers, the base itself when it has none. A terrain without layers carries no bake and pays nothing for the feature.
- Every raster mark in `TerrainDirtyChunks` also queues its region for a re-bake, which `apply_terrain_dirty_chunks` runs before it remeshes, so writers never need to know layers exist. Replacing the layer list re-bakes the old and new bounds of every changed layer (for a spline, the coarse cells its corridor occupies rather than its whole box, so dragging a point of a long diagonal road re-bakes the road and not the map), which restores the base where a layer used to be. A base edit under a spline's elevation knot re-bakes that spline's whole corridor.
- The bake is derived state and is never saved.
- Layers are ordinary instances, one class per kind: `TerrainSpline` (mode Road, Path, River, Canyon or Embankment, its control points `TerrainSplinePoint` children joined in `Index` order), `TerrainStamp`, `TerrainFlattenPad`, `TerrainNoise` and `TerrainMaterialFill`. Their properties are field tables (the `ParticleSimulation` machinery), so Properties edits them and undo covers them; position and heading come from the instance's Transform. They persist as folders under `Workspace/Terrain/Layers`, which the loader descends into although it skips the terrain directory itself, on disk and from the WorldDb alike. Clear Terrain deletes the raster files beside that folder and keeps it.
- Layers are not parented to the `TerrainRoot`: that root is respawned by regenerating, importing and the Terrain class sync, and a despawn would take its children with it. They sit beside the Workspace's own children, belong to the one terrain root, and the Explorer lists them under it. `sync_terrain_layers` (in `TerrainLayersPlugin`, added by the Client's `TerrainPlugin` and by the engine) gathers the enabled layers whenever one is added, edited, moved (directly or through an ancestor such as a Model holding it), reparented, unparented or removed, or a new root appears, and hands the list to each root's bake: at most every 50 ms during a drag, always the final state. A root whose last layer goes loses its bake, and the chunks under the old layers remesh from the base (every chunk, when the bake had given a base without a material layer one).
- Undo needs nothing layer-specific. A Properties edit of a layer field replays through `ChangeClassField`, a Position or Rotation edit and a Move tool drag through `TransformEntities`, and Insert and Delete through the instance undo actions; each changes a layer component, a Transform or the set of layer entities, which the sync above sees. A brush stroke records base tiles; its undo writes them back and marks their chunks, and those marks re-bake the region with the layers still on top.
- In the Studio, selecting a `TerrainSpline` or one of its points draws the spline with gizmos (`terrain_layers::draw_spline_gizmos`, sized for the order 0 scene camera): its control points, the centreline, the bed edges and the shoulders' outer edges on the ground the view shows, in the mode's colour. The corridor follows the stations the bake carved it along; a disabled spline, or one whose bake is still catching up with a drag, is drawn through its points as they stand. The Terrain panel's Layers section has a button for each layer class and one for a spline point, each sending the Insert menu's own `insert:<Class>` action, so the layer lands where the view meets the ground it shows (a point extends the selected spline) and undo covers it the same way.
- A road is a Road-mode `TerrainSpline`. The Studio's Road Builder (Civil mode, Plugins tab) creates one per road through `instance_create`, with an Order one above every layer already baked so it carves last, and one `TerrainSplinePoint` per clicked node, so it never writes the base: Remove Road deletes the spline and the base shows again, and undo covers every node. Every Road spline, however it was made, also gets a drivable surface (`terrain::road_surface`, in `TerrainLayersPlugin`): a ribbon mesh 5 cm above the carved bed, in the swatch of its bed material (asphalt grey when unpainted), and a static compound collider of boxes whose top faces meet it. Both are laid along the stations the bake carved the corridor along (`TerrainBaked::baked_spline`), at the heights of the finished bake under them, and rebuilt whenever those change, so a layer ordered after the road that reshapes the corridor is followed too (along the centreline: the ribbon has one height per station). The surface is derived state like the bake: not an instance, never saved.

### Scatter: Grass, Shrubs, Rocks, Trees
- A `TerrainScatter` instance is a layer class in every way but one: it lives in `Workspace/Terrain/Layers`, keeps its properties in a field table (Kind Grass, Shrubs, Rocks, Trees or Custom; TreeType; MeshAsset; Density per 100 m²; scale range; AlignToNormal; Collide; Seed; Material, slope and height rules; AvoidRoads; a Size X by Z footprint where 0 covers the whole terrain along that axis; a streaming Radius where 0 uses the Kind's distance, grass 120 m, shrubs 250 m, rocks 450 m, trees 1500 m), and loads, saves, edits and undoes like the others, but it bakes nothing.
- Placement is deterministic and non-destructive (`terrain::scatter::place_tile`, and `place_chunk` for a whole chunk). Each chunk is cut into equal square tiles, at most 32 m across for grass, shrubs and rocks and 128 m for trees and custom meshes, and each tile into cells about one instance apart; a generator seeded from the layer's Seed, its id and the tile draws the same numbers per cell every time (whether it holds an instance, where, scale, turn, variant, tint), and a candidate is kept when the surface data passes the rules there: footprint, height, slope from the normal, material weight (kept with that weight as its chance, so a material blend thins the scatter rather than cutting it at a line), and with AvoidRoads the Road and Path corridors of the bake's prepared splines (`PreparedLayers::in_corridor`). Nothing placed is stored.
- The meshes are procedural (`terrain::scatter_meshes`: grass tufts, shrubs, faceted rocks, conifers and broadleaf trees, a few seeded variants each, with normals and vertex colours), since the repository ships no vegetation assets; a Custom layer draws the first mesh and material of its MeshAsset through the `space://` source. Grass, shrubs and rocks up to 1.5 m are concatenated into one mesh per tile and layer (grass casts no shadow); trees, large rocks and custom meshes are entities sharing one mesh and material handle per variant, which Bevy batches, at most 2048 to a batch (past that the same draw that thins cells thins them, so a capped batch shows a subset of the pattern); with Collide, trunks get capsules and large rocks balls, in one static compound collider per batch.
- `TerrainScatterPlugin` (added by the Client's `TerrainPlugin` and by the engine) keeps batches only within each layer's radius of the scene camera, drops them past the radius plus a margin, and builds a budgeted few per frame, lowest Order first and nearest first. A batch is placed again when its layer changes, or when a mark stamps its chunk in `TerrainDirtyChunks`' surface sequence (brush strokes, undo and bakes all do); the stale batch stays drawn until its replacement is built, which waits until the ground and the layer have gone 0.15 s without a change, so a stroke or a drag is not placed again every frame. A new layer's Density is its Kind's (grass 40, shrubs and rocks 4, trees and custom meshes 1), and a Density still at the old Kind's follows a change of Kind. Batches are derived state: not instances, never saved.

### Default Layers of a Generated World
- Generate World writes the world's default layers beside its terrain (`terrain::worldgen::default_layers`, called by `export_to_space`): `TerrainScatter` and `TerrainWaterBody` instances in `Workspace/Terrain/Layers`, in the files Insert writes, so the Studio and the Client load them and the user edits, moves and deletes them like any other layer. `WorldSpec::default_layers`, on by default, turns them off.
- The scatter follows the materials the biome pass painted, one layer per rule since a scatter keys on one material: meadow grass on Grass and on LeafyGrass, broadleaf forest on LeafyGrass up to 25 degrees below the conifer line, conifers above it (a forest on LeafyGrass, open woodland on Grass), scree rocks on Rock and on Slate between 30 and 70 degrees, and beach grass on Sand up to 8 m above sea level. The conifer line is the height 60 % of the world's Grass and LeafyGrass lies below, because the generated climate follows latitude far more than altitude. Every layer covers the whole terrain. Trees stream to 600 m rather than the Trees kind's 1500 m, since each is an entity; trees and the large rocks collide.
- Lakes are the depressions the hydrology fill (priority flood over the whole stitched world, whose edges drain) raises by more than 10 cm, at least 2000 m² and 1 m deep, the 32 largest. Each gets a `TerrainWaterBody` on its lowest ground with Level 0 and the water surface as its Position Y, 25 cm below the height the depression spills at, since resampling onto the terrain raster can bring a narrow rim out a little lower, and a footprint reaching past the depression on every side.
- A generated layer's uuid is two halves: a hash of the world seed and the folder name, and a hash of that half. It is minted when an export first writes the folder, and an export writing the same folder again keeps it, so a layer Studio has open keeps its id and the pattern placed from it. Every later export, the flat plate and a heightmap import included, recognises by it each layer an export wrote, edited or renamed since, and removes it; every other layer stays, and a generated layer whose name another one holds takes the next free `Name-2`. The same world always writes the same bytes.

### Decals on the Terrain
- The Studio's surface-placement tool (`engine::decal_place_tool`, entered from the media-import radial menu) takes the terrain as a target for a Decal: its raycast accepts a chunk collider (`TerrainChunkCollider`) or a road's drivable surface as well as an unlocked part. A Texture or an Image needs a part face and is refused on the terrain, with the reason in a notification, as is a click on a locked part or anything else.
- A decal on the terrain is an ordinary `Decal` instance written directly under the Workspace, not under the terrain root, which regenerating and importing respawn. It lies in the least-squares plane of the finished surface (the bake, through `surface_data`) under its footprint, centred over the cursor, with the image's top edge toward the top of the view; it is 4 m along its longer side, the shorter following the image's aspect, and the Transform carries all of it (X and Z scale are the footprint). A hit the heightfield does not hold, such as a cave wall carved by the volume, keeps the raycast's own point and normal.
- It draws as a Bevy `ForwardDecal`, which reads the depth prepass: every Studio camera carries `DepthPrepass` (with `Msaa::Off`), and the terrain surface material, roads and scatter are opaque and write it, so the decal lands on them; alpha-blended water writes no prepass depth and takes no decal. Each pixel of the flat quad shifts its UV by the parallax to the surface behind it and fades out at `depth_fade_factor` metres from the plane, so the placement writes a fade of four times the farthest the ground strays from the plane (1 m to 8 m): three quarters of the image remains over the roughest point, and as little as possible bleeds onto whatever stands on the ground.
- Bevy's shader derives that parallax scale from the model matrix applied to (1, 1, 1), which is the scale only for an unrotated model. A standalone Decal (one that is neither a part nor on one, however it was made) is therefore drawn by a separate entity whose transform is only the decal's world position and whose quad carries its rotation and size in the vertices, with UVs in metres that the material's `uv_transform` scales back to the image (`decal_place_tool::sync_standalone_decals`). The visual follows the instance's pose and visibility, redraws when its `[decal]` section changes (hot reload included) and goes with it; it is not an instance and is never saved.

### Data Flow
```
_terrain.toml (config)
    │
    ├─ chunks/x{N}_z{N}.r16     ← 16-bit heightmap per chunk (git-friendly)
    ├─ chunks/x{N}_z{N}.voxel   ← Sparse voxel overlay (only if caves/overhangs exist)
    ├─ matmap/x{N}_z{N}.png     ← Material slots per cell (RGBA8: id_a, id_b, blend_b, 0)
    │
    └─ .eustress/cache/terrain/  ← Derived meshes (gitignored, rebuilt on load)
```

---

## 3. Infinite Streaming

### Architecture
- **No fixed world boundary** — chunks extend infinitely in all directions
- Camera position determines which chunks are loaded (`IVec2` chunk coordinates)
- `view_distance` from `TerrainConfig` controls the streaming radius
- Chunks beyond `view_distance + cull_margin` are despawned (entities + mesh handles dropped)
- New chunks entering view radius are generated on-demand

### Chunk Lifecycle
1. **Request**: Camera moves, new chunk coords enter view radius
2. **Generate**: Height data sourced from: imported R16 file, procedural noise, or flat default
3. **Spawn**: Mesh generated, entity spawned as child of TerrainRoot, physics collider added
4. **LOD Update**: Distance-based LOD swap (existing `update_lod_system`)
5. **Edit**: Brush modifies height cache → chunk marked dirty → mesh regenerated
6. **Save**: Dirty chunks flushed to `chunks/x{N}_z{N}.r16` on save
7. **Despawn**: Beyond view distance → entity despawned, mesh handle dropped

### Procedural Generation for Unvisited Chunks
- Default: flat at sea level (height 0)
- Optional: Perlin/simplex noise with seed from `_terrain.toml`
- Optional: Rune script generates height per chunk coordinate (scriptable world gen)

---

## 4. Data Model (File-System-First)

### Filesystem Layout
```
Space1/
  Workspace/
    Terrain/
      _terrain.toml             ← Master config (chunk size, materials, seed, water level)
      chunks/                   ← Per-chunk heightmap data (16-bit, git-friendly)
        x0_z0.r16
        x0_z1.r16
        x1_z0.r16
        ...
      matmap/                   ← Per-chunk material map (RGBA8, one pixel per height cell)
        x0_z0.png
        ...
      voxels/                   ← Sparse voxel overlays (only for cave/overhang chunks)
        x3_z-2.voxel
      materials/                ← PBR material definitions
        grass.mat.toml
        rock.mat.toml
        sand.mat.toml
        snow.mat.toml
      Layers/                   ← Non-destructive layer instances (one folder each)
        MainRoad/_instance.toml ← TerrainSpline
          Point1/_instance.toml ← TerrainSplinePoint
        Crater/_instance.toml   ← TerrainStamp
        Meadow/_instance.toml   ← TerrainScatter (places from its rules; stores nothing placed)
      scripts/                  ← Rune terrain generation/erosion scripts
        erosion.rune
        worldgen.rune
```

### `_terrain.toml` Format
```toml
[terrain]
chunk_size = 64.0               # World units per chunk side
chunk_resolution = 64           # Vertices per chunk side
height_scale = 50.0             # Height multiplier
seed = 42                       # Procedural generation seed
water_level = 0.0               # Global sea level (world Y)

[streaming]
view_distance = 1000.0          # Chunk load radius
cull_margin = 200.0             # Extra distance before despawn (prevents popping)
chunks_per_frame = 4            # Max chunks generated per frame

[lod]
levels = 4
distances = [100.0, 200.0, 400.0, 800.0]

[materials]
# Material palette: `slot` names a material-map slot id. Slots 0-22 are the
# built-in TerrainMaterial variants (an entry here overrides only the keys its
# file sets); 23-254 are this Space's custom materials, each starting from its
# `base` built-in; 255 means no material. A `materials/*.mat.toml` with its
# own `slot` key is picked up without a palette entry.
[[materials.palette]]
slot = 0
name = "Grass"
file = "materials/grass.mat.toml"

[[materials.palette]]
slot = 1
name = "Rock"
file = "materials/rock.mat.toml"

[[materials.palette]]
slot = 3
name = "Snow"
file = "materials/snow.mat.toml"

[[materials.palette]]
slot = 23
name = "Red Rock"
file = "materials/red_rock.mat.toml"

[water]
enabled = true
sea_level = 0.0
# "static" = voxel plane, "dynamic" = realism crate hydro simulation
mode = "static"
color = [0.1, 0.3, 0.6, 0.8]
```

### `.mat.toml` Material Format
```toml
[material]
name = "Red Rock"
slot = 23                       # a file the palette lists takes the palette's slot
base = "Rock"                   # the built-in this material starts from
tint = [1.2, 0.7, 0.6]          # linear RGB multiplier on the albedo
tiling = 4.0                    # metres of ground per texture repeat
roughness = 0.85
metallic = 0.0
physics_material = "Granite"    # realism material the collider friction comes from
# texture_set = "granite"       # a bundled set, "none" for a flat tint, or your own maps:
# albedo = "textures/red_rock_albedo.png"
# normal = "textures/red_rock_normal.png"
# orm = "textures/red_rock_orm.png"   # occlusion, roughness, metallic in R, G, B
```

Every key but `name` is optional: a custom slot copies its `base` built-in and
each key set here overrides it.

### Chunk R16 Format
- Raw 16-bit unsigned integers, little-endian
- Size: `chunk_resolution × chunk_resolution × 2 bytes`
- For 64×64 resolution: 8 KB per chunk (very git-friendly)
- Height range: 0-65535 mapped to `0.0 .. height_scale`

---

## 5. Material System (Fully Custom PBR)

### Material Map
- `TerrainData.material_cache` holds one `MaterialCell` per height sample:
  `[id_a, id_b, blend_b, reserved]`. `blend_b` is `id_b`'s weight in 1/255
  steps, and a single-material cell is `[slot, 255, 0, 0]`. Writers keep the
  stronger material in `id_a` (`canonical_material_cell`), so a reader can
  take `id_a` as the dominant material.
- Slot ids: 0-22 are the built-in `TerrainMaterial` variants by discriminant,
  23-254 are the Space's custom materials, and 255 means no material.
  `TerrainMaterialSlots` (`material_slots.rs`) says how each slot looks and
  behaves: texture set, tint, tiling, roughness, metallic and physics
  material.
- On disk each chunk is `Workspace/Terrain/matmap/x{cx}_z{cz}.png`, RGBA8,
  each pixel a cell's four bytes unchanged, with the R16's tile offsets and
  row order. `encode_material_tile_png` is shared by every writer: Save
  (`save_material_chunks_to_disk`), the worldgen and flat exporters, and the
  heightmap importer.
- A chunk without a matmap but with an old 4-bucket `splatmap/` PNG is
  converted on load by `legacy_splat_to_material_cell` (the fourth bucket
  becomes Snow or Water by the snow line). The next Save or export writes its
  matmap and deletes the splatmap. Nothing writes splatmaps. A chunk with
  neither file loads as Grass.
- Rendering: `TerrainSurfaceMaterial` (`surface_material.rs` +
  `terrain_surface.wgsl`) reads the material map with the `sample_height`
  bilinear footprint, keeps the four strongest slots, sharpens the blend by
  height, and samples the terrain texture arrays (`texture_arrays.rs`), planar
  on flat ground and triplanar on slopes, before Bevy's standard PBR lighting.
  Volumetric chunks carry their brick material in `ATTRIBUTE_UV_1`.
- The vertex-colour mesher is the fallback: until the texture arrays are
  ready it colours by slot swatch, and procedural terrain with no material
  map keeps the height-band colours (`height_to_color`).
- CPU consumers read the same cells through `height_query`:
  `material_at_world` (the top two plus blend) and `material_weights_at_world`
  (bilinear slot weights). Gameplay systems ask `TerrainMaterialQuery` what the
  ground at a point is made of.

### Paint Brush Flow
1. User selects a material slot in the palette (`TerrainBrush.paint_material`).
2. `BrushMode::PaintTexture` calls `height_query::paint_material_at_world` per
   cell. By the top-two rule, repeated dabs converge on the slot, and a third
   material displaces the weaker of the two in the cell.
3. `material_dirty` is set, and Save writes the chunk's matmap (and removes
   its legacy splatmap, if one is left).
4. The surface material re-uploads the material map, so the change shows
   immediately.

---

## 6. Water System (Hybrid Simulated + Static Voxel)

### Static Water (Default)
- Every water surface draws with one shared material, `WaterSurfaceMaterial` (`terrain::water`, fragment shader `water_surface.wgsl` embedded in the common crate so the Client draws it too): a `StandardMaterial` extended with the terrain height texture and the water's colours, alpha blended, under Bevy's standard PBR lighting and fog. Per pixel it measures the depth, the water's own height less the terrain height under it: shallow water is a clear tint with a band of foam along the shore, the colour and opacity deepen to the deep colour with depth, and pixels over ground that stands above the water are dropped, so the waterline follows the ground between raster cells. Procedural detail ripples perturb the normal; they drift on still water and run along the mesh's flow direction (`ATTRIBUTE_UV_1`, world XZ metres per second) on a river, blended over two phases so they never stretch. `WaterConfig`'s colour tints every surface.
- The terrain height texture (`surface_material::TerrainHeightTexture`) holds the root's SURFACE heights (the bake when there is one) in world metres, one R32Float texel per raster cell with the material map's raster mapping, read with `textureLoad` and blended in the shader. It exists only while a water surface does, and is uploaded again, whole and at most every 0.1 s, when `TerrainDirtyChunks`' height sequence moves (sculpting, undo and bakes; paint strokes and volume edits leave it), the raster is replaced or the config changes.
- The ocean: one plane at `WaterConfig::sea_level` sized to the terrain's footprint, shown while `WaterConfig::enabled` (the ribbon's Water button).
- Lakes: a `TerrainWaterBody` instance (in `Workspace/Terrain/Layers`, a field-table class like the layers, baking nothing: Enabled, Order, Level, Size X by Z) floods the surface from its position (`terrain::water_bodies::flood_fill_water`): every raster cell joined to the one under it through cells below the water, inside the footprint (0 on an axis lets it spread across the terrain), is wet; higher ground holds the water back. Level is the water's height above the body's position, so a body Insert drops on the ground under the view holds water at once and moving it raises or lowers the lake. The wet cells and the dry cells along their shore are meshed flat at the level, one mesh per chunk, a shore cell only in the quarters that touch a wet cell (so the far side of a one-cell ridge shows no water), and the body fills again when it changes or the ground under its water or along its rim does.
- Rivers: a River-mode `TerrainSpline` with WaterSurface on (the default) carries a water ribbon along the stations its channel was carved along, at the smoothed profile less Depth times (1 - WaterFill), spanning as far up the banks as that water reaches, and never higher than the lowest of the finished ground at its two waterlines and its two edges (a reach whose banks cannot hold the water shows dry), with its flow along the spline, downhill, faster on steeper grades. WaterSurface and WaterFill bake nothing, so filling a river never re-carves it.
- Water has no colliders: swimming and floating are not part of it, and a body walks through a water surface onto the ground below. `TerrainWaterPlugin` (added by the Client's `TerrainPlugin` and by the engine) builds all of it, and nothing without a renderer.

### Dynamic Water (Realism Crate Integration)
- Activated per-region when water is "in motion" (rivers, waterfalls, splashes)
- Uses `eustress_common::realism` hydro simulation (SPH or shallow-water equations)
- Particle-based for splashes and impacts
- Transitions:
  - **Static → Dynamic**: Object enters water, or terrain edit creates slope → simulation activates
  - **Dynamic → Static**: Water settles below velocity threshold → simulation pauses, voxel snapshot taken
- Blending: Smooth alpha crossfade at static/dynamic boundary

---

## 7. Toolbar / Panel Design (Non-Redundant)

### Principle
**Ribbon = Actions (verbs). Panel = Settings (nouns/adjectives).**

The ribbon's Terrain tab triggers actions: Generate, toggle Edit Mode, select brush, import/export.
The panel shows context-sensitive settings for the currently active tool.

### Ribbon Tab (Already Exists)
| Group | Buttons | Action |
|-------|---------|--------|
| Generate | Small / Medium / Large | Spawn flat terrain with preset config |
| Edit | Edit toggle | Enter/exit edit mode |
| Brushes | Raise / Lower / Smooth / Flatten / Paint / Region / Fill | Select active brush |
| Water | Water | Set sea level |
| Assets | Import / Export / Clear | Heightmap I/O |

### Panel (Left Dock, Right of Assets)
Visible when terrain exists OR when Terrain ribbon tab is active.

**Top: Create Tab** (shown when no terrain exists)
- Generate buttons (Small / Medium / Large)
- Import Heightmap button
- "Create from DEM" button (future: SRTM/GeoTIFF)

**Top: Edit Tab** (shown when terrain exists)
- Edit Mode toggle
- Active brush indicator + brush grid (same as Roblox screenshot)
- Brush Settings (context-sensitive):
  - **All brushes**: Size slider, Strength slider, Falloff curve selector
  - **Flatten**: Target height input
  - **Paint**: Material palette grid with PBR previews
  - **Region**: Selection size XYZ, Position XYZ, Snap to Voxels toggle
  - **Fill**: Material selector, fill region bounds
  - **Erode** (future): Erosion type, iterations, script selector
- Selection Settings (when Region brush active):
  - Size X/Y/Z
  - Position X/Y/Z
  - Snap to Voxels checkbox

**Bottom: Terrain Info**
- Chunk count, total area, memory usage
- Generation progress bar (while generating)

---

## 8. Phase Plan

### Phase 0 — Data Model + Panel (Current)
- [x] `TerrainConfig`, `TerrainData`, `Chunk` components
- [x] `TerrainPlugin` with async chunk generation
- [x] LOD system, chunk spawn/cull
- [x] Basic procedural noise generation
- [x] Slint `terrain_editor.slint` panel
- [x] Ribbon Terrain tab with all buttons
- [x] `_terrain.toml` loader (file-system-first config) — `toml_loader.rs`
- [x] Chunked R16 save/load (`chunks/x{N}_z{N}.r16`) — `toml_loader.rs`
- [x] Panel placement: left dock, right of Assets tab — `main.slint` tab index 3
- [x] Wire panel callbacks to engine terrain systems — `slint_ui.rs` GenerateTerrain→SpawnTerrainEvent, has-terrain/brush/mode synced

### Phase 0.5 — Heightmap Import (Killer Feature)
- [x] Import .r16 (16-bit raw heightmap) — `import_r16()` in formats.rs
- [x] Import .png (8-bit/16-bit grayscale → height) — `import_png_heightmap()` via image crate
- [x] Import SRTM .hgt (NASA elevation data) — existing `import_hgt()`
- [x] Import GeoTIFF (via `tiff` crate) — existing `import_geotiff()`
- [x] Auto-chunk imported heightmap into per-chunk R16 files — `handle_import_terrain` in spawn_events.rs
- [x] Render imported terrain immediately — spawn_terrain called after import + R16 save

### Phase 1 — Sculpt Brushes
- [x] Brush types: Raise, Lower, Smooth, Flatten, Paint
- [x] Brush settings: size, strength, falloff
- [x] Undo/Redo history
- [x] Advanced brushes (noise stamp, erosion)
- [ ] GPU-accelerated brush application (existing compute.rs) — deferred, CPU is adequate for now
- [x] Brush preview overlay (circle on terrain surface) — `update_brush_preview` in terrain_plugin.rs
- [x] Real-time mesh update for edited chunks only — `terrain_paint_system` regenerates dirty chunk meshes

### Phase 2 — Material Painting
- [x] Material map per cell (two slot ids + blend): `material_cache` on TerrainData, `matmap/*.png` via `save_material_chunks_to_disk` in toml_loader.rs
- [x] Material palette from `_terrain.toml` — `load_material_palette()` in toml_loader.rs
- [x] `.mat.toml` PBR material definitions — `MaterialTomlDef`, `load_material_toml()`
- [x] Material slots (23 built-ins plus the Space's custom slots) and the Terrain panel picker with Add material: `TerrainMaterialSlots`, `write_custom_material_toml` in material_slots.rs
- [x] Custom terrain shader with multi-material blending: `TerrainSurfaceMaterial` + `terrain_surface.wgsl`, texture arrays in texture_arrays.rs
- [x] Paint brush writes the material map: `BrushMode::PaintTexture` -> `paint_material_at_world` in height_query.rs
- [x] Height-based blend at material boundaries: the surface shader's height-sharpened blend. `calculate_splat_weights()` / `height_to_color()` in material.rs give procedural terrain (no material map) its height-band colours in the vertex-colour fallback

### Phase 3 — Water
- [x] Static water plane at sea_level: `water.rs`, `spawn_water_plane()`
- [x] Water shader: depth from the terrain height texture, shallow tint, shore foam, deep colour and opacity, flowing detail ripples, PBR lighting and fog (`water_surface.wgsl`); no refraction or caustics
- [x] Water fill for enclosed areas: `TerrainWaterBody` flood fill over the surface data (`water_bodies.rs`)
- [x] River water along River splines, at the smoothed profile capped by the ground at its banks (`water_bodies.rs`)
- [ ] Dynamic water activation (realism crate hydro) — future
- [ ] Static ↔ Dynamic transition with velocity threshold — future
- [ ] Splash particle blending at boundary — future

### Phase 4 — Infinite Streaming
- [x] Camera-driven chunk loading — existing `chunk_spawn_system` + `chunk_cull_system`
- [x] Chunk despawn beyond view_distance + cull_margin — `chunk_cull_system` in chunk.rs
- [x] On-demand generation for unvisited chunks — `TerrainGenerationQueue` async
- [ ] Rune-scriptable world generation per chunk — requires Rune VM terrain bindings
- [x] Async chunk generation on background thread (rayon) — `cpu::generate_mesh_parallel` in compute.rs

### Phase 5 — Advanced (Future)
- [ ] Rune-scriptable erosion (live editor preview) — requires Rune VM + terrain API bindings
- [ ] Voxel overlay for caves/overhangs (Marching Cubes) — requires voxel data layer
- [x] Foliage scattering: `TerrainScatter` layers placed per chunk by `scatter::place_chunk`, drawn and streamed by `TerrainScatterPlugin` (scatter.rs, scatter_meshes.rs)
- [x] Stamp brushes (height texture stamps) — `NoiseBrush` presets in brushes.rs
- [x] Real-world DEM import (SRTM + coordinate projection) — `import_hgt`, `import_geotiff`, `elevation_import.rs`
- [x] Terrain-pinned scatter (trees follow the ground when it is sculpted): a batch is placed again whenever its chunk's surface sequence in `TerrainDirtyChunks` advances (brush strokes, undo, bakes)

---

## 9. Existing Code Inventory

### `eustress_common::terrain/` (core files)
| File | Purpose | Status |
|------|---------|--------|
| `mod.rs` | Plugin, TerrainRoot, spawn_terrain, generation queue | Complete |
| `config.rs` | TerrainConfig, TerrainData, height sampling | Complete |
| `chunk.rs` | Chunk component, spawn/cull systems | Complete |
| `mesh.rs` | Mesh generation from heightmap (CPU) | Complete |
| `lod.rs` | LOD update system, distance-based swap | Complete |
| `editor.rs` | Brush application, terrain painting system | Complete |
| `material.rs` | Built-in material ids, `MaterialCell` encoding, legacy splat conversion, height-band fallback colours | Complete |
| `material_slots.rs` | `TerrainMaterialSlots`: slot table, `.mat.toml` loading, Add material, friction, `TerrainMaterialQuery` | Complete |
| `surface_material.rs` | `TerrainSurfaceMaterial`: textured terrain material, material-map texture, chunk material swaps; the terrain height texture | Complete |
| `terrain_surface.wgsl` | Fragment shader: material-map blend, height sharpening, planar and triplanar sampling | Complete |
| `water.rs` | `WaterConfig`, `WaterSurfaceMaterial`, the ocean plane, `TerrainWaterPlugin` | Complete |
| `water_surface.wgsl` | Fragment shader: depth against the terrain height texture, shore foam, flowing ripples | Complete |
| `water_bodies.rs` | Water body flood fill and meshes, river water ribbons | Complete |
| `worldgen/default_layers.rs` | The default scatter and lake layers written beside a generated world | Complete |
| `scatter.rs` | `TerrainScatter` placement (`place_chunk`), merged batches, camera streaming, `TerrainScatterPlugin` | Complete |
| `scatter_meshes.rs` | Procedural grass, shrub, rock, conifer and broadleaf meshes | Complete |
| `texture_arrays.rs` | Albedo, normal and ORM texture arrays, built off the main thread | Complete |
| `history.rs` | Undo/redo for terrain edits | Complete |
| `brushes.rs` | Advanced brushes (noise, erosion) | Complete |
| `compute.rs` | GPU compute mesh generation | Partial |

### Engine Side
| File | Purpose | Status |
|------|---------|--------|
| `terrain_plugin.rs` | Engine terrain plugin, shortcuts, gizmos | Complete |
| `decal_place_tool.rs` | Decal, Texture and Image placement onto parts; Decals onto the terrain; standalone Decal rendering | Complete |
| `terrain_editor.slint` | Panel UI (brush tools, settings, import/export) | Complete |
| `ribbon.slint` | Terrain tab (Generate, Edit, Brushes, Water, Assets) | Complete |

### What's Missing (Phase 0 Gaps)
1. `_terrain.toml` file loader — config from filesystem
2. Chunked R16 save/load — per-chunk heightmap persistence
3. Panel placement in left dock — currently standalone, needs docking
4. Ribbon → Panel callback wiring — some callbacks are stubs
5. Heightmap import pipeline — file dialog → parse → chunk → render
