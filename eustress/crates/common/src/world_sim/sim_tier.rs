//! # Simulation LOD: Full, Reduced, Dormant
//!
//! RAGE simulates the block around the player in full and lets the rest of
//! the city run cheaper: far pedestrians and cars drop to simpler models or
//! freeze. Eustress does the same per rigid body, on the session's
//! authority only:
//!
//! - **Full:** Avian simulates the body as it always has; scripts and NPC
//!   AI run every frame.
//! - **Reduced:** Avian still simulates it, but the body is allowed to fall
//!   asleep sooner (a higher `SleepThreshold`), so far bodies that are
//!   nearly at rest stop costing solver time. Nothing about its motion is
//!   approximated.
//! - **Dormant:** the body's island is put to sleep (`Sleeping`): no
//!   physics, NPC AI skips it, and its state (pose, velocity, joints) is
//!   kept exactly, so it resumes where it stopped. Only near-still bodies
//!   go Dormant (`dormant_max_speed`); freezing far movers is a decision
//!   the design doc leaves to McKale.
//!
//! The policy here is pure. The play runtime's `sim_lod` module applies it
//! to Avian. Like the render cascade's tiers, every band has an *in* and an
//! *out* threshold, so a body parked on a boundary never flickers; a body's
//! on-screen size can keep it at a higher tier than its distance alone;
//! the Full tier is capped (closest wins); and a physics island takes its
//! best member's tier, so a jointed or touching group is never split
//! between asleep and awake.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The hidden property a body's instance carries its tier under, written
/// quietly by the authority when the tier changes: `"Full"`, `"Reduced"`
/// or `"Dormant"`. Like the drawn-pose properties, it never replicates and
/// no script hears it change; the script VMs read it.
pub const SIM_TIER_PROP: &str = "__SimTier";

/// A body's simulation tier. Ordered by fidelity: `Full < Reduced <
/// Dormant`, so the *smaller* of two tiers is the more faithful.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub enum SimTier {
    Full,
    Reduced,
    Dormant,
}

impl SimTier {
    pub fn as_str(self) -> &'static str {
        match self {
            SimTier::Full => "Full",
            SimTier::Reduced => "Reduced",
            SimTier::Dormant => "Dormant",
        }
    }
}

/// Tunables for simulation LOD. Distances are metres from the nearest
/// focus.
#[derive(Resource, Debug, Clone, PartialEq, Serialize)]
pub struct SimTierConfig {
    /// Master switch. Env `EUSTRESS_SIM_LOD=0` turns it off.
    pub enabled: bool,
    /// Full drops to Reduced beyond this.
    pub reduced_out_m: f32,
    /// Reduced (or Dormant) returns to Full inside this.
    pub full_in_m: f32,
    /// Reduced drops to Dormant beyond this.
    pub dormant_out_m: f32,
    /// Dormant returns to Reduced inside this.
    pub reduced_in_m: f32,
    /// A body at least this fraction of the view's half-height stays Full.
    pub screen_full_fraction: f32,
    /// A body at least this fraction of the view's half-height stays at
    /// least Reduced.
    pub screen_reduced_fraction: f32,
    /// A body moving faster than this (m/s) is never made Dormant.
    pub dormant_max_speed: f32,
    /// Most Full bodies; the farthest overflow drops to Reduced.
    pub full_cap: usize,
    /// Re-evaluate every N frames.
    pub cadence_frames: u32,
    /// Reduced and Dormant bodies sleep at this multiple of their own
    /// `SleepThreshold`.
    pub reduced_sleep_scale: f32,
}

impl Default for SimTierConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            reduced_out_m: 200.0,
            full_in_m: 160.0,
            dormant_out_m: 700.0,
            reduced_in_m: 600.0,
            screen_full_fraction: 0.05,
            screen_reduced_fraction: 0.01,
            dormant_max_speed: 3.0,
            full_cap: 4_000,
            cadence_frames: 8,
            reduced_sleep_scale: 4.0,
        }
    }
}

