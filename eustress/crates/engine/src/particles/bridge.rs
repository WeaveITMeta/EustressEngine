//! Engine side of the `ParticleSimulation` class, everything that needs
//! Avian, the Play-mode state machine, sim values or the Space's files:
//!
//! - **spawn**: attach the class components from their TOML sections;
//! - **hierarchy**: a species always belongs to the simulation whose folder
//!   holds it, whatever order the file watcher delivered the two files in;
//! - **obstacles**: collidable parts inside a domain become solver
//!   obstacles, and unanchored ones receive the particles' momentum;
//! - **Play / Stop**: simulations step only while the physics clock runs
//!   (Play, scripted steps) and show their initial state in Edit. Play
//!   starts every simulation from its authored state; Stop restores the
//!   authored properties and resets the runs, so edits a script or the AI
//!   made during Play never leak into the Space;
//! - **sim values**: every simulation publishes its measurements as
//!   `psim.<Name>.<Stat>` and accepts writes to `psim.<Name>.<Property>`,
//!   which is how Rune scripts, MCP `set_sim_value` and data bindings drive
//!   a run without new plumbing;
//! - **persistence**: section writes that go to the WorldDb and the disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use avian3d::prelude::*;
use bevy::prelude::*;

use eustress_common::classes::{BasePart, ClassName, Instance, Part, PartType};
use eustress_common::realism::particle_sim::class::{
    from_section, FieldKind, FieldTable, FieldValue, SIMULATION_FIELDS, SIMULATION_SECTION, SPECIES_SECTION,
};
use eustress_common::realism::particle_sim::{
    Obstacle, ObstacleShape, ParticleSimAction, ParticleSimCommand, ParticleSimControl, ParticleSimRuntime,
    ParticleSimSet, ParticleSimulation, ParticleSpecies, SIMULATION_STATS, SPECIES_STATS,
};

use crate::play_mode::PlayModeState;
use crate::simulation::plugin::SimValuesResource;
use crate::space::instance_loader::InstanceFile;

// ============================================================================
// Spawn and persistence
// ============================================================================

/// Attach the class component for a `ParticleSimulation` / `ParticleSpecies`
/// instance from its flattened TOML sections. Called by the instance loader
/// for data-only instances; unknown or bad keys keep their defaults and are
/// logged, so a newer or hand-edited file still loads.
pub fn attach_class_component(
    ec: &mut bevy::ecs::system::EntityCommands,
    class_name: ClassName,
    extra: &HashMap<String, toml::Value>,
    source: &Path,
) {
    let section = |name: &str| extra.get(name).and_then(|v| v.as_table());
    match class_name {
        ClassName::ParticleSimulation => {
            let (c, problems): (ParticleSimulation, _) = from_section(section(SIMULATION_SECTION));
            report(source, &problems);
            ec.insert(c);
        }
        ClassName::ParticleSpecies => {
            let (c, problems): (ParticleSpecies, _) = from_section(section(SPECIES_SECTION));
            report(source, &problems);
            ec.insert(c);
        }
        _ => {}
    }
}

fn report(source: &Path, problems: &[String]) {
    for p in problems {
        warn!("{}: {p}", source.display());
    }
}

/// True for the sections these classes own (kept out of the generic
/// Attributes fold, which would duplicate them as user attributes).
pub fn is_class_section(section: &str) -> bool {
    section == SIMULATION_SECTION || section == SPECIES_SECTION
}

/// Write a class section into its instance file: the WorldDb copy when a
/// DB is active (it is authoritative on migrated Spaces) and the disk
/// mirror. Every other section is preserved untouched.
pub fn save_class_section<T: FieldTable>(toml_path: &Path, value: &T) -> Result<(), String> {
    let text = match crate::space::active_db::get_instance_text(toml_path) {
        Some(t) => t,
        None => std::fs::read_to_string(toml_path).map_err(|e| format!("read {}: {e}", toml_path.display()))?,
    };
    let mut doc: toml::Value = text.parse().map_err(|e| format!("parse {}: {e}", toml_path.display()))?;
    let root = doc
        .as_table_mut()
        .ok_or_else(|| format!("{} is not a TOML table", toml_path.display()))?;
    root.insert(T::SECTION.to_string(), toml::Value::Table(value.to_toml_table()));
    let out = toml::to_string_pretty(&doc).map_err(|e| format!("serialize {}: {e}", toml_path.display()))?;
    let db_ok = crate::space::active_db::put_instance_text(toml_path, &out);
    if let Err(e) = crate::space::gui_loader::write_atomic(toml_path, out.as_bytes()) {
        if !db_ok {
            return Err(format!("write {}: {e}", toml_path.display()));
        }
    }
    Ok(())
}

