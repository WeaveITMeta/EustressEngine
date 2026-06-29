//! # Determinism — global RNG seed for simulation-affecting randomness.
//!
//! Avian itself uses no RNG; the physics step is deterministic once the fixed
//! timestep, `SubstepCount`, and `SolverConfig` are pinned (see the engine
//! binary's physics setup). This module covers the *other* half of "same inputs
//! → same world": any randomness that feeds simulation state must derive from a
//! single, reproducible seed.
//!
//! Cosmetic RNG (entity-id generation, peer-ids, teleport codes, marketing-site
//! demos) and security RNG (key material, OsRng challenges) intentionally do
//! **not** go through this and remain entropy-seeded.

use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::SeedableRng;

/// The single source of truth for simulation-affecting randomness.
///
/// Fixing this value makes scripted/random simulation behavior reproducible.
/// The default is a fixed constant (not entropy) so the engine is deterministic
/// out of the box; set a new seed before a run to vary it reproducibly.
#[derive(Resource, Reflect, Clone, Copy, Debug, PartialEq, Eq)]
#[reflect(Resource)]
pub struct GlobalRngSeed(pub u64);

impl Default for GlobalRngSeed {
    fn default() -> Self {
        // Arbitrary fixed constant — reproducible across runs by default.
        Self(0x5EED_E057_1234_ABCD)
    }
}

impl GlobalRngSeed {
    /// A fresh `StdRng` derived from this seed, optionally salted with a stream
    /// id so independent sub-systems (per-emitter, per-sample, …) get
    /// independent but still reproducible streams from the same global seed.
    pub fn rng(&self, stream: u64) -> StdRng {
        StdRng::seed_from_u64(self.0.wrapping_add(stream))
    }
}

/// Convenience for non-system callers (e.g. scripting host types) that only
/// have access to a seed value: build a reproducible `StdRng`.
pub fn sim_rng(seed: u64, stream: u64) -> StdRng {
    StdRng::seed_from_u64(seed.wrapping_add(stream))
}

/// Registers [`GlobalRngSeed`] so simulation RNG is reproducible by default.
pub struct DeterminismPlugin;

impl Plugin for DeterminismPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GlobalRngSeed>()
            .register_type::<GlobalRngSeed>();
    }
}
