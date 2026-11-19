//! # Hysteresis Radius Gate
//!
//! ## Table of Contents
//! - HysteresisRadiusGate — two-threshold promote/demote logic
//! - GateDecision         — enum: Promote, Demote, Hold
//! - GateStats            — per-frame counters for telemetry
//!
//! ## Design
//! Uses two radii with a dead zone between them (hysteresis band) to prevent
//! entities from thrashing between Active ↔ Hot at the boundary:
//!
//!   active_radius (500m) ──── entity enters Active zone (promote)
//!   [hysteresis band: 500–600m] ── no action, hold current tier
//!   evict_radius  (600m) ──── entity leaves Active zone (demote)
//!   cold_radius   (2000m) ── entity removed from Hot cache entirely
//!
//! ## Benchmark-Proven Numbers
//! - Eviction pass: 4.7ms @ 2.10M total
//! - Active cap: ~2.10M entities @ 24 FPS

use std::collections::HashSet;

use super::types::{ChunkCoord, StreamingConfig, Tier};

// ─────────────────────────────────────────────────────────────────────────────
// GateDecision — what to do with an instance this frame
// ─────────────────────────────────────────────────────────────────────────────

/// Decision returned by the radius gate for each instance or chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    /// Promote from Hot → Active (spawn Bevy entity).
    Promote,
    /// Demote from Active → Hot (despawn Bevy entity, keep in DashMap).
    DemoteToHot,
    /// Demote from Hot → Cold (remove from DashMap, data stays on disk).
    DemoteToCold,
    /// No change — entity stays in its current tier.
    Hold,
}

// ─────────────────────────────────────────────────────────────────────────────
// GateStats — per-frame telemetry
// ─────────────────────────────────────────────────────────────────────────────

/// Counters from one radius gate evaluation pass.
#[derive(Debug, Clone, Default)]
pub struct GateStats {
    /// Chunks promoted Hot → Active this frame.
    pub promoted_chunks: usize,
    /// Instances promoted Hot → Active this frame.
    pub promoted_instances: usize,
    /// Chunks demoted Active → Hot this frame.
    pub demoted_to_hot_chunks: usize,
    /// Instances demoted Active → Hot this frame.
    pub demoted_to_hot_instances: usize,
    /// Chunks demoted Hot → Cold this frame.
    pub demoted_to_cold_chunks: usize,
    /// Instances demoted Hot → Cold this frame.
    pub demoted_to_cold_instances: usize,
    /// Total active instances after this pass.
    pub active_total: usize,
    /// Total hot cache instances after this pass.
    pub hot_total: usize,
    /// Chunks promoted ahead of need because prefetch predicted them.
    pub prefetch_promoted_chunks: usize,
    /// Demotions held back because prefetch pins the chunk.
    pub pinned_holds: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// HysteresisRadiusGate
// ─────────────────────────────────────────────────────────────────────────────

/// Two-threshold radius gate with hysteresis to prevent boundary thrashing.
///
/// Operates on chunks (not individual instances) for O(1) batch decisions.
/// The gate evaluates chunk-center distance to the camera and returns a
/// GateDecision per chunk.
pub struct HysteresisRadiusGate {
    /// Squared active radius (promote threshold).
    active_radius_sq: f32,
    /// Squared evict radius (demote-to-hot threshold).
    evict_radius_sq: f32,
    /// Squared cold radius (demote-to-cold threshold).
    cold_radius_sq: f32,
    /// Maximum active instances allowed (caps promotion).
    active_cap: usize,
}

impl HysteresisRadiusGate {
    /// Create a new gate from streaming config.
    pub fn new(config: &StreamingConfig) -> Self {
        Self {
            active_radius_sq: config.active_radius * config.active_radius,
            evict_radius_sq:  config.evict_radius * config.evict_radius,
            cold_radius_sq:   config.cold_radius * config.cold_radius,
            active_cap:       config.active_cap,
        }
    }

