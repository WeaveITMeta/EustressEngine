//! Bevy integration of the `ParticleSimulation` class.
//!
//! Every `ParticleSimulation` entity gets a [`ParticleSimRuntime`] (the
//! solver state, never saved) built from its component and its
//! `ParticleSpecies` children, and a [`ParticleCloud`] the renderer draws.
//!
//! Schedule (`FixedUpdate`, the physics tick):
//! - [`ParticleSimSet::Configure`]: rebuild or live-update runtimes from
//!   the components; apply [`ParticleSimCommand`]s.
//! - [`ParticleSimSet::Obstacles`]: hosts with a physics engine hand the
//!   solver the parts inside each domain (`ParticleSimRuntime::sim`).
//! - [`ParticleSimSet::Step`]: owe `TimeScale` x the fixed tick and pay it
//!   within the simulation's `FrameBudget`, unless the host paused
//!   ([`ParticleSimControl`]) or `Running` is off.
//! - [`ParticleSimSet::Publish`] (in `PostUpdate`): refresh the clouds.
//!
//! Stepping is tied to the host's world clock: in the editor a simulation
//! only runs in Play (and during scripted steps) and shows its initial
//! state while editing. Each frame (`First`) publishes the solver time the
//! last frame used and restores every budget.
//!
//! The runtime lives in `eustress-common` so the editor, the headless
//! runner and the player all simulate identically; only rendering and
//! physics coupling are host-side.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::prelude::*;

use super::class::{ParticleSimulation, ParticleSpecies};
use super::diagnostics::SimStats;
use super::params::{SimParams, SpeciesParams};
use super::solver::{ParticleInstance, ParticleSim};
use crate::physics::determinism::GlobalRngSeed;

/// Ordering hooks for host integrations.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParticleSimSet {
    Configure,
    Obstacles,
    Step,
    Publish,
}

/// Host-wide switches.
#[derive(Resource, Debug, Clone)]
pub struct ParticleSimControl {
    /// Hold every simulation where it is. Hosts tie this to their world
    /// clock: the editor holds it in Edit (a run shows its initial state)
    /// and in Pause, and releases it in Play and during scripted steps.
    pub paused: bool,
    /// Hold each simulation to its `FrameBudget`. Off during deterministic
    /// scripted stepping, where every tick must pay its whole debt.
    pub budgeted: bool,
}

impl Default for ParticleSimControl {
    fn default() -> Self {
        Self { paused: false, budgeted: true }
    }
}

/// Something to do to one simulation (scripts, MCP, the Properties panel).
#[derive(Message, Debug, Clone)]
pub struct ParticleSimCommand {
    pub entity: Entity,
    pub action: ParticleSimAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParticleSimAction {
    /// Back to the initial state.
    Reset,
    /// Advance this many frames' worth of simulated time, even when paused.
    StepFrames(u32),
}

/// Live solver state of a `ParticleSimulation` (runtime only).
#[derive(Component)]
pub struct ParticleSimRuntime {
    pub sim: ParticleSim,
    /// Species entity for each solver species index.
    pub species_entities: Vec<Entity>,
    /// Bumped whenever the particles change (a step or a reset).
    pub revision: u64,
    /// Resets since spawn (initial-condition edits, commands, Play/Stop).
    pub resets: u64,
    last_params: SimParams,
    last_species: Vec<SpeciesParams>,
    /// Frames of time to advance on the next step regardless of pause.
    pending_frames: u32,
    /// Solver wall time used so far this frame.
    frame_spent: Duration,
    /// Substeps taken so far this frame.
    frame_substeps: u32,
}

impl ParticleSimRuntime {
    pub fn stats(&self) -> &SimStats {
        &self.sim.stats
    }
}

/// What the renderer draws for one simulation.
#[derive(Component, Default, Clone)]
pub struct ParticleCloud {
    /// Positions in domain coordinates (m), world display radius, colour.
    pub instances: Arc<Vec<ParticleInstance>>,
    pub revision: u64,
    /// Physical domain size (m).
    pub domain_size: Vec3,
    /// World metres per simulated metre.
    pub display_scale: f32,
    /// Colour range in use (for a legend).
    pub color_range: (f32, f32),
    pub show_domain: bool,
}

pub struct ParticleSimulationPlugin;

impl Plugin for ParticleSimulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParticleSimControl>()
            .add_message::<ParticleSimCommand>()
            .configure_sets(
                FixedUpdate,
                (ParticleSimSet::Configure, ParticleSimSet::Obstacles, ParticleSimSet::Step).chain(),
            )
            .add_systems(
                FixedUpdate,
                (
                    (configure_particle_sims, apply_particle_sim_commands)
                        .chain()
                        .in_set(ParticleSimSet::Configure),
                    step_particle_sims.in_set(ParticleSimSet::Step),
                ),
            )
            .add_systems(First, begin_particle_frame)
            .add_systems(PostUpdate, publish_particle_clouds.in_set(ParticleSimSet::Publish));
    }
}