/// Set one field from its text form on the instance whose file is
/// `toml_path`, and persist it unless a Play session is running. The undo
/// stack's `ChangeClassField` replays through here, for the terrain layer
/// classes too (see `terrain_layers::set_field_text`, which always saves).
pub fn apply_class_field_text(world: &mut World, toml_path: &Path, property: &str, text: &str) {
    let mut files = world.query::<(Entity, &InstanceFile)>();
    let Some(entity) = files.iter(world).find(|(_, f)| f.toml_path == toml_path).map(|(e, _)| e) else {
        warn!("undo: no instance loaded from {}", toml_path.display());
        return;
    };
    let playing = world.get_resource::<State<PlayModeState>>().is_some_and(|s| *s.get() != PlayModeState::Editing);
    let saved = if let Some(mut c) = world.get_mut::<ParticleSimulation>(entity) {
        c.set_text(property, text).map(|_| (!playing).then(|| save_class_section(toml_path, &*c)))
    } else if let Some(mut c) = world.get_mut::<ParticleSpecies>(entity) {
        c.set_text(property, text).map(|_| (!playing).then(|| save_class_section(toml_path, &*c)))
    } else if let Some(saved) = crate::terrain_layers::set_field_text(world, entity, toml_path, property, text) {
        saved
    } else {
        return;
    };
    match saved {
        Ok(Some(Err(e))) => warn!("undo {property}: {e}"),
        Err(e) => warn!("undo {property} = {text}: {e}"),
        _ => {}
    }
}

/// Hot reload: re-read the class sections of an instance whose file
/// changed on disk (MCP edits, git checkouts, a text editor). Queued as an
/// entity command so the file watcher needs no new system parameters.
pub fn queue_section_reload(commands: &mut Commands, entity: Entity, toml_text: &str) {
    let Ok(doc) = toml_text.parse::<toml::Value>() else { return };
    let sim = doc.get(SIMULATION_SECTION).and_then(|v| v.as_table()).cloned();
    let species = doc.get(SPECIES_SECTION).and_then(|v| v.as_table()).cloned();
    if sim.is_none() && species.is_none() {
        return;
    }
    commands.entity(entity).queue(move |mut e: EntityWorldMut| {
        if let Some(t) = sim.as_ref() {
            let (fresh, _): (ParticleSimulation, _) = from_section(Some(t));
            if let Some(mut c) = e.get_mut::<ParticleSimulation>() {
                if *c != fresh {
                    *c = fresh;
                }
            }
        }
        if let Some(t) = species.as_ref() {
            let (fresh, _): (ParticleSpecies, _) = from_section(Some(t));
            if let Some(mut c) = e.get_mut::<ParticleSpecies>() {
                if *c != fresh {
                    *c = fresh;
                }
            }
        }
    });
}

// ============================================================================
// Hierarchy
// ============================================================================

/// Parent each species to the simulation whose folder contains it. The
/// watcher parents a new file to whatever its grandparent folder resolved
/// to at that instant; when a species file lands before its simulation's,
/// that is the service root. This repairs it once both exist.
fn adopt_species_by_folder(
    mut commands: Commands,
    new_species: Query<Entity, Added<ParticleSpecies>>,
    new_sims: Query<(), Added<ParticleSimulation>>,
    species: Query<(Entity, &InstanceFile, Option<&ChildOf>), With<ParticleSpecies>>,
    sims: Query<(Entity, &InstanceFile), With<ParticleSimulation>>,
) {
    if new_species.is_empty() && new_sims.is_empty() {
        return;
    }
    let by_folder: HashMap<PathBuf, Entity> = sims
        .iter()
        .filter_map(|(e, f)| f.toml_path.parent().map(|p| (p.to_path_buf(), e)))
        .collect();
    for (entity, file, parent) in &species {
        let Some(sim_dir) = file.toml_path.parent().and_then(|p| p.parent()) else { continue };
        let Some(&sim) = by_folder.get(sim_dir) else { continue };
        if parent.map(|p| p.parent()) != Some(sim) {
            commands.entity(entity).insert(ChildOf(sim));
        }
    }
}