    /// Evaluate a single chunk against the camera position.
    ///
    /// # Arguments
    /// - `chunk_coord` — the chunk being evaluated
    /// - `chunk_size` — world-space size of one chunk
    /// - `camera_x`, `camera_z` — camera world-space position (XZ plane)
    /// - `current_tier` — what tier the chunk's instances are currently in
    /// - `current_active_count` — how many instances are already Active (for cap)
    /// - `chunk_instance_count` — how many instances are in this chunk
    pub fn evaluate(
        &self,
        chunk_coord: ChunkCoord,
        chunk_size: f32,
        camera_x: f32,
        camera_z: f32,
        current_tier: Tier,
        current_active_count: usize,
        chunk_instance_count: usize,
    ) -> GateDecision {
        let dist_sq = chunk_coord.distance_squared_to(camera_x, camera_z, chunk_size);

        match current_tier {
            Tier::Cold => {
                // Cold chunks only get promoted if within active radius AND under cap.
                if dist_sq <= self.active_radius_sq
                    && current_active_count + chunk_instance_count <= self.active_cap
                {
                    GateDecision::Promote
                } else {
                    GateDecision::Hold
                }
            }
            Tier::Hot => {
                // Hot → Active: within active radius AND under cap.
                if dist_sq <= self.active_radius_sq
                    && current_active_count + chunk_instance_count <= self.active_cap
                {
                    GateDecision::Promote
                }
                // Hot → Cold: beyond cold radius.
                else if dist_sq > self.cold_radius_sq {
                    GateDecision::DemoteToCold
                }
                // Otherwise hold in hot cache.
                else {
                    GateDecision::Hold
                }
            }
            Tier::Active => {
                // Active → Hot: beyond evict radius (hysteresis — NOT at active_radius).
                if dist_sq > self.evict_radius_sq {
                    GateDecision::DemoteToHot
                } else {
                    GateDecision::Hold
                }
            }
        }
    }

    /// Run a full evaluation pass over all chunks in the grid.
    /// Returns a list of (ChunkCoord, GateDecision) pairs that require action.
    /// Hold decisions are filtered out.
    pub fn evaluate_all(
        &self,
        chunks: &[(ChunkCoord, Tier, usize)], // (coord, current_tier, instance_count)
        chunk_size: f32,
        camera_x: f32,
        camera_z: f32,
        current_active_count: usize,
    ) -> (Vec<(ChunkCoord, GateDecision)>, GateStats) {
        let mut decisions = Vec::new();
        let mut stats = GateStats::default();
        let mut running_active = current_active_count;

        // Sort by distance (closest first) so promotions fill the cap optimally.
        let mut sorted: Vec<_> = chunks.iter().collect();
        sorted.sort_by(|a, b| {
            let da = a.0.distance_squared_to(camera_x, camera_z, chunk_size);
            let db = b.0.distance_squared_to(camera_x, camera_z, chunk_size);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });

        for &&(coord, tier, count) in &sorted {
            let decision = self.evaluate(
                coord, chunk_size, camera_x, camera_z,
                tier, running_active, count,
            );

            match decision {
                GateDecision::Hold => continue,
                GateDecision::Promote => {
                    running_active += count;
                    stats.promoted_chunks += 1;
                    stats.promoted_instances += count;
                }
                GateDecision::DemoteToHot => {
                    running_active = running_active.saturating_sub(count);
                    stats.demoted_to_hot_chunks += 1;
                    stats.demoted_to_hot_instances += count;
                }
                GateDecision::DemoteToCold => {
                    stats.demoted_to_cold_chunks += 1;
                    stats.demoted_to_cold_instances += count;
                }
            }

            decisions.push((coord, decision));
        }

        stats.active_total = running_active;
        (decisions, stats)
    }

