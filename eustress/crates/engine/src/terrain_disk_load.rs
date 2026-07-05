//! # Disk-terrain auto-loader — `Workspace/Terrain/_terrain.toml` on Space open
//!
//! Closes the gap documented in `worldgen/export.rs` (INTEGRATOR NOTE):
//! `load_terrain_toml` / `load_chunks_from_disk` previously had ZERO call
//! sites, so an exported (or imported) `Workspace/Terrain/` directory was
//! inert until the user re-imported. This module mirrors
//! [`crate::terrain_voxel_load`] one-for-one — once-per-Space latch keyed on
//! the Space path, gated behind `LoadInProgress`, reset on Space switch —
//! but reads the legacy/disk R16 + toml format instead of Fjall voxels.
//!
//! ## Guards (do-not-fight rules)
//!
//! 1. **Migrated Spaces stand down** — [`space_is_migrated`] Spaces are owned
//!    by the Fjall voxel loader (`terrain_voxel_load`, `world-db` feature);
//!    two loaders must never race on one Space.
//! 2. **No `_terrain.toml`, no load** — most Spaces have no terrain.
//! 3. **Existing `TerrainRoot` wins** — if a class-sync
//!    (`sync_terrain_class_to_system`) or a panel Generate already produced a
//!    terrain this session, the auto-loader does not double-spawn.
//!
//! The spawned `TerrainRoot` carries the [`DiskSourcedTerrain`] marker and is
//! meshed by the ALREADY-REGISTERED `process_terrain_generation_queue` /
//! `chunk_spawn_system` chain in [`crate::terrain_plugin::EngineTerrainPlugin`]
//! — no new render wiring.

use bevy::prelude::*;
use std::path::Path;

use eustress_common::terrain::{
    spawn_terrain, toml_loader, TerrainConfig, TerrainData, TerrainRoot,
};

use crate::space::file_loader::LoadInProgress;
use crate::space::space_ops::space_is_migrated;
use crate::space::SpaceRoot;

/// Latch: the Space path the disk-terrain load already ran for. A genuine
/// Space switch (path change) re-arms it, so the load runs exactly once per
/// Space — mirrors `VoxelTerrainLoadLatch`.
#[derive(Resource, Default)]
pub struct TerrainDiskLoadLatch(pub Option<std::path::PathBuf>);

/// Marker on the `TerrainRoot` this loader spawns, so a Space switch can
/// despawn exactly the disk-sourced terrain (and not a procedural one).
#[derive(Component, Debug, Default)]
pub struct DiskSourcedTerrain;

/// Hydrate `(TerrainConfig, TerrainData)` from a `Workspace/Terrain/`
/// directory — the exact recipe from `worldgen/export.rs` (INTEGRATOR NOTE):
/// `_terrain.toml` → `to_terrain_config()` → `resize_cache` →
/// `load_chunks_from_disk` (SIGNED centered `[-N, +N]` chunk coords — never
/// the importer's unsigned math). Shared by the Space-open auto-loader, the
/// worldgen poll system (`ui/spawn_events.rs`), and the class-sync guard
/// (`terrain_plugin.rs`).
///
/// Returns `Err` when `_terrain.toml` is missing/unparseable; a toml with
/// zero readable chunks still returns `Ok` (flat terrain — the config is
/// valid, chunks may stream in later or simply not exist yet).
pub fn hydrate_terrain_from_disk(
    terrain_dir: &Path,
) -> Result<(TerrainConfig, TerrainData, usize), String> {
    let toml = toml_loader::load_terrain_toml(&terrain_dir.join("_terrain.toml"))?;
    let config = toml.to_terrain_config();
    let mut data = TerrainData::procedural();
    data.resize_cache(&config);
    let loaded = toml_loader::load_chunks_from_disk(terrain_dir, &config, &mut data);
    Ok((config, data, loaded.len()))
}

/// Boot-load a legacy/disk Space's `Workspace/Terrain/` (R16 + toml) into the
/// runtime heightfield, once per Space. See the module docs for the guards.
fn load_disk_terrain_on_space_open(
    mut commands: Commands,
    space_root: Res<SpaceRoot>,
    load_in_progress: Res<LoadInProgress>,
    mut latch: ResMut<TerrainDiskLoadLatch>,
    existing: Query<Entity, With<TerrainRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Run once per Space path. Latch BEFORE any early-return-after-decision
    // so a Space without terrain doesn't re-scan the disk every frame.
    if latch.0.as_deref() == Some(space_root.0.as_path()) {
        return;
    }
    // Wait until the file loader's pass finishes — a Terrain-class TOML in
    // this Space may spawn its own TerrainRoot (class-sync path), and
    // `space_is_migrated`'s header read must be stable.
    if load_in_progress.active {
        return;
    }

    // Commit the decision for this Space — from here this is a one-shot.
    latch.0 = Some(space_root.0.clone());

    // GATE 1: migrated `.eustress` Spaces are owned by the Fjall voxel
    // loader (terrain_voxel_load) — stand down entirely.
    if space_is_migrated(&space_root.0) {
        return;
    }

    // GATE 2: nothing to load.
    let terrain_dir = space_root.0.join("Workspace").join("Terrain");
    if !terrain_dir.join("_terrain.toml").exists() {
        return;
    }

    // GATE 3: a TerrainRoot already exists (class-sync during load, or a
    // generate that beat us) — never double-spawn.
    if !existing.is_empty() {
        return;
    }

    match hydrate_terrain_from_disk(&terrain_dir) {
        Ok((config, data, chunk_files)) => {
            let entity = spawn_terrain(&mut commands, &mut meshes, &mut materials, config, data);
            commands.entity(entity).insert(DiskSourcedTerrain);
            info!(
                target: "eustress_engine::terrain_disk",
                chunk_files,
                space = %space_root.0.display(),
                "disk-terrain load: TerrainRoot spawned from Workspace/Terrain \
                 (R16 + toml); chunk_spawn_system will mesh it"
            );
        }
        Err(e) => {
            warn!(
                target: "eustress_engine::terrain_disk",
                error = %e,
                space = %space_root.0.display(),
                "disk-terrain load: _terrain.toml present but unloadable; no terrain this Space"
            );
        }
    }
}

/// Reset the load latch on a Space switch so the next Space reloads its disk
/// terrain, and despawn the previous Space's disk-sourced terrain so it never
/// bleeds into the new Space. Mirrors `terrain_voxel_load`.
fn reset_latch_on_space_switch(
    mut commands: Commands,
    space_root: Res<SpaceRoot>,
    mut latch: ResMut<TerrainDiskLoadLatch>,
    disk_terrain: Query<Entity, (With<TerrainRoot>, With<DiskSourcedTerrain>)>,
) {
    if space_root.is_changed() && latch.0.as_deref() != Some(space_root.0.as_path()) {
        // A different Space is now active — re-arm, and clear the stale
        // disk-sourced terrain so GATE 3 above sees a clean slate.
        if latch.0.is_some() {
            for e in disk_terrain.iter() {
                commands.entity(e).despawn();
            }
        }
        latch.0 = None;
    }
}

/// Register the disk-terrain auto-loader. Called UNGATED from
/// [`crate::terrain_plugin::EngineTerrainPlugin`] — terrain from disk is a
/// default engine capability (the migrated-Space stand-down happens at
/// runtime via `space_is_migrated`, not at compile time).
pub fn register(app: &mut App) {
    app.init_resource::<TerrainDiskLoadLatch>().add_systems(
        Update,
        (reset_latch_on_space_switch, load_disk_terrain_on_space_open).chain(),
    );
}
