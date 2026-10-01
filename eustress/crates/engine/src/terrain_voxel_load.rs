//! # Wave 9.C: imported-terrain voxel loader (engine-side)
//!
//! Spec: `docs/architecture/TERRAIN_FJALL_MIGRATION.md` §9.C + SCOPE DECISION.
//!
//! ## Why this lives in the engine (not `eustress-common`)
//!
//! `eustress-common` must NOT depend on `eustress-worlddb` (cycle risk), so
//! the half of the loader that reads the world database cannot live in
//! `common/terrain`. The engine crate already depends on BOTH
//! `eustress-worlddb` and `eustress-common::terrain`, so the Fjall-reading
//! loader lives here. It reads voxel chunks via the `WorldDb` API
//! ([`eustress_worlddb::WorldDb::iter_all_voxel_chunks`] /
//! [`iter_voxel_chunks_in_region`](eustress_worlddb::WorldDb::iter_voxel_chunks_in_region))
//! and builds the terrain through the engine-free
//! [`voxel_import`], whose module docs cover the Terrain instance, the load
//! window and every step of the build. The Player runs the same build over
//! the chunk files the importer writes beside the world database.
//!
//! ## What it does
//!
//! On Space open, once per Space path, the main thread decides whether there
//! is imported terrain to load, the way the Player's Space opener does: a
//! Space with disk terrain (`Workspace/Terrain/_terrain.toml`) loads that
//! through the disk loader, unless it is a converted Space
//! ([`crate::space::space_ops::space_is_migrated`]), whose terrain is this
//! loader's. Otherwise the import's voxel chunk records come from the world
//! database's `voxels` partition when it holds any (a converted Space), else
//! from the `voxel_chunks/*.bin` files the importer writes beside it (a
//! re-import, and what the Player reads). Neither holds a record for most
//! Spaces, and both answer that in one step.
//!
//! 1. Reads the Terrain instance, `Workspace/Terrain/_instance.toml`
//!    (`read_terrain_instance`, from the world database's copy when the file
//!    is not on disk): the unit the import was authored in, its water style,
//!    and whether it holds terrain at all. A Terrain whose `[terrain] source`
//!    is `"none"` loads nothing, since the world database keeps a cleared
//!    import's voxels.
//!
//! The rest is a background build on the `AsyncComputeTaskPool`
//! (`build_voxel_terrain`, see "Background build" below), which touches no
//! Bevy world:
//!
//! 2. Reads every voxel chunk record from its source.
//! 3. Hands them to [`voxel_import::build_from_chunks`], which sizes the
//!    voxel cell from the unit, picks the load window, fills the top surface
//!    of every column, the water and the holes into a `TerrainData`, and
//!    carves the caves under the surface into a `TerrainVolume`.
//!
//! When the build lands, the main thread:
//!
//! 4. Spawns a single `TerrainRoot` entity carrying that `TerrainData`, the
//!    volume, a matching `TerrainConfig` and, when the import holds water, a
//!    [`TerrainVoxelWater`](eustress_common::terrain::TerrainVoxelWater) with
//!    the levels and the import's water colour and transparency. The
//!    EXISTING `chunk_spawn_system` (registered in
//!    [`crate::terrain_plugin::EngineTerrainPlugin`]) meshes and renders it
//!    as the camera moves.
//!
//! ## Background build
//!
//! [`VoxelTerrainBuild`] holds at most one build, with the Space path it was
//! started for, and `poll_voxel_terrain_build` polls it every frame without
//! waiting on it. The build lets go of the world database once the chunks
//! are read, so a Space switched away from can close its database before the
//! decode rather than after it, and a panic inside the build ends it with an
//! error instead of reaching the main thread.
//!
//! A build that lands while its Space is still open replaces the
//! voxel-sourced root: the previous one is despawned and the new one spawned
//! in the same command flush, so no system ever sees two. These systems run
//! ahead of the terrain streaming chain, which therefore meshes the new root
//! from the frame it arrives and never spawns chunks under a root despawned
//! in that frame.
//!
//! A Space switch despawns the previous Space's voxel roots and counts a new
//! generation: a build started before the switch is dropped when it lands,
//! and the new Space's load waits for it, since only one build runs at a
//! time. A build is also dropped when another terrain (a Generate, a flat
//! plate) took the Space over while it ran, when the Terrain was cleared
//! meanwhile, and, for a re-centre, when the root it replaces is gone or
//! holds edits.
//!
//! ## Re-centring
//!
//! The root of an import wider than the window on some axis carries the
//! import's chunk column bounding box ([`VoxelTerrainWindow`]). Four times a
//! second, `recentre_voxel_terrain_window` finds the scene camera's chunk
//! (its XZ over the root's `chunk_size`, floored). When that chunk is more
//! than `MAX_HALF_EXTENT_CHUNKS - RECENTRE_MARGIN_CHUNKS` (111) chunks from
//! the window's centre along an axis wider than the window, a build starts
//! for a window centred on the camera's chunk, held inside the bounding box,
//! along every such axis; an axis the window spans whole keeps its centre
//! ([`voxel_import::recentre_target`]). The old terrain stays drawn until the
//! new one lands and replaces it. An import that fits the window never
//! re-centres.
//!
//! A re-centre reads only the chunks its window can hold, through
//! `iter_voxel_chunks_in_region` (`window_region`): `MAX_HALF_EXTENT_CHUNKS`
//! columns either side of the new centre, and every chunk Y the store can
//! key. That box is in the store's own units, chunk coordinates times
//! `VOXEL_CHUNK_EDGE_STUDS`, not world metres.
//!
//! The window stays where it is while the undo history holds an edit of the
//! root's terrain, which a rebuilt root would drop (a migrated Space cannot
//! save terrain edits yet), and for `RECENTRE_RETRY_SECS` after a re-centre
//! fails.
//!
//! ## What renders
//!
//! The TOP surface of every voxel column (heightfield) with correct per-cell
//! material colour (from the material map), no ground where the import has
//! no voxels, and around every cave a marching cubes surface at LOD 0 that
//! opens the caves, tunnels and overhang undersides and colours their walls
//! by voxel material. At coarser LODs those chunks fall back to the
//! heightfield, so caves are not drawn far away. The water surface reads its
//! levels from the root's `TerrainVoxelWater`. What is still eager is noted
//! at `TODO(stream)` in `build_voxel_terrain`.

