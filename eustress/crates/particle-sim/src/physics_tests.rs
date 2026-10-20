//! Physics acceptance tests: each one checks an emergent quantity against
//! its analytic value, so a wrong force, sign, unit or integrator fails
//! loudly rather than "looking plausible".

use bevy_math::Vec3;

use super::params::*;
use super::presets::{self, ParticlePreset};
use super::solver::ParticleSim;
use crate::constants;

fn electron_gas(count: u32, n: f64, tau: f32, e_field: f32, size: f32) -> ParticleSim {
    let (q, m) = ParticlePreset::Electron.charge_mass().unwrap();
    let params = SimParams {
        domain_size: Vec3::splat(size),
        boundary: [Boundary::Periodic; 3],
        gravity: Vec3::ZERO,
        electric_field: Vec3::new(e_field, 0.0, 0.0),
        electrostatics: Electrostatics::Off,
        lattice_temperature: 293.15,
        max_substeps: 100_000,
        ..Default::default()
    };
    let species = SpeciesParams {
        name: "Electrons".into(),
        charge: q,
        mass: m,
        count,
        number_density: n,
        temperature: 293.15,
        collision_time: tau,
        ..Default::default()
    };
    ParticleSim::new(params, vec![species])
}

/// Ohm's law from particle motion: copper's free electrons, driven by a
/// field and scattered by the lattice, carry J = sigma E with the Drude
/// sigma = n e^2 tau / m (6.44e7 S/m).
#[test]
fn drude_conductivity_emerges_for_copper() {
    let cu = presets::conductor("Copper").unwrap();
    let tau = cu.relaxation_time as f32;
    let mut sim = electron_gas(20_000, cu.electron_density, tau, 1.0e7, 20e-9);
    let expect = presets::drude_conductivity(
        cu.electron_density,
        constants::ELEMENTARY_CHARGE,
        tau as f64,
        constants::ELECTRON_MASS,
    );
    // Settle for 20 tau, then average J over 60 tau.
    for _ in 0..20 {
        sim.advance(tau as f64);
    }
    let mut j = 0.0f64;
    let samples = 60;
    for _ in 0..samples {
        sim.advance(tau as f64);
        let s = &sim.stats.species[0];
        j += (s.real_count / sim.params.volume()) * constants::ELEMENTARY_CHARGE * -(s.drift_velocity.x as f64);
    }
    let sigma = (j / samples as f64) / 1.0e7;
    let err = (sigma - expect).abs() / expect;
    assert!(err < 0.03, "sigma {sigma:.4e} vs drude {expect:.4e} ({:.2}%)", err * 100.0);
    // The analytic value the class reports agrees too.
    let reported = sim.stats.drude_conductivity;
    assert!((reported - expect).abs() / expect < 1e-6, "reported {reported:e}");
    // Field heating: between collisions each electron gains (q/m) E t along
    // the field, with t exponentially distributed (mean tau), which adds
    // m v_d^2 / k_B to the kinetic temperature of that axis, a third of it
    // to the average. Hot electrons, sitting above the lattice.
    let s = &sim.stats.species[0];
    let vd = s.drift_velocity.x as f64;
    let expect_t = 293.15 + constants::ELECTRON_MASS * vd * vd / (3.0 * constants::K_B);
    let t = s.temperature as f64;
    assert!((t - expect_t).abs() / expect_t < 0.05, "electron temperature {t:.1} K vs {expect_t:.1} K");
}

/// The time-averaged current reported in stats matches the drift current.
#[test]
fn reported_conductivity_tracks_drude() {
    let cu = presets::conductor("Copper").unwrap();
    let tau = cu.relaxation_time as f32;
    let mut sim = electron_gas(20_000, cu.electron_density, tau, 1.0e7, 20e-9);
    for _ in 0..200 {
        sim.advance(0.5 * tau as f64);
    }
    let expect = sim.stats.drude_conductivity;
    let got = sim.stats.conductivity;
    assert!((got - expect).abs() / expect < 0.05, "measured {got:e} vs {expect:e}");
    assert!(sim.stats.joule_power_density > 0.0);
}

