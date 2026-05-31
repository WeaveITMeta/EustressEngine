//! Camera-locality streaming residency manager (Phase 2).
//!
//! Keeps the live Bevy ECS set bounded (~tens of thousands) while up to
//! ~10M entity cores live in Fjall. As the camera moves, cells entering
//! the `load_radius` are scanned from the `entities` partition and their
//! cores spawned; cells leaving the `evict_radius` are despawned. A
//! hysteresis band (load vs evict radius) stops thrash at boundaries.
//!
//! ## Division of labour
//! - This manager governs **existence** (spawn / despawn by camera cell).
//! - [`super::world_db_binary::mirror_binary_ecs_changes`] persists edits;
//!   it is chained to run BEFORE eviction so an edited entity's core is
//!   written before it can be despawned (Phase 2 risk R1).
//! - `streaming::render_cascade` governs visibility/LOD for the *disk*
//!   streaming path (`StreamingInstanceRef`); it does not see binary
//!   entities, so in v1 the `active_radius` IS the coarse cull for them.
//!
//! Only active for a "large" Space (core count over a threshold). A small
//! Space is boot-loaded whole and this manager stays idle — no regression.

#![cfg(feature = "world-db")]

use std::collections::{HashSet, VecDeque};

use bevy::prelude::*;
use eustress_worlddb::{keys::world_to_cell, MortonKeyEncoder};

use super::active_db;
use super::file_loader::LoadInProgress;
use super::instance_loader::PrimitiveMeshCache;
use super::material_loader::MaterialRegistry;
use super::service_loader::ServiceComponent;
use super::world_db_binary::{spawn_binary_core, BinaryEcsInstance};
use super::SpaceRoot;

/// Unsigned 21-bit Morton cell coordinate triple, at the encoder's
/// chunk_size (256). The unit the manager loads / evicts in.
type Cell = (u32, u32, u32);

/// Tunables for the residency manager. Radii mirror `StreamingConfig`.
#[derive(Resource)]
pub struct ResidencyConfig {
    /// Cells within this radius of the camera are loaded.
    pub load_radius: f32,
    /// Cells beyond this radius are evicted. `> load_radius` ⇒ hysteresis.
    pub evict_radius: f32,
    /// Max cells SCANNED from Fjall per tick (bounds DB work).
    pub max_cell_loads_per_tick: usize,
    /// Max cores SPAWNED per tick (bounds main-thread spawn cost).
    pub spawn_budget_per_tick: usize,
    /// Max entities DESPAWNED per tick.
    pub despawn_budget_per_tick: usize,
    /// Re-evaluate the camera box every N frames (when the queue is idle).
    pub cadence_frames: u32,
    /// Core count above which a Space streams instead of boot-loading all.
    pub big_space_threshold: usize,
}

impl Default for ResidencyConfig {
    fn default() -> Self {
        Self {
            load_radius: 500.0,
            evict_radius: 600.0,
            max_cell_loads_per_tick: 8,
            spawn_budget_per_tick: 2048,
            despawn_budget_per_tick: 4096,
            cadence_frames: 8,
            big_space_threshold: 100_000,
        }
    }
}

/// Live state of the residency manager. `enabled` is set by the boot-load
/// (true only for large Spaces).
#[derive(Resource, Default)]
pub struct ResidencyState {
    pub enabled: bool,
    frame: u32,
    pub last_camera_cell: Option<Cell>,
    /// Cells considered loaded (within the keep box). Prevents re-scanning.
    resident_cells: HashSet<Cell>,
    /// Cells queued to scan from Fjall.
    pending_cells: VecDeque<Cell>,
    pending_set: HashSet<Cell>,
    /// Cores scanned but not yet spawned (drained under the spawn budget).
    pending_cores: VecDeque<(u64, Vec<u8>)>,
    /// Inclusive (min, max) cell bounds of the current keep box — the
    /// authority `sys_residency_evict` checks live entity positions against.
    keep_box: Option<(Cell, Cell)>,
}

// ── pure cell math (unit-tested) ────────────────────────────────────────

