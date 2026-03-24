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
}