/// Joule heating and energy conservation: the work the field does on the
/// electrons (sum q E.v dt) ends up as lattice heat plus the change in the
/// electrons' own kinetic energy, and the lattice temperature rises by its
/// share over the heat capacity. (A classical electron gas holds 3/2 n k_B
/// per kelvin, about half the lattice's here; the Sommerfeld value is ~100x
/// smaller, which is why the textbook ignores it and this test cannot.)
#[test]
fn joule_heating_conserves_energy() {
    let cu = presets::conductor("Copper").unwrap();
    let tau = cu.relaxation_time as f32;
    let e_field = 3.0e7f32;
    let mut sim = electron_gas(10_000, cu.electron_density, tau, e_field, 20e-9);
    sim.params.joule_heating = true;
    sim.params.lattice_heat_capacity = cu.heat_capacity as f32;
    for _ in 0..40 {
        sim.advance(tau as f64);
    }
    let t0 = sim.lattice_temperature as f64;
    let lattice0 = sim.lattice_energy;
    let ke0 = sim.stats.kinetic_energy;
    let q_macro = sim.species[0].q as f64;
    let dt = 0.1 * tau as f64;
    let mut work = 0.0f64;
    for _ in 0..1000 {
        // Work over one step at the mean of the start and end velocities.
        let before: f64 = sim.vel.iter().map(|v| v.x as f64).sum();
        sim.advance(dt);
        let after: f64 = sim.vel.iter().map(|v| v.x as f64).sum();
        work += q_macro * e_field as f64 * 0.5 * (before + after) * dt;
    }
    let to_lattice = sim.lattice_energy - lattice0;
    let d_ke = sim.stats.kinetic_energy - ke0;
    let balance = (to_lattice + d_ke - work).abs() / work;
    assert!(work > 0.0);
    assert!(balance < 0.03, "work {work:e} J vs lattice {to_lattice:e} + dKE {d_ke:e} ({:.2}%)", balance * 100.0);
    let dt_lat = sim.lattice_temperature as f64 - t0;
    let expect_dt = to_lattice / (cu.heat_capacity * sim.params.volume());
    assert!((dt_lat - expect_dt).abs() / expect_dt < 1e-3, "{dt_lat} vs {expect_dt}");
    assert!(dt_lat > 0.0);
}

/// Cold plasma oscillation: a sinusoidal velocity perturbation of an
/// electron slab over a neutralising background rings at the plasma
/// frequency sqrt(n e^2 / (eps0 m)).
#[test]
fn plasma_oscillates_at_plasma_frequency() {
    let (q, m) = ParticlePreset::Electron.charge_mass().unwrap();
    let n = 1.0e18;
    let lx = 1.0e-2f32;
    let size = Vec3::new(lx, 1.25e-3, 1.25e-3);
    let params = SimParams {
        domain_size: size,
        boundary: [Boundary::Periodic; 3],
        gravity: Vec3::ZERO,
        electrostatics: Electrostatics::Mesh,
        grid_resolution: 32,
        neutralizing_background: true,
        max_substeps: 100_000,
        ..Default::default()
    };
    // Quiet start: exactly 2 x 2 x 2 electrons per cell of the 32 x 4 x 4
    // mesh, so the unperturbed plasma deposits a perfectly uniform charge.
    let species = SpeciesParams {
        name: "Electrons".into(),
        charge: q,
        mass: m,
        count: 64 * 8 * 8,
        number_density: n,
        temperature: 0.0,
        arrangement: Arrangement::Lattice,
        ..Default::default()
    };
    let mut sim = ParticleSim::new(params, vec![species]);
    let k = std::f32::consts::TAU / lx;
    // Displacement amplitude v0/omega_p ~ 1e-3 of a wavelength: linear regime.
    let v0 = 5.0e5f32;
    for (p, v) in sim.pos.iter().zip(sim.vel.iter_mut()) {
        v.x = v0 * (k * (p.x + 0.5 * lx)).sin();
    }
    let wp = presets::plasma_frequency(n, q, m);
    let period = std::f64::consts::TAU / wp;
    let dt = period / 80.0;
    let mode = |sim: &ParticleSim| -> f64 {
        sim.pos
            .iter()
            .zip(&sim.vel)
            .map(|(p, v)| v.x as f64 * (k * (p.x + 0.5 * lx)).sin() as f64)
            .sum()
    };
    let mut prev = mode(&sim);
    let mut crossings = Vec::new();
    let mut t = 0.0f64;
    while t < 3.2 * period {
        sim.advance(dt);
        t += dt;
        let a = mode(&sim);
        if prev.signum() != a.signum() && prev != 0.0 {
            // Linear interpolation of the crossing time.
            crossings.push(t - dt * a / (a - prev));
        }
        prev = a;
    }
    assert!(crossings.len() >= 5, "only {} crossings", crossings.len());
    let measured_half = (crossings[crossings.len() - 1] - crossings[0]) / (crossings.len() - 1) as f64;
    let w = std::f64::consts::PI / measured_half;
    let err = (w - wp).abs() / wp;
    assert!(err < 0.03, "omega {w:e} vs omega_p {wp:e} ({:.2}%)", err * 100.0);
}