#![cfg(feature = "world-db")]

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use bevy::prelude::*;
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};
use eustress_common::terrain::voxel_import::{
    self, BuiltVoxelTerrain, TerrainInstanceProps, MAX_HALF_EXTENT_CHUNKS,
};
use eustress_common::terrain::{scene_camera_translation, ChunkSpawnThrottle, TerrainConfig, TerrainRoot};
use eustress_worlddb::keys::{VOXEL_CHUNK_BIAS, VOXEL_CHUNK_EDGE_STUDS};
use eustress_worlddb::WorldDb;

use crate::space::file_loader::LoadInProgress;
use crate::space::space_ops::space_is_migrated;
use crate::space::world_db_plugin::WorldDbHandle;
use crate::space::SpaceRoot;
use crate::undo::UndoStack;

/// Latch: the Space path the voxel-terrain load decision already ran for. A
/// genuine Space switch (path change) re-arms it, so the decision runs
/// exactly once per Space, as `WorldDbDecision` and `BinaryEcsLoadLatch` do.
#[derive(Resource, Default)]
pub struct VoxelTerrainLoadLatch(pub Option<PathBuf>);

/// Marker on the `TerrainRoot` this loader spawns, so a Space switch and a
/// landing build can despawn exactly the voxel-sourced terrain (and not a
/// legacy/procedural one).
#[derive(Component, Debug, Default)]
pub struct VoxelSourcedTerrain;

/// On the root of an import wider than the load window on some axis: the
/// import's chunk column bounding box, inside which the window follows the
/// camera (see "Re-centring" in the module docs), and the Terrain instance's
/// properties every build of it uses. An import that fits the window has
/// none, and never re-centres.
#[derive(Component, Clone, Copy, Debug)]
pub struct VoxelTerrainWindow {
    /// Smallest chunk column coordinate of the import on each axis.
    pub bbox_min: IVec2,
    /// Largest chunk column coordinate of the import on each axis.
    pub bbox_max: IVec2,
    /// What the Space's load read off the Terrain instance.
    instance: TerrainInstanceProps,
}