// ============================================================================
// Obstacles and two-way coupling
// ============================================================================

/// Which part each solver obstacle came from, per simulation.
#[derive(Resource, Default)]
pub struct ParticleObstacleLinks(pub HashMap<Entity, Vec<Entity>>);

fn gather_particle_obstacles(
    spatial: SpatialQuery,
    mut links: ResMut<ParticleObstacleLinks>,
    mut sims: Query<(Entity, &ParticleSimulation, &GlobalTransform, &mut ParticleSimRuntime)>,
    parts: Query<(&GlobalTransform, Option<&BasePart>, Option<&Part>, Option<&LinearVelocity>)>,
    children: Query<&Children>,
) {
    links.0.retain(|e, _| sims.contains(*e));
    for (sim_entity, class, global, mut rt) in &mut sims {
        let targets = links.0.entry(sim_entity).or_default();
        targets.clear();
        if !class.collide_with_parts {
            if !rt.sim.obstacles().is_empty() {
                rt.sim.set_obstacles(Vec::new());
            }
            continue;
        }
        let scale = class.display_scale as f32;
        let (_, rotation, translation) = global.to_scale_rotation_translation();
        let world = rt.sim.params.domain_size * scale;
        let probe = Collider::cuboid(world.x, world.y, world.z);
        let own: Vec<Entity> = children.iter_descendants(sim_entity).collect();
        let inverse = rotation.inverse();
        let mut obstacles = Vec::new();
        for hit in spatial.shape_intersections(&probe, translation, rotation, &SpatialQueryFilter::default()) {
            if hit == sim_entity || own.contains(&hit) {
                continue;
            }
            let Ok((part_global, base, part, velocity)) = parts.get(hit) else { continue };
            let (part_scale, part_rotation, part_translation) = part_global.to_scale_rotation_translation();
            let size = base.map(|b| b.size).unwrap_or(part_scale);
            let half = size * 0.5 / scale;
            let (shape, half_extents) = match part.map(|p| p.shape) {
                Some(PartType::Ball) => (ObstacleShape::Sphere, Vec3::splat(half.min_element())),
                // Eustress cylinders run along local Y, as the solver's do.
                Some(PartType::Cylinder) => (ObstacleShape::Cylinder, Vec3::new(half.x.max(half.z), half.y, 0.0)),
                _ => (ObstacleShape::Box, half),
            };
            obstacles.push(Obstacle {
                shape,
                center: inverse * (part_translation - translation) / scale,
                rotation: inverse * part_rotation,
                half_extents,
                velocity: velocity.map(|v| inverse * v.0 / scale).unwrap_or(Vec3::ZERO),
            });
            targets.push(hit);
        }
        rt.sim.set_obstacles(obstacles);
    }
}

fn apply_particle_impulses(
    links: Res<ParticleObstacleLinks>,
    mut sims: Query<(Entity, &ParticleSimulation, &GlobalTransform, &mut ParticleSimRuntime)>,
    bodies: Query<&RigidBody>,
    mut forces: Query<Forces>,
) {
    for (sim_entity, class, global, mut rt) in &mut sims {
        let impulses = rt.sim.take_obstacle_impulses();
        if !class.push_parts {
            continue;
        }
        let Some(targets) = links.0.get(&sim_entity) else { continue };
        let (_, rotation, _) = global.to_scale_rotation_translation();
        // Momentum scales with length (display scale) and angular momentum
        // with length squared; both are 1 at true scale.
        let s = class.display_scale as f32;
        for (imp, &body) in impulses.iter().zip(targets) {
            if !matches!(bodies.get(body), Ok(RigidBody::Dynamic)) {
                continue;
            }
            let Ok(mut f) = forces.get_mut(body) else { continue };
            let linear = rotation * imp.linear * s;
            let angular = rotation * imp.angular * (s * s);
            if linear.is_finite() && angular.is_finite() {
                f.apply_linear_impulse(linear);
                f.apply_angular_impulse(angular);
            }
        }
    }
}

