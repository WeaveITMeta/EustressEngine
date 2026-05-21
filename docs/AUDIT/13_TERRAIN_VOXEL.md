# 13 — Terrain & Voxel

> Heightmap paint, voxel chunks, biomes, water simulation, navmesh, GeoTIFF.
> A first-class Studio editing subsystem (paint brushes + undo + chunk LOD), distinct
> from [05_SPACE_STREAMING](05_SPACE_STREAMING.md) (which is layer loading).

## Pass changelog

- **P3 (2026-05-14):** New doc proposed by missing-systems hunter; 10 features.

---

## Concept summary

The **Terrain & Voxel** subsystem owns everything below the gameplay layer that isn't a discrete instance: heightmap data, voxel grids, biome painting, water simulation, navmesh generation, and the editor UX for shaping them. The Studio has dedicated paint brushes with their own state machine, undo history (`TerrainHistory`), and a chunk LOD system that culls per-chunk by distance. GeoTIFF import (via the [eustress-geo](../../eustress/crates/geo/) crate) brings real-world heightmaps into projects.

This is **NOT [05_SPACE_STREAMING](05_SPACE_STREAMING.md)** — Space Streaming is the wire protocol for delivering chunks. Terrain & Voxel is the in-Studio modelling tool for *creating* the terrain those chunks describe, plus the in-engine simulation (water flow, biome blending, IK-friendly ground) that consumes them. Some overlap exists at the chunk-layout level; the systems share radius math and chunk addressing.

---

## Implementation snapshot

**Crates / files:**
- [engine/src/terrain_plugin.rs](../../eustress/crates/engine/src/) — main Studio integration
- [common/src/terrain/](../../eustress/crates/common/src/terrain/) — chunk, LOD, editor, water, navmesh modules
- [eustress-geo](../../eustress/crates/geo/) — GeoTIFF + raster heightmap import
- [docs/development/TERRAIN_ARCHITECTURE.md](../development/TERRAIN_ARCHITECTURE.md)

**Working:**
- Paint brush state machine
- Heightmap import (raw + GeoTIFF)
- Chunk LOD culling (per-chunk distance bands)
- Water simulation primitives

**Stubbed / missing:**
- Heightmap export from Studio (Terrain editor dialog has `TODO`)
- Biome painting UI
- Navmesh generation pipeline
- Terrain → Part conversion (e.g. "extract this ridge as a Mesh")
- Multi-resolution voxel grids (octree)

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | Heightmap paint brushes (raise / lower / flatten / smooth) | 🟡 |
| 2 | Material paint brush (biome / texture splat) | 🔴 |
| 3 | Chunk LOD + spawn/cull | 🟡 |
| 4 | Heightmap import (raw / GeoTIFF) | ✅ |
| 5 | Heightmap export (back to file) | 🔴 |
| 6 | Water simulation (buoyancy, flow) | 🟡 |
| 7 | Navmesh generation per chunk | 🔴 |
| 8 | Terrain undo / redo history | 🟡 |
| 9 | Terrain → Part conversion | 🔴 |
| 10 | Biome procedural fill | 🔴 |

---

## Detailed per-feature cards (top 5)

### Feature 1 — Heightmap paint brushes

**State:** 🟡 · **Effort:** M · **Risk:** Low · **Touches:** [02_STUDIO], [05_SPACE_STREAMING], [10_TELEMETRY]
**Sub-features:** raise / lower / flatten / smooth · brush radius + falloff · pressure-sensitive (stylus) · viewport overlay disc · brush previews

**Concept.** A modal tool in the Studio. Click-drag on terrain raises (or lowers etc.) the heightmap at the brush footprint, projected by camera ray. Update is local to the dirty chunks; only those re-bake on flush.

**Forecasted feedback (R)**
- R1.1 Brush radius units default ambiguous; honour the active [C1] DisplayUnit.
- R1.2 Falloff curve (smooth / linear / squared) needs a preset dropdown.
- R1.3 Pressure-sensitive input via Wintab / Stylus API is non-trivial; defer.
- R1.4 Overlay disc must respect terrain normal — flat disc on a slope looks broken.
- R1.5 Performance: re-bake mesh on every drag-frame is wasteful; need a dirty-rect coalescer.

**Implications (I)**
- *Architectural:* the brush dirty-rect is the input to [05_SPACE_STREAMING] chunk re-bake events.
- *Cross-system:* terrain edits in **Multiplayer Studio mode** ([02_STUDIO] F15) need server-authoritative paint replay.
- *Operational:* huge brushes touch many chunks → batched dirty events.
- *Support burden:* "my brush doesn't paint" is a top user ticket on every editor; pre-empt with overlay feedback.

**Risks (X)**
- X1.1 Brush drag on an LOD2 chunk paints visibly-wrong heights when LOD0 re-bakes.
- X1.2 Multiplayer paint conflicts (two creators on same chunk) need CRDT or last-writer-wins.

**Mitigations (M)**
- M1.1 Force LOD0 for chunks under the brush footprint.
- M1.2 Last-writer-wins per chunk; surface conflict in collab UI.

---

### Feature 3 — Chunk LOD + spawn/cull

**State:** 🟡 · **Effort:** M · **Risk:** Med · **Touches:** [05_SPACE_STREAMING], [13]
**Sub-features:** per-chunk LOD selection · spawn/despawn by camera distance · cross-chunk seam stitching · LOD blend skirts

**Concept.** Per-chunk LOD0–LOD3 chosen by distance; finer detail close, coarser far. Cross-chunk seams require an extra ring of skirts so the LOD transition doesn't show a gap.

