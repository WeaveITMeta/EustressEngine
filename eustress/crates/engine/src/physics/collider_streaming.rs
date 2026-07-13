//! # Collider streaming — Perf QW5 follow-up
//!
//! The huge-scene collider gate (`instance_loader.rs`, "Perf QW5") used to
//! be all-or-nothing: on a "huge" / streaming `Space` (`world-db` feature,
//! `active_db::streaming_active()` true), anchored decorative parts got
//! **no** `Collider` / `RigidBody` at all, because attaching one to a
//! 100K–400K-part import flooded Avian's broadphase (measured: ~8.1 ms/step
//! at 100K resident static colliders vs ~1.8 ms/step at 10K).
//!
//! This module replaces that gate with a streaming tier: instead of never
//! attaching a collider, the loader stashes a small [`DeferredCollider`]
//! descriptor on the entity. The systems here materialize the real
//! `Collider` + `RigidBody::Static` only while the part is near physics
//! *activity* — an awake dynamic rigid body, the play-mode character, or the
//! active camera — and dematerialize it once the part drifts far enough
//! away. The resident collider count stays bounded (~10K) no matter how
//! large the authored scene is.
//!
//! ## Division of labour vs `residency.rs`
//!
//! [`super::super::space::residency`] governs **existence** (spawn/despawn)
//! of `BinaryEcsInstance` entities by camera locality, using a Morton-cell
//! grid keyed to the Fjall on-disk chunking (`chunk_size` 256, biased 21-bit
//! coordinates). This module governs **collider presence** on entities that
//! already exist and are already rendered — a narrower, cheaper concern —
//! and its anchors are physics activity (dynamic bodies + player + camera),
//! not just camera distance. A dedicated coarse grid (plain `f32` world
//! cells, no Morton bias) is simpler to reason about here and avoids a
//! `world-db`-only dependency on the Fjall key encoder for a purely
//! in-memory index; see [`ColliderStreamingGrid`].
//!
//! ## Interaction with `activate_physics_for_unanchored_parts`
//!
//! `play_mode.rs`'s `activate_physics_for_unanchored_parts` only flips
//! **unanchored** parts (`!basepart.anchored`) with an existing `Collider`
//! from `Static` to `Dynamic` on entering Play. `DeferredCollider` is only
//! ever attached to the huge-scene anchored/decorative branch in
//! `instance_loader.rs` (`is_static: true`), so streamed-in colliders are
//! never touched by that system — no ordering conflict.
//!
//! ## Picking (step 6 finding)
//!
//! `interaction::click::click_detector_system` (gameplay `ClickDetector`)
//! raycasts purely via `avian3d::prelude::SpatialQuery` — there is no
//! non-physics fallback, so a huge-scene part with no *resident* collider is
//! simply unclickable via `ClickDetector` until this module streams a
//! collider onto it. The editor's `part_selection.rs`, by contrast, already
//! has an OBB-based fallback pass for entities with no `Collider` (added for
//! Gaussian-splat clouds / degenerate-scale imports), so editor part
//! selection in a huge scene already worked before this change and is
//! unaffected by it. Fixing gameplay picking is out of scope here.

use std::collections::{HashMap, HashSet};

use avian3d::prelude::{Collider, RigidBody, Sleeping};
use bevy::prelude::*;

use crate::play_mode::{PlayModeCharacter, PlayModeState};
use crate::space::instance_loader::{apply_physics_material, safe_collider_from, PhysicsProperties};

// ── components ──────────────────────────────────────────────────────────

/// Stashed on an anchored/decorative part in a huge scene instead of an
/// eager `Collider` + `RigidBody`. Small (an enum + `Vec3` + `bool` +
/// `Option<PhysicsProperties>`, the last `None` for the overwhelming
/// majority of parts) — cheap to carry on every deferred entity.
///
/// `size` mirrors the `scale` parameter `safe_collider_from` expects (full
/// extents; the helper halves it internally), so materializing just forwards
/// straight into the same collider-building path the eager loader uses —
/// the two can never drift.
#[derive(Component, Clone, Debug)]
pub struct DeferredCollider {
    pub part_shape: eustress_common::classes::PartType,
    pub size: Vec3,
    pub is_static: bool,
    pub physics: Option<PhysicsProperties>,
}

