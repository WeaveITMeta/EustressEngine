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
    /// Max cores SPAWNED per tick in STEADY STATE (bounds main-thread spawn
    /// cost once the camera bubble is full). Kept conservative (2048) to
    /// avoid post-load frame jitter while flying.
    pub spawn_budget_per_tick: usize,
    /// Max cores SPAWNED per tick WHILE THE INITIAL CAMERA BUBBLE IS STILL
    /// FILLING (queue non-idle). Higher than the steady value so the first
    /// keep-box reaches the screen faster; `sys_residency_load` falls back to
    /// `spawn_budget_per_tick` once the queue drains, so the boost cannot
    /// cause steady-state jitter. Env: `EUSTRESS_RESIDENCY_SPAWN_BUDGET`.
    pub spawn_budget_boost: usize,
    /// Max entities DESPAWNED per tick.
    pub despawn_budget_per_tick: usize,
    /// Re-evaluate the camera box every N frames (when the queue is idle).
    /// Env: `EUSTRESS_RESIDENCY_CADENCE`. Lower = the camera box re-fills in
    /// tighter batches (faster tail-load) at the cost of more frequent DB
    /// scans on a moving camera.
    pub cadence_frames: u32,
    /// Core count above which a Space streams instead of boot-loading all.
    pub big_space_threshold: usize,
    /// M2a — per-entity DRAW cull radius (metres). A `BinaryEcsInstance`
    /// whose live distance from the camera exceeds this is despawned, on top
    /// of (and independent from) the keep-box eviction. Defaults to
    /// `load_radius`, so it only trims the hysteresis band between
    /// `load_radius` and `evict_radius` — i.e. a near-no-op that despawns a
    /// far ring slightly sooner than the keep-box alone would.
    ///
    /// THE DIAGNOSTIC KNOB: set `EUSTRESS_RESIDENCY_CULL` BELOW `load_radius`
    /// to shrink the live/drawn binary set at runtime WITHOUT touching the
    /// keep-box load logic (cells still scan/spawn out to `load_radius`;
    /// this just hides/despawns the outer shell). Pair with EUSTRESS_PROFILE
    /// and read whether render+present ms drops with the live count
    /// (draw-call-bound) or not (pixel-bound) — the M2a gate.
    pub cull_radius: f32,
}

impl Default for ResidencyConfig {
    fn default() -> Self {
        // load_radius is the BINDING render-distance limit: parts beyond it are
        // never streamed, so nothing renders past it. It was tightened to 150
        // because, BEFORE lazy non-Workspace load, a big bubble pulled ~all
        // Workspace parts AND the ~196K storage entities were also live — the
        // budget was gone. With storage now lazy (file_loader skips non-eager
        // services), there is headroom to push distance out again.
        //
        // Env-tunable so the optimization↔distance balance can be dialed WITHOUT
        // a 16-min rebuild: EUSTRESS_RESIDENCY_LOAD / EUSTRESS_RESIDENCY_EVICT
        // (metres). Evict must exceed load (hysteresis); it defaults to 1.4×load.
        // Pair load_radius with WorkspaceComponent.render_distance (the
        // VisibilityRange cull) so streamed parts actually draw to the edge.
        let load = std::env::var("EUSTRESS_RESIDENCY_LOAD")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|v| *v > 0.0)
            .unwrap_or(350.0);
        let evict = std::env::var("EUSTRESS_RESIDENCY_EVICT")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|v| *v > load)
            .unwrap_or((load * 1.4).max(500.0));
        // Cadence: how often (frames) the camera box is re-evaluated while the
        // residency queue is idle. Raised 4→12 (P2 Update-spike fix): the
        // full-scan eviction pass (`sys_residency_evict` walks ALL
        // `With<BinaryEcsInstance>` entities, ~120K on Vehicle Simulator) is the
        // ~20 ms periodic Update spike. `sys_residency_load`'s cadence gate
        // already only RE-EVALUATES the camera box every `cadence` idle frames,
        // so widening it to 12 spreads that work over 3× fewer frames. The cost
        // is a slightly slower tail-load on a fast-moving camera (the box
        // re-fills in larger, less frequent batches) — acceptable, and the knob
        // stays env-tunable (EUSTRESS_RESIDENCY_CADENCE) down to 1 for a faster
        // tail-load or back up for an even calmer steady state, no rebuild.
        let cadence = std::env::var("EUSTRESS_RESIDENCY_CADENCE")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|v| *v >= 1)
            .unwrap_or(12);
        // Spawn budget BOOST (cores/tick while the initial bubble fills).
        // Raised 2048→4096 (verdict: halves spawn stalls during bubble fill).
        // Only applied while the queue is non-idle (see sys_residency_load),
        // so steady-state flying stays at the conservative 2048 — no post-load
        // jitter. Env-tunable for low-end hardware without a rebuild.
        let spawn_boost = std::env::var("EUSTRESS_RESIDENCY_SPAWN_BUDGET")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|v| *v >= 1)
            .unwrap_or(4096);
        // M2a — per-entity draw cull radius. Defaults to load_radius (only
        // trims the hysteresis band → near-no-op vs the keep-box). Set
        // EUSTRESS_RESIDENCY_CULL below load_radius to tighten the live set
        // at runtime for the draw-call-vs-pixel-bound measurement. A value
        // ABOVE evict_radius would never fire (the keep-box already evicts
        // first), but it is accepted and simply means "no extra cull".
        let cull = std::env::var("EUSTRESS_RESIDENCY_CULL")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|v| *v > 0.0)
            .unwrap_or(load);
        Self {
            load_radius: load,
            evict_radius: evict,
            max_cell_loads_per_tick: 8,
            spawn_budget_per_tick: 2048,
            spawn_budget_boost: spawn_boost,
            despawn_budget_per_tick: 4096,
            cadence_frames: cadence,
            big_space_threshold: 100_000,
            cull_radius: cull,
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
    /// One-shot latch for the LOAD-PHASE "first residency fill" milestone:
    /// set true once the initial camera keep-box has been fully scanned +
    /// spawned (queue first drains to idle after having had work), so the
    /// milestone logs exactly once per Space load. Purely diagnostic.
    first_fill_logged: bool,
}