// ============================================================================
// Play / Stop
// ============================================================================

/// Authored properties captured when Play starts.
#[derive(Resource, Default)]
struct ParticlePlaySnapshot {
    taken: bool,
    sims: Vec<(Entity, ParticleSimulation)>,
    species: Vec<(Entity, ParticleSpecies)>,
}

fn snapshot_on_play(
    mut snapshot: ResMut<ParticlePlaySnapshot>,
    sims: Query<(Entity, &ParticleSimulation)>,
    species: Query<(Entity, &ParticleSpecies)>,
    mut commands: MessageWriter<ParticleSimCommand>,
) {
    snapshot.sims = sims.iter().map(|(e, c)| (e, c.clone())).collect();
    snapshot.species = species.iter().map(|(e, c)| (e, c.clone())).collect();
    snapshot.taken = true;
    for (entity, _) in &sims {
        commands.write(ParticleSimCommand { entity, action: ParticleSimAction::Reset });
    }
}

fn restore_on_stop(
    mut snapshot: ResMut<ParticlePlaySnapshot>,
    mut sims: Query<(Entity, &mut ParticleSimulation)>,
    mut species: Query<&mut ParticleSpecies>,
    mut commands: MessageWriter<ParticleSimCommand>,
) {
    if !snapshot.taken {
        return;
    }
    for (entity, authored) in std::mem::take(&mut snapshot.sims) {
        if let Ok((_, mut c)) = sims.get_mut(entity) {
            if *c != authored {
                *c = authored;
            }
        }
    }
    for (entity, authored) in std::mem::take(&mut snapshot.species) {
        if let Ok(mut c) = species.get_mut(entity) {
            if *c != authored {
                *c = authored;
            }
        }
    }
    snapshot.taken = false;
    for (entity, _) in &sims {
        commands.write(ParticleSimCommand { entity, action: ParticleSimAction::Reset });
    }
}

/// Simulations run on the world clock. Avian's physics time is paused in
/// Edit (every simulation shows its initial state) and in Pause, and runs
/// in Play and while a scripted `sim.step` pumps the fixed schedule, so
/// particles and rigid bodies always agree on whether the world is live.
/// Mirrored every fixed tick, right before the simulations step.
fn mirror_physics_clock(physics: Option<Res<Time<Physics>>>, mut control: ResMut<ParticleSimControl>) {
    let paused = physics.is_some_and(|t| t.is_paused());
    if control.paused != paused {
        control.paused = paused;
    }
}

/// True while a Play session is running or paused: edits then apply live
/// but are not written to the Space (Stop restores the authored values).
pub fn is_playing(state: Option<&State<PlayModeState>>) -> bool {
    state.is_some_and(|s| *s.get() != PlayModeState::Editing)
}

// ============================================================================
// Sim values
// ============================================================================

/// What was last published per key, so a value that differs was written by
/// someone else (a script, MCP, a data binding) and should be applied.
#[derive(Resource, Default)]
struct ParticleSimValueLedger {
    published: HashMap<String, f64>,
}

/// Property keys scripts can write: scalars as `psim.<Sim>.<Property>`,
/// vectors per component as `psim.<Sim>.<Property>.x|y|z`.
fn writable_keys(prefix: &str) -> Vec<(String, &'static str, Option<usize>)> {
    let mut out = Vec::new();
    for f in SIMULATION_FIELDS {
        match f.kind {
            FieldKind::Bool | FieldKind::Int | FieldKind::Float => out.push((format!("{prefix}.{}", f.name), f.name, None)),
            FieldKind::Vector3 => {
                for (i, axis) in ["x", "y", "z"].iter().enumerate() {
                    out.push((format!("{prefix}.{}.{axis}", f.name), f.name, Some(i)));
                }
            }
            _ => {}
        }
    }
    out
}

fn value_as_f64(v: &FieldValue, component: Option<usize>) -> Option<f64> {
    match (v, component) {
        (FieldValue::Bool(b), None) => Some(if *b { 1.0 } else { 0.0 }),
        (FieldValue::Int(i), None) => Some(*i as f64),
        (FieldValue::Float(f), None) => Some(*f),
        (FieldValue::Vec3(a), Some(i)) => Some(a[i]),
        _ => None,
    }
}

