#![cfg(feature = "physics")]
//! Determinism integration test (C2/C7).
//!
//! Builds a minimal Avian 0.7 world with the SAME determinism pins the engine
//! binary applies (fixed 60 Hz `Time<Fixed>`, `SubstepCount(6)`,
//! `SolverConfig::default()`), drops dynamic bodies under gravity, steps the
//! `FixedUpdate` schedule a fixed number of times, and hashes the end state.
//! Running the scenario twice from the same `GlobalRngSeed` must produce byte-
//! identical hashes — "same inputs → same world".

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::Fixed;
use eustress_common::physics::GlobalRngSeed;
use std::hash::{Hash, Hasher};

fn build_world(seed: GlobalRngSeed) -> App {
    let mut app = App::new();
    // Headless: MinimalPlugins gives the schedules + Time without a window.
    app.add_plugins(MinimalPlugins)
        .add_plugins(PhysicsPlugins::default())
        // Same pins as the engine binary.
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .insert_resource(SubstepCount(6))
        .insert_resource(avian3d::dynamics::solver::SolverConfig::default())
        .insert_resource(Gravity(Vec3::NEG_Y * 9.80665))
        .insert_resource(seed);

    // Seed positions deterministically from the global seed so the *layout*
    // also exercises C7, not just the solver.
    let mut rng = seed.rng(0);
    use rand::Rng;
    // A static floor.
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(50.0, 1.0, 50.0),
        Transform::from_xyz(0.0, -1.0, 0.0),
    ));
    // A pile of dynamic cubes.
    for _ in 0..16 {
        let x = rng.gen_range(-2.0_f32..2.0);
        let z = rng.gen_range(-2.0_f32..2.0);
        let y = rng.gen_range(3.0_f32..8.0);
        app.world_mut().spawn((
            RigidBody::Dynamic,
            Collider::cuboid(0.5, 0.5, 0.5),
            Transform::from_xyz(x, y, z),
        ));
    }
    app
}

/// Quantize + hash all dynamic-body transforms to a single u64.
fn hash_state(app: &mut App) -> u64 {
    // Stable ordering: collect (entity bits, quantized pose) then sort.
    let mut rows: Vec<(u64, [i64; 7])> = app
        .world_mut()
        .query_filtered::<(Entity, &Transform), With<RigidBody>>()
        .iter(app.world())
        .map(|(e, t)| {
            // Quantize to 1e-4 to absorb only true float noise (there should be
            // none between identical runs, but this keeps the test robust to
            // platform fp settling without hiding real divergence).
            let q = |v: f32| (v as f64 * 10_000.0).round() as i64;
            (
                e.to_bits(),
                [
                    q(t.translation.x),
                    q(t.translation.y),
                    q(t.translation.z),
                    q(t.rotation.x),
                    q(t.rotation.y),
                    q(t.rotation.z),
                    q(t.rotation.w),
                ],
            )
        })
        .collect();
    rows.sort_by_key(|(bits, _)| *bits);

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    rows.hash(&mut hasher);
    hasher.finish()
}

fn run_n_steps(seed: GlobalRngSeed, steps: usize) -> u64 {
    let mut app = build_world(seed);
    // Drive the fixed schedule a fixed number of times. We advance `Time<Fixed>`
    // directly and run FixedMain so the step count does not depend on wall clock.
    for _ in 0..steps {
        app.world_mut()
            .resource_mut::<Time<Fixed>>()
            .advance_by(std::time::Duration::from_secs_f64(1.0 / 60.0));
        app.world_mut().run_schedule(FixedMain);
    }
    hash_state(&mut app)
}

#[test]
fn same_seed_same_world() {
    let seed = GlobalRngSeed(0xABCD_1234);
    let a = run_n_steps(seed, 120); // 2 simulated seconds at 60 Hz
    let b = run_n_steps(seed, 120);
    assert_eq!(a, b, "identical seed + fixed steps must yield identical state");
}

#[test]
fn different_seed_diverges() {
    let a = run_n_steps(GlobalRngSeed(1), 120);
    let b = run_n_steps(GlobalRngSeed(2), 120);
    assert_ne!(a, b, "different seeds should produce different layouts/outcomes");
}