impl ResidencyState {
    /// Snapshot of the cells currently considered resident (within the keep
    /// box). Exposed so the sim-orchestration driver
    /// ([`super::sim_orchestration`]) can enumerate residency cells for its
    /// per-cell gang-placement latch WITHOUT duplicating the camera box math
    /// (which would risk a second, drifting coordinate authority). A cell is
    /// inserted here at SCAN time, before its cores finish spawning — fine for
    /// per-cell sim placement (one SimCell per spatial cell, not per entity).
    pub fn resident_cells(&self) -> impl Iterator<Item = Cell> + '_ {
        self.resident_cells.iter().copied()
    }
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
    //
    // BUDGET GATE (Task C, verdict correction #3 — "boost only while loading,
    // drop to steady after settle to avoid frame jitter"). The verdict's
    // literal `LoadInProgress.active` gate is unreachable HERE: this system
    // early-returns while `load_in_progress.active` (top of fn), so residency
    // only ever runs AFTER the file-loader settles. The genuine "still
    // loading" window for the residency subsystem is the INITIAL CAMERA
    // BUBBLE FILL — i.e. before the keep-box first drains to idle
    // (`first_fill_logged` flips true at that point, milestone 6). So: use
    // the boosted budget while the initial bubble is still filling, then the
    // conservative steady-state budget for all later (flying) streaming —
    // which is exactly the no-post-load-jitter behaviour the verdict wants.
    let budget = if state.first_fill_logged {
        cfg.spawn_budget_per_tick // steady state (flying) — conservative
    } else {
        cfg.spawn_budget_boost.max(cfg.spawn_budget_per_tick) // initial fill
    };
    if !state.pending_cores.is_empty() {
        let existing_ids: HashSet<u64> = existing.iter().map(|b| b.stored_id).collect();
        let mut spawned = 0;
        while spawned < budget {
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

    // LOAD-PHASE milestone 6: the initial camera keep-box is satisfied —
    // the first batch of cells has been scanned AND fully spawned, so the
    // queue has drained to idle. One-shot per load (latched). For a
    // streaming Space this is "time-to-first-visible-geometry near camera".
    if !state.first_fill_logged
        && !state.resident_cells.is_empty()
        && state.pending_cells.is_empty()
        && state.pending_cores.is_empty()
    {
        state.first_fill_logged = true;
        super::load_phase::mark("residency-first-fill");
        // M0 (diagnostics): for a LARGE (streaming) Space the boot-load is
        // skipped, so the file-loader `eager-spawn-complete` SPAWN-COST emit
        // saw ~no binary spawns. The cores are spawned HERE during the first
        // camera-bubble fill, so emit the breakdown again at this one-shot
        // milestone — now it reflects the streaming spawn population.
        // Env-gated on EUSTRESS_PROFILE; silent otherwise.
        super::world_db_binary::spawn_cost::log_summary();
    }
}

/// Despawn binary entities whose LIVE position has left the keep box, OR
/// (M2a) whose live distance from the camera exceeds `cull_radius`.
///
/// Runs AFTER the mirror (see `register`) so edits are persisted first —
/// the mirror-before-evict ordering (Phase 2 risk R1) is preserved: this
/// only ADDS a second despawn predicate to the SAME system, it does not
/// reorder anything. Decides by live position (not a cached cell), so a
/// moved entity is evicted from where it actually is (Phase 2 risk R2).
/// Selected entities are kept resident so the user never loses a selection
/// to streaming — the `Selected` pin `sys_stream_in_on_select` relies on is
/// honoured for BOTH predicates (checked once, before either fires).
///
/// ## Two independent evict predicates (M2a)
///
/// 1. **Keep-box** (existing): the authoritative load-lifecycle cull. A
///    cell outside the `evict_radius` keep-box is forgotten by
///    `sys_residency_load`; its entities are despawned here. UNCHANGED.
/// 2. **Cull-radius** (new, env `EUSTRESS_RESIDENCY_CULL`, default =
///    `load_radius`): a tighter per-entity DRAW cull. With the env set
///    BELOW `load_radius`, the live `BinaryEcsInstance` set shrinks to a
///    ball of `cull_radius` around the camera, WITHOUT changing what cells
///    `sys_residency_load` scans/spawns (that still uses load/evict radius).
///    This is the measurement lever: shrink the drawn set, then read
///    `eustress_profile_phases.txt` to decide draw-call-bound vs pixel-bound.
///
/// Default (`cull_radius == load_radius`) is near-no-op: it only trims the
/// hysteresis band between `load_radius` and `evict_radius` — entities the
/// keep-box would evict a little later anyway. It never culls inside the
/// load ball, so geometry the camera can reach always stays present.
pub fn sys_residency_evict(
    mut commands: Commands,
    state: Res<ResidencyState>,
    cfg: Res<ResidencyConfig>,
    entities: Query<(Entity, &GlobalTransform), With<BinaryEcsInstance>>,
    selected: Query<(), With<crate::selection_box::Selected>>,
    cameras: Query<(&GlobalTransform, &Camera), With<Camera3d>>,
    // P2 two-tier: record each unload so emit_scene_change_deltas suppresses the
    // resulting Name-removal — an evict is not a delete (the part stays in the
    // Fjall DB and re-streams). See eustress_common::change_queue::EvictedRecently.
    // `Option` because the resource is owned by the change-queue StreamingPlugin
    // (`#[cfg(feature = "streaming")]`); a `world-db`-without-`streaming` build
    // has residency but no delta emitter, so there is nothing to suppress and
    // the resource is simply absent — recording is then a no-op.
    mut evicted: Option<ResMut<eustress_common::change_queue::EvictedRecently>>,
) {
    if !state.enabled {
        return;
    }
    let Some(keep) = state.keep_box else {
        return;
    };
    // Main viewport camera (order 0) — the SAME camera sys_residency_load
    // fills around, so the cull ball is centred on the load box's centre.
    // Squared compare avoids a per-entity sqrt. `None` (camera not up yet)
    // disables ONLY the distance cull; the keep-box cull still runs.
    let cam = cameras
        .iter()
        .find(|(_, c)| c.order == 0)
        .map(|(t, _)| t.translation());
    let cull_sq = cfg.cull_radius * cfg.cull_radius;

    let mut despawned = 0usize;
    for (e, gt) in entities.iter() {
        if despawned >= cfg.despawn_budget_per_tick {
            break;
        }
        let pos = gt.translation();
        let in_keep = in_box(cell_of(pos), keep);
        // M2a: beyond the per-entity cull radius? (only when a camera exists)
        let beyond_cull = cam
            .map(|c| (pos - c).length_squared() > cull_sq)
            .unwrap_or(false);
        // Survives only if inside the keep-box AND within the cull radius.
        if in_keep && !beyond_cull {
            continue;
        }
        // Selection pin wins over BOTH predicates: a selected entity is
        // never despawned, so sys_stream_in_on_select's pin holds (the user
        // never loses a selection to streaming OR to the tighter cull).
        if selected.contains(e) {
            continue;
        }
        commands.entity(e).despawn();
        // Mark this as an EVICT (not a delete) so the change-delta emitter
        // drops the Name-removal it is about to see — the part is unloaded from
        // the live ECS but still lives in the Fjall DB and re-streams on demand.
        if let Some(ev) = evicted.as_mut() {
            ev.record(e);
        }
        despawned += 1;
    }
}

// ── M0 diagnostics: live entity-count-by-type ───────────────────────────

/// Read `EUSTRESS_PROFILE` exactly once. Same knob the phase profiler, the
/// LOAD-PHASE milestones, and the SPAWN-COST breakdown read — one env var
/// arms every diagnostic. Dormant otherwise (one relaxed `OnceLock` read).
fn diag_armed() -> bool {
    use std::sync::OnceLock;
    static ARMED: OnceLock<bool> = OnceLock::new();
    *ARMED.get_or_init(|| {
        std::env::var_os("EUSTRESS_PROFILE")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    })
}

/// How often (frames) the entity-count line is logged while armed. Default
/// 60; env `EUSTRESS_PROFILE_COUNT_FRAMES` (>=1) overrides without a rebuild.
fn diag_count_cadence() -> u32 {
    use std::sync::OnceLock;
    static N: OnceLock<u32> = OnceLock::new();
    *N.get_or_init(|| {
        std::env::var("EUSTRESS_PROFILE_COUNT_FRAMES")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|v| *v >= 1)
            .unwrap_or(60)
    })
}