/// Marks a `DeferredCollider` entity already bucketed into
/// [`ColliderStreamingGrid`] so re-indexing systems can skip it.
#[derive(Component)]
struct GridIndexed;

/// Marks an entity whose `Collider` + `RigidBody` were inserted by
/// [`sys_stream_in_colliders`] (as opposed to an eagerly-loaded collider on
/// a normal-sized scene). [`sys_stream_out_colliders`] only ever removes a
/// collider from an entity carrying this marker — it never touches
/// eagerly-loaded colliders.
#[derive(Component)]
pub struct StreamedCollider;

// ── config ───────────────────────────────────────────────────────────────

/// Tunables for the collider-streaming tier.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ColliderStreamingConfig {
    /// Parts within this radius (metres) of any anchor get a materialized
    /// collider.
    pub radius: f32,
    /// Parts beyond `radius * exit_radius_factor` of every anchor have
    /// their streamed-in collider removed. Must be `> 1.0` for hysteresis
    /// (mirrors `residency.rs`'s load/evict-radius band) so a part sitting
    /// right at `radius` doesn't stream in/out every frame.
    pub exit_radius_factor: f32,
    /// Max collider insert + remove commands issued across both streaming
    /// systems in a single frame. Bounds the per-frame command-buffer /
    /// Avian broadphase-insertion cost when a fast-moving anchor suddenly
    /// exposes a large new region.
    pub max_ops_per_frame: usize,
    /// Master switch. `false` ⇒ both streaming systems no-op (deferred
    /// colliders simply never materialize — same behavior as the old
    /// all-or-nothing gate's "never" side).
    pub enabled: bool,
    /// Coarse spatial-grid cell size (metres) used to bucket deferred
    /// colliders for the stream-IN distance test. Independent of `radius`,
    /// but should stay well below it (a handful of cells should cover one
    /// anchor's search box).
    pub cell_size: f32,
    /// Re-evaluate anchors against the deferred/streamed sets every N
    /// frames, REGARDLESS of anchor movement — the catch-all for a
    /// dynamic body waking up in place or drifting slowly. Env-tunable
    /// would mirror `residency.rs`'s `cadence_frames`, but this is a purely
    /// in-memory system with no DB-scan cost to amortize, so a plain
    /// constant default is fine.
    pub rescan_every_frames: u32,
    /// A rescan is ALSO forced early (before `rescan_every_frames` elapses)
    /// once the primary anchor (camera) has moved this far (metres) since
    /// the last rescan — keeps a fast-flying camera from waiting a full
    /// cadence window to stream in newly-nearby colliders.
    pub anchor_move_threshold: f32,
}

impl Default for ColliderStreamingConfig {
    fn default() -> Self {
        Self {
            radius: 128.0,
            exit_radius_factor: 1.25,
            max_ops_per_frame: 512,
            enabled: true,
            cell_size: 32.0,
            rescan_every_frames: 10,
            anchor_move_threshold: 16.0,
        }
    }
}

// ── spatial index ────────────────────────────────────────────────────────

type Cell = (i32, i32, i32);

fn cell_of(pos: Vec3, cell_size: f32) -> Cell {
    let cs = cell_size.max(0.001);
    (
        (pos.x / cs).floor() as i32,
        (pos.y / cs).floor() as i32,
        (pos.z / cs).floor() as i32,
    )
}

/// Coarse grid bucketing every `DeferredCollider` entity by its (static,
/// never-moving) position, so the stream-IN pass only has to distance-test
/// candidates in the handful of cells overlapping each anchor's search box
/// instead of every deferred entity in the scene (which can be 100K+ in a
/// huge import).
///
/// Entities are inserted once, at index time, and never removed on stream
/// in/out (streaming a collider on/off doesn't change the part's position,
/// so its cell membership never changes). A despawned part can leave a
/// stale `Entity` in a bucket; every consumer re-validates via `Query::get`
/// and simply skips misses, so this is safe without a removal pass.
#[derive(Resource, Default)]
pub struct ColliderStreamingGrid {
    cells: HashMap<Cell, Vec<Entity>>,
    /// Cell size the grid was built with — pinned on first insert so a
    /// runtime config change can't desync already-bucketed entities from
    /// newly-bucketed ones.
    cell_size: f32,
}