/// Boris push: an electron in a uniform B field gyrates on a circle of
/// radius m v / (|q| B) with period 2 pi m / (|q| B), speed conserved.
#[test]
fn electron_gyrates_in_magnetic_field() {
    let (q, m) = ParticlePreset::Electron.charge_mass().unwrap();
    let b = 0.01f32;
    let v = 1.0e6f32;
    let params = SimParams {
        domain_size: Vec3::splat(1.0e-2),
        boundary: [Boundary::Periodic; 3],
        gravity: Vec3::ZERO,
        magnetic_field: Vec3::new(0.0, 0.0, b),
        max_substeps: 1_000_000,
        ..Default::default()
    };
    let species = SpeciesParams {
        name: "e".into(),
        charge: q,
        mass: m,
        count: 1,
        temperature: 0.0,
        region_min: Vec3::splat(0.5),
        region_max: Vec3::splat(0.5),
        ..Default::default()
    };
    let mut sim = ParticleSim::new(params, vec![species]);
    sim.pos[0] = Vec3::ZERO;
    sim.vel[0] = Vec3::new(v, 0.0, 0.0);
    let omega = (q.abs() * b as f64 / m) as f32;
    let period = std::f32::consts::TAU / omega;
    let radius = v / omega;
    let steps = 2000;
    let mut max_dist = 0.0f32;
    for _ in 0..steps {
        sim.step(period / steps as f32);
        max_dist = max_dist.max(sim.pos[0].length());
    }
    // Diameter of the orbit, and back to the start after one period.
    assert!((max_dist - 2.0 * radius).abs() < 0.01 * radius, "orbit diameter {max_dist} vs {}", 2.0 * radius);
    assert!(sim.pos[0].length() < 0.01 * radius, "did not close: {:?}", sim.pos[0]);
    assert!((sim.vel[0].length() - v).abs() < 1e-4 * v, "speed drifted");
}

/// Two electrons released at rest repel; total energy (kinetic +
/// Coulomb) is conserved by the direct solver.
#[test]
fn coulomb_pair_conserves_energy() {
    let (q, m) = ParticlePreset::Electron.charge_mass().unwrap();
    let d = 1.0e-9f32;
    let params = SimParams {
        domain_size: Vec3::splat(1.0e-6),
        boundary: [Boundary::Reflect; 3],
        gravity: Vec3::ZERO,
        electrostatics: Electrostatics::Direct,
        softening: 0.0,
        max_substeps: 1_000_000,
        ..Default::default()
    };
    let species = SpeciesParams {
        name: "e".into(),
        charge: q,
        mass: m,
        count: 2,
        temperature: 0.0,
        ..Default::default()
    };
    let mut sim = ParticleSim::new(params, vec![species]);
    sim.pos[0] = Vec3::new(-0.5 * d, 0.0, 0.0);
    sim.pos[1] = Vec3::new(0.5 * d, 0.0, 0.0);
    sim.vel[0] = Vec3::ZERO;
    sim.vel[1] = Vec3::ZERO;
    let k = 8.987_551_8e9f64;
    let energy = |s: &ParticleSim| -> f64 {
        let r = (s.pos[0] - s.pos[1]).length() as f64;
        let ke: f64 = s.vel.iter().map(|v| 0.5 * m * v.length_squared() as f64).sum();
        ke + k * q * q / r
    };
    let e0 = energy(&sim);
    // Characteristic time: sqrt(m d^3 / (k q^2)).
    let t_c = (m * (d as f64).powi(3) / (k * q * q)).sqrt();
    let dt = (t_c / 400.0) as f32;
    for _ in 0..4000 {
        sim.step(dt);
    }
    let e1 = energy(&sim);
    assert!((e1 - e0).abs() / e0 < 1e-3, "energy {e0:e} -> {e1:e}");
    assert!((sim.pos[0] - sim.pos[1]).length() > 3.0 * d, "they did not fly apart");
}