/// The background build of the imported terrain, at most one at a time (see
/// "Background build" in the module docs).
#[derive(Resource, Default)]
pub struct VoxelTerrainBuild {
    /// The build running on the `AsyncComputeTaskPool`.
    in_flight: Option<InFlightBuild>,
    /// Genuine Space switches so far. A build started under an earlier count
    /// belongs to a Space no longer open.
    generation: u64,
    /// `Time::elapsed_secs_f64` before which no re-centre starts, pushed out
    /// by `RECENTRE_RETRY_SECS` when one fails.
    recentre_not_before: f64,
    /// The root whose re-centre its terrain edits hold back, so the log says
    /// so once.
    held_for_edits: Option<Entity>,
    /// Where the open Space's first build read its chunks, which its
    /// re-centres read from too.
    source: Option<VoxelChunkSource>,
}

impl VoxelTerrainBuild {
    /// Whether a build is running.
    pub fn is_running(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Start a build of Space `space` from `source`: the Space's first build
    /// when `recentre` is `None`, else one with the window where `recentre`
    /// puts it.
    fn start(
        &mut self,
        space: PathBuf,
        source: VoxelChunkSource,
        instance: TerrainInstanceProps,
        recentre: Option<Recentre>,
    ) {
        let window_center = recentre.map(|recentre| recentre.center);
        self.source = Some(source.clone());
        let task = AsyncComputeTaskPool::get().spawn(async move {
            // Caught here: a panic would close the task, and polling a closed
            // task panics the main thread.
            std::panic::catch_unwind(AssertUnwindSafe(move || build_voxel_terrain(source, instance, window_center)))
                .unwrap_or_else(|panic| Err(format!("the build panicked: {}", panic_text(&*panic))))
        });
        self.in_flight = Some(InFlightBuild { space, generation: self.generation, recentre, instance, task });
    }
}

/// A build running on the `AsyncComputeTaskPool`.
struct InFlightBuild {
    /// The Space it was started for.
    space: PathBuf,
    /// `VoxelTerrainBuild::generation` when it started.
    generation: u64,
    /// The re-centre it is, `None` for a Space's first build.
    recentre: Option<Recentre>,
    /// What the Space's load read off the Terrain instance.
    instance: TerrainInstanceProps,
    /// Resolves to the built terrain, or to why there is none.
    task: Task<Result<BuiltVoxelTerrain, String>>,
}

/// What a re-centre replaces, and where it moves the window.
#[derive(Clone, Copy, Debug)]
struct Recentre {
    /// The root it replaces.
    root: Entity,
    /// Chunk column the new window is centred on.
    center: IVec2,
    /// Smallest chunk column coordinate of the import on each axis, which the
    /// region a re-centre reads cannot tell.
    bbox_min: IVec2,
    /// Largest chunk column coordinate of the import on each axis.
    bbox_max: IVec2,
}

/// The Terrain instance's file, relative to the Space root.
const TERRAIN_INSTANCE_REL: &str = "Workspace/Terrain/_instance.toml";

/// Seconds between two looks at whether the camera has neared the edge of a
/// load window that can move.
const RECENTRE_CHECK_SECS: f64 = 0.25;

/// Seconds after a failed re-centre before the window may move again, so a
/// region that cannot be built is not rebuilt at every look.
const RECENTRE_RETRY_SECS: f64 = 10.0;

/// Lowest chunk coordinate the voxel store can key on an axis: its keys bias
/// every coordinate by `VOXEL_CHUNK_BIAS` into 21 bits.
const STORE_CHUNK_MIN: i64 = -VOXEL_CHUNK_BIAS;

/// Highest chunk coordinate the voxel store can key on an axis.
const STORE_CHUNK_MAX: i64 = VOXEL_CHUNK_BIAS - 1;

/// A point inside chunk `(cx, cy, cz)` in the units
/// `WorldDb::iter_voxel_chunks_in_region` takes: chunk coordinates times
/// `VOXEL_CHUNK_EDGE_STUDS`, which the store floor-divides back out, whatever
/// unit the import is in. The chunk's centre, so that division lands on the
/// chunk however the product rounds.
fn store_point(cx: i64, cy: i64, cz: i64) -> (f32, f32, f32) {
    let at = |c: i64| (c as f32 + 0.5) * VOXEL_CHUNK_EDGE_STUDS;
    (at(cx), at(cy), at(cz))
}

/// The box `WorldDb::iter_voxel_chunks_in_region` takes for every chunk a
/// window centred on chunk column `center` can hold: [`MAX_HALF_EXTENT_CHUNKS`]
/// columns either side of it on X and Z, the most any window spans, and every
/// chunk Y the store can key, so each chunk stacked on those columns comes
/// back. Held to the coordinates the store can key.
fn window_region(center: IVec2) -> ((f32, f32, f32), (f32, f32, f32)) {
    let half = MAX_HALF_EXTENT_CHUNKS as i64;
    let span = |c: i32| {
        let c = c as i64;
        (
            (c - half).clamp(STORE_CHUNK_MIN, STORE_CHUNK_MAX),
            (c + half).clamp(STORE_CHUNK_MIN, STORE_CHUNK_MAX),
        )
    };
    let ((x0, x1), (z0, z1)) = (span(center.x), span(center.y));
    (store_point(x0, STORE_CHUNK_MIN, z0), store_point(x1, STORE_CHUNK_MAX, z1))
}

/// Read the Terrain instance's `Workspace/Terrain/_instance.toml`, where the
/// Roblox importer stamps the unit and records the water style. A Space
/// whose loose files live only in its world database is read from the
/// database's copy, when there is a database. A missing file, table or key
/// leaves the default.
fn read_terrain_instance(space_root: &Path, db: Option<&dyn WorldDb>) -> TerrainInstanceProps {
    let path = space_root.join("Workspace").join("Terrain").join("_instance.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => db
            .and_then(|db| db.get_file(TERRAIN_INSTANCE_REL).ok().flatten())
            .and_then(|bytes| String::from_utf8(bytes).ok()),
        Err(error) => {
            warn!(
                target: "eustress_engine::terrain_voxel",
                error = %error,
                file = %path.display(),
                "voxel-terrain load: the Terrain instance could not be read; the terrain loads in metres without a water style"
            );
            None
        }
    };
    let instance = text.map(|text| voxel_import::parse_terrain_instance(&text)).unwrap_or_default();
    // Parts convert from their authored unit only in a `units_v1` build
    // (`space::instance_loader`), and the terrain they stand on follows them,
    // so the two always agree.
    if cfg!(feature = "units_v1") {
        instance
    } else {
        TerrainInstanceProps { unit: eustress_common::units::Unit::Meter, ..instance }
    }
}

/// Where a build reads the import's voxel chunk records (see the module
/// docs): the world database, or the terrain directory whose
/// `voxel_chunks/*.bin` files hold them.
#[derive(Clone)]
enum VoxelChunkSource {
    Db(Arc<dyn WorldDb>),
    Files(PathBuf),
}

/// Build the imported terrain from `source`, off the main thread: read the
/// chunks, then everything [`voxel_import::build_from_chunks`] does. A
/// Space's first build (`window_center` `None`) reads every chunk, since its
/// window and the import's bounding box need every chunk column; a re-centre
/// reads only the chunks a window centred on `window_center` can hold
/// (`window_region`, or the same columns by file name).
fn build_voxel_terrain(
    source: VoxelChunkSource,
    instance: TerrainInstanceProps,
    window_center: Option<IVec2>,
) -> Result<BuiltVoxelTerrain, String> {
    let started = Instant::now();
    // TODO(stream): a first build reads every chunk's bytes although the
    // bounding box needs only their coordinates, and every build decodes its
    // whole window before the root spawns. A keys-only column scan, and a
    // fill chunk by chunk on `chunk_spawn_system`'s camera cadence through
    // the region query a re-centre reads with, would bound both.
    //
    // Nothing reads the store past this match, which consumes it, so a Space
    // switched away from can close its database before the decode rather
    // than after.
    let chunks = match (source, window_center) {
        (VoxelChunkSource::Db(db), None) => db
            .iter_all_voxel_chunks()
            .map_err(|error| format!("iter_all_voxel_chunks failed: {error}"))?,
        (VoxelChunkSource::Db(db), Some(center)) => {
            let (min, max) = window_region(center);
            db.iter_voxel_chunks_in_region(min, max)
                .map_err(|error| format!("iter_voxel_chunks_in_region failed: {error}"))?
        }
        (VoxelChunkSource::Files(dir), None) => voxel_import::read_voxel_chunk_files(&dir)?,
        (VoxelChunkSource::Files(dir), Some(center)) => {
            let half = MAX_HALF_EXTENT_CHUNKS as i64;
            voxel_import::read_voxel_chunk_files_where(&dir, |(cx, _, cz)| {
                (cx as i64 - center.x as i64).abs() <= half && (cz as i64 - center.y as i64).abs() <= half
            })?
        }
    };
    let mut built = voxel_import::build_from_chunks(chunks, instance, window_center)?;
    built.stats.elapsed = started.elapsed();
    Ok(built)
}

/// The message a panic carried, as far as it had one.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "no message".to_string()
    }
}