impl ColliderStreamingGrid {
    fn cell_size_or(&mut self, default: f32) -> f32 {
        if self.cell_size <= 0.0 {
            self.cell_size = default.max(0.001);
        }
        self.cell_size
    }

    /// Every candidate entity in the cell box covering `center ± radius`.
    fn candidates_in_radius(&self, center: Vec3, radius: f32) -> HashSet<Entity> {
        let mut out = HashSet::new();
        if self.cell_size <= 0.0 {
            return out;
        }
        let lo = cell_of(center - Vec3::splat(radius), self.cell_size);
        let hi = cell_of(center + Vec3::splat(radius), self.cell_size);
        for x in lo.0..=hi.0 {
            for y in lo.1..=hi.1 {
                for z in lo.2..=hi.2 {
                    if let Some(v) = self.cells.get(&(x, y, z)) {
                        out.extend(v.iter().copied());
                    }
                }
            }
        }
        out
    }
}

// ── plugin ───────────────────────────────────────────────────────────────

/// Registers the collider-streaming tier. All systems gate to
/// `PlayModeState::Playing` — colliders never need to stream in Edit mode
/// (physics is paused there; see `play_mode::pause_physics_on_startup`) and
/// a fresh Play session re-indexes/re-streams correctly every time (no
/// teardown needed on Stop — see module docs / OnExit note below).
///
/// Mount alongside `MoversPlugin` / `JointResolverPlugin`, after Avian's
/// `PhysicsPlugins` and after `PlayModeState` exists.
pub struct ColliderStreamingPlugin;

impl Plugin for ColliderStreamingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ColliderStreamingConfig>()
            .init_resource::<ColliderStreamingGrid>()
            .add_systems(
                Update,
                (
                    sys_index_deferred_colliders,
                    sys_stream_in_colliders,
                    sys_stream_out_colliders,
                )
                    .chain()
                    .run_if(in_state(PlayModeState::Playing)),
            );
        // No `OnExit(PlayModeState::Playing)` cleanup: physics is paused on
        // stop (`play_mode::deactivate_physics_for_parts` pauses
        // `Time<Physics>`), so leftover streamed-in colliders are inert —
        // they cost nothing while paused, and the next Play session's first
        // rescan reconciles the resident set against the (possibly moved)
        // anchors from scratch.
    }
}

// ── systems ──────────────────────────────────────────────────────────────

/// Bucket newly-appeared `DeferredCollider` entities into the grid. Runs
/// every frame but only touches entities without `GridIndexed` — after the
/// initial backlog (the whole huge-scene deferred set, indexed across the
/// first Playing frame or few) this is an empty query most frames.
fn sys_index_deferred_colliders(
    mut commands: Commands,
    mut grid: ResMut<ColliderStreamingGrid>,
    cfg: Res<ColliderStreamingConfig>,
    added: Query<(Entity, &Transform), (With<DeferredCollider>, Without<GridIndexed>)>,
) {
    if !cfg.enabled {
        return;
    }
    let cs = grid.cell_size_or(cfg.cell_size);
    for (entity, transform) in added.iter() {
        grid.cells
            .entry(cell_of(transform.translation, cs))
            .or_default()
            .push(entity);
        commands.entity(entity).insert(GridIndexed);
    }
}

/// Collect physics-activity anchor positions: awake dynamic rigid bodies,
/// the play-mode character(s), and the active (order-0) camera.
fn collect_anchors(
    dynamic_bodies: &Query<(&RigidBody, &GlobalTransform), Without<Sleeping>>,
    characters: &Query<&GlobalTransform, With<PlayModeCharacter>>,
    cameras: &Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) -> Vec<Vec3> {
    let mut anchors = Vec::new();
    for (rb, gt) in dynamic_bodies.iter() {
        if rb.is_dynamic() {
            anchors.push(gt.translation());
        }
    }
    for gt in characters.iter() {
        anchors.push(gt.translation());
    }
    if let Some((_, gt)) = cameras.iter().find(|(c, _)| c.order == 0) {
        anchors.push(gt.translation());
    }
    anchors
}