**Forecasted feedback (R)**
- R3.1 Seam stitching algorithm is non-trivial; document the approach.
- R3.2 LOD pop on transition is visible; cross-fade adds cost.
- R3.3 Per-LOD navmesh complicates pathfinding consistency.
- R3.4 Chunk spawn order: nearest-first to minimise visible holes.

**Implications (I)**
- *Cross-system:* shares chunk addressing with [05_SPACE_STREAMING] — unify.
- *Architectural:* LOD selection per-chunk + per-pixel (impostor billboards for distant) hybrid.
- *Operational:* mobile force-cap at LOD2 minimum.

**Risks (X)** — X3.1 Seam holes visible at low LOD without skirts.

**Mitigations (M)** — M3.1 Always emit a 1-cell skirt overlap; M3.2 mesh shader fills cracks.

---

### Feature 4 — Heightmap import (raw + GeoTIFF)

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [04_ASSETS], [13], [14_GEO]
**Sub-features:** RAW (16-bit grayscale) reader · GeoTIFF via [eustress-geo](../../eustress/crates/geo/) · CRS reprojection · UTM / lat-lon · seam alignment

**Concept.** Load a `.tif` / `.tiff` GeoTIFF heightmap, reproject to the project's CRS, align to chunk grid. Real-world terrains (USGS DEM, Mapbox Terrarium) drop in cleanly.

**Forecasted feedback (R)**
- R4.1 CRS detection from GeoTIFF metadata; fall back to user-specified.
- R4.2 Big DEMs (1 GB+) need streaming chunk-by-chunk import.
- R4.3 Aliasing on downsample → use Mitchell-Netravali filter.

**Implications (I)**
- *Strategic:* lets creators import "their backyard" → a hook for real-world creators.
- *Cross-system:* deeply ties to [14_GEO_COORDINATES] for CRS handling.

**Risks (X)** — X4.1 Misaligned chunks at the import boundary.

**Mitigations (M)** — M4.1 Snap import to nearest chunk-aligned bounding box.

---

### Feature 6 — Water simulation (buoyancy, flow)

**State:** 🟡 · **Effort:** L · **Risk:** Med · **Touches:** [11_SIMULATION], [13], [19_REALISM]
**Sub-features:** flat water plane · height-mapped water · buoyancy on rigid bodies · flow direction · rendering with normal maps + reflections

**Concept.** Each chunk can opt in to water (flat plane or per-cell height grid). Rigid bodies submerged or partially submerged receive buoyancy force; flow direction nudges floating objects. Rendering does reflections + normal-mapped ripple.

**Forecasted feedback (R)**
- R6.1 Heightmap water is expensive; flat-water default with opt-in cell grid.
- R6.2 Buoyancy needs Avian rigidbody integration — confirm interface.
- R6.3 Reflections probe at runtime is expensive; planar reflection vs. screen-space pick.
- R6.4 Multi-water bodies in one chunk (pond + river) need per-water shaders.

**Implications (I)**
- *Cross-system:* [11_SIMULATION] V-Cell + thermal model could feed water temperature.
- *Strategic:* good water is "wow factor" content — invest in shipping.

**Risks (X)** — X6.1 Z-fighting at water surface with terrain at exactly water height.

**Mitigations (M)** — M6.1 Offset water surface by epsilon; M6.2 depth-aware blend.

---

### Feature 7 — Navmesh generation per chunk

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [13], [03_MULTIPLAYER]
**Sub-features:** per-chunk navmesh bake · cross-chunk stitching · pathfinding service · dynamic obstacles · NPC pathing API

**Concept.** Per-chunk navmesh baked on terrain change. Cross-chunk stitching via overlap rings. Pathfinding service consumes the stitched mesh. NPCs request paths; dynamic obstacles (closed door) generate temp blockers.

**Forecasted feedback (R)**
- R7.1 Recast / Detour as upstream — port to Rust or bind.
- R7.2 Per-chunk bake is fast (seconds); cross-chunk stitching is slow.
- R7.3 Multi-level navmesh (bridges, stairs) — out of scope for P3?

**Implications (I)** — *Cross-system:* unlocks AI NPCs in [07_AI_PLATFORM] Korah-generated worlds.

**Risks (X)** — X7.1 Pathfinder hangs on huge graphs; bound search.

**Mitigations (M)** — M7.1 A* with weighted distance cap.

---

## Wiring / import gaps (top 8)

1. Heightmap export writer (round-trip with TOML)
2. Biome paint brush UI + splatmap shader
3. Navmesh bake pipeline (port Recast or build native)
4. Terrain → Part conversion tool
5. Procedural biome fill (Perlin + voronoi)
6. Multi-resolution voxel (octree) for caves / overhangs
7. Wind / weather influence on water + grass
8. Splatmap LOD (mip-mapped material maps)

---

## Cross-system dependencies

- **[02_STUDIO]** brush UI + Terrain panel.
- **[05_SPACE_STREAMING]** shared chunk addressing.
- **[14_GEO_COORDINATES]** GeoTIFF / CRS.
- **[11_SIMULATION]** water + thermal coupling.
- **[19_REALISM]** material stress / erosion (out of scope today).

---

## Open questions

- Q13.1 Heightmap resolution per chunk: 256² or 512²?
- Q13.2 Voxel cell size: 1 m default? Smaller for caves?
- Q13.3 Recast / Detour port vs. native Rust pathfinder?
- Q13.4 Multi-water bodies per chunk supported?
- Q13.5 Terrain authority in Multiplayer Studio — server replay or CRDT?