/// Decide, once per Space, whether a migrated Space has imported terrain to
/// load, and start its build. See the module docs.
fn load_voxel_terrain_on_space_open(
    space_root: Res<SpaceRoot>,
    handle: Res<WorldDbHandle>,
    load_in_progress: Res<LoadInProgress>,
    mut latch: ResMut<VoxelTerrainLoadLatch>,
    mut build: ResMut<VoxelTerrainBuild>,
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
    // One build at a time: a build still running belongs to the Space before
    // (its result is dropped when it lands), and this Space's starts after it.
    if build.is_running() {
        return;
    }

    // Commit the decision for this Space: from here this is a one-shot.
    latch.0 = Some(space_root.0.clone());

    // Disk terrain is the disk loader's, as it is the Player's, except in a
    // converted Space, whose terrain is this loader's.
    let terrain_dir = space_root.0.join("Workspace").join("Terrain");
    if terrain_dir.join("_terrain.toml").exists() && !space_is_migrated(&space_root.0) {
        return;
    }
    // The records: the world database's when it holds any, else the
    // importer's chunk files. Most Spaces have neither, and both answer that
    // in one step, so no build starts for them.
    let db = handle.0.as_ref();
    let source = match db.filter(|db| db.has_voxel_chunks()) {
        Some(db) => VoxelChunkSource::Db(Arc::clone(db)),
        None if voxel_import::has_voxel_chunk_files(&terrain_dir) => VoxelChunkSource::Files(terrain_dir),
        None => return,
    };

    // ── The Terrain instance: the unit the cells are in, the water style ──
    let instance = read_terrain_instance(&space_root.0, db.map(|db| &**db));
    if instance.cleared {
        info!(
            target: "eustress_engine::terrain_voxel",
            space = %space_root.0.display(),
            "voxel-terrain load: the Terrain instance's source is \"none\"; no terrain this Space"
        );
        return;
    }
    info!(
        target: "eustress_engine::terrain_voxel",
        space = %space_root.0.display(),
        source = match source {
            VoxelChunkSource::Db(_) => "the world database",
            VoxelChunkSource::Files(_) => "Workspace/Terrain/voxel_chunks",
        },
        "voxel-terrain load: building the imported terrain"
    );
    build.start(space_root.0.clone(), source, instance, None);
}