/// Stream a real `Collider` + `RigidBody::Static` onto any `DeferredCollider`
/// entity within `radius` of an anchor. Bounded by `max_ops_per_frame`;
/// entities not reached this frame simply stay deferred until a later
/// rescan picks them up (no correctness loss, just a later materialization).
///
/// Rescans are gated: on a plain cadence (`rescan_every_frames`) OR early
/// when the primary anchor (camera) has moved past `anchor_move_threshold`
/// since the last rescan. This avoids an O(N_deferred) grid box query every
/// single frame at 100K+ deferred entities — the box query itself is cheap
/// (grid-bucketed), but skipping it entirely on most frames is cheaper
/// still and the spec's minimum bar.
fn sys_stream_in_colliders(
    mut commands: Commands,
    cfg: Res<ColliderStreamingConfig>,
    grid: Res<ColliderStreamingGrid>,
    mut frame: Local<u32>,
    mut last_scan_camera_pos: Local<Option<Vec3>>,
    mut ops_budget: Local<usize>,
    deferred_q: Query<(&DeferredCollider, &Transform), (Without<StreamedCollider>, With<GridIndexed>)>,
    dynamic_bodies: Query<(&RigidBody, &GlobalTransform), Without<Sleeping>>,
    characters: Query<&GlobalTransform, With<PlayModeCharacter>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) {
    if !cfg.enabled {
        return;
    }
    *frame = frame.wrapping_add(1);
    let cam_pos = cameras.iter().find(|(c, _)| c.order == 0).map(|(_, gt)| gt.translation());
    let moved_enough = match (cam_pos, *last_scan_camera_pos) {
        (Some(now), Some(prev)) => now.distance(prev) > cfg.anchor_move_threshold,
        (Some(_), None) => true,
        _ => false,
    };
    let due_by_cadence = *frame % cfg.rescan_every_frames.max(1) == 0;
    if !moved_enough && !due_by_cadence {
        return;
    }
    if let Some(now) = cam_pos {
        *last_scan_camera_pos = Some(now);
    }

    let anchors = collect_anchors(&dynamic_bodies, &characters, &cameras);
    if anchors.is_empty() {
        return;
    }

    // Union of grid candidates within `radius` of any anchor.
    let mut candidates: HashSet<Entity> = HashSet::new();
    for anchor in &anchors {
        candidates.extend(grid.candidates_in_radius(*anchor, cfg.radius));
    }

    *ops_budget = cfg.max_ops_per_frame;
    let radius_sq = cfg.radius * cfg.radius;
    for entity in candidates {
        if *ops_budget == 0 {
            break;
        }
        let Ok((deferred, transform)) = deferred_q.get(entity) else {
            continue; // not deferred (already streamed) or despawned
        };
        let pos = transform.translation;
        let within_radius = anchors.iter().any(|a| a.distance_squared(pos) <= radius_sq);
        if !within_radius {
            continue;
        }
        let Some(collider) = safe_collider_from(deferred.part_shape, deferred.size, transform) else {
            // Degenerate transform (mirrored/negative scale) — same "skip
            // physics for this part" fallback the eager loader uses. Mark
            // it streamed anyway so we don't keep re-testing it every scan.
            commands.entity(entity).insert(StreamedCollider);
            *ops_budget -= 1;
            continue;
        };
        let body = if deferred.is_static {
            RigidBody::Static
        } else {
            RigidBody::Dynamic
        };
        let mut ec = commands.entity(entity);
        ec.insert((collider, body, StreamedCollider));
        apply_physics_material(&mut ec, deferred.physics.as_ref());
        *ops_budget -= 1;
    }
}

