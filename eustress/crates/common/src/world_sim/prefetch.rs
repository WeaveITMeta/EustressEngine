//! # Predictive prefetch from velocity
//!
//! RAGE streams for where the player will be, not where the player is: the
//! request for the next block goes out while the car is still a few seconds
//! short of it. This module is that predictor, pure and engine-free.
//!
//! ## The model
//!
//! A cell is **needed** while its centre is within `need_radius` of a focus
//! (the same rule as a streamer's load ball). With the focus moving at `v`,
//! the time until a cell becomes needed is the earliest `t` with
//! `|p + v t - c| <= need_radius`, a quadratic solved exactly by
//! [`eta_within`]. Every cell with an ETA inside the lead time is
//! **predicted**; the ETA is its deadline and its priority. The predicted
//! region is therefore a capsule swept along the path, ordered by arrival.
//!
//! [`PrefetchPlanner`] turns successive plans into load requests:
//!
//! - **Hysteresis:** a predicted cell stays *pinned* for `linger_secs`
//!   after it drops out of the plan, so velocity noise cannot flap it.
//! - **Never evict what is in use:** [`PrefetchPlanner::is_pinned`] is the
//!   question an evictor asks before dropping a cell.
//! - **I/O budget:** a token bucket in caller-defined cost units (bytes for a
//!   disk streamer, instances for a spawner). A cell needed now is always
//!   admitted, up to `max_loads_per_tick`, even into debt; a cell only
//!   predicted waits for tokens. Prefetch never starves demand.
//! - **Counters:** a hit is a cell already resident when it became needed;
//!   a prefetch hit is a hit that a request made ahead of need produced; a
//!   miss is a cell that was not resident when needed; waste is a cell
//!   requested ahead that expired without ever being needed.
//!
//! Output order is deterministic: by ETA, then by cell key.

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

use bevy::prelude::*;
use serde::Serialize;

use super::focus::STATIONARY_SPEED;

/// A point the world streams around, with its velocity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Focus {
    pub pos: Vec3,
    pub vel: Vec3,
}

/// Tunables for one prefetch source.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PrefetchConfig {
    /// Seconds of warning the predictor aims for. `0` turns prediction off
    /// and leaves only the cells needed now.
    pub lead_time_secs: f32,
    /// Hard cap on how far ahead the swept capsule reaches, metres, so the
    /// cost follows the radius rather than the speed a player reaches.
    pub max_lead_distance: f32,
    /// A cell is needed while its centre is within this of a focus, metres.
    pub need_radius: f32,
    /// Seconds a cell stays pinned after it leaves the plan.
    pub linger_secs: f32,
    /// Seconds before a request that has not made the cell resident is
    /// sent again.
    pub retry_secs: f32,
    /// Budget refill, cost units per second.
    pub budget_units_per_sec: f32,
    /// Budget cap, cost units.
    pub budget_burst: f32,
    /// Most load requests per update, needed and predicted together.
    pub max_loads_per_tick: usize,
    /// Most candidate cells one focus may enumerate. A larger box (a tiny
    /// cell size, a huge radius) falls back to the cells needed now.
    pub max_candidates: usize,
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        Self {
            lead_time_secs: 2.0,
            max_lead_distance: 600.0,
            need_radius: 500.0,
            linger_secs: 1.5,
            retry_secs: 1.0,
            budget_units_per_sec: 200_000.0,
            budget_burst: 50_000.0,
            max_loads_per_tick: 8,
            max_candidates: 4_096,
        }
    }
}