impl SimTierConfig {
    /// Defaults with environment overrides: `EUSTRESS_SIM_LOD` (`0` off),
    /// `EUSTRESS_SIM_LOD_REDUCED_M` (the Full to Reduced distance; the
    /// return threshold follows at 0.8x) and `EUSTRESS_SIM_LOD_DORMANT_M`
    /// (the Reduced to Dormant distance; return at 0.85x).
    pub fn from_env() -> Self {
        let mut c = Self::default();
        if std::env::var("EUSTRESS_SIM_LOD").map(|v| v.trim() == "0" || v.eq_ignore_ascii_case("off")).unwrap_or(false) {
            c.enabled = false;
        }
        let envf = |k: &str| std::env::var(k).ok().and_then(|s| s.trim().parse::<f32>().ok()).filter(|v| v.is_finite() && *v > 0.0);
        if let Some(r) = envf("EUSTRESS_SIM_LOD_REDUCED_M") {
            c.reduced_out_m = r;
            c.full_in_m = r * 0.8;
        }
        if let Some(d) = envf("EUSTRESS_SIM_LOD_DORMANT_M") {
            c.dormant_out_m = d.max(c.reduced_out_m);
            c.reduced_in_m = (d * 0.85).max(c.full_in_m);
        }
        c
    }
}

/// Fraction of the view's half-height a sphere of `radius` at `distance`
/// covers, for a vertical field of view `fov_y` (radians). Inside the
/// sphere it covers the whole view.
pub fn screen_fraction(radius: f32, distance: f32, fov_y: f32) -> f32 {
    if distance <= radius {
        return f32::INFINITY;
    }
    let half = (fov_y * 0.5).tan().max(1e-4);
    radius / (distance * half)
}

/// The tier one body earns from its distance to the nearest focus, its
/// largest on-screen fraction (`None` where no camera sees the world, as on
/// a headless host), its speed, and its current tier (for hysteresis).
pub fn classify_tier(distance: f32, screen: Option<f32>, speed: f32, current: Option<SimTier>, cfg: &SimTierConfig) -> SimTier {
    let d = if distance.is_finite() { distance } else { f32::MAX };
    let by_distance = match current {
        None => {
            if d <= cfg.reduced_out_m {
                SimTier::Full
            } else if d <= cfg.dormant_out_m {
                SimTier::Reduced
            } else {
                SimTier::Dormant
            }
        }
        Some(SimTier::Full) => {
            if d <= cfg.reduced_out_m {
                SimTier::Full
            } else if d <= cfg.dormant_out_m {
                SimTier::Reduced
            } else {
                SimTier::Dormant
            }
        }
        Some(SimTier::Reduced) => {
            if d < cfg.full_in_m {
                SimTier::Full
            } else if d > cfg.dormant_out_m {
                SimTier::Dormant
            } else {
                SimTier::Reduced
            }
        }
        Some(SimTier::Dormant) => {
            if d < cfg.full_in_m {
                SimTier::Full
            } else if d < cfg.reduced_in_m {
                SimTier::Reduced
            } else {
                SimTier::Dormant
            }
        }
    };
    // On-screen size floor, with a 0.8x band to leave a tier it holds.
    let floor = screen.map_or(SimTier::Dormant, |s| {
        let keep = |t: SimTier| if current.is_some_and(|c| c <= t) { 0.8 } else { 1.0 };
        if s >= cfg.screen_full_fraction * keep(SimTier::Full) {
            SimTier::Full
        } else if s >= cfg.screen_reduced_fraction * keep(SimTier::Reduced) {
            SimTier::Reduced
        } else {
            SimTier::Dormant
        }
    });
    let mut tier = by_distance.min(floor);
    // Conservative subset: a moving body keeps simulating.
    if tier == SimTier::Dormant && !(speed <= cfg.dormant_max_speed) {
        tier = SimTier::Reduced;
    }
    tier
}

/// One body as the planner sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodySample {
    /// A stable key (the entity's bits); ties break on it.
    pub key: u64,
    /// The body's physics island, when it has one.
    pub island: Option<u64>,
    pub distance: f32,
    pub screen: Option<f32>,
    pub speed: f32,
    pub current: Option<SimTier>,
    /// The body may never sleep (`SleepingDisabled`): at most Reduced.
    pub never_sleeps: bool,
}

/// What a planning pass changed beyond the per-body classification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct PlanStats {
    /// Bodies held out of Full by the cap.
    pub cap_demotions: usize,
    /// Bodies raised because a member of their island ranks higher.
    pub island_raises: usize,
    /// Islands held out of Dormant because a member never sleeps.
    pub never_sleep_holds: usize,
}

