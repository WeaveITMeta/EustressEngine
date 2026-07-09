//! Avian3D physics performance BASELINE.
//!
//! Unlike `instance-capacity` (main.rs), which only simulates a hand-rolled
//! AABB sort/query as a stand-in for "physics", this binary drives the real
//! Avian3D `PhysicsPlugins` stepping pipeline (broad phase + narrow phase +
//! solver) headless, with a deterministic fixed-timestep driver
//! (`TimeUpdateStrategy::FixedTimesteps(1)`), so every `App::update()` call
//! advances the physics world by exactly one 60 Hz step.
//!
//! Scenarios:
//!   A. `falling`      — N dynamic cuboids falling onto a static ground plane.
//!   B. `static_heavy`  — N static colliders scattered in world space + a
//!                        constant 100 dynamic bodies (broad-phase scaling
//!                        with a mostly-static world).
//!
//! For each run we report:
//!   - overall mean ms/step across all STEPS
//!   - "early" mean ms/step (first 100 steps — bodies actively falling/colliding)
//!   - "steady-state" mean ms/step (last 100 steps — bodies at rest / asleep)

use std::time::{Duration, Instant};

use avian3d::prelude::*;
use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

/// Physics steps to run per scenario. 600 steps @ 60 Hz = 10 simulated seconds,
/// enough for a falling stack to settle and sleep.
const STEPS: u32 = 600;
const HZ: f64 = 60.0;
/// Window size (in steps) used for the "early" and "steady-state" means.
const WINDOW: usize = 100;

struct RunResult {
    scenario: &'static str,
    n_dynamic: usize,
    n_static: usize,
    overall_ms: f64,
    early_ms: f64,
    steady_ms: f64,
    awake_at_end: usize,
}

fn new_headless_app() -> App {
    let mut app = App::new();
    // Plugin set mirrors avian3d's own headless test harness
    // (avian3d-0.7.0/src/tests/mod.rs::create_app):
    // MinimalPlugins + TransformPlugin + PhysicsPlugins + AssetPlugin.
    app.add_plugins((
        MinimalPlugins.build().disable::<ScheduleRunnerPlugin>(),
        bevy::transform::TransformPlugin,
        // Real broad + narrow phase + solver. Feature set from Cargo.toml:
        // "3d", "f32", "parry-f32", "parallel" (rayon-backed parallel solving).
        PhysicsPlugins::default(),
        bevy::asset::AssetPlugin::default(),
    ));
    // Deterministic fixed-step driver: every App::update() runs the FixedMain
    // schedule (where Avian's physics step lives) exactly once, regardless of
    // real wall-clock time between calls. This is the standard bevy technique
    // for reproducible fixed-timestep benchmarks/tests.
    app.insert_resource(Time::<Fixed>::from_hz(HZ));
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.finish();
    app.cleanup();
    app
}

/// Scenario A: N dynamic 1m cuboids dropped from a staggered height grid onto
/// a large static ground plane. Bodies settle into a resting pile/floor and
/// (once slow enough) go to sleep — this exercises Avian's sleeping system as
/// well as the solver under an initially-dense contact-generation load.
fn run_falling(n: usize) -> RunResult {
    let mut app = new_headless_app();

    app.world_mut().spawn((
        Transform::from_xyz(0.0, -0.5, 0.0),
        RigidBody::Static,
        Collider::cuboid(1000.0, 1.0, 1000.0),
    ));

    let side = (n as f64).sqrt().ceil() as i32;
    let spacing = 1.2_f32;
    let mut spawned = 0usize;
    'outer: for x in 0..side {
        for z in 0..side {
            if spawned >= n {
                break 'outer;
            }
            let px = (x as f32 - side as f32 * 0.5) * spacing;
            let pz = (z as f32 - side as f32 * 0.5) * spacing;
            // Stagger heights 0..~22m so bodies don't all land in the same
            // instant (mirrors a real spawn/drop burst rather than a single
            // synchronized frame of impacts).
            let py = 5.0 + (spawned % 20) as f32 * 1.1;
            app.world_mut().spawn((
                Transform::from_xyz(px, py, pz),
                RigidBody::Dynamic,
                Collider::cuboid(1.0, 1.0, 1.0),
            ));
            spawned += 1;
        }
    }

    step_and_measure(&mut app, "falling", spawned, 1)
}