/// Camera world position + radius → inclusive cell box, using the SAME
/// encoder (chunk_size 256) the cores were written with — so the box maps
/// 1:1 onto the Morton cells `put_instance_core` used.
fn camera_cell_box(cam: Vec3, radius: f32) -> (Cell, Cell) {
    let cs = MortonKeyEncoder::default().chunk_size;
    let lo = |c: f32| world_to_cell(c - radius, cs);
    let hi = |c: f32| world_to_cell(c + radius, cs);
    (
        (lo(cam.x), lo(cam.y), lo(cam.z)),
        (hi(cam.x), hi(cam.y), hi(cam.z)),
    )
}

/// The cell a world position falls in.
fn cell_of(pos: Vec3) -> Cell {
    let cs = MortonKeyEncoder::default().chunk_size;
    (
        world_to_cell(pos.x, cs),
        world_to_cell(pos.y, cs),
        world_to_cell(pos.z, cs),
    )
}

fn in_box(c: Cell, b: (Cell, Cell)) -> bool {
    let (lo, hi) = b;
    c.0 >= lo.0 && c.0 <= hi.0 && c.1 >= lo.1 && c.1 <= hi.1 && c.2 >= lo.2 && c.2 <= hi.2
}

fn cells_in_box(b: (Cell, Cell)) -> Vec<Cell> {
    let (lo, hi) = b;
    let mut out = Vec::new();
    for x in lo.0..=hi.0 {
        for y in lo.1..=hi.1 {
            for z in lo.2..=hi.2 {
                out.push((x, y, z));
            }
        }
    }
    out
}

// ── systems ─────────────────────────────────────────────────────────────

/// Scan + spawn cores for cells entering the camera's load box. Bounded by
/// `max_cell_loads_per_tick` (DB scans) and `spawn_budget_per_tick`
/// (spawns), so a fast camera over a dense region never blows a frame.
#[allow(clippy::too_many_arguments)]
pub fn sys_residency_load(
    mut commands: Commands,
    space_root: Res<SpaceRoot>,
    load_in_progress: Res<LoadInProgress>,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material_registry: ResMut<MaterialRegistry>,
    mut mesh_cache: ResMut<PrimitiveMeshCache>,
    services: Query<(Entity, &ServiceComponent)>,
    existing: Query<&BinaryEcsInstance>,
    cameras: Query<(&GlobalTransform, &Camera), With<Camera3d>>,
    mut state: ResMut<ResidencyState>,
    cfg: Res<ResidencyConfig>,
) {
    if !state.enabled || load_in_progress.active || !active_db::is_active() {
        return;
    }
    // Cadence gate, but always allow draining a non-empty queue so a big
    // region keeps filling across frames.
    state.frame = state.frame.wrapping_add(1);
    let cadence = cfg.cadence_frames.max(1);
    let queue_idle = state.pending_cells.is_empty() && state.pending_cores.is_empty();
    if state.frame % cadence != 0 && queue_idle {
        return;
    }

    let Some(workspace) = services
        .iter()
        .find(|(_, s)| s.class_name == "Workspace")
        .map(|(e, _)| e)
    else {
        return; // services not ready yet — retry next tick
    };
    // Main viewport camera (order 0) — not the AI / gizmo cameras.
    let Some((cam_tf, _)) = cameras.iter().find(|(_, c)| c.order == 0) else {
        return;
    };
    let campos = cam_tf.translation();
    let cam_cell = cell_of(campos);

    // Recompute desired/keep boxes only when the camera crosses a cell
    // boundary (most frames it hasn't).
    if state.last_camera_cell != Some(cam_cell) {
        state.last_camera_cell = Some(cam_cell);
        let load_box = camera_cell_box(campos, cfg.load_radius);
        let keep_box = camera_cell_box(campos, cfg.evict_radius);
        state.keep_box = Some(keep_box);

        // Enqueue load-box cells not already resident or pending.
        let new_cells: Vec<Cell> = cells_in_box(load_box)
            .into_iter()
            .filter(|c| !state.resident_cells.contains(c) && !state.pending_set.contains(c))
            .collect();
        for c in new_cells {
            state.pending_set.insert(c);
            state.pending_cells.push_back(c);
        }
        // Forget resident cells now outside the keep box; their entities
        // are despawned by sys_residency_evict (by live position). If the
        // camera returns, they're no longer resident so they reload.
        state.resident_cells.retain(|c| in_box(*c, keep_box));
    }

    // Scan up to N cells into the core buffer (mark resident on scan).
    let mut cells_scanned = 0;
    while cells_scanned < cfg.max_cell_loads_per_tick {
        let Some(cell) = state.pending_cells.pop_front() else {
            break;
        };
        state.pending_set.remove(&cell);
        let cores = active_db::iter_instance_cores_in_region(
            (cell.0, cell.0),
            (cell.1, cell.1),
            (cell.2, cell.2),
        );
        for cb in cores {
            state.pending_cores.push_back(cb);
        }
        state.resident_cells.insert(cell);
        cells_scanned += 1;
    }

    // Spawn from the buffer up to the per-tick budget, deduped against the
    // live set (covers overlap with runtime-created entities).
    if !state.pending_cores.is_empty() {
        let existing_ids: HashSet<u64> = existing.iter().map(|b| b.stored_id).collect();
        let mut spawned = 0;
        while spawned < cfg.spawn_budget_per_tick {
            let Some((stored_id, bytes)) = state.pending_cores.pop_front() else {
                break;
            };
            if existing_ids.contains(&stored_id) {
                continue;
            }
            spawn_binary_core(
                &mut commands,
                &asset_server,
                &mut materials,
                &mut material_registry,
                &mut mesh_cache,
                &space_root.0,
                workspace,
                stored_id,
                &bytes,
            );
            spawned += 1;
        }
    }
}