/// Hydrostatics (DFSPH): a water block settles to rest density with a
/// pressure gradient of rho g, and stays in its tank.
#[test]
fn water_column_settles_hydrostatically() {
    settle_water_column(FluidSolver::Dfsph, 0.005);
}

/// The same with weakly compressible SPH, which compresses more.
#[test]
fn wcsph_water_column_settles_hydrostatically() {
    settle_water_column(FluidSolver::Wcsph, 0.02);
}

fn settle_water_column(fluid_solver: FluidSolver, max_compression: f32) {
    let params = SimParams {
        domain_size: Vec3::new(0.1, 0.25, 0.1),
        boundary: [Boundary::Reflect; 3],
        restitution: 0.0,
        wall_friction: 0.0,
        max_substeps: 100_000,
        fluid_solver,
        ..Default::default()
    };
    let (rho0, mu) = ParticlePreset::Water.fluid_properties().unwrap();
    let species = SpeciesParams {
        name: "Water".into(),
        kind: SpeciesKind::Fluid,
        count: 10 * 14 * 10,
        rest_density: rho0,
        viscosity: mu,
        region_min: Vec3::ZERO,
        region_max: Vec3::new(1.0, 0.55, 1.0),
        arrangement: Arrangement::Lattice,
        ..Default::default()
    };
    let mut sim = ParticleSim::new(params, vec![species]);
    let mut ke = Vec::new();
    for f in 0..120 {
        sim.advance(1.0 / 60.0);
        if f % 20 == 19 {
            ke.push(sim.stats.kinetic_energy);
        }
    }
    let st = &sim.stats;
    // Settling: the energy decays rather than being pumped.
    assert!(ke.last().unwrap() < &ke[0], "kinetic energy did not decay: {ke:?}");
    assert!(st.max_speed < 0.1, "still moving: {} m/s (KE {ke:?})", st.max_speed);
    assert!(st.density_error < max_compression, "{fluid_solver:?} compression {}", st.density_error);
    // Hydrostatics inside the column: dp/dy = -rho g between an interior
    // layer near the bottom and one near the top, away from the walls. (The
    // layer touching a wall reads up to 2x high: Akinci's one-sided wall
    // term makes it carry the column's weight alone.)
    let spacing = sim.species[0].spacing;
    let top = sim.pos.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let bottom = sim.pos.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let half = sim.params.half();
    let interior = |p: Vec3| p.x.abs() < half.x - 2.5 * spacing && p.z.abs() < half.z - 2.5 * spacing;
    let band = |y0: f32| -> (f64, f64) {
        let (mut p, mut y, mut n) = (0.0f64, 0.0f64, 0);
        for i in 0..sim.pos.len() {
            let x = sim.pos[i];
            if interior(x) && (x.y - y0).abs() < 0.5 * spacing {
                p += sim.pressure[i] as f64;
                y += x.y as f64;
                n += 1;
            }
        }
        assert!(n > 0, "empty band at {y0}");
        (p / n as f64, y / n as f64)
    };
    let (p_low, y_low) = band(bottom + 3.0 * spacing);
    let (p_high, y_high) = band(top - 3.0 * spacing);
    let gradient = (p_low - p_high) / (y_high - y_low);
    let expect = rho0 as f64 * 9.80665;
    assert!(
        (gradient - expect).abs() / expect < 0.2,
        "hydrostatic gradient {gradient:.0} Pa/m vs rho g {expect:.0} Pa/m"
    );
    // Stayed in the tank, and the column kept its height.
    assert!(sim.pos.iter().all(|x| x.abs().cmple(sim.params.half() + Vec3::splat(1e-6)).all()));
    assert!(top - bottom < 0.16, "column grew to {}", top - bottom);
}

