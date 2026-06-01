# Terrain → Fjall Migration (Wave 9)

Decision (2026-06-01): migrate terrain into a single canonical Fjall voxel
store so importer + runtime share one source of truth. Fixes BOTH the
no-Fjall inconsistency AND the importer/runtime format mismatch.

## The problem (verified, not assumed)

Three terrain storage paths today, NONE using Fjall:
- Importer (roblox-import/terrain.rs): writes `voxel_chunks/chunk_X_Y_Z.bin`
  (LZ4) — a 3D VOXEL VOLUME (per-cell material u8 + occupancy u8), Roblox
  SmoothGrid style.
- Runtime (common/terrain/{toml_loader,config,chunk}.rs): reads `.r16`
  heightmaps into `TerrainData { height_cache: Vec<f32>, splat_cache:
  Vec<f32> }` — a 2.5D HEIGHTFIELD + splatmap.
- WorldDb: NO terrain API (the "chunk" refs in keys.rs are the entity Morton
  spatial key, unrelated to voxels).

Two distinct gaps:
1. STORAGE: terrain is loose files, inconsistent with the one-handle Fjall
   streaming world everything else uses.
2. FORMAT/DIMENSIONALITY: importer writes 3D voxels; runtime reads 2.5D
   heightfield; runtime NEVER reads the importer's voxel .bin (grep-confirmed).
   So imported terrain currently does NOT render.

## The honest representation question

Roblox terrain is TRUE 3D voxels (caves, overhangs). Eustress runtime terrain
is 2.5D heightfield + splat. A faithful import cannot be a pure heightmap if
the source has overhangs. Two sub-options inside the Fjall migration:

- (A) Store voxels canonically in Fjall; runtime gains a voxel→mesh path
  (marching-cubes/surface-nets) alongside the heightfield path. Highest
  fidelity (caves survive), most work — a real voxel mesher.
- (B) Store voxels in Fjall but the runtime samples a heightfield FROM the
  voxel column (top solid cell per (x,z)) for its existing heightfield
  renderer. Overhangs flatten to their top surface — lossy for caves, but
  reuses the entire existing terrain renderer. Far less work; correct for the
  ~95% of terrain that IS heightfield-shaped (vehicle-game maps especially).

RECOMMENDATION: ship (B) first (voxels in Fjall + height-sample bridge → map
renders, scales, single store), then (A) as a fidelity follow-up if a target
game actually uses overhangs. Vehicle Simulator is a driving map → almost
certainly heightfield-shaped, so (B) makes its terrain appear with far less risk.

## WorldDb terrain API (new — Wave 9.A)

Add to the WorldDb trait + fjall_backend, a dedicated voxel partition keyed by
chunk Morton (reuse keys.rs world_to_cell / chunk morton so a region scan ==
a chunk request, 1:1 with entity streaming):
```
fn put_voxel_chunk(&self, cx: i32, cy: i32, cz: i32, bytes: &[u8]) -> Result<()>;
fn get_voxel_chunk(&self, cx: i32, cy: i32, cz: i32) -> Result<Option<Vec<u8>>>;
fn iter_voxel_chunks_in_region(&self, min: (f32,f32,f32), max: (f32,f32,f32))
    -> Result<Vec<((i32,i32,i32), Vec<u8>)>>;
fn iter_all_voxel_chunks(&self) -> Result<Vec<((i32,i32,i32), Vec<u8>)>>;
```
Value bytes = the LZ4 voxel-chunk format the importer ALREADY produces
(material+occupancy) — so the importer's decode work is reused verbatim, just
redirected from fs::write to put_voxel_chunk. A new fjall partition "voxels".

## Workstreams

### 9.A — WorldDb voxel partition
WorldDb trait methods above + fjall_backend impl (new "voxels" partition) +
in-memory test backend impl. Morton-keyed.

### 9.B — Importer writes voxels to Fjall (folds into Wave 8.A ImportSink)
roblox-import/terrain.rs::import_terrain: when ImportStorage==BinaryDirect,
call put_voxel_chunk instead of fs::write(voxel_chunks/*.bin). TomlFolders
keeps the loose-file path. (Importer gains optional worlddb dep — same as 8.A.)

### 9.C — Runtime loads voxels from Fjall + height-sample bridge (option B)
common/terrain: a loader that iter_voxel_chunks_in_region → for each column
take the top solid cell → fill TerrainData.height_cache + splat_cache from the
voxel material. Feeds the EXISTING chunk_spawn_system / mesh path. Gate behind
space_is_migrated (migrated spaces use Fjall voxels; legacy spaces keep .r16).

### 9.D — eustress-space tool: terrain in open/verify (extends 8.B)
`open` reports voxel chunk count + terrain bounds; `verify` checks each voxel
chunk decompresses + decodes. Terrain becomes part of the portability story.

## Sequencing note
9 depends on 8.A's ImportSink/worlddb-dep plumbing (shared optional dep). Do
8.A first, then 9.A→9.B→9.C. 9.C (option B) is the gate that makes imported
terrain actually VISIBLE — the headline fix.

## SCOPE DECISION (2026-06-01): Voxel storage + multi-span heightfield

Chosen over both pure-heightfield (lossy on import) and full-voxel-mesher
(biggest effort). The balance:

- STORAGE: full Roblox voxels stored WHOLE in Fjall (lossless — nothing
  dropped on import; a future full-voxel mesher can read the same store).
- RENDER: upgrade the heightfield mesher from single-surface to MULTI-SPAN
  per column — each (x,z) column yields a list of solid spans (floor+ceiling
  pairs), so a top surface AND an under-surface (overhang/tunnel roof) both
  mesh. Covers most Roblox overhangs; deep cave networks render partially
  (the dominant span per column) — acceptable, documented, upgradeable.
- MATERIALS: expand TerrainMaterial + splat from 4 layers to the full ~23
  Roblox terrain materials. splat_cache becomes per-material weight (not the
  hardcoded [grass,rock,dirt,snow]); the importer's material map (terrain.rs
  MATERIAL_TABLE) already covers the 23→Eustress mapping, so reuse it.

### Wave 9 (revised) workstreams
- 9.A WorldDb voxel partition (unchanged — stores the whole voxel chunk).
- 9.B importer → put_voxel_chunk (unchanged; folds into 8.A ImportSink).
- 9.C runtime: MULTI-SPAN column extractor (not just top-solid). For each
  column, walk the voxel z-stack, emit solid spans; the mesher generates a
  surface per span boundary. Feeds an expanded TerrainData (N spans + 23-mat
  splat) into the existing GPU compute mesh path (compute.rs).
- 9.E (NEW) materials: TerrainMaterial enum → 23 variants; splat_cache →
  per-material; material.rs PBR set for each; the importer mapping reused.
- Full 3D voxel surface-extraction (surface-nets/dual-contouring) remains the
  documented fidelity follow-up if a target game needs true cave networks.
