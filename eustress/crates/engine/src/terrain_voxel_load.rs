//! # Wave 9.C — imported-terrain voxel loader (engine-side)
//!
//! Spec: `docs/architecture/TERRAIN_FJALL_MIGRATION.md` §9.C + SCOPE DECISION.
//!
//! ## Why this lives in the engine (not `eustress-common`)
//!
//! `eustress-common` must NOT depend on `eustress-worlddb` (cycle risk), so
//! the voxel-READING half cannot live in `common/terrain`. The engine crate
//! already depends on BOTH `eustress-worlddb` and `eustress-common::terrain`,
//! so the Fjall-reading loader lives here. It reads voxel chunks via the
//! `WorldDb` API ([`eustress_worlddb::WorldDb::iter_all_voxel_chunks`] /
//! [`iter_voxel_chunks_in_region`](eustress_worlddb::WorldDb::iter_voxel_chunks_in_region))
//! and fills `eustress_common::terrain::TerrainData` through the pure
//! engine-free extractor in
//! [`eustress_common::terrain::voxel_extract`].
//!
//! ## What it does
//!
//! On Space open of a **MIGRATED** Space (gated on
//! [`crate::space::space_ops::space_is_migrated`] — legacy `.r16`/`.toml`
//! Spaces keep their existing path untouched), once per Space path:
//!
//! 1. Reads every voxel chunk from the Fjall `voxels` partition (this wave
//!    loads ALL chunks — see the `TODO(stream)` below for the camera-locality
//!    region-query upgrade).
//! 2. Decodes each chunk and runs the MULTI-SPAN column extractor
//!    ([`voxel_extract::fill_terrain_from_chunk`]): for every `(x, z)` column
//!    it finds the solid spans and writes the TOP surface height (raw studs)
//!    + the surface material's splat bucket into a `TerrainData`.
//! 3. Spawns a single `TerrainRoot` entity carrying that `TerrainData` +
//!    a matching `TerrainConfig`. The EXISTING `chunk_spawn_system`
//!    (registered in [`crate::terrain_plugin::EngineTerrainPlugin`]) meshes
//!    and renders it as the camera moves — this loader produces the same
//!    `height_cache` / `splat_cache` shape that path already consumes, so
//!    the mesher/renderer is untouched.
//!
//! ## What renders vs. what is deferred
//!
//! - **Renders:** the TOP surface of every voxel column (heightfield) with
//!   correct per-cell material colour (via `splat_bucket`). Scales by the
//!   region query (camera-local chunks) once `TODO(stream)` lands.
//! - **Deferred:** true multi-span CAVE / overhang under-surface geometry.
//!   The extractor already detects every span per column, but the current
//!   renderer is a single-surface heightfield, so only the top span's top
//!   surface meshes. Deep caves render partially (their dominant top
//!   surface) — acceptable + documented per the SCOPE DECISION. Full
//!   volumetric meshing (surface-nets / dual-contouring) reading the same
//!   whole-voxel Fjall store is a later wave.

#![cfg(feature = "world-db")]

use bevy::prelude::*;
use eustress_common::terrain::voxel_extract;
use eustress_common::terrain::{TerrainConfig, TerrainData, TerrainRoot};

use crate::space::file_loader::LoadInProgress;
use crate::space::space_ops::space_is_migrated;
use crate::space::world_db_plugin::WorldDbHandle;
use crate::space::SpaceRoot;

/// Latch: the Space path the voxel-terrain load already ran for. A genuine
/// Space switch (path change) re-arms it, so the load runs exactly once per
/// Space — mirrors `WorldDbDecision` / `BinaryEcsLoadLatch`.
#[derive(Resource, Default)]
pub struct VoxelTerrainLoadLatch(pub Option<std::path::PathBuf>);

/// Marker on the `TerrainRoot` this loader spawns, so a Space switch can
/// despawn exactly the voxel-sourced terrain (and not a legacy/procedural
/// one) before reloading.
#[derive(Component, Debug, Default)]
pub struct VoxelSourcedTerrain;

/// Hard cap on chunks materialized in one load pass — a safety bound for the
/// load-ALL path this wave uses (a pathological import can't OOM the cache).
/// At 32 cells × 4 studs a chunk is 128 studs; 4096 chunks ≈ a 512 km span,
/// far beyond any real place, yet finite. `TODO(stream)` removes the need
/// for this by region-querying only the camera neighbourhood.
const MAX_CHUNKS_PER_LOAD: usize = 4096;