/// Species children of a simulation, in a stable order (by name, then by
/// entity) so a reload rebuilds the same solver species indices.
fn collect_species(
    children: Option<&Children>,
    species: &Query<(&ParticleSpecies, Option<&crate::classes::Instance>, Option<&Name>)>,
) -> Vec<(Entity, String, ParticleSpecies)> {
    let mut out: Vec<(Entity, String, ParticleSpecies)> = children
        .map(|c| {
            c.iter()
                .filter_map(|e| {
                    let (sp, inst, name) = species.get(e).ok()?;
                    let name = inst
                        .map(|i| i.name.clone())
                        .or_else(|| name.map(|n| n.as_str().to_string()))
                        .unwrap_or_else(|| "Species".into());
                    Some((e, name, sp.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    out
}

pub fn configure_particle_sims(
    mut commands: Commands,
    seed: Option<Res<GlobalRngSeed>>,
    mut sims: Query<(Entity, &ParticleSimulation, Option<&Children>, Option<&mut ParticleSimRuntime>)>,
    species: Query<(&ParticleSpecies, Option<&crate::classes::Instance>, Option<&Name>)>,
) {
    let global = seed.map(|s| s.0).unwrap_or_else(|| GlobalRngSeed::default().0);
    for (entity, class, children, runtime) in &mut sims {
        if !class.enabled {
            if runtime.is_some() {
                commands
                    .entity(entity)
                    .remove::<ParticleSimRuntime>()
                    .insert(ParticleCloud::default());
            }
            continue;
        }
        let params = class.sim_params(global);
        let found = collect_species(children, &species);
        let species_params: Vec<SpeciesParams> =
            found.iter().map(|(_, name, sp)| sp.species_params(name)).collect();
        let entities: Vec<Entity> = found.iter().filter(|(_, _, sp)| sp.enabled).map(|(e, _, _)| *e).collect();
        match runtime {
            None => {
                let sim = ParticleSim::new(params.clone(), species_params.clone());
                commands.entity(entity).insert((
                    ParticleSimRuntime {
                        sim,
                        species_entities: entities,
                        revision: 1,
                        resets: 0,
                        last_params: params,
                        last_species: species_params,
                        pending_frames: 0,
                        frame_spent: Duration::ZERO,
                        frame_substeps: 0,
                    },
                    ParticleCloud::default(),
                ));
            }
            Some(mut rt) => {
                if rt.last_params != params || rt.last_species != species_params {
                    if rt.sim.reconfigure(params.clone(), species_params.clone()) {
                        rt.resets += 1;
                    }
                    rt.last_params = params;
                    rt.last_species = species_params;
                    rt.species_entities = entities;
                    rt.revision += 1;
                }
            }
        }
    }
}

pub fn apply_particle_sim_commands(
    mut messages: MessageReader<ParticleSimCommand>,
    mut sims: Query<&mut ParticleSimRuntime>,
) {
    for cmd in messages.read() {
        let Ok(mut rt) = sims.get_mut(cmd.entity) else { continue };
        match cmd.action {
            ParticleSimAction::Reset => {
                rt.sim.reset();
                rt.resets += 1;
                rt.revision += 1;
            }
            ParticleSimAction::StepFrames(n) => rt.pending_frames = rt.pending_frames.saturating_add(n),
        }
    }
}

pub fn step_particle_sims(
    time: Res<Time>,
    control: Res<ParticleSimControl>,
    mut sims: Query<(&ParticleSimulation, &mut ParticleSimRuntime)>,
) {
    let frame = time.delta_secs_f64();
    if frame <= 0.0 {
        return;
    }
    for (class, mut rt) in &mut sims {
        let rt = &mut *rt;
        let start = Instant::now();
        let mut stepped = false;
        if class.running && !control.paused {
            let budget = (control.budgeted && class.frame_budget > 0.0)
                .then(|| Duration::from_secs_f64(class.frame_budget * 1e-3).saturating_sub(rt.frame_spent));
            // At least one substep per frame (not per fixed tick), so a
            // run whose single substep outcosts the budget still moves
            // without a slow frame's extra ticks each adding another.
            let must_progress = rt.frame_substeps == 0;
            let n = rt.sim.advance_realtime(frame * class.time_scale, budget, must_progress);
            rt.frame_substeps += n;
            stepped |= n > 0;
        }
        if rt.pending_frames > 0 {
            // An explicit step request advances the whole frame, budget or not.
            rt.pending_frames -= 1;
            rt.sim.advance(frame * class.time_scale);
            stepped = true;
        }
        rt.frame_spent += start.elapsed();
        if stepped {
            rt.revision += 1;
        }
    }
}

/// Start of a frame: publish the solver time the last frame used and give
/// every simulation its whole `FrameBudget` again.
pub fn begin_particle_frame(mut sims: Query<&mut ParticleSimRuntime>) {
    for mut rt in &mut sims {
        if rt.frame_spent.is_zero() && rt.frame_substeps == 0 && rt.sim.stats.solver_ms == 0.0 {
            continue;
        }
        rt.sim.stats.solver_ms = rt.frame_spent.as_secs_f64() * 1e3;
        rt.frame_spent = Duration::ZERO;
        rt.frame_substeps = 0;
    }
}

/// Display radius per solver species: the species' own radius, or half its
/// spacing at display scale, times the simulation's particle scale.
pub fn species_display_radii(class: &ParticleSimulation, sim: &ParticleSim) -> Vec<f32> {
    let scale = class.display_scale as f32;
    let mult = class.particle_scale as f32;
    sim.species
        .iter()
        .map(|s| {
            let r = if s.params.display_radius > 0.0 { s.params.display_radius } else { 0.5 * s.spacing * scale };
            (r * mult).max(1e-6)
        })
        .collect()
}

pub fn publish_particle_clouds(mut sims: Query<(&ParticleSimulation, &ParticleSimRuntime, &mut ParticleCloud)>) {
    for (class, rt, mut cloud) in &mut sims {
        let display_changed = cloud.display_scale != class.display_scale as f32
            || cloud.show_domain != class.show_domain;
        if cloud.revision == rt.revision && !display_changed {
            continue;
        }
        let radii = species_display_radii(class, &rt.sim);
        let mut buf = Vec::with_capacity(rt.sim.particle_count());
        let range = rt.sim.write_instances(class.color_mode, class.color_range(), &radii, &mut buf);
        cloud.instances = Arc::new(buf);
        cloud.revision = rt.revision;
        cloud.domain_size = rt.sim.params.domain_size;
        cloud.display_scale = class.display_scale as f32;
        cloud.color_range = range;
        cloud.show_domain = class.show_domain;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::realism::particle_sim::class::FieldTable;

    /// An app whose every `update()` is exactly one 60 Hz fixed tick.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(ParticleSimulationPlugin);
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1.0 / 60.0),
        ));
        app
    }

    fn ticks(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
        }
    }

    /// A simulation with a water child builds a runtime, steps, and
    /// publishes a cloud with one instance per particle.
    #[test]
    fn simulation_entity_runs_and_publishes() {
        let mut app = app();
        let mut sim = ParticleSimulation::default();
        sim.set_text("DomainSize", "0.4, 0.4, 0.4").unwrap();
        let mut water = ParticleSpecies::default();
        water.set_text("Count", "500").unwrap();
        let sim_e = app.world_mut().spawn((sim, Name::new("Tank"))).id();
        app.world_mut().spawn((water, Name::new("Water"), ChildOf(sim_e)));
        ticks(&mut app, 30);
        let rt = app.world().get::<ParticleSimRuntime>(sim_e).expect("runtime");
        assert_eq!(rt.sim.particle_count(), 500);
        assert!(rt.stats().sim_time > 0.0, "it never stepped");
        let cloud = app.world().get::<ParticleCloud>(sim_e).expect("cloud");
        assert_eq!(cloud.instances.len(), 500);
        assert!(cloud.instances.iter().all(|i| i.radius > 0.0));
    }

    /// Editing a live field keeps the run; editing an initial condition
    /// restarts it; disabling clears it.
    #[test]
    fn edits_reconfigure_or_restart() {
        let mut app = app();
        let sim_e = app.world_mut().spawn((ParticleSimulation::default(), Name::new("Tank"))).id();
        let mut water = ParticleSpecies::default();
        water.set_text("Count", "200").unwrap();
        let sp_e = app.world_mut().spawn((water, Name::new("Water"), ChildOf(sim_e))).id();
        ticks(&mut app, 5);
        let t0 = app.world().get::<ParticleSimRuntime>(sim_e).unwrap().sim.time;
        assert!(t0 > 0.0);
        // Live: viscosity.
        app.world_mut().get_mut::<ParticleSimulation>(sim_e).unwrap().set_text("ArtificialViscosity", "0.2").unwrap();
        ticks(&mut app, 1);
        let rt = app.world().get::<ParticleSimRuntime>(sim_e).unwrap();
        assert!(rt.sim.time > t0, "a live edit must not restart");
        assert_eq!(rt.resets, 0);
        // Initial condition: count.
        app.world_mut().get_mut::<ParticleSpecies>(sp_e).unwrap().set_text("Count", "300").unwrap();
        ticks(&mut app, 1);
        let rt = app.world().get::<ParticleSimRuntime>(sim_e).unwrap();
        assert_eq!(rt.resets, 1);
        assert_eq!(rt.sim.particle_count(), 300);
        // Disable.
        app.world_mut().get_mut::<ParticleSimulation>(sim_e).unwrap().enabled = false;
        ticks(&mut app, 1);
        assert!(app.world().get::<ParticleSimRuntime>(sim_e).is_none());
    }

    /// Pausing freezes time; a step command still advances one frame; a
    /// reset command returns to t = 0.
    #[test]
    fn pause_and_step_commands() {
        let mut app = app();
        let sim_e = app.world_mut().spawn(ParticleSimulation::default()).id();
        let mut water = ParticleSpecies::default();
        water.set_text("Count", "100").unwrap();
        app.world_mut().spawn((water, ChildOf(sim_e)));
        ticks(&mut app, 3);
        app.world_mut().resource_mut::<ParticleSimControl>().paused = true;
        let t0 = app.world().get::<ParticleSimRuntime>(sim_e).unwrap().sim.time;
        ticks(&mut app, 2);
        assert_eq!(app.world().get::<ParticleSimRuntime>(sim_e).unwrap().sim.time, t0);
        app.world_mut().write_message(ParticleSimCommand { entity: sim_e, action: ParticleSimAction::StepFrames(1) });
        ticks(&mut app, 1);
        assert!(app.world().get::<ParticleSimRuntime>(sim_e).unwrap().sim.time > t0);
        app.world_mut().write_message(ParticleSimCommand { entity: sim_e, action: ParticleSimAction::Reset });
        ticks(&mut app, 1);
        let rt = app.world().get::<ParticleSimRuntime>(sim_e).unwrap();
        assert_eq!(rt.sim.time, 0.0);
        assert_eq!(rt.resets, 1);
    }

    /// A host whose world clock is stopped (the editor in Edit) shows each
    /// simulation's initial state: built, drawn, never stepped.
    #[test]
    fn paused_host_shows_the_initial_state() {
        let mut app = app();
        app.world_mut().resource_mut::<ParticleSimControl>().paused = true;
        let sim_e = app.world_mut().spawn(ParticleSimulation::default()).id();
        let mut water = ParticleSpecies::default();
        water.set_text("Count", "150").unwrap();
        app.world_mut().spawn((water, ChildOf(sim_e)));
        ticks(&mut app, 10);
        let rt = app.world().get::<ParticleSimRuntime>(sim_e).expect("runtime");
        assert_eq!(rt.sim.time, 0.0);
        assert_eq!(rt.sim.steps, 0);
        let initial = ParticleSim::new(rt.sim.params.clone(), rt.sim.species.iter().map(|s| s.params.clone()).collect());
        assert_eq!(rt.sim.pos, initial.pos);
        let cloud = app.world().get::<ParticleCloud>(sim_e).expect("cloud");
        assert_eq!(cloud.instances.len(), 150, "the initial state is still drawn");
    }

    /// A frame whose budget is spent takes one substep, not one per fixed
    /// tick, so a slow frame (several ticks) does not multiply the cost.
    #[test]
    fn spent_budget_takes_one_substep_per_frame() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(ParticleSimulationPlugin);
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        // Three fixed ticks per update.
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(3.0 / 60.0),
        ));
        let mut sim = ParticleSimulation::default();
        // One nanosecond: spent by the first substep of every frame.
        sim.set_text("FrameBudget", "0.000001").unwrap();
        let sim_e = app.world_mut().spawn(sim).id();
        let mut water = ParticleSpecies::default();
        water.set_text("Count", "300").unwrap();
        app.world_mut().spawn((water, ChildOf(sim_e)));
        ticks(&mut app, 2);
        let before = app.world().get::<ParticleSimRuntime>(sim_e).unwrap().sim.steps;
        ticks(&mut app, 5);
        let after = app.world().get::<ParticleSimRuntime>(sim_e).unwrap().sim.steps;
        assert_eq!(after - before, 5, "substeps over 5 frames of 3 ticks each");
        let rt = app.world().get::<ParticleSimRuntime>(sim_e).unwrap();
        assert!(rt.stats().realtime_ratio < 0.9, "it should report falling behind");
    }

    /// The solver's wall time is measured per frame and published as a
    /// stat; a run held to a budget reports how far it fell behind.
    #[test]
    fn solver_time_is_published() {
        let mut app = app();
        let sim_e = app.world_mut().spawn(ParticleSimulation::default()).id();
        let mut water = ParticleSpecies::default();
        water.set_text("Count", "400").unwrap();
        app.world_mut().spawn((water, ChildOf(sim_e)));
        ticks(&mut app, 6);
        let rt = app.world().get::<ParticleSimRuntime>(sim_e).unwrap();
        assert!(rt.stats().solver_ms > 0.0, "no solver time published");
        assert!(rt.stats().realtime_ratio > 0.0);
    }
}