/// Plan every body's tier: classify, cap Full (closest first, ties on key),
/// then give each island its best member's tier. Returns tiers in input
/// order.
pub fn plan_tiers(bodies: &[BodySample], cfg: &SimTierConfig) -> (Vec<SimTier>, PlanStats) {
    let mut stats = PlanStats::default();
    let mut tiers: Vec<SimTier> =
        bodies.iter().map(|b| classify_tier(b.distance, b.screen, b.speed, b.current, cfg)).collect();

    // Full cap: the closest `full_cap` keep Full.
    let mut full: Vec<usize> = (0..bodies.len()).filter(|&i| tiers[i] == SimTier::Full).collect();
    if full.len() > cfg.full_cap {
        full.sort_by(|&a, &b| bodies[a].distance.total_cmp(&bodies[b].distance).then(bodies[a].key.cmp(&bodies[b].key)));
        for &i in &full[cfg.full_cap..] {
            tiers[i] = SimTier::Reduced;
            stats.cap_demotions += 1;
        }
    }

    // Islands: best member's tier; a member that never sleeps keeps the
    // whole island awake.
    let mut best: HashMap<u64, SimTier> = HashMap::new();
    let mut awake: HashMap<u64, bool> = HashMap::new();
    for (i, b) in bodies.iter().enumerate() {
        if let Some(island) = b.island {
            best.entry(island).and_modify(|t| *t = (*t).min(tiers[i])).or_insert(tiers[i]);
            *awake.entry(island).or_insert(false) |= b.never_sleeps;
        }
    }
    for (i, b) in bodies.iter().enumerate() {
        let mut t = tiers[i];
        if let Some(island) = b.island {
            let island_best = best[&island];
            if island_best < t {
                t = island_best;
                stats.island_raises += 1;
            }
            if t == SimTier::Dormant && awake[&island] {
                t = SimTier::Reduced;
                stats.never_sleep_holds += 1;
            }
        } else if t == SimTier::Dormant && b.never_sleeps {
            t = SimTier::Reduced;
            stats.never_sleep_holds += 1;
        }
        tiers[i] = t;
    }
    (tiers, stats)
}

/// One body's tier as the inspector shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TierSample {
    pub entity: u64,
    pub tier: SimTier,
    pub distance: f32,
}

/// What simulation LOD is doing: counts per tier, transitions, the cost of
/// the last pass, and how often physics woke a body LOD had put to sleep.
#[derive(Resource, Debug, Clone, Default, Serialize, PartialEq)]
pub struct SimTierReport {
    pub enabled: bool,
    /// "authority", "replica" (tiers never applied) or "idle" (no session).
    pub role: String,
    pub foci: usize,
    pub bodies: usize,
    pub full: usize,
    pub reduced: usize,
    pub dormant: usize,
    pub transitions_last_pass: usize,
    pub transitions_total: u64,
    pub stats_last_pass: PlanStats,
    /// Islands LOD put to sleep.
    pub forced_sleeps: u64,
    /// Bodies LOD put to sleep that physics woke (a contact, a script's push).
    pub woken_by_physics: u64,
    pub last_pass_ms: f32,
    /// Up to 64 most recent transitions, newest last.
    pub recent: Vec<TierSample>,
}