/// Scenario B: N static colliders scattered across a wide area (broad-phase
/// stress: many static AABBs the broad phase must still consider/skip) plus a
/// constant 100 dynamic falling bodies clustered near the origin.
fn run_static_heavy(n_static: usize) -> RunResult {
    let mut app = new_headless_app();

    // Ground plane so the 100 dynamic bodies have something to land on too.
    app.world_mut().spawn((
        Transform::from_xyz(0.0, -0.5, 0.0),
        RigidBody::Static,
        Collider::cuboid(2000.0, 1.0, 2000.0),
    ));

    // Scatter N static colliders over a wide grid, well clear of the ground
    // plane's own extent, at varying heights (never touching each other) so
    // the broad phase must maintain N static AABBs in its structure.
    let side = (n_static as f64).sqrt().ceil() as i32;
    let spacing = 3.0_f32;
    let mut spawned_static = 0usize;
    'outer: for x in 0..side {
        for z in 0..side {
            if spawned_static >= n_static {
                break 'outer;
            }
            let px = (x as f32 - side as f32 * 0.5) * spacing;
            let pz = (z as f32 - side as f32 * 0.5) * spacing;
            let py = 20.0 + (spawned_static % 7) as f32 * 3.0;
            app.world_mut().spawn((
                Transform::from_xyz(px, py, pz),
                RigidBody::Static,
                Collider::cuboid(1.0, 1.0, 1.0),
            ));
            spawned_static += 1;
        }
    }

    const N_DYNAMIC: usize = 100;
    for i in 0..N_DYNAMIC {
        let px = (i % 10) as f32 * 1.2 - 6.0;
        let pz = (i / 10) as f32 * 1.2 - 6.0;
        let py = 5.0 + (i % 20) as f32 * 1.1;
        app.world_mut().spawn((
            Transform::from_xyz(px, py, pz),
            RigidBody::Dynamic,
            Collider::cuboid(1.0, 1.0, 1.0),
        ));
    }

    step_and_measure(&mut app, "static_heavy", N_DYNAMIC, spawned_static)
}

fn step_and_measure(app: &mut App, scenario: &'static str, n_dynamic: usize, n_static: usize) -> RunResult {
    let mut times: Vec<Duration> = Vec::with_capacity(STEPS as usize);
    for _ in 0..STEPS {
        let t0 = Instant::now();
        app.update();
        times.push(t0.elapsed());
    }

    let to_ms = |d: &Duration| d.as_secs_f64() * 1e3;
    let mean = |s: &[Duration]| -> f64 {
        if s.is_empty() { 0.0 } else { s.iter().map(to_ms).sum::<f64>() / s.len() as f64 }
    };

    let early_end = WINDOW.min(times.len());
    let steady_start = times.len().saturating_sub(WINDOW);

    let awake_at_end = {
        let world = app.world_mut();
        let mut q = world.query_filtered::<Entity, (With<RigidBody>, Without<Sleeping>)>();
        q.iter(world).count()
    };

    RunResult {
        scenario,
        n_dynamic,
        n_static,
        overall_ms: mean(&times),
        early_ms: mean(&times[0..early_end]),
        steady_ms: mean(&times[steady_start..]),
        awake_at_end,
    }
}

fn print_header() {
    println!(
        "{:<14} {:>10} {:>10} {:>12} {:>12} {:>12} {:>10}",
        "scenario", "n_dynamic", "n_static", "overall_ms", "early_ms", "steady_ms", "awake_end"
    );
}

fn print_row(r: &RunResult) {
    println!(
        "{:<14} {:>10} {:>10} {:>12.4} {:>12.4} {:>12.4} {:>10}",
        r.scenario, r.n_dynamic, r.n_static, r.overall_ms, r.early_ms, r.steady_ms, r.awake_at_end
    );
}

fn main() {
    println!("Avian3D physics baseline — {STEPS} steps @ {HZ} Hz (fixed-timestep, deterministic driver)");
    println!("avian3d feature set: 3d, f32, parry-f32, parallel\n");

    print_header();

    for n in [1_000usize, 5_000, 10_000, 25_000] {
        let r = run_falling(n);
        print_row(&r);
    }

    for n_static in [10_000usize, 100_000] {
        let r = run_static_heavy(n_static);
        print_row(&r);
    }
}