/// Poll the running build without waiting on it, and when it lands spawn
/// what it built in place of the voxel-sourced root, unless the Space or its
/// terrain moved on meanwhile (see "Background build" in the module docs).
#[allow(clippy::too_many_arguments)]
fn poll_voxel_terrain_build(
    mut commands: Commands,
    space_root: Res<SpaceRoot>,
    handle: Res<WorldDbHandle>,
    mut build: ResMut<VoxelTerrainBuild>,
    roots: Query<(Entity, Has<VoxelSourcedTerrain>), With<TerrainRoot>>,
    undo: Option<Res<UndoStack>>,
    throttle: Option<ResMut<ChunkSpawnThrottle>>,
    time: Res<Time>,
) {
    if !build.is_running() {
        return;
    }
    let Some(in_flight) = build.in_flight.as_mut() else { return };
    let Some(result) = block_on(poll_once(&mut in_flight.task)) else { return };
    let Some(in_flight) = build.in_flight.take() else { return };
    let space = space_root.0.as_path();

    // Started for a Space no longer open. A switch counts a new generation,
    // so a switch away and back to the same path drops it too.
    if in_flight.generation != build.generation || in_flight.space.as_path() != space {
        debug!(
            target: "eustress_engine::terrain_voxel",
            space = %in_flight.space.display(),
            "voxel-terrain load: a build for a Space no longer open finished; its result is dropped"
        );
        return;
    }

    let built = match result {
        Ok(built) => built,
        Err(error) => {
            match in_flight.recentre {
                None => warn!(
                    target: "eustress_engine::terrain_voxel",
                    error = %error,
                    space = %space.display(),
                    "voxel-terrain load: no TerrainRoot spawned; no terrain this Space"
                ),
                Some(recentre) => {
                    build.recentre_not_before = time.elapsed_secs_f64() + RECENTRE_RETRY_SECS;
                    warn!(
                        target: "eustress_engine::terrain_voxel",
                        error = %error,
                        center = ?recentre.center,
                        space = %space.display(),
                        "voxel-terrain load: the load window could not move to follow the camera; the terrain stays where it is"
                    );
                }
            }
            return;
        }
    };

    // Another terrain took the Space over while this one built (a Generate,
    // a flat plate): it stays, rather than two roots drawing at once.
    if roots.iter().any(|(_, voxel)| !voxel) {
        info!(
            target: "eustress_engine::terrain_voxel",
            space = %space.display(),
            "voxel-terrain load: another terrain replaced the imported one while it was building; the build is dropped"
        );
        return;
    }
    match in_flight.recentre {
        None => {
            // Clear marks the Terrain as holding nothing, and may have run
            // while this built.
            if read_terrain_instance(space, handle.0.as_deref()).cleared {
                info!(
                    target: "eustress_engine::terrain_voxel",
                    space = %space.display(),
                    "voxel-terrain load: the Terrain was cleared while its imported terrain was building; the build is dropped"
                );
                return;
            }
        }
        Some(recentre) => {
            // The root it re-centres went (Clear, another terrain), or holds
            // edits a rebuilt root would drop: that root stays.
            if !roots.iter().any(|(root, voxel)| voxel && root == recentre.root) {
                debug!(
                    target: "eustress_engine::terrain_voxel",
                    space = %space.display(),
                    "voxel-terrain load: the terrain a re-centre was to replace is gone; the build is dropped"
                );
                return;
            }
            if undo.as_deref().is_some_and(|undo| undo.has_terrain_edits(recentre.root.to_bits())) {
                info!(
                    target: "eustress_engine::terrain_voxel",
                    space = %space.display(),
                    "voxel-terrain load: the imported terrain was edited while its load window moved; the edited \
                     terrain stays and the build is dropped"
                );
                return;
            }
        }
    }

    // ── Replace the voxel-sourced root ────────────────────────────────
    // Despawned and spawned in one command flush, so no system sees two.
    for (root, voxel) in roots.iter() {
        if voxel {
            commands.entity(root).try_despawn();
        }
    }
    let (bbox_min, bbox_max) = match in_flight.recentre {
        // A re-centre reads only its region, so it carries the import's box.
        Some(recentre) => (
            recentre.bbox_min.min(built.window.columns_min),
            recentre.bbox_max.max(built.window.columns_max),
        ),
        None => (built.window.columns_min, built.window.columns_max),
    };
    let movable = voxel_import::wider_than_window(bbox_min.x, bbox_max.x)
        || voxel_import::wider_than_window(bbox_min.y, bbox_max.y);
    let BuiltVoxelTerrain { config, data, volume, water, window, stats } = built;

    // chunk_spawn_system (EngineTerrainPlugin) queries the root's config,
    // raster and volume and meshes each chunk as the camera moves: by
    // marching cubes at LOD 0 where the volume holds cave bricks, from
    // `data.height_cache` everywhere else.
    let mut root = commands.spawn((
        TerrainRoot,
        VoxelSourcedTerrain,
        config,
        data,
        volume,
        Transform::default(),
        Visibility::default(),
        Name::new("Terrain (imported voxels)"),
    ));
    if let Some(water) = water {
        root.insert(water);
    }
    if movable {
        root.insert(VoxelTerrainWindow { bbox_min, bbox_max, instance: in_flight.instance });
    }
    // The replaced root's chunks went with it: the spawn scan looks for the
    // new root's on its next pass rather than after its interval.
    if let Some(mut throttle) = throttle {
        throttle.backlog = true;
    }

    let summary = match in_flight.recentre {
        None => {
            "voxel-terrain load: imported terrain built from Fjall voxels (top surfaces in the \
             heightfield, the air under them carved into volume bricks, no ground where there are \
             no voxels); TerrainRoot spawned, chunk_spawn_system will mesh it as the camera moves"
        }
        Some(_) => {
            "voxel-terrain load: the load window moved to follow the camera; TerrainRoot rebuilt \
             around its new centre from Fjall voxels, chunk_spawn_system will mesh it"
        }
    };
    info!(
        target: "eustress_engine::terrain_voxel",
        chunks_read = stats.chunks_read,
        chunks_filled = stats.chunks_filled,
        decode_errors = stats.decode_errors,
        skipped_off_grid = stats.skipped_off_grid,
        window_center = ?window.center,
        window_half = ?window.half,
        columns_min = ?bbox_min,
        columns_max = ?bbox_max,
        unit = stats.unit.symbol(),
        cell_size = stats.cell_size,
        water_cells = stats.water_cells,
        cave_points = stats.caves.carved_points,
        cave_bricks = stats.caves.bricks,
        build_ms = stats.elapsed.as_millis() as u64,
        space = %space.display(),
        "{}",
        summary
    );
}