    /// [`Self::evaluate_all`] with predictive prefetch folded in:
    ///
    /// - a chunk in `admitted` (prefetch asked for it this tick, inside its
    ///   I/O budget) is promoted before it enters the active radius, under
    ///   the same active cap;
    /// - a chunk in `pinned` (predicted, or lingering in the hysteresis
    ///   window) is never demoted: nothing evicts what is about to be used.
    ///
    /// With both sets empty this is exactly `evaluate_all`.
    pub fn evaluate_all_with_prefetch(
        &self,
        chunks: &[(ChunkCoord, Tier, usize)],
        chunk_size: f32,
        camera_x: f32,
        camera_z: f32,
        current_active_count: usize,
        admitted: &HashSet<ChunkCoord>,
        pinned: &HashSet<ChunkCoord>,
    ) -> (Vec<(ChunkCoord, GateDecision)>, GateStats) {
        let mut decisions = Vec::new();
        let mut stats = GateStats::default();
        let mut running_active = current_active_count;

        let mut sorted: Vec<_> = chunks.iter().collect();
        sorted.sort_by(|a, b| {
            let da = a.0.distance_squared_to(camera_x, camera_z, chunk_size);
            let db = b.0.distance_squared_to(camera_x, camera_z, chunk_size);
            da.partial_cmp(&db)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then((a.0.x, a.0.z).cmp(&(b.0.x, b.0.z)))
        });

        for &&(coord, tier, count) in &sorted {
            let mut decision = self.evaluate(
                coord, chunk_size, camera_x, camera_z,
                tier, running_active, count,
            );
            match decision {
                GateDecision::DemoteToHot | GateDecision::DemoteToCold if pinned.contains(&coord) => {
                    stats.pinned_holds += 1;
                    decision = GateDecision::Hold;
                }
                GateDecision::Hold
                    if tier != Tier::Active
                        && admitted.contains(&coord)
                        && running_active + count <= self.active_cap =>
                {
                    stats.prefetch_promoted_chunks += 1;
                    decision = GateDecision::Promote;
                }
                _ => {}
            }

            match decision {
                GateDecision::Hold => continue,
                GateDecision::Promote => {
                    running_active += count;
                    stats.promoted_chunks += 1;
                    stats.promoted_instances += count;
                }
                GateDecision::DemoteToHot => {
                    running_active = running_active.saturating_sub(count);
                    stats.demoted_to_hot_chunks += 1;
                    stats.demoted_to_hot_instances += count;
                }
                GateDecision::DemoteToCold => {
                    stats.demoted_to_cold_chunks += 1;
                    stats.demoted_to_cold_instances += count;
                }
            }
            decisions.push((coord, decision));
        }

        stats.active_total = running_active;
        (decisions, stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> HysteresisRadiusGate {
        HysteresisRadiusGate::new(&StreamingConfig {
            chunk_size: 100.0,
            active_radius: 150.0,
            evict_radius: 200.0,
            cold_radius: 1_000.0,
            active_cap: 1_000,
            ..StreamingConfig::default()
        })
    }

    fn cc(x: i32, z: i32) -> ChunkCoord {
        ChunkCoord { x, z }
    }

    #[test]
    fn rage_gate_promotes_an_admitted_chunk_before_it_is_in_range() {
        let g = gate();
        // Chunk (5,0) centre x=550: far outside the 150 m active radius.
        let chunks = [(cc(5, 0), Tier::Hot, 10)];
        let none = HashSet::new();
        let (plain, _) = g.evaluate_all_with_prefetch(&chunks, 100.0, 50.0, 50.0, 0, &none, &none);
        assert!(plain.is_empty(), "out of range and not predicted: held");
        let admitted: HashSet<_> = [cc(5, 0)].into_iter().collect();
        let (d, stats) = g.evaluate_all_with_prefetch(&chunks, 100.0, 50.0, 50.0, 0, &admitted, &none);
        assert_eq!(d, vec![(cc(5, 0), GateDecision::Promote)]);
        assert_eq!(stats.prefetch_promoted_chunks, 1);
    }

    #[test]
    fn rage_gate_never_demotes_a_pinned_chunk() {
        let g = gate();
        // Active chunk past the evict radius.
        let chunks = [(cc(-5, 0), Tier::Active, 10)];
        let none = HashSet::new();
        let (d, _) = g.evaluate_all_with_prefetch(&chunks, 100.0, 50.0, 50.0, 10, &none, &none);
        assert_eq!(d, vec![(cc(-5, 0), GateDecision::DemoteToHot)]);
        let pinned: HashSet<_> = [cc(-5, 0)].into_iter().collect();
        let (d, stats) = g.evaluate_all_with_prefetch(&chunks, 100.0, 50.0, 50.0, 10, &none, &pinned);
        assert!(d.is_empty(), "pinned chunk held: {d:?}");
        assert_eq!(stats.pinned_holds, 1);
        assert_eq!(stats.active_total, 10);
    }

    #[test]
    fn rage_gate_prefetch_respects_the_active_cap() {
        let g = HysteresisRadiusGate::new(&StreamingConfig {
            chunk_size: 100.0,
            active_radius: 150.0,
            evict_radius: 200.0,
            cold_radius: 1_000.0,
            active_cap: 15,
            ..StreamingConfig::default()
        });
        let chunks = [(cc(5, 0), Tier::Hot, 10)];
        let admitted: HashSet<_> = [cc(5, 0)].into_iter().collect();
        let (d, _) = g.evaluate_all_with_prefetch(&chunks, 100.0, 50.0, 50.0, 10, &admitted, &HashSet::new());
        assert!(d.is_empty(), "10 + 10 would pass the cap of 15");
    }

    #[test]
    fn rage_gate_matches_the_plain_pass_without_prefetch() {
        let g = gate();
        let chunks = [
            (cc(0, 0), Tier::Hot, 3),
            (cc(1, 0), Tier::Active, 4),
            (cc(3, 0), Tier::Active, 5),
            (cc(12, 0), Tier::Hot, 6),
        ];
        let (a, sa) = g.evaluate_all(&chunks, 100.0, 50.0, 50.0, 9);
        let (b, sb) = g.evaluate_all_with_prefetch(&chunks, 100.0, 50.0, 50.0, 9, &HashSet::new(), &HashSet::new());
        assert_eq!(a, b);
        assert_eq!(sa.active_total, sb.active_total);
    }
}