/// Thermal initialisation and an elastic box: a neutral gas starts at the
/// requested temperature and keeps its energy exactly.
#[test]
fn neutral_gas_temperature_and_energy() {
    let (q, m) = ParticlePreset::Argon.charge_mass().unwrap();
    let params = SimParams {
        domain_size: Vec3::splat(1.0e-6),
        boundary: [Boundary::Reflect; 3],
        restitution: 1.0,
        wall_friction: 0.0,
        gravity: Vec3::ZERO,
        max_substeps: 100_000,
        ..Default::default()
    };
    let species = SpeciesParams {
        name: "Ar".into(),
        charge: q,
        mass: m,
        count: 20_000,
        temperature: 300.0,
        ..Default::default()
    };
    let mut sim = ParticleSim::new(params, vec![species]);
    let t0 = sim.stats.species[0].temperature;
    assert!((t0 - 300.0).abs() < 6.0, "initial temperature {t0}");
    let e0 = sim.stats.kinetic_energy;
    for _ in 0..50 {
        sim.advance(1.0e-9);
    }
    let e1 = sim.stats.kinetic_energy;
    assert!((e1 - e0).abs() / e0 < 1e-4, "energy {e0:e} -> {e1:e}");
}

/// A beam emitted at one face and absorbed at the other carries the
/// current the emission rate implies, measured at the mid-plane.
#[test]
fn absorbed_beam_current_matches_emission() {
    let (q, m) = ParticlePreset::Electron.charge_mass().unwrap();
    let rate = 2.0e12f32; // simulated particles per second
    let params = SimParams {
        domain_size: Vec3::new(1.0e-3, 2.0e-4, 2.0e-4),
        boundary: [Boundary::Absorb, Boundary::Reflect, Boundary::Reflect],
        gravity: Vec3::ZERO,
        voltage_axis: 0,
        max_substeps: 100_000,
        ..Default::default()
    };
    let species = SpeciesParams {
        name: "Beam".into(),
        charge: q,
        mass: m,
        count: 0,
        temperature: 0.0,
        drift_velocity: Vec3::new(1.0e6, 0.0, 0.0),
        region_min: Vec3::new(0.0, 0.4, 0.4),
        region_max: Vec3::new(0.02, 0.6, 0.6),
        emission_rate: rate,
        ..Default::default()
    };
    let mut sim = ParticleSim::new(params, vec![species]);
    let transit = 1.0e-3 / 1.0e6;
    for _ in 0..400 {
        sim.advance(transit / 20.0);
    }
    // Emitted weight is 1 (no number density), so I = rate * q.
    let expect = rate as f64 * q;
    let got = sim.stats.current;
    assert!((got - expect).abs() / expect.abs() < 0.05, "current {got:e} vs {expect:e}");
    assert!(sim.stats.absorbed_charge < 0.0, "absorbed charge is negative for electrons");
}

/// Same seed and parameters give the same trajectory on 1 or 4 threads.
#[test]
fn deterministic_across_thread_counts() {
    let run = |threads: usize| -> Vec<Vec3> {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        pool.install(|| {
            let cu = presets::conductor("Copper").unwrap();
            // A dilute electron gas: at metal density the plasma frequency forces
            // ~1e-17 s substeps, each a Poisson solve.
            let mut sim = electron_gas(5000, 1.0e24, cu.relaxation_time as f32, 1.0e7, 10e-9);
            sim.params.electrostatics = Electrostatics::Mesh;
            sim.params.grid_resolution = 16;
            for _ in 0..30 {
                sim.advance(2.0e-15);
            }
            let params = SimParams {
                domain_size: Vec3::new(0.2, 0.3, 0.2),
                max_substeps: 10_000,
                ..Default::default()
            };
            let water = SpeciesParams {
                kind: SpeciesKind::Fluid,
                count: 1500,
                region_max: Vec3::new(1.0, 0.4, 1.0),
                arrangement: Arrangement::Lattice,
                ..Default::default()
            };
            let mut fluid = ParticleSim::new(params, vec![water]);
            for _ in 0..10 {
                fluid.advance(1.0 / 60.0);
            }
            let mut out = sim.pos.clone();
            out.extend(fluid.pos.iter().copied());
            out
        })
    };
    let a = run(1);
    let b = run(4);
    assert_eq!(a.len(), b.len());
    assert!(a.iter().zip(&b).all(|(x, y)| x == y), "trajectories differ between thread counts");
}