/// Hard cap on the heightfield half-extent (in chunks). The cache is sized
/// `(2*radius+1)^2 * 32^2` floats for heights + ×4 for splat, so the radius
/// drives memory. A single stray far-flung chunk (corrupt coord, or a tiny
/// detail kilometres from spawn) must not balloon that to gigabytes. At
/// radius 64 the cache is `(129*32)^2 ≈ 17M` height floats (~68 MB) + splat
/// (~272 MB) — already generous for any real place; chunks beyond this are
/// skipped + counted (so the log explains a clipped far edge). `TODO(stream)`
/// makes this moot by only ever sizing the camera neighbourhood.
const MAX_RADIUS_CHUNKS: u32 = 64;

/// Boot-load imported voxel terrain for a migrated Space into the runtime
/// heightfield, once per Space. See the module docs.
fn load_voxel_terrain_on_space_open(
    mut commands: Commands,
    space_root: Res<SpaceRoot>,
    handle: Res<WorldDbHandle>,
    load_in_progress: Res<LoadInProgress>,
    mut latch: ResMut<VoxelTerrainLoadLatch>,
    existing: Query<Entity, (With<TerrainRoot>, With<VoxelSourcedTerrain>)>,
) {
    // Run once per Space path. Latch BEFORE any early-return-after-decision
    // so a migrated Space with zero voxels (or a failed read) doesn't re-scan
    // every frame.
    if latch.0.as_deref() == Some(space_root.0.as_path()) {
        return;
    }
    // Wait until the file loader's pass finishes so we don't race service
    // spawning (and so `space_is_migrated`'s header read is stable).
    if load_in_progress.active {
        return;
    }

    // Commit the decision for this Space — from here this is a one-shot.
    latch.0 = Some(space_root.0.clone());

    // GATE: only migrated `.eustress` Spaces use the Fjall voxel path. A
    // legacy disk Space keeps its existing `.r16`/`_terrain.toml` path
    // entirely untouched (this branch stands down). This is the whole point
    // of the gate — branch, don't replace.
    if !space_is_migrated(&space_root.0) {
        return;
    }

    // Need the open WorldDb for this Space's `voxels` partition.
    let Some(db) = handle.0.as_ref() else {
        // Migrated header but no DB handle (open fell back to disk) — nothing
        // to read. Latched, so this is a one-shot no-op.
        return;
    };

    // TODO(stream): region-query only the camera-local chunk window
    // (`iter_voxel_chunks_in_region(min, max)`) on the chunk-streaming
    // cadence, mirroring `chunk_spawn_system`'s camera locality, so a
    // 10M-cell terrain streams instead of loading whole. For this wave we
    // load ALL chunks (bounded by MAX_CHUNKS_PER_LOAD) since the heightfield
    // cache is sized to the full extent anyway; the region API is wired and
    // tested in worlddb (Wave 9.A), so this is a cadence change, not new
    // plumbing.
    let chunks = match db.iter_all_voxel_chunks() {
        Ok(c) => c,
        Err(e) => {
            warn!(
                target: "eustress_engine::terrain_voxel",
                error = %e,
                space = %space_root.0.display(),
                "voxel-terrain load: iter_all_voxel_chunks failed; no terrain this Space"
            );
            return;
        }
    };
    if chunks.is_empty() {
        // Migrated Space with no terrain — common (most places have none).
        return;
    }

    // Despawn any prior voxel-sourced terrain (Space switch / re-open) so we
    // never stack duplicate TerrainRoots.
    for e in existing.iter() {
        commands.entity(e).despawn();
    }

    // ── Size the terrain to cover the voxel chunk extent ──────────────
    // The heightfield cache is centered on the origin and spans
    // `chunks_x` chunks each side. Pick a radius that contains every voxel
    // chunk's (cx, cz) so write_chunk_to_cache's `(coord + half)` offset
    // never lands out of bounds.
    let mut max_abs_x: i32 = 0;
    let mut max_abs_z: i32 = 0;
    for &((cx, _cy, cz), _) in &chunks {
        max_abs_x = max_abs_x.max(cx.abs());
        max_abs_z = max_abs_z.max(cz.abs());
    }
    // Clamp the half-extent so one stray far chunk can't balloon the cache.
    // Chunks beyond the clamp are skipped + counted below (clipped far edge).
    let radius_chunks = (max_abs_x.max(max_abs_z) as u32)
        .clamp(1, MAX_RADIUS_CHUNKS);
    let config = voxel_extract::voxel_terrain_config(radius_chunks);

    // ── Decode + multi-span fill, per chunk ───────────────────────────
    let mut data = TerrainData::procedural();
    let mut filled = 0usize;
    let mut decode_errors = 0usize;
    let mut skipped_oob = 0usize;
    for &((cx, cy, cz), ref bytes) in chunks.iter().take(MAX_CHUNKS_PER_LOAD) {
        // Defensive: a coord beyond the sized radius would index OOB; the
        // cache-fill helper already bounds-checks the copy, but skip+count
        // here so the log explains a partial render rather than silently
        // dropping rows mid-copy.
        if cx.abs() as u32 > radius_chunks || cz.abs() as u32 > radius_chunks {
            skipped_oob += 1;
            continue;
        }
        match voxel_extract::decode_voxel_chunk(bytes) {
            Ok(chunk) => {
                voxel_extract::fill_terrain_from_chunk(&mut data, &config, cx, cy, cz, &chunk);
                filled += 1;
            }
            Err(e) => {
                decode_errors += 1;
                // Per-chunk, so DEBUG to avoid log spam on a big terrain; the
                // aggregate count is surfaced in the summary below.
                debug!(
                    target: "eustress_engine::terrain_voxel",
                    cx, cy, cz, error = %e,
                    "voxel-terrain load: chunk decode failed; skipping"
                );
            }
        }
    }

    if filled == 0 {
        warn!(
            target: "eustress_engine::terrain_voxel",
            total = chunks.len(),
            decode_errors,
            skipped_oob,
            space = %space_root.0.display(),
            "voxel-terrain load: every chunk failed/empty — no TerrainRoot spawned"
        );
        return;
    }

    // ── Spawn the TerrainRoot the existing mesher/renderer consumes ────
    // chunk_spawn_system (EngineTerrainPlugin) queries (TerrainConfig,
    // TerrainData, Children) on TerrainRoot and generates chunk meshes from
    // `data.height_cache` as the camera moves — unchanged.
    commands.spawn((
        TerrainRoot,
        VoxelSourcedTerrain,
        config,
        data,
        Transform::default(),
        Visibility::default(),
        Name::new("Terrain (imported voxels)"),
    ));

    info!(
        target: "eustress_engine::terrain_voxel",
        chunks_filled = filled,
        decode_errors,
        skipped_oob,
        radius_chunks,
        truncated = chunks.len() > MAX_CHUNKS_PER_LOAD,
        space = %space_root.0.display(),
        "voxel-terrain load: imported terrain TOP-surface heightfield built from Fjall voxels \
         (multi-span detected; under-surface cave geometry deferred to the volumetric mesher) — \
         TerrainRoot spawned; chunk_spawn_system will mesh it as the camera moves"
    );
}

/// Reset the load latch on a Space switch so the next migrated Space reloads
/// its voxel terrain. `SpaceRoot` change is detected via `is_changed()` —
/// cheap, no extra resource.
fn reset_latch_on_space_switch(
    space_root: Res<SpaceRoot>,
    mut latch: ResMut<VoxelTerrainLoadLatch>,
) {
    if space_root.is_changed() && latch.0.as_deref() != Some(space_root.0.as_path()) {
        // A different Space is now active — re-arm so the load runs for it.
        // (When the path is unchanged this is a no-op; the load system's own
        // latch comparison still guards against re-running for the same path.)
        latch.0 = None;
    }
}

/// Register the Wave 9.C voxel-terrain loader. Called from
/// [`crate::terrain_plugin::EngineTerrainPlugin`] only when the `world-db`
/// feature is enabled (this whole module is `#![cfg(feature = "world-db")]`).
pub fn register(app: &mut App) {
    app.init_resource::<VoxelTerrainLoadLatch>().add_systems(
        Update,
        (reset_latch_on_space_switch, load_voxel_terrain_on_space_open).chain(),
    );
}