/// Remove the `Collider` + `RigidBody` from any `StreamedCollider` entity
/// once it's beyond `radius * exit_radius_factor` of every anchor. Bounded
/// by the SAME `max_ops_per_frame` pool as the stream-in pass would use in
/// isolation — reusing `cfg.max_ops_per_frame` directly here (rather than
/// sharing the `Local` budget across systems, which Bevy's parallel
/// scheduler makes awkward) keeps each pass independently bounded, which is
/// the property that actually matters (no unbounded per-frame command
/// count) even though the two pools aren't strictly summed to one total.
///
/// Cheap even unthrottled: the resident streamed set is exactly the
/// quantity this whole feature keeps bounded (~10K), so a full scan over it
/// every rescan is by design inexpensive — this is the count the baseline
/// measurement showed costs ~1.8 ms/step in Avian's broadphase, several
/// orders below the 8.1 ms/step the old eager-100K-colliders path cost.
fn sys_stream_out_colliders(
    mut commands: Commands,
    cfg: Res<ColliderStreamingConfig>,
    streamed_q: Query<(Entity, &Transform), (With<DeferredCollider>, With<StreamedCollider>)>,
    dynamic_bodies: Query<(&RigidBody, &GlobalTransform), Without<Sleeping>>,
    characters: Query<&GlobalTransform, With<PlayModeCharacter>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) {
    if !cfg.enabled {
        return;
    }
    let anchors = collect_anchors(&dynamic_bodies, &characters, &cameras);
    // No anchors at all (e.g. mid-teardown) ⇒ nothing to evict against;
    // leave streamed colliders resident rather than guessing.
    if anchors.is_empty() {
        return;
    }
    let exit_radius = cfg.radius * cfg.exit_radius_factor.max(1.0);
    let exit_radius_sq = exit_radius * exit_radius;

    let mut ops = 0usize;
    for (entity, transform) in streamed_q.iter() {
        if ops >= cfg.max_ops_per_frame {
            break;
        }
        let pos = transform.translation;
        let beyond_all = anchors.iter().all(|a| a.distance_squared(pos) > exit_radius_sq);
        if !beyond_all {
            continue;
        }
        commands
            .entity(entity)
            .remove::<Collider>()
            .remove::<RigidBody>()
            .remove::<StreamedCollider>();
        ops += 1;
    }
}

// ── tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_of_buckets_by_floor_division() {
        assert_eq!(cell_of(Vec3::new(0.0, 0.0, 0.0), 32.0), (0, 0, 0));
        assert_eq!(cell_of(Vec3::new(31.9, 0.0, 0.0), 32.0), (0, 0, 0));
        assert_eq!(cell_of(Vec3::new(32.0, 0.0, 0.0), 32.0), (1, 0, 0));
        assert_eq!(cell_of(Vec3::new(-0.1, 0.0, 0.0), 32.0), (-1, 0, 0));
        assert_eq!(cell_of(Vec3::new(-32.0, 0.0, 0.0), 32.0), (-1, 0, 0));
    }

    #[test]
    fn grid_candidates_in_radius_covers_center_and_misses_far_cell() {
        let mut grid = ColliderStreamingGrid::default();
        let cs = grid.cell_size_or(32.0);
        assert_eq!(cs, 32.0);

        let near = Entity::from_raw_u32(1).unwrap();
        let far = Entity::from_raw_u32(2).unwrap();
        grid.cells
            .entry(cell_of(Vec3::new(10.0, 0.0, 0.0), cs))
            .or_default()
            .push(near);
        grid.cells
            .entry(cell_of(Vec3::new(1000.0, 0.0, 0.0), cs))
            .or_default()
            .push(far);

        let candidates = grid.candidates_in_radius(Vec3::ZERO, 50.0);
        assert!(candidates.contains(&near));
        assert!(!candidates.contains(&far));
    }

    #[test]
    fn hysteresis_exit_radius_wider_than_radius() {
        // Exit radius must exceed the stream-in radius so a part sitting
        // right at the boundary doesn't flicker in/out every rescan.
        let cfg = ColliderStreamingConfig::default();
        assert!(cfg.exit_radius_factor > 1.0);
        assert!(cfg.radius * cfg.exit_radius_factor > cfg.radius);
    }
}