/// Obstacles: fluid poured on a box comes to rest on it, and the
/// impulse handed to the box over time balances the weight it carries.
#[test]
fn obstacle_supports_fluid_and_reports_impulse() {
    let params = SimParams {
        domain_size: Vec3::new(0.3, 0.3, 0.3),
        boundary: [Boundary::Reflect; 3],
        restitution: 0.0,
        max_substeps: 100_000,
        ..Default::default()
    };
    let species = SpeciesParams {
        name: "Water".into(),
        kind: SpeciesKind::Fluid,
        count: 1000,
        region_min: Vec3::new(0.3, 0.55, 0.3),
        region_max: Vec3::new(0.7, 0.75, 0.7),
        arrangement: Arrangement::Lattice,
        ..Default::default()
    };
    let mut sim = ParticleSim::new(params, vec![species]);
    sim.set_obstacles(vec![Obstacle {
        shape: ObstacleShape::Box,
        center: Vec3::new(0.0, -0.05, 0.0),
        rotation: bevy_math::Quat::IDENTITY,
        half_extents: Vec3::new(0.15, 0.05, 0.15),
        velocity: Vec3::ZERO,
    }]);
    // Let it land and spread; the box spans the full width so it holds the lot.
    for _ in 0..60 {
        sim.advance(1.0 / 60.0);
    }
    sim.take_obstacle_impulses();
    let span = 0.5f64;
    for _ in 0..30 {
        sim.advance(1.0 / 60.0);
    }
    let taken = sim.take_obstacle_impulses()[0];
    let impulse = taken.linear;
    // Symmetric load on a centred box: no net twist (to within the lever
    // arm of one particle spacing times the carried impulse).
    let lever = sim.species[0].spacing;
    assert!(taken.angular.length() < lever * impulse.length(), "spurious torque {:?}", taken.angular);
    let total_mass: f64 = sim.sp.iter().map(|&s| sim.species[s as usize].m as f64).sum();
    let weight = total_mass * 9.80665;
    let force = -(impulse.y as f64) / span;
    // At rest on a box spanning the tank, the box carries the whole weight.
    assert!((force - weight).abs() < 0.25 * weight, "force {force} N vs weight {weight} N");
    let above = sim.pos.iter().filter(|p| p.y > 0.0).count();
    assert!(above as f64 > 0.9 * sim.pos.len() as f64, "fluid fell through: {above}/{}", sim.pos.len());
}

fn small_tank(max_substeps: u32) -> ParticleSim {
    let params = SimParams {
        domain_size: Vec3::new(0.2, 0.2, 0.2),
        boundary: [Boundary::Reflect; 3],
        max_substeps,
        ..Default::default()
    };
    let (rho0, mu) = ParticlePreset::Water.fluid_properties().unwrap();
    let species = SpeciesParams {
        name: "Water".into(),
        kind: SpeciesKind::Fluid,
        count: 600,
        rest_density: rho0,
        viscosity: mu,
        region_min: Vec3::ZERO,
        region_max: Vec3::new(1.0, 0.5, 1.0),
        arrangement: Arrangement::Lattice,
        ..Default::default()
    };
    ParticleSim::new(params, vec![species])
}

/// A run too heavy for real time plays in slow motion but walks through
/// exactly the states an unhurried run does: the frame budget changes how
/// fast the trajectory is shown, never the trajectory.
#[test]
fn slow_motion_replays_the_same_trajectory() {
    // One substep per frame, although each 60 Hz frame owes many.
    let mut slow = small_tank(1);
    for _ in 0..40 {
        slow.advance_realtime(1.0 / 60.0, None, true);
    }
    assert_eq!(slow.steps, 40);
    assert!(slow.stats.realtime_ratio < 0.5, "ratio {}", slow.stats.realtime_ratio);
    let mut reference = small_tank(1);
    for _ in 0..40 {
        let dt = reference.substep_size();
        reference.step(dt);
    }
    assert_eq!(slow.pos, reference.pos, "slow motion changed the trajectory");
    assert_eq!(slow.vel, reference.vel);
}