/// Move the load window of an import wider than it to follow the scene
/// camera: a few looks a second, and a re-centre build when the camera nears
/// the edge of what the last build read (see "Re-centring" in the module
/// docs).
#[allow(clippy::too_many_arguments)]
fn recentre_voxel_terrain_window(
    space_root: Res<SpaceRoot>,
    latch: Res<VoxelTerrainLoadLatch>,
    mut build: ResMut<VoxelTerrainBuild>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    roots: Query<(Entity, &TerrainConfig, &VoxelTerrainWindow), (With<TerrainRoot>, With<VoxelSourcedTerrain>)>,
    undo: Option<Res<UndoStack>>,
    time: Res<Time>,
    mut last_look: Local<f64>,
) {
    let now = time.elapsed_secs_f64();
    if now - *last_look < RECENTRE_CHECK_SECS {
        return;
    }
    *last_look = now;
    // One build at a time, none for a while after a failed re-centre, and
    // only the window of the open Space, once its load has run.
    if build.is_running() || now < build.recentre_not_before || latch.0.as_deref() != Some(space_root.0.as_path()) {
        return;
    }
    let Ok((root, config, window)) = roots.single() else { return };
    let Some(camera) = scene_camera_translation(&cameras) else { return };
    if !camera.is_finite() {
        return;
    }
    let size = config.chunk_size.max(1e-3);
    let camera_chunk = IVec2::new((camera.x / size).floor() as i32, (camera.z / size).floor() as i32);
    // Measured against the reach of the read behind the window rather than
    // its grid: every chunk within that reach of the centre is already on
    // the grid, which over a sparse stretch of the import is narrower.
    let reach = UVec2::splat(MAX_HALF_EXTENT_CHUNKS);
    let Some(center) =
        voxel_import::recentre_target(camera_chunk, config.center_chunk, reach, window.bbox_min, window.bbox_max)
    else {
        return;
    };
    if undo.as_deref().is_some_and(|undo| undo.has_terrain_edits(root.to_bits())) {
        if build.held_for_edits != Some(root) {
            build.held_for_edits = Some(root);
            info!(
                target: "eustress_engine::terrain_voxel",
                space = %space_root.0.display(),
                "voxel-terrain load: the imported terrain holds edits a rebuilt root would drop, so its load \
                 window stays where it is while they are in the undo history"
            );
        }
        return;
    }
    let Some(source) = build.source.clone() else { return };
    debug!(
        target: "eustress_engine::terrain_voxel",
        from = ?config.center_chunk,
        to = ?center,
        "voxel-terrain load: moving the load window to follow the camera"
    );
    let recentre = Recentre { root, center, bbox_min: window.bbox_min, bbox_max: window.bbox_max };
    build.start(space_root.0.clone(), source, window.instance, Some(recentre));
}

