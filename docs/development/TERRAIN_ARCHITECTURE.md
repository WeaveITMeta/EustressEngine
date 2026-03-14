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
| Material system | **Fully Custom PBR** | No preset limit. Material Palette in `_terrain.toml`, splatmap texture array, 4-8 blended layers per chunk. |
| Streaming | **Infinite, Minecraft-style** | Chunks stream in/out based on camera distance. No fixed world boundary. |
| Water | **Hybrid** | Simulated (realism crate hydro) when in motion, voxel when static, clever blending for splashes. |
| Git friendliness | **Tiled chunk files** | `chunks/x0_z0.r16` — editing one corner changes one small file, not a 50MB monolith. |
| Erosion | **Rune-scriptable** | Hydraulic/thermal erosion loops in Rune, live editor preview. Strongest technical differentiator. |
| Foliage | **Height-pinned** | Foliage entities track terrain height via ECS. Sculpt up = trees move up automatically. |

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

### Data Flow
```
_terrain.toml (config)
    │
    ├─ chunks/x{N}_z{N}.r16     ← 16-bit heightmap per chunk (git-friendly)
    ├─ chunks/x{N}_z{N}.voxel   ← Sparse voxel overlay (only if caves/overhangs exist)
    ├─ splatmap/x{N}_z{N}.png   ← Material blend weights per chunk
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
      splatmap/                 ← Per-chunk material blend weights
        x0_z0.png
        ...
      voxels/                   ← Sparse voxel overlays (only for cave/overhang chunks)
        x3_z-2.voxel
      materials/                ← PBR material definitions
        grass.mat.toml
        rock.mat.toml
        sand.mat.toml
        snow.mat.toml
      foliage/                  ← Foliage scatter data per chunk
        x0_z0.foliage.toml
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
# Material palette — slot index maps to splatmap channel
# Splatmap RGBA = slots 0-3, second splatmap for slots 4-7
[[materials.palette]]
slot = 0
name = "Grass"
file = "materials/grass.mat.toml"

[[materials.palette]]
slot = 1
name = "Rock"
file = "materials/rock.mat.toml"

[[materials.palette]]
slot = 2
name = "Sand"
file = "materials/sand.mat.toml"

[[materials.palette]]
slot = 3
name = "Snow"
file = "materials/snow.mat.toml"

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
name = "Grass"
albedo = "textures/grass_albedo.png"
normal = "textures/grass_normal.png"
roughness = 0.85
metallic = 0.0
ao = "textures/grass_ao.png"
tiling = [8.0, 8.0]            # UV repeat per chunk
```

### Chunk R16 Format
- Raw 16-bit unsigned integers, little-endian
- Size: `chunk_resolution × chunk_resolution × 2 bytes`
- For 64×64 resolution: 8 KB per chunk (very git-friendly)
- Height range: 0-65535 mapped to `0.0 .. height_scale`

---

## 5. Material System (Fully Custom PBR)

### Splatmap Architecture
- Each chunk has a splatmap PNG (RGBA = 4 material weights)
- For >4 materials: second splatmap image (slots 4-7)
- Material blending in custom terrain shader:
  - Sample all active materials at fragment position
  - Blend by splatmap weights (normalized per pixel)
  - Height-based blending at material boundaries for natural transitions
- Material palette defined in `_terrain.toml`, user-editable

### Paint Brush Flow
1. User selects material slot in panel palette
2. Paint brush modifies splatmap pixels in affected chunks
3. Chunks marked dirty → splatmap saved on next auto-save
4. Shader reads updated splatmap → visual update immediate

---

## 6. Water System (Hybrid Simulated + Static Voxel)

### Static Water (Default)
- Water plane entity at `water_level` Y coordinate
- Rendered as translucent plane with PBR water shader (reflection, refraction, foam)
- Voxel water fill: fills enclosed areas below `water_level`
- Efficient: single draw call for water plane

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
- [x] Splatmap per chunk (RGBA = 4 material weights) — `splat_cache` on TerrainData
- [x] Material palette from `_terrain.toml` — `load_material_palette()` in toml_loader.rs
- [x] `.mat.toml` PBR material definitions — `MaterialTomlDef`, `load_material_toml()`
- [ ] Custom terrain shader with multi-material blending — deferred (uses vertex-color fallback)
- [x] Paint brush writes to splatmap — `BrushMode::PaintTexture` in editor.rs
- [x] Height-based auto-blend at material boundaries — `calculate_splat_weights()` in material.rs

### Phase 3 — Water
- [x] Static water plane at sea_level — `water.rs`, `spawn_water_plane()`
- [ ] Water shader (reflection, refraction, foam, caustics) — deferred (uses PBR alpha blend)
- [ ] Voxel water fill for enclosed areas — future
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
- [ ] Foliage scattering (height-pinned, ECS entities) — requires entity-terrain binding system
- [x] Stamp brushes (height texture stamps) — `NoiseBrush` presets in brushes.rs
- [x] Real-world DEM import (SRTM + coordinate projection) — `import_hgt`, `import_geotiff`, `elevation_import.rs`
- [ ] Terrain-pinned entity system (trees move when ground sculpted) — future

---

## 9. Existing Code Inventory

### `eustress_common::terrain/` (10 files)
| File | Purpose | Status |
|------|---------|--------|
| `mod.rs` | Plugin, TerrainRoot, spawn_terrain, generation queue | Complete |
| `config.rs` | TerrainConfig, TerrainData, height sampling | Complete |
| `chunk.rs` | Chunk component, spawn/cull systems | Complete |
| `mesh.rs` | Mesh generation from heightmap (CPU) | Complete |
| `lod.rs` | LOD update system, distance-based swap | Complete |
| `editor.rs` | Brush application, terrain painting system | Complete |
| `material.rs` | Material definitions, splatmap types | Partial |
| `history.rs` | Undo/redo for terrain edits | Complete |
| `brushes.rs` | Advanced brushes (noise, erosion) | Complete |
| `compute.rs` | GPU compute mesh generation | Partial |

### Engine Side
| File | Purpose | Status |
|------|---------|--------|
| `terrain_plugin.rs` | Engine terrain plugin, shortcuts, gizmos | Complete |
| `terrain_editor.slint` | Panel UI (brush tools, settings, import/export) | Complete |
| `ribbon.slint` | Terrain tab (Generate, Edit, Brushes, Water, Assets) | Complete |

### What's Missing (Phase 0 Gaps)
1. `_terrain.toml` file loader — config from filesystem
2. Chunked R16 save/load — per-chunk heightmap persistence
3. Panel placement in left dock — currently standalone, needs docking
4. Ribbon → Panel callback wiring — some callbacks are stubs
5. Heightmap import pipeline — file dialog → parse → chunk → render