impl PrefetchConfig {
    /// Defaults with environment overrides: `EUSTRESS_PREFETCH_LEAD_SECS`,
    /// `EUSTRESS_PREFETCH_MAX_LEAD`, `EUSTRESS_PREFETCH_BUDGET` (units per
    /// second) and `EUSTRESS_PREFETCH_MAX_LOADS`.
    pub fn from_env(need_radius: f32) -> Self {
        let envf = |key: &str| std::env::var(key).ok().and_then(|s| s.trim().parse::<f32>().ok()).filter(|v| v.is_finite() && *v >= 0.0);
        let d = Self { need_radius, ..Self::default() };
        Self {
            lead_time_secs: envf("EUSTRESS_PREFETCH_LEAD_SECS").unwrap_or(d.lead_time_secs),
            max_lead_distance: envf("EUSTRESS_PREFETCH_MAX_LEAD").unwrap_or(d.max_lead_distance).max(1.0),
            budget_units_per_sec: envf("EUSTRESS_PREFETCH_BUDGET").unwrap_or(d.budget_units_per_sec),
            max_loads_per_tick: envf("EUSTRESS_PREFETCH_MAX_LOADS").map(|v| v as usize).unwrap_or(d.max_loads_per_tick).max(1),
            ..d
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Geometry
// ─────────────────────────────────────────────────────────────────────────────

/// How far ahead a focus moving at `speed` is predicted, in seconds: the
/// lead time, shortened so the capsule never reaches past `max_lead`.
/// Stationary foci predict nothing.
pub fn effective_horizon(speed: f32, lead_time: f32, max_lead: f32) -> f32 {
    if !speed.is_finite() || speed <= STATIONARY_SPEED || lead_time <= 0.0 {
        return 0.0;
    }
    lead_time.min(max_lead.max(0.0) / speed)
}

/// Earliest time in `[0, horizon]` at which a point at `pos` moving at
/// `vel` comes within `radius` of `target`; `Some(0.0)` if it already is,
/// `None` if it never does within the horizon.
pub fn eta_within(pos: Vec3, vel: Vec3, target: Vec3, radius: f32, horizon: f32) -> Option<f32> {
    let d = pos - target;
    let c = d.length_squared() - radius * radius;
    if c <= 0.0 {
        return Some(0.0);
    }
    if horizon <= 0.0 {
        return None;
    }
    let a = vel.length_squared();
    if a <= f32::EPSILON {
        return None;
    }
    let b = 2.0 * d.dot(vel);
    if b >= 0.0 {
        // Moving away or along the tangent: the distance never shrinks.
        return None;
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / (2.0 * a);
    (t >= 0.0 && t <= horizon).then_some(t)
}

/// A spatial cell layout the predictor enumerates.
pub trait CellSpace {
    type Key: Copy + Eq + Hash + Ord + Debug;
    /// The point as this space measures distance (a 2D grid drops `y`).
    fn project(&self, v: Vec3) -> Vec3;
    /// Centre of a cell, in projected space.
    fn center(&self, key: Self::Key) -> Vec3;
    /// How many cells overlap the box, without enumerating them.
    fn count_in_aabb(&self, min: Vec3, max: Vec3) -> u64;
    /// Every cell overlapping the projected box.
    fn keys_in_aabb(&self, min: Vec3, max: Vec3) -> Vec<Self::Key>;
    /// Edge length of a cell, metres.
    fn cell_size(&self) -> f32;
}

fn cell_index(c: f32, size: f32) -> i32 {
    (c / size).floor().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

/// A square grid on the XZ plane: `SpatialChunkGrid`'s `ChunkCoord` layout.
#[derive(Debug, Clone, Copy)]
pub struct XzGrid {
    pub cell_size: f32,
}

impl CellSpace for XzGrid {
    type Key = (i32, i32);
    fn project(&self, v: Vec3) -> Vec3 {
        Vec3::new(v.x, 0.0, v.z)
    }
    fn center(&self, k: (i32, i32)) -> Vec3 {
        Vec3::new((k.0 as f32 + 0.5) * self.cell_size, 0.0, (k.1 as f32 + 0.5) * self.cell_size)
    }
    fn count_in_aabb(&self, min: Vec3, max: Vec3) -> u64 {
        let s = self.cell_size;
        let nx = (cell_index(max.x, s) as i64 - cell_index(min.x, s) as i64 + 1).max(0) as u64;
        let nz = (cell_index(max.z, s) as i64 - cell_index(min.z, s) as i64 + 1).max(0) as u64;
        nx.saturating_mul(nz)
    }
    fn keys_in_aabb(&self, min: Vec3, max: Vec3) -> Vec<(i32, i32)> {
        let s = self.cell_size;
        let mut out = Vec::new();
        for x in cell_index(min.x, s)..=cell_index(max.x, s) {
            for z in cell_index(min.z, s)..=cell_index(max.z, s) {
                out.push((x, z));
            }
        }
        out
    }
    fn cell_size(&self) -> f32 {
        self.cell_size
    }
}

/// A cubic grid: the world database's Morton cell layout, signed.
#[derive(Debug, Clone, Copy)]
pub struct XyzGrid {
    pub cell_size: f32,
}

impl CellSpace for XyzGrid {
    type Key = (i32, i32, i32);
    fn project(&self, v: Vec3) -> Vec3 {
        v
    }
    fn center(&self, k: (i32, i32, i32)) -> Vec3 {
        (Vec3::new(k.0 as f32, k.1 as f32, k.2 as f32) + Vec3::splat(0.5)) * self.cell_size
    }
    fn count_in_aabb(&self, min: Vec3, max: Vec3) -> u64 {
        let s = self.cell_size;
        let n = |lo: f32, hi: f32| (cell_index(hi, s) as i64 - cell_index(lo, s) as i64 + 1).max(0) as u64;
        n(min.x, max.x).saturating_mul(n(min.y, max.y)).saturating_mul(n(min.z, max.z))
    }
    fn keys_in_aabb(&self, min: Vec3, max: Vec3) -> Vec<(i32, i32, i32)> {
        let s = self.cell_size;
        let mut out = Vec::new();
        for x in cell_index(min.x, s)..=cell_index(max.x, s) {
            for y in cell_index(min.y, s)..=cell_index(max.y, s) {
                for z in cell_index(min.z, s)..=cell_index(max.z, s) {
                    out.push((x, y, z));
                }
            }
        }
        out
    }
    fn cell_size(&self) -> f32 {
        self.cell_size
    }
}

/// The predicted cells of one update.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan<K> {
    /// Every cell needed now (ETA 0) or within the lead time, each with its
    /// earliest ETA over all foci, by ETA then key.
    pub cells: Vec<(K, f32)>,
    /// A focus's box was larger than `max_candidates`; it contributed only
    /// the cells it needs now.
    pub truncated: bool,
}

impl<K> Default for Plan<K> {
    fn default() -> Self {
        Self { cells: Vec::new(), truncated: false }
    }
}

/// Predict the cells `foci` need within the lead time.
pub fn plan_cells<S: CellSpace>(space: &S, foci: &[Focus], cfg: &PrefetchConfig) -> Plan<S::Key> {
    let r = cfg.need_radius.max(0.0);
    let mut best: HashMap<S::Key, f32> = HashMap::new();
    let mut truncated = false;
    for f in foci {
        let p = space.project(f.pos);
        let v = space.project(f.vel);
        if !p.is_finite() || !v.is_finite() {
            continue;
        }
        let mut horizon = effective_horizon(v.length(), cfg.lead_time_secs, cfg.max_lead_distance);
        let end = p + v * horizon;
        let (mut lo, mut hi) = (p.min(end) - Vec3::splat(r), p.max(end) + Vec3::splat(r));
        if space.count_in_aabb(lo, hi) > cfg.max_candidates as u64 {
            truncated = true;
            horizon = 0.0;
            lo = p - Vec3::splat(r);
            hi = p + Vec3::splat(r);
            if space.count_in_aabb(lo, hi) > cfg.max_candidates as u64 {
                continue;
            }
        }
        for k in space.keys_in_aabb(lo, hi) {
            if let Some(t) = eta_within(p, v, space.center(k), r, horizon) {
                best.entry(k).and_modify(|e| *e = e.min(t)).or_insert(t);
            }
        }
    }
    let mut cells: Vec<(S::Key, f32)> = best.into_iter().collect();
    cells.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    Plan { cells, truncated }
}

// ─────────────────────────────────────────────────────────────────────────────
// Planner: hysteresis, budget, counters
// ─────────────────────────────────────────────────────────────────────────────

/// What prefetch did, since the planner started.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PrefetchCounters {
    /// Cells already resident when they became needed.
    pub hits: u64,
    /// Hits a request made ahead of need produced.
    pub prefetch_hits: u64,
    /// Cells not resident when they became needed.
    pub misses: u64,
    /// Load requests sent.
    pub requested: u64,
    /// Requests sent while the cell was only predicted.
    pub requested_ahead: u64,
    /// Cells requested ahead that expired without ever being needed.
    pub wasted: u64,
    /// Times a predicted cell waited because the budget was spent.
    pub deferred_by_budget: u64,
    /// Sum of seconds between an ahead request and the cell's need.
    pub lead_achieved_secs_sum: f64,
    /// Sum of seconds a missed cell waited to become resident.
    pub miss_latency_secs_sum: f64,
    /// Missed cells that have since become resident.
    pub miss_latency_n: u64,
    /// Longest wait of a missed cell, seconds.
    pub miss_latency_secs_max: f64,
}

impl PrefetchCounters {
    /// Hits over hits plus misses; `None` before any cell was needed.
    pub fn hit_rate(&self) -> Option<f64> {
        let n = self.hits + self.misses;
        (n > 0).then(|| self.hits as f64 / n as f64)
    }
}

#[derive(Debug, Clone)]
struct Pin {
    last_wanted: f64,
    eta: f32,
    requested_at: Option<f64>,
    requested_ahead: bool,
    needed_since: Option<f64>,
    missed: bool,
    miss_resolved: bool,
}

/// The load requests of one update.
#[derive(Debug, Clone, PartialEq)]
pub struct Requests<K> {
    /// Cells to load now, needed ones first, then by ETA.
    pub load: Vec<K>,
    /// Predicted cells that waited for budget this update.
    pub deferred: usize,
}

impl<K> Default for Requests<K> {
    fn default() -> Self {
        Self { load: Vec::new(), deferred: 0 }
    }
}

/// Stateful side of prefetch for one source.
#[derive(Debug, Clone)]
pub struct PrefetchPlanner<K: Copy + Eq + Hash + Ord + Debug> {
    pins: HashMap<K, Pin>,
    tokens: f32,
    last_now: Option<f64>,
    pub counters: PrefetchCounters,
}

impl<K: Copy + Eq + Hash + Ord + Debug> Default for PrefetchPlanner<K> {
    fn default() -> Self {
        Self { pins: HashMap::new(), tokens: 0.0, last_now: None, counters: PrefetchCounters::default() }
    }
}

impl<K: Copy + Eq + Hash + Ord + Debug> PrefetchPlanner<K> {
    /// Fold one plan in at time `now` (seconds, any monotonic origin) and
    /// decide what to load. `is_resident` answers whether a cell is loaded;
    /// `cost` prices a load in the budget's units.
    pub fn update(
        &mut self,
        now: f64,
        plan: &Plan<K>,
        cfg: &PrefetchConfig,
        is_resident: impl Fn(K) -> bool,
        cost: impl Fn(K) -> f32,
    ) -> Requests<K> {
        let dt = self.last_now.map_or(0.0, |l| (now - l).clamp(0.0, 1.0)) as f32;
        if self.last_now.is_none() {
            self.tokens = cfg.budget_burst;
        }
        self.last_now = Some(now);
        self.tokens = (self.tokens + cfg.budget_units_per_sec * dt).min(cfg.budget_burst);

        // 1. Pin what the plan wants; count each cell's first moment of need.
        for &(k, eta) in &plan.cells {
            let pin = self.pins.entry(k).or_insert(Pin {
                last_wanted: now,
                eta,
                requested_at: None,
                requested_ahead: false,
                needed_since: None,
                missed: false,
                miss_resolved: false,
            });
            pin.last_wanted = now;
            pin.eta = eta;
            if eta <= 0.0 && pin.needed_since.is_none() {
                pin.needed_since = Some(now);
                if is_resident(k) {
                    self.counters.hits += 1;
                    if pin.requested_ahead {
                        self.counters.prefetch_hits += 1;
                        if let Some(at) = pin.requested_at {
                            self.counters.lead_achieved_secs_sum += now - at;
                        }
                    }
                } else {
                    self.counters.misses += 1;
                    pin.missed = true;
                }
            }
        }

        // 2. Missed cells that have become resident: how late they were.
        for (k, pin) in self.pins.iter_mut() {
            if pin.missed && !pin.miss_resolved && is_resident(*k) {
                pin.miss_resolved = true;
                let wait = now - pin.needed_since.unwrap_or(now);
                self.counters.miss_latency_n += 1;
                self.counters.miss_latency_secs_sum += wait;
                self.counters.miss_latency_secs_max = self.counters.miss_latency_secs_max.max(wait);
            }
        }

        // 3. Expire pins the plan has not wanted for `linger_secs`.
        let linger = cfg.linger_secs.max(0.0) as f64;
        let counters = &mut self.counters;
        self.pins.retain(|_, pin| {
            let keep = now - pin.last_wanted <= linger;
            if !keep && pin.requested_ahead && pin.needed_since.is_none() {
                counters.wasted += 1;
            }
            keep
        });

        // 4. Admit loads: needed cells always (into debt), predicted ones on budget.
        let retry = cfg.retry_secs.max(0.0) as f64;
        let mut out = Requests::default();
        let max = cfg.max_loads_per_tick.max(1);
        let mut order: Vec<(K, f32)> = plan.cells.iter().copied().filter(|(k, _)| !is_resident(*k)).collect();
        // Needed first (ETA 0 sorts first already); stable on ties.
        order.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        for (k, eta) in order {
            if out.load.len() >= max {
                break;
            }
            let Some(pin) = self.pins.get_mut(&k) else { continue };
            if pin.requested_at.is_some_and(|at| now - at < retry) {
                continue;
            }
            let c = cost(k).max(0.0);
            let needed = eta <= 0.0;
            if !needed && self.tokens < c && self.tokens < cfg.budget_burst {
                out.deferred += 1;
                self.counters.deferred_by_budget += 1;
                continue;
            }
            self.tokens -= c;
            pin.requested_at = Some(now);
            self.counters.requested += 1;
            if !needed {
                pin.requested_ahead = true;
                self.counters.requested_ahead += 1;
            }
            out.load.push(k);
        }
        out
    }

    /// Whether `k` is predicted or lingering: an evictor must keep it.
    pub fn is_pinned(&self, k: K) -> bool {
        self.pins.contains_key(&k)
    }

    /// Cells pinned right now.
    pub fn pinned_count(&self) -> usize {
        self.pins.len()
    }

    /// Budget tokens left.
    pub fn tokens(&self) -> f32 {
        self.tokens
    }

    /// Pinned cells with their last ETA, by ETA then key.
    pub fn pinned(&self) -> Vec<(K, f32)> {
        let mut v: Vec<(K, f32)> = self.pins.iter().map(|(k, p)| (*k, p.eta)).collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        v
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Report (the inspector's view)
// ─────────────────────────────────────────────────────────────────────────────

/// One predicted cell as the inspector shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PredictedCell {
    pub cell: String,
    /// World centre (projected; a 2D grid reports `y = 0`).
    pub center: [f32; 3],
    pub size: f32,
    pub eta_secs: f32,
    pub resident: bool,
}

/// One focus as the inspector shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FocusReport {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    /// Seconds of prediction this focus received after the distance cap.
    pub horizon_secs: f32,
}

/// What one prefetch source is doing: its foci, its predicted set with
/// lead times, its budget and its hit and miss counts.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct PrefetchReport {
    pub source: String,
    pub config: Option<PrefetchConfig>,
    pub foci: Vec<FocusReport>,
    pub predicted_total: usize,
    pub pinned_total: usize,
    /// Up to 64 predicted cells, soonest first.
    pub predicted: Vec<PredictedCell>,
    pub loads_last_update: usize,
    pub deferred_last_update: usize,
    pub budget_tokens: f32,
    pub truncated: bool,
    pub counters: PrefetchCounters,
    pub hit_rate: Option<f64>,
}

/// Every prefetch source's latest report, by source name. The inspector
/// (`sim.inspect` on the engine bridge) reads this.
#[derive(Resource, Debug, Clone, Default)]
pub struct PrefetchReports {
    pub by_source: std::collections::BTreeMap<String, PrefetchReport>,
}

/// Build a source's report after an update.
pub fn report_of<S: CellSpace>(
    source: &str,
    space: &S,
    cfg: &PrefetchConfig,
    foci: &[Focus],
    plan: &Plan<S::Key>,
    planner: &PrefetchPlanner<S::Key>,
    requests: &Requests<S::Key>,
    is_resident: impl Fn(S::Key) -> bool,
) -> PrefetchReport {
    PrefetchReport {
        source: source.to_string(),
        config: Some(cfg.clone()),
        foci: foci
            .iter()
            .map(|f| {
                let v = space.project(f.vel);
                FocusReport {
                    pos: f.pos.to_array(),
                    vel: f.vel.to_array(),
                    horizon_secs: effective_horizon(v.length(), cfg.lead_time_secs, cfg.max_lead_distance),
                }
            })
            .collect(),
        predicted_total: plan.cells.len(),
        pinned_total: planner.pinned_count(),
        predicted: plan
            .cells
            .iter()
            .take(64)
            .map(|&(k, eta)| PredictedCell {
                cell: format!("{k:?}"),
                center: space.center(k).to_array(),
                size: space.cell_size(),
                eta_secs: eta,
                resident: is_resident(k),
            })
            .collect(),
        loads_last_update: requests.load.len(),
        deferred_last_update: requests.deferred,
        budget_tokens: planner.tokens(),
        truncated: plan.truncated,
        hit_rate: planner.counters.hit_rate(),
        counters: planner.counters.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn cfg(need: f32) -> PrefetchConfig {
        PrefetchConfig { need_radius: need, ..PrefetchConfig::default() }
    }

    #[test]
    fn rage_eta_solves_the_approach_exactly() {
        // 100 m away along +X, closing at 10 m/s, radius 20: reaches 20 m at 8 s.
        let t = eta_within(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 0.0), 20.0, 10.0);
        assert!((t.unwrap() - 8.0).abs() < 1e-4, "{t:?}");
        // Already inside: needed now.
        assert_eq!(eta_within(Vec3::ZERO, Vec3::ZERO, Vec3::new(5.0, 0.0, 0.0), 20.0, 0.0), Some(0.0));
        // Beyond the horizon.
        assert_eq!(eta_within(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 0.0), 20.0, 7.9), None);
        // Moving away never arrives.
        assert_eq!(eta_within(Vec3::ZERO, Vec3::new(-10.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 0.0), 20.0, 100.0), None);
        // A pass that misses wide never arrives.
        assert_eq!(eta_within(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(50.0, 0.0, 40.0), 20.0, 100.0), None);
    }

    #[test]
    fn rage_horizon_is_capped_by_distance_and_off_when_still() {
        assert_eq!(effective_horizon(0.1, 2.0, 600.0), 0.0);
        assert_eq!(effective_horizon(50.0, 2.0, 600.0), 2.0);
        // 600 m/s would sweep 1200 m in 2 s; capped to 600 m = 1 s.
        assert!((effective_horizon(600.0, 2.0, 600.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn rage_moving_focus_predicts_cells_ahead_not_behind() {
        let space = XzGrid { cell_size: 100.0 };
        let c = PrefetchConfig { lead_time_secs: 3.0, max_lead_distance: 1_000.0, ..cfg(150.0) };
        let still = plan_cells(&space, &[Focus { pos: Vec3::new(50.0, 0.0, 50.0), vel: Vec3::ZERO }], &c);
        let moving = plan_cells(&space, &[Focus { pos: Vec3::new(50.0, 0.0, 50.0), vel: Vec3::new(100.0, 0.0, 0.0) }], &c);
        let still_keys: HashSet<_> = still.cells.iter().map(|(k, _)| *k).collect();
        let ahead = (4, 0);
        let behind = (-3, 0);
        assert!(!still_keys.contains(&ahead), "a still focus does not predict 400 m away");
        let eta_ahead = moving.cells.iter().find(|(k, _)| *k == ahead).map(|(_, t)| *t);
        // Cell (4,0) centre is x=450; reach within 150 m at x=300, i.e. 2.5 s.
        assert!((eta_ahead.expect("ahead cell predicted") - 2.5).abs() < 1e-3, "{eta_ahead:?}");
        assert!(moving.cells.iter().all(|(k, _)| *k != behind), "nothing behind beyond the need ball");
        // Every cell needed now is still in the moving plan, at ETA 0.
        for (k, t) in &still.cells {
            assert_eq!(*t, 0.0);
            assert!(moving.cells.iter().any(|(m, e)| m == k && *e == 0.0), "near ball kept: {k:?}");
        }
    }

    #[test]
    fn rage_plan_is_sorted_by_eta_then_key_and_unions_foci() {
        let space = XzGrid { cell_size: 100.0 };
        let c = PrefetchConfig { lead_time_secs: 3.0, max_lead_distance: 1_000.0, ..cfg(120.0) };
        let a = Focus { pos: Vec3::new(50.0, 0.0, 50.0), vel: Vec3::new(100.0, 0.0, 0.0) };
        let b = Focus { pos: Vec3::new(5_050.0, 0.0, 50.0), vel: Vec3::ZERO };
        let p = plan_cells(&space, &[a, b], &c);
        assert!(p.cells.windows(2).all(|w| w[0].1 < w[1].1 || (w[0].1 == w[1].1 && w[0].0 < w[1].0)));
        assert!(p.cells.iter().any(|(k, t)| *k == (50, 0) && *t == 0.0), "second focus's own cell");
        let again = plan_cells(&space, &[b, a], &c);
        assert_eq!(p, again, "focus order does not change the plan");
    }

    #[test]
    fn rage_huge_box_falls_back_to_the_need_ball() {
        let space = XzGrid { cell_size: 1.0 };
        let c = PrefetchConfig { max_candidates: 10_000, lead_time_secs: 5.0, max_lead_distance: 10_000.0, ..cfg(10.0) };
        let p = plan_cells(&space, &[Focus { pos: Vec3::ZERO, vel: Vec3::new(1_000.0, 0.0, 0.0) }], &c);
        assert!(p.truncated);
        assert!(p.cells.iter().all(|(_, t)| *t == 0.0));
        assert!(!p.cells.is_empty());
    }

    fn plan_of(cells: &[((i32, i32), f32)]) -> Plan<(i32, i32)> {
        Plan { cells: cells.to_vec(), truncated: false }
    }

    #[test]
    fn rage_prefetched_cell_counts_as_a_prefetch_hit() {
        let c = cfg(100.0);
        let mut p = PrefetchPlanner::default();
        let mut resident: HashSet<(i32, i32)> = HashSet::new();
        // t=0: predicted 1.5 s out, not resident -> requested ahead.
        let r = p.update(0.0, &plan_of(&[((1, 0), 1.5)]), &c, |k| resident.contains(&k), |_| 1.0);
        assert_eq!(r.load, vec![(1, 0)]);
        assert_eq!(p.counters.requested_ahead, 1);
        resident.insert((1, 0));
        // t=1.5: needed and resident -> hit, prefetch hit, 1.5 s lead.
        p.update(1.5, &plan_of(&[((1, 0), 0.0)]), &c, |k| resident.contains(&k), |_| 1.0);
        assert_eq!((p.counters.hits, p.counters.prefetch_hits, p.counters.misses), (1, 1, 0));
        assert!((p.counters.lead_achieved_secs_sum - 1.5).abs() < 1e-9);
        assert_eq!(p.counters.hit_rate(), Some(1.0));
    }

    #[test]
    fn rage_unpredicted_need_is_a_miss_with_its_latency() {
        let c = cfg(100.0);
        let mut p = PrefetchPlanner::default();
        let mut resident: HashSet<(i32, i32)> = HashSet::new();
        let r = p.update(0.0, &plan_of(&[((2, 2), 0.0)]), &c, |k| resident.contains(&k), |_| 1.0);
        assert_eq!(r.load, vec![(2, 2)], "a needed cell is requested at once");
        assert_eq!(p.counters.misses, 1);
        resident.insert((2, 2));
        p.update(0.25, &plan_of(&[((2, 2), 0.0)]), &c, |k| resident.contains(&k), |_| 1.0);
        assert_eq!(p.counters.miss_latency_n, 1);
        assert!((p.counters.miss_latency_secs_max - 0.25).abs() < 1e-9);
        assert_eq!(p.counters.misses, 1, "a need is counted once");
    }

    #[test]
    fn rage_pin_lingers_then_expires_and_counts_waste() {
        let c = PrefetchConfig { linger_secs: 1.0, ..cfg(100.0) };
        let mut p = PrefetchPlanner::default();
        p.update(0.0, &plan_of(&[((3, 0), 1.0)]), &c, |_| false, |_| 1.0);
        assert!(p.is_pinned((3, 0)));
        // Dropped from the plan (a jitter of heading): still pinned, never evicted.
        p.update(0.5, &plan_of(&[]), &c, |_| false, |_| 1.0);
        assert!(p.is_pinned((3, 0)), "hysteresis keeps the cell through a short dropout");
        p.update(1.6, &plan_of(&[]), &c, |_| false, |_| 1.0);
        assert!(!p.is_pinned((3, 0)));
        assert_eq!(p.counters.wasted, 1, "requested ahead, never needed");
    }

    #[test]
    fn rage_budget_defers_prediction_but_never_demand() {
        let c = PrefetchConfig { budget_units_per_sec: 0.0, budget_burst: 10.0, max_loads_per_tick: 8, ..cfg(100.0) };
        let mut p = PrefetchPlanner::default();
        // Burst 10, each cell costs 6: one predicted cell fits, then debt.
        let plan = plan_of(&[((0, 0), 0.0), ((1, 0), 0.5), ((2, 0), 0.7), ((3, 0), 0.0)]);
        let r = p.update(0.0, &plan, &c, |_| false, |_| 6.0);
        // Sorted: (0,0) 0, (3,0) 0, (1,0) .5, (2,0) .7. Needed ones spend 12 -> -2.
        assert_eq!(r.load, vec![(0, 0), (3, 0)], "both needed cells go, into debt");
        assert_eq!(r.deferred, 2, "predicted cells wait for tokens");
        assert!(p.tokens() < 0.0);
    }

    #[test]
    fn rage_requests_cap_per_tick_and_retry_after_a_while() {
        let c = PrefetchConfig { max_loads_per_tick: 1, retry_secs: 1.0, ..cfg(100.0) };
        let mut p = PrefetchPlanner::default();
        let plan = plan_of(&[((0, 0), 0.0), ((1, 0), 0.0)]);
        assert_eq!(p.update(0.0, &plan, &c, |_| false, |_| 1.0).load, vec![(0, 0)]);
        assert_eq!(p.update(0.1, &plan, &c, |_| false, |_| 1.0).load, vec![(1, 0)], "the pending one waits");
        assert!(p.update(0.2, &plan, &c, |_| false, |_| 1.0).load.is_empty(), "both in flight");
        assert_eq!(p.update(1.05, &plan, &c, |_| false, |_| 1.0).load, vec![(0, 0)], "retried");
    }

    #[test]
    fn rage_3d_grid_predicts_a_climb() {
        let space = XyzGrid { cell_size: 256.0 };
        let c = PrefetchConfig { lead_time_secs: 4.0, max_lead_distance: 2_000.0, ..cfg(200.0) };
        let p = plan_cells(&space, &[Focus { pos: Vec3::splat(128.0), vel: Vec3::new(0.0, 150.0, 0.0) }], &c);
        assert!(p.cells.iter().any(|(k, t)| *k == (0, 2, 0) && *t > 0.0), "cell above is predicted");
    }
}