/// With room to spare, real-time stepping keeps the simulated clock within
/// one substep of the requested time and reports it is keeping up.
#[test]
fn realtime_run_keeps_up() {
    let mut sim = small_tank(10_000);
    let frames = 30;
    for _ in 0..frames {
        sim.advance_realtime(1.0 / 60.0, None, true);
    }
    let wanted = frames as f64 / 60.0;
    let lag = wanted - sim.time;
    assert!(lag >= -1e-9 && lag < sim.substep_size() as f64 + 1e-9, "sim time {} vs {wanted}", sim.time);
    assert!(sim.stats.realtime_ratio > 0.95, "ratio {}", sim.stats.realtime_ratio);
}

/// DFSPH substeps are limited by the flow, not by an artificial speed of
/// sound, so the same tank takes several times fewer of them than WCSPH.
#[test]
fn dfsph_substeps_follow_the_flow() {
    let run = |fluid_solver| {
        let mut sim = small_tank(10_000);
        sim.params.fluid_solver = fluid_solver;
        for _ in 0..20 {
            sim.advance(1.0 / 60.0);
        }
        sim.substep_size()
    };
    let (dfsph, wcsph) = (run(FluidSolver::Dfsph), run(FluidSolver::Wcsph));
    assert!(dfsph > 4.0 * wcsph, "DFSPH substep {dfsph} s vs WCSPH {wcsph} s");
}

/// Dam break: a water column collapses across its tank without compressing
/// or gaining energy, and settles at the depth its volume dictates.
#[test]
fn dfsph_dam_break_stays_incompressible() {
    let (lx, ly, lz) = (0.4f32, 0.25f32, 0.08f32);
    let params = SimParams {
        domain_size: Vec3::new(lx, ly, lz),
        boundary: [Boundary::Reflect; 3],
        restitution: 0.0,
        wall_friction: 0.0,
        max_substeps: 100_000,
        ..Default::default()
    };
    let (rho0, mu) = ParticlePreset::Water.fluid_properties().unwrap();
    let species = SpeciesParams {
        name: "Water".into(),
        kind: SpeciesKind::Fluid,
        count: 10 * 16 * 8,
        rest_density: rho0,
        viscosity: mu,
        region_min: Vec3::ZERO,
        region_max: Vec3::new(0.35, 0.75, 1.0),
        arrangement: Arrangement::Lattice,
        ..Default::default()
    };
    let mut sim = ParticleSim::new(params, vec![species]);
    let g = 9.80665f64;
    let m = sim.species[0].m as f64;
    let potential = |s: &ParticleSim| s.pos.iter().map(|p| m * g * p.y as f64).sum::<f64>();
    let pe0 = potential(&sim);
    let mut worst_compression = 0.0f32;
    for _ in 0..180 {
        sim.advance(1.0 / 60.0);
        worst_compression = worst_compression.max(sim.stats.density_error);
        // Energy is never created: what moves came from the fall (the
        // collapse releases about 0.4 J; 2 mJ covers solver tolerance).
        let released = pe0 - potential(&sim);
        assert!(
            sim.stats.kinetic_energy <= released * 1.05 + 2e-3,
            "kinetic energy {} J exceeds the {} J released",
            sim.stats.kinetic_energy,
            released
        );
    }
    assert!(worst_compression < 0.01, "compressed {:.2}% while flowing", worst_compression * 100.0);
    // Spread over the whole floor: the depth its volume fixes.
    let spacing = sim.species[0].spacing;
    let volume = sim.pos.len() as f32 * spacing.powi(3);
    let depth = volume / (lx * lz);
    let top = sim.pos.iter().map(|p| p.y).fold(f32::MIN, f32::max) + 0.5 * spacing + 0.5 * ly;
    let reach = sim.pos.iter().map(|p| p.x).fold(f32::MIN, f32::max) + 0.5 * lx;
    assert!(reach > 0.8 * lx, "the column did not spread: front at {reach} m of {lx} m");
    assert!((top - depth).abs() < 0.35 * depth + spacing, "settled {top} m deep, volume says {depth} m");
}