/// On a Space switch: re-arm the load latch so the next migrated Space loads
/// its voxel terrain, count a new generation so a build still running for
/// the previous Space is dropped when it lands, and despawn the previous
/// Space's voxel-sourced roots. `SpaceRoot` change is detected via
/// `is_changed()`: cheap, no extra resource.
fn reset_latch_on_space_switch(
    mut commands: Commands,
    space_root: Res<SpaceRoot>,
    mut latch: ResMut<VoxelTerrainLoadLatch>,
    mut build: ResMut<VoxelTerrainBuild>,
    voxel_roots: Query<Entity, With<VoxelSourcedTerrain>>,
) {
    if space_root.is_changed() && latch.0.as_deref() != Some(space_root.0.as_path()) {
        // A different Space is now active: re-arm so the load runs for it.
        // (When the path is unchanged this is a no-op; the load system's own
        // latch comparison still guards against re-running for the same path.)
        latch.0 = None;
        build.generation = build.generation.wrapping_add(1);
        build.recentre_not_before = 0.0;
        build.held_for_edits = None;
        build.source = None;
        // The disk loader's reset clears every terrain root on a switch too,
        // but not one added since it last ran, which a build that landed the
        // frame before the switch can be.
        for root in voxel_roots.iter() {
            commands.entity(root).try_despawn();
        }
    }
}