/// M0 — periodically log the live entity count split by representation so a
/// reader can correlate live counts with the render/present ms in
/// `eustress_profile_phases.txt`:
///
/// ```text
/// ENTITY-COUNT: binary=<N> streaming=<N> total=<N>
/// ```
///
/// * `binary`    — `BinaryEcsInstance` (the Fjall-backed scalable path; the
///   set the M2a cull shrinks).
/// * `streaming` — `StreamingInstanceRef` (the disk-streaming render-cascade
///   path).
/// * `total`     — `binary + streaming` (the two authored-part populations;
///   not the whole World, which also holds services, cameras, UI, gizmos).
///
/// Cheap: two `Query::iter().count()`s on a 60-frame cadence, ONLY when
/// `EUSTRESS_PROFILE` is armed. Returns immediately (one `OnceLock` read)
/// otherwise, so it costs nothing in a normal run.
///
/// `StreamingInstanceRef` only exists when the `streaming` feature is on
/// (it lives in `eustress_common::streaming`, gated by `eustress-common/
/// streaming`). `streaming` is part of the default `core` feature, so this
/// is the normal build; the `#[cfg(not(feature = "streaming"))]` twin keeps
/// a `world-db`-only build (e.g. `--no-default-features --features world-db`)
/// compiling, reporting `streaming=0`.
#[cfg(feature = "streaming")]
pub fn sys_entity_count_diag(
    mut frame: Local<u32>,
    binary: Query<(), With<BinaryEcsInstance>>,
    streaming: Query<(), With<eustress_common::streaming::plugin::StreamingInstanceRef>>,
) {
    if !diag_armed() {
        return;
    }
    *frame = frame.wrapping_add(1);
    if *frame % diag_count_cadence() != 0 {
        return;
    }
    let n_binary = binary.iter().count();
    let n_streaming = streaming.iter().count();
    info!(
        target: "eustress_engine::world_db",
        "ENTITY-COUNT: binary={n_binary} streaming={n_streaming} total={}",
        n_binary + n_streaming
    );
}

/// `world-db`-only fallback (no `streaming` feature): the
/// `StreamingInstanceRef` type is absent, so report `streaming=0`. Same
/// arming + cadence semantics as the full variant above.
#[cfg(not(feature = "streaming"))]
pub fn sys_entity_count_diag(
    mut frame: Local<u32>,
    binary: Query<(), With<BinaryEcsInstance>>,
) {
    if !diag_armed() {
        return;
    }
    *frame = frame.wrapping_add(1);
    if *frame % diag_count_cadence() != 0 {
        return;
    }
    let n_binary = binary.iter().count();
    info!(
        target: "eustress_engine::world_db",
        "ENTITY-COUNT: binary={n_binary} streaming=0 total={n_binary}"
    );
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
