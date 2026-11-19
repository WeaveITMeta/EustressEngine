//! # Predictive prefetch for the `SpatialChunkGrid`
//!
//! Feeds [`crate::world_sim::prefetch`] with the grid's foci and chunks:
//! every active `Camera3d` and every [`StreamFocus`] (the play runtime tags
//! each avatar), each with a velocity measured from its own motion. The
//! plan says which chunks will be needed within the lead time; the planner
//! admits loads within the budget (cost = the chunk's instance count, so
//! the budget is spawns per second) and pins every predicted chunk.
//! [`super::plugin`]'s radius gate then promotes admitted chunks ahead of
//! need and never demotes a pinned one.
//!
//! The report lands in [`PrefetchReports`] under the source name
//! `"chunk_grid"`, which the inspector (`sim.inspect`) and the overlay read.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use super::plugin::StreamingState;
use super::types::{ChunkCoord, Tier};
use crate::world_sim::focus::{FocusTracker, StreamFocus};
use crate::world_sim::prefetch::{plan_cells, report_of, Focus, PrefetchConfig, PrefetchPlanner, PrefetchReports, XzGrid};

/// The report's source name.
pub const SOURCE: &str = "chunk_grid";

/// EMA weight for focus velocity samples.
const VELOCITY_SMOOTHING: f32 = 0.25;

/// The grid's prefetch state: what to promote this tick and what no pass
/// may demote.
#[derive(Resource, Debug, Clone)]
pub struct ChunkPrefetch {
    pub config: PrefetchConfig,
    pub planner: PrefetchPlanner<(i32, i32)>,
    /// Chunks to promote this tick, ahead of need.
    pub admitted: HashSet<ChunkCoord>,
    /// Chunks predicted or lingering: never demoted.
    pub pinned: HashSet<ChunkCoord>,
}

impl ChunkPrefetch {
    pub fn new(need_radius: f32) -> Self {
        Self {
            config: PrefetchConfig::from_env(need_radius),
            planner: PrefetchPlanner::default(),
            admitted: HashSet::new(),
            pinned: HashSet::new(),
        }
    }
}

fn key(c: ChunkCoord) -> (i32, i32) {
    (c.x, c.z)
}

fn coord(k: (i32, i32)) -> ChunkCoord {
    ChunkCoord { x: k.0, z: k.1 }
}

/// Sample the foci, plan, and decide this tick's prefetch.
#[allow(clippy::too_many_arguments)]
pub fn sys_predictive_prefetch(
    state: Res<StreamingState>,
    cameras: Query<(Entity, &GlobalTransform, &Camera), With<Camera3d>>,
    foci: Query<(Entity, &GlobalTransform), With<StreamFocus>>,
    time: Res<Time>,
    mut tracker: ResMut<FocusTracker>,
    mut prefetch: ResMut<ChunkPrefetch>,
    mut reports: ResMut<PrefetchReports>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs_f64();
    // A jump past the evict radius is a teleport, not motion.
    let teleport = state.config.evict_radius.max(1.0);
    let mut sampled: Vec<(u64, Vec3)> = cameras
        .iter()
        .filter(|(_, _, c)| c.is_active)
        .map(|(e, t, _)| (e.to_bits(), t.translation()))
        .collect();
    sampled.extend(foci.iter().map(|(e, t)| (e.to_bits(), t.translation())));
    let mut points: Vec<Focus> = Vec::with_capacity(sampled.len());
    for (key, pos) in sampled {
        let m = tracker.sample(key, pos, dt, VELOCITY_SMOOTHING, teleport);
        points.push(Focus { pos: m.pos, vel: m.vel });
    }
    tracker.retain_seen();

    // Chunks that still need loading (anything not yet Active), by cost.
    let mut needs_load: HashMap<(i32, i32), f32> = HashMap::new();
    let mut exists: HashSet<(i32, i32)> = HashSet::new();
    state.grid.for_each_chunk(|c, chunk| {
        exists.insert(key(*c));
        let inactive = chunk.len() - chunk.count_by_tier(Tier::Active);
        if inactive > 0 {
            needs_load.insert(key(*c), inactive as f32);
        }
    });

    let space = XzGrid { cell_size: state.config.chunk_size.max(1.0) };
    let mut plan = plan_cells(&space, &points, &prefetch.config);
    // Empty ground needs nothing loaded; keep the counters about real chunks.
    plan.cells.retain(|(k, _)| exists.contains(k));

    let resident = |k: (i32, i32)| !needs_load.contains_key(&k);
    let cost = |k: (i32, i32)| needs_load.get(&k).copied().unwrap_or(0.0);
    let cfg = prefetch.config.clone();
    let requests = prefetch.planner.update(now, &plan, &cfg, resident, cost);

    prefetch.admitted = requests.load.iter().copied().map(coord).collect();
    prefetch.pinned = prefetch.planner.pinned().into_iter().map(|(k, _)| coord(k)).collect();

    let report = report_of(SOURCE, &space, &cfg, &points, &plan, &prefetch.planner, &requests, resident);
    reports.by_source.insert(SOURCE.to_string(), report);
}