/// Despawn binary entities whose LIVE position has left the keep box.
/// Runs AFTER the mirror (see `register`) so edits are persisted first.
/// Decides by live position (not a cached cell), so a moved entity is
/// evicted from where it actually is (Phase 2 risk R2). Selected entities
/// are kept resident so the user never loses a selection to streaming.
pub fn sys_residency_evict(
    mut commands: Commands,
    state: Res<ResidencyState>,
    cfg: Res<ResidencyConfig>,
    entities: Query<(Entity, &GlobalTransform), With<BinaryEcsInstance>>,
    selected: Query<(), With<crate::selection_box::Selected>>,
) {
    if !state.enabled {
        return;
    }
    let Some(keep) = state.keep_box else {
        return;
    };
    let mut despawned = 0usize;
    for (e, gt) in entities.iter() {
        if despawned >= cfg.despawn_budget_per_tick {
            break;
        }
        if in_box(cell_of(gt.translation()), keep) {
            continue;
        }
        if selected.contains(e) {
            continue; // keep selected entities alive
        }
        commands.entity(e).despawn();
        despawned += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // world_to_cell(c, 256) = floor(c/256) + 2^20, clamped to 21 bits.
    const BIAS: u32 = 1 << 20;

    #[test]
    fn camera_box_at_origin_radius_500() {
        let (lo, hi) = camera_cell_box(Vec3::ZERO, 500.0);
        // floor(-500/256) = -2 → BIAS-2 ; floor(500/256) = 1 → BIAS+1.
        assert_eq!(lo, (BIAS - 2, BIAS - 2, BIAS - 2));
        assert_eq!(hi, (BIAS + 1, BIAS + 1, BIAS + 1));
    }

    #[test]
    fn in_box_and_cells_count() {
        let b = ((10, 10, 10), (12, 11, 10)); // 3 × 2 × 1 = 6 cells
        let cells = cells_in_box(b);
        assert_eq!(cells.len(), 6);
        assert!(in_box((11, 10, 10), b));
        assert!(!in_box((13, 10, 10), b)); // x out
        assert!(!in_box((11, 12, 10), b)); // y out
    }

    #[test]
    fn cell_of_origin_is_biased_center() {
        assert_eq!(cell_of(Vec3::ZERO), (BIAS, BIAS, BIAS));
    }

    #[test]
    fn hysteresis_keep_box_wider_than_load_box() {
        // The evict box must be a superset of the load box so a cell loaded
        // at the load boundary isn't immediately evicted (no thrash).
        let cam = Vec3::new(123.0, 0.0, -456.0);
        let load = camera_cell_box(cam, 500.0);
        let keep = camera_cell_box(cam, 600.0);
        for c in cells_in_box(load) {
            assert!(in_box(c, keep), "load cell {c:?} must be inside keep box");
        }
    }
}