/// Register the Wave 9.C voxel-terrain loader. Called from
/// [`crate::terrain_plugin::EngineTerrainPlugin`] only when the `world-db`
/// feature is enabled (this whole module is `#![cfg(feature = "world-db")]`).
pub fn register(app: &mut App) {
    app.init_resource::<VoxelTerrainLoadLatch>()
        .init_resource::<VoxelTerrainBuild>()
        .add_systems(
            Update,
            (
                reset_latch_on_space_switch,
                load_voxel_terrain_on_space_open,
                poll_voxel_terrain_build,
                recentre_voxel_terrain_window,
            )
                .chain()
                // Ahead of the streaming chain, so a root swapped here is the
                // one it meshes this frame, never one despawned under it.
                .before(eustress_common::terrain::process_terrain_generation_queue),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_worlddb::keys::world_to_chunk_coord;

    /// The region box, put through the store's own conversion
    /// (`world_to_chunk_coord` at `VOXEL_CHUNK_EDGE_STUDS`), names exactly the
    /// window's columns and every chunk Y the store can key.
    #[test]
    fn a_re_centre_reads_the_window_columns_and_every_chunk_stacked_on_them() {
        let edge = VOXEL_CHUNK_EDGE_STUDS;
        let chunk_of = |point: (f32, f32, f32)| {
            (
                world_to_chunk_coord(point.0, edge),
                world_to_chunk_coord(point.1, edge),
                world_to_chunk_coord(point.2, edge),
            )
        };
        let (half, bias) = (MAX_HALF_EXTENT_CHUNKS as i32, VOXEL_CHUNK_BIAS as i32);
        for center in [IVec2::ZERO, IVec2::new(33, -1), IVec2::new(-300, 4_000)] {
            let (min, max) = window_region(center);
            assert_eq!(chunk_of(min), (center.x - half, -bias, center.y - half), "a centre of {center}");
            assert_eq!(chunk_of(max), (center.x + half, bias - 1, center.y + half), "a centre of {center}");
        }
        // The box never leaves the coordinates the store can key.
        let (min, max) = window_region(IVec2::new(i32::MAX, i32::MIN));
        assert_eq!(chunk_of(min), (bias - 1, -bias, -bias));
        assert_eq!(chunk_of(max), (bias - 1, bias - 1, -bias));
    }
}