impl SimTierReport {
    /// Remember a transition, keeping the newest 64.
    pub fn push_recent(&mut self, s: TierSample) {
        if self.recent.len() >= 64 {
            self.recent.remove(0);
        }
        self.recent.push(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c() -> SimTierConfig {
        SimTierConfig::default()
    }

    fn body(key: u64, distance: f32) -> BodySample {
        BodySample { key, island: None, distance, screen: None, speed: 0.0, current: None, never_sleeps: false }
    }

    #[test]
    fn rage_tier_bands_for_a_fresh_body() {
        let c = c();
        assert_eq!(classify_tier(10.0, None, 0.0, None, &c), SimTier::Full);
        assert_eq!(classify_tier(200.0, None, 0.0, None, &c), SimTier::Full);
        assert_eq!(classify_tier(200.1, None, 0.0, None, &c), SimTier::Reduced);
        assert_eq!(classify_tier(700.0, None, 0.0, None, &c), SimTier::Reduced);
        assert_eq!(classify_tier(700.1, None, 0.0, None, &c), SimTier::Dormant);
    }

    #[test]
    fn rage_tier_hysteresis_holds_at_every_edge() {
        let c = c();
        // Reduced does not return to Full until inside 160.
        assert_eq!(classify_tier(170.0, None, 0.0, Some(SimTier::Reduced), &c), SimTier::Reduced);
        assert_eq!(classify_tier(159.0, None, 0.0, Some(SimTier::Reduced), &c), SimTier::Full);
        // Dormant does not wake until inside 600.
        assert_eq!(classify_tier(650.0, None, 0.0, Some(SimTier::Dormant), &c), SimTier::Dormant);
        assert_eq!(classify_tier(599.0, None, 0.0, Some(SimTier::Dormant), &c), SimTier::Reduced);
        // Fixed points inside each dead zone.
        for (d, t) in [(180.0, SimTier::Full), (180.0, SimTier::Reduced), (650.0, SimTier::Reduced), (650.0, SimTier::Dormant)] {
            let mut tier = t;
            for _ in 0..5 {
                tier = classify_tier(d, None, 0.0, Some(tier), &c);
            }
            assert_eq!(tier, t, "{t:?} at {d} m must hold");
        }
    }

    #[test]
    fn rage_big_on_screen_body_stays_full_far_away() {
        let c = c();
        let fov = 70f32.to_radians();
        // A 60 m ship at 900 m covers ~9.5% of the half-height.
        let s = screen_fraction(60.0, 900.0, fov);
        assert!(s > c.screen_full_fraction, "{s}");
        assert_eq!(classify_tier(900.0, Some(s), 0.0, None, &c), SimTier::Full);
        // A crate at the same distance goes Dormant.
        let crate_s = screen_fraction(0.5, 900.0, fov);
        assert_eq!(classify_tier(900.0, Some(crate_s), 0.0, None, &c), SimTier::Dormant);
    }

    #[test]
    fn rage_moving_body_is_never_frozen() {
        let c = c();
        assert_eq!(classify_tier(5_000.0, None, 12.0, None, &c), SimTier::Reduced);
        assert_eq!(classify_tier(5_000.0, None, 12.0, Some(SimTier::Dormant), &c), SimTier::Reduced, "woken and moving");
        assert_eq!(classify_tier(5_000.0, None, f32::NAN, None, &c), SimTier::Reduced, "a NaN speed is not still");
        assert_eq!(classify_tier(5_000.0, None, 1.0, None, &c), SimTier::Dormant);
    }

    #[test]
    fn rage_full_cap_keeps_the_closest_with_stable_ties() {
        let cfg = SimTierConfig { full_cap: 2, ..c() };
        let bodies = [body(9, 50.0), body(3, 10.0), body(5, 50.0), body(1, 30.0)];
        let (tiers, stats) = plan_tiers(&bodies, &cfg);
        // Closest two: key 3 (10 m), key 1 (30 m). The 50 m pair overflow.
        assert_eq!(tiers, vec![SimTier::Reduced, SimTier::Full, SimTier::Reduced, SimTier::Full]);
        assert_eq!(stats.cap_demotions, 2);
    }

    #[test]
    fn rage_island_takes_its_best_members_tier() {
        let cfg = c();
        // A train: one car near, one far, jointed into island 7.
        let near = BodySample { island: Some(7), ..body(1, 50.0) };
        let far = BodySample { island: Some(7), ..body(2, 2_000.0) };
        let lone = body(3, 2_000.0);
        let (tiers, stats) = plan_tiers(&[near, far, lone], &cfg);
        assert_eq!(tiers, vec![SimTier::Full, SimTier::Full, SimTier::Dormant]);
        assert_eq!(stats.island_raises, 1);
    }

    #[test]
    fn rage_never_sleeping_member_keeps_its_island_awake() {
        let cfg = c();
        let a = BodySample { island: Some(4), ..body(1, 2_000.0) };
        let b = BodySample { island: Some(4), never_sleeps: true, ..body(2, 2_000.0) };
        let solo = BodySample { never_sleeps: true, ..body(3, 2_000.0) };
        let (tiers, stats) = plan_tiers(&[a, b, solo], &cfg);
        assert_eq!(tiers, vec![SimTier::Reduced; 3]);
        assert_eq!(stats.never_sleep_holds, 3);
    }

    #[test]
    fn rage_tier_order_means_fidelity() {
        assert!(SimTier::Full < SimTier::Reduced && SimTier::Reduced < SimTier::Dormant);
    }
}