fn sync_particle_sim_values(
    values: Option<ResMut<SimValuesResource>>,
    mut ledger: ResMut<ParticleSimValueLedger>,
    mut sims: Query<(Entity, &Instance, &mut ParticleSimulation, Option<&ParticleSimRuntime>)>,
    names: Query<&Instance>,
    mut commands: MessageWriter<ParticleSimCommand>,
) {
    let Some(mut values) = values else { return };
    let ledger = &mut *ledger;
    for (entity, instance, mut class, runtime) in &mut sims {
        let prefix = format!("psim.{}", instance.name);

        // Writes from outside since the last publish.
        for (key, name, component) in writable_keys(&prefix) {
            let Some(&written) = values.0.get(&key) else { continue };
            if ledger.published.get(&key) == Some(&written) || !written.is_finite() {
                continue;
            }
            let Some(current) = class.get(name) else { continue };
            let spec = eustress_common::realism::particle_sim::class::field(SIMULATION_FIELDS, name);
            let new = match (current, component, spec.map(|s| s.kind)) {
                (FieldValue::Vec3(mut a), Some(i), _) => {
                    a[i] = written;
                    FieldValue::Vec3(a)
                }
                (_, None, Some(FieldKind::Bool)) => FieldValue::Bool(written != 0.0),
                (_, None, Some(FieldKind::Int)) => FieldValue::Int(written.round() as i64),
                (_, None, Some(FieldKind::Float)) => FieldValue::Float(written),
                _ => continue,
            };
            if class.get(name).as_ref() != Some(&new) {
                if let Err(e) = class.set(name, new) {
                    warn!("{key} = {written}: {e}");
                }
            }
        }
        let reset_key = format!("{prefix}.Reset");
        if values.0.get(&reset_key).is_some_and(|v| *v != 0.0) {
            commands.write(ParticleSimCommand { entity, action: ParticleSimAction::Reset });
        }
        values.0.insert(reset_key.clone(), 0.0);
        ledger.published.insert(reset_key, 0.0);

        // Publish current properties and measurements.
        for (key, name, component) in writable_keys(&prefix) {
            if let Some(v) = class.get(name).and_then(|v| value_as_f64(&v, component)) {
                values.0.insert(key.clone(), v);
                ledger.published.insert(key, v);
            }
        }
        let Some(rt) = runtime else { continue };
        let stats = rt.stats();
        for s in SIMULATION_STATS {
            let key = format!("{prefix}.{}", s.name);
            let v = (s.get)(stats);
            values.0.insert(key.clone(), v);
            ledger.published.insert(key, v);
        }
        for (i, sp) in stats.species.iter().enumerate() {
            let species_name = rt
                .species_entities
                .get(i)
                .and_then(|e| names.get(*e).ok())
                .map(|n| n.name.clone())
                .unwrap_or_else(|| sp.name.clone());
            for s in SPECIES_STATS {
                let key = format!("{prefix}.{species_name}.{}", s.name);
                let v = (s.get)(sp);
                values.0.insert(key.clone(), v);
                ledger.published.insert(key, v);
            }
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Engine integration that must also run headless (no rendering, no UI).
pub struct ParticleSimBridgePlugin;

impl Plugin for ParticleSimBridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParticleObstacleLinks>()
            .init_resource::<ParticlePlaySnapshot>()
            .init_resource::<ParticleSimValueLedger>()
            .add_systems(Update, (adopt_species_by_folder, sync_particle_sim_values))
            .add_systems(
                FixedUpdate,
                (
                    mirror_physics_clock.before(ParticleSimSet::Step),
                    gather_particle_obstacles.in_set(ParticleSimSet::Obstacles),
                    apply_particle_impulses.after(ParticleSimSet::Step),
                ),
            )
            .add_systems(
                OnTransition { exited: PlayModeState::Editing, entered: PlayModeState::Playing },
                snapshot_on_play,
            )
            .add_systems(OnEnter(PlayModeState::Editing), restore_on_stop);
    }
}
