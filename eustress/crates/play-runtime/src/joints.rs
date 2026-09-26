//! # Physics constraints scripts make during Play
//!
//! A script's `Instance.new("HingeConstraint")` (or `PrismaticConstraint`,
//! `CylindricalConstraint`, `SpringConstraint`, `NoCollisionConstraint`,
//! `WeldConstraint`, `Weld`, `ManualWeld`, `VectorForce`) lives in the tree
//! only: [`crate::apply`] spawns no entity for it. This module turns each one
//! into Avian joints between the rigid bodies its two ends belong to, keeps
//! every property the script writes on the joint, and removes the joint when
//! the constraint goes. The contract is `docs/architecture/SCRIPTED_CONSTRAINTS.md`.
//!
//! Constraints loaded from the Space's files are the loader's
//! (`engine::physics::joint_resolver`): a constraint that has an entity of its
//! own is left to it.
//!
//! Joints only act on Dynamic bodies, so they are built where the assembly is
//! simulated: the machine with authority over the session (Studio's Play,
//! hosting or alone). A replica draws the poses it is sent.
//!
//! How a constraint maps onto Avian 0.7:
//!
//! | Class | Joint |
//! |---|---|
//! | `HingeConstraint` | `RevoluteJoint`, hinge on the attachment frame's X |
//! | `PrismaticConstraint` | `PrismaticJoint`, slider on the attachment frame's X |
//! | `CylindricalConstraint` | a `PrismaticJoint` to a physics-only carrier body, and a `RevoluteJoint` from it |
//! | `SpringConstraint` | [`SpringJoint`], its own XPBD constraint, plus a `DistanceJoint` for its limits |
//! | `NoCollisionConstraint` | a slack `DistanceJoint` marked `JointCollisionDisabled` |
//! | `WeldConstraint`, `Weld`, `ManualWeld` | `FixedJoint` (from the parts' poses, or from `C0` and `C1`) |
//! | `VectorForce` | a force applied every physics step ([`apply_scripted_forces`]) |
//!
//! Avian 0.7 facts the mapping respects: a motor's `max_torque`/`max_force` of
//! 0 means NO cap, so a constraint maximum of 0 disables the motor; the
//! acceleration-based model with damping 1 reaches its target speed within a
//! substep when the cap allows, so a Motor neither overshoots nor chatters.

use std::collections::{HashMap, HashSet};

use avian3d::dynamics::joints::EntityConstraint;
use avian3d::dynamics::rigid_body::RigidBodyQueryReadOnlyItem;
use avian3d::dynamics::solver::solver_body::{SolverBody, SolverBodyInertia};
use avian3d::dynamics::solver::xpbd::{
    prepare_xpbd_joint, solve_xpbd_joint, AngularConstraint, PositionConstraint,
    XpbdConstraint, XpbdConstraintSolverData, XpbdSolverSystems,
};
use avian3d::dynamics::solver::joint_graph::JointGraphPlugin;
use avian3d::prelude::*;
use bevy::ecs::entity::{EntityMapper, MapEntities};
use bevy::math::{Affine3A, Isometry3d};
use bevy::prelude::*;

use eustress_common::avatar::spawn::AvatarBody;
use eustress_common::datamodel::{DataModel, DmEvent, DmValue, InstanceId};
use eustress_common::play_session::{DataModelSpawned, PlayDataModel, PlayScriptSet};

/// Classes this module builds.
const CLASSES: &[&str] = &[
    "HingeConstraint",
    "PrismaticConstraint",
    "CylindricalConstraint",
    "SpringConstraint",
    "NoCollisionConstraint",
    "WeldConstraint",
    "Weld",
    "ManualWeld",
    "VectorForce",
];

/// A servo's stiffness: Avian's spring-damper motor at this frequency, critically damped.
const SERVO_HZ: f32 = 8.0;
/// The least mass of a cylindrical constraint's physics-only carrier body, kg.
const CARRIER_MASS: f32 = 1.0;

/// A cylindrical constraint's carrier: a tenth of the lighter body's mass, and
/// as much inertia as the body that turns on it. A carrier far lighter than
/// that body takes a motor's impulse itself, and its rotation lock then undoes
/// it, so the motor barely turns the body.
fn carrier_mass_properties(world: &World, b0: Entity, b1: Entity) -> (f32, f32) {
    // A body's mass and its largest principal inertia: Avian's once it has
    // computed them, else its part's shape (a part spawned in this same pass
    // has no computed figures until the next physics step).
    let weigh = |b: Entity| -> Option<(f32, f32)> {
        let mass = world.get::<ComputedMass>(b).map(|m| m.value()).filter(|m| m.is_finite() && *m > 0.0);
        let inertia = world
            .get::<ComputedAngularInertia>(b)
            .map(|i| i.principal_angular_inertia_with_local_frame().0.max_element())
            .filter(|i| i.is_finite() && *i > 0.0);
        mass.zip(inertia).or_else(|| {
            let base = world.get::<eustress_common::classes::BasePart>(b)?;
            let shape = world.get::<eustress_common::classes::Part>(b).map_or(eustress_common::classes::PartType::Block, |p| p.shape);
            let (m, i) = crate::part_mass::shape_mass(shape, base.size, base.effective_density());
            Some((m, i.max_element()))
        })
    };
    let (w0, w1) = (weigh(b0), weigh(b1));
    let lighter = match (w0, w1) {
        (Some(a), Some(b)) => Some(a.0.min(b.0)),
        (a, b) => a.or(b).map(|w| w.0),
    };
    let turning = w1.map(|w| w.1).filter(|i| i.is_finite() && *i > 0.0);
    (lighter.filter(|m| m.is_finite()).map_or(CARRIER_MASS, |m| 0.1 * m).max(CARRIER_MASS), turning.unwrap_or(0.1).max(0.1))
}

/// Registers the scripted-constraint systems and the spring constraint.
pub struct ScriptedJointsPlugin;

impl Plugin for ScriptedJointsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptedJoints>()
            .init_resource::<ScriptedForces>()
            // A joint's non-colliding part (a strut, a knuckle) must weigh.
            .add_plugins(crate::part_mass::PartMassPlugin)
            .register_required_components::<SpringJoint, SpringJointSolverData>()
            .add_plugins(JointGraphPlugin::<SpringJoint>::default())
            .add_systems(PhysicsSchedule, prepare_xpbd_joint::<SpringJoint>.in_set(SolverSystems::PrepareJoints))
            .add_systems(SubstepSchedule, solve_xpbd_joint::<SpringJoint>.in_set(XpbdSolverSystems::SolveUserConstraints))
            .add_systems(
                Update,
                sync_scripted_joints.in_set(PlayScriptSet::Apply).after(crate::apply::apply_frame),
            )
            .add_systems(FixedUpdate, apply_scripted_forces);
    }
}

// ── The spring ──────────────────────────────────────────────────────────

/// A spring and damper between two anchor points, solved every substep as a
/// compliant XPBD distance constraint (compliance `1 / stiffness`) with XPBD's
/// damping term, so it is stable however stiff it is and however light the
/// bodies. Force along the line between the anchors:
/// `stiffness * (free_length - length) - damping * d(length)/dt`.
///
/// The damping acts on how far the spring's length actually moved since the
/// last substep, never on the bodies' velocities: Avian relaxes those before
/// its joint pass, so they differ from the motion the positions made, and
/// damping them pumps energy into the spring when a wheel lands.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct SpringJoint {
    pub body1: Entity,
    pub body2: Entity,
    /// Anchors relative to each body's origin.
    pub local_anchor1: Vec3,
    pub local_anchor2: Vec3,
    pub free_length: f32,
    /// N/m; 0 disables the spring.
    pub stiffness: f32,
    /// N·s/m.
    pub damping: f32,
}

/// The spring's per-step data.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct SpringJointSolverData {
    world_r1: Vec3,
    world_r2: Vec3,
    center_difference: Vec3,
    total_lagrange: Vec3,
    /// The spring's length after the last substep's correction (at the step's
    /// start, its length then): the damping acts on the change from it.
    last_length: f32,
}

impl XpbdConstraintSolverData for SpringJointSolverData {
    fn clear_lagrange_multipliers(&mut self) {
        self.total_lagrange = Vec3::ZERO;
    }

    fn total_position_lagrange(&self) -> Vec3 {
        self.total_lagrange
    }
}

impl EntityConstraint<2> for SpringJoint {
    fn entities(&self) -> [Entity; 2] {
        [self.body1, self.body2]
    }
}

impl MapEntities for SpringJoint {
    fn map_entities<M: EntityMapper>(&mut self, entity_mapper: &mut M) {
        self.body1 = entity_mapper.get_mapped(self.body1);
        self.body2 = entity_mapper.get_mapped(self.body2);
    }
}

impl XpbdConstraint<2> for SpringJoint {
    type SolverData = SpringJointSolverData;

    fn prepare(&mut self, bodies: [&RigidBodyQueryReadOnlyItem; 2], solver_data: &mut SpringJointSolverData) {
        let [body1, body2] = bodies;
        solver_data.world_r1 = body1.rotation * (self.local_anchor1 - body1.center_of_mass.0);
        solver_data.world_r2 = body2.rotation * (self.local_anchor2 - body2.center_of_mass.0);
        solver_data.center_difference = (body2.position.0 - body1.position.0)
            + (body2.rotation * body2.center_of_mass.0 - body1.rotation * body1.center_of_mass.0);
        solver_data.last_length = (solver_data.center_difference + solver_data.world_r2 - solver_data.world_r1).length();
    }

    fn solve(
        &mut self,
        bodies: [&mut SolverBody; 2],
        inertias: [&SolverBodyInertia; 2],
        solver_data: &mut SpringJointSolverData,
        dt: f32,
    ) {
        let [body1, body2] = bodies;
        let [inertia1, inertia2] = inertias;

        let world_r1 = body1.delta_rotation * solver_data.world_r1;
        let world_r2 = body2.delta_rotation * solver_data.world_r2;
        let separation = (body2.delta_position - body1.delta_position) + (world_r2 - world_r1) + solver_data.center_difference;
        let length = separation.length();
        // An idle spring still tracks its length, so one switched on mid-step
        // damps only the motion after it.
        if self.stiffness <= 0.0 || dt <= 0.0 || length <= f32::EPSILON {
            solver_data.last_length = length;
            return;
        }
        // Avian's convention (as its DistanceJoint): the correction direction
        // points from body 2 toward body 1, and an impulse is added to body 1
        // and subtracted from body 2.
        let dir = -separation / length;

        let w1 = PositionConstraint::compute_generalized_inverse_mass(
            self,
            inertia1.effective_inv_mass().max_element(),
            inertia1.effective_inv_angular_inertia(),
            world_r1,
            dir,
        );
        let w2 = PositionConstraint::compute_generalized_inverse_mass(
            self,
            inertia2.effective_inv_mass().max_element(),
            inertia2.effective_inv_angular_inertia(),
            world_r2,
            dir,
        );
        let w_sum = w1 + w2;
        if w_sum <= f32::EPSILON {
            return;
        }

        // C = stretch; compliance alpha = 1 / k; XPBD damping gamma = alpha * c / dt,
        // applied to how far the length moved since the last substep's correction.
        let c = length - self.free_length;
        let compliance = 1.0 / self.stiffness;
        let tilde = compliance / (dt * dt);
        let gamma = compliance * self.damping / dt;
        let length_change = length - solver_data.last_length;
        let delta_lagrange = (-c - gamma * length_change) / ((1.0 + gamma) * w_sum + tilde);
        let impulse = delta_lagrange * dir;
        solver_data.total_lagrange += impulse;
        // A positive multiplier lengthens the spring by w_sum per unit.
        solver_data.last_length = length + delta_lagrange * w_sum;
        self.apply_positional_impulse(body1, body2, inertia1, inertia2, impulse, world_r1, world_r2);
    }
}

impl PositionConstraint for SpringJoint {}

impl AngularConstraint for SpringJoint {}

// ── Bookkeeping ─────────────────────────────────────────────────────────

/// Every scripted constraint this machine tracks, and what was built for it.
#[derive(Resource, Default)]
pub struct ScriptedJoints {
    /// The session these belong to (the tree's address); a new session starts clean.
    session: usize,
    cursor: u64,
    tracked: HashMap<InstanceId, Tracked>,
    /// An Attachment or part a tracked constraint depends on -> those constraints.
    dependents: HashMap<InstanceId, HashSet<InstanceId>>,
}

struct Tracked {
    class: &'static str,
    /// Needs building again (created, or a property, reference or end changed).
    stale: bool,
    built: Option<Built>,
    /// A servo's moving setpoint (radians, or metres for a linear servo).
    servo_angle: f32,
    servo_linear: f32,
}

struct Built {
    /// Everything spawned for the constraint (joints and carrier bodies).
    entities: Vec<Entity>,
    /// The joint whose motor, limits and reads track the constraint's properties.
    primary: Entity,
    /// A cylindrical constraint's revolute joint.
    secondary: Option<Entity>,
    body0: Entity,
    body1: Entity,
    frame0: Isometry3d,
    frame1: Isometry3d,
    /// Bodies this build switched sleeping off for.
    no_sleep: Vec<Entity>,
}

/// The VectorForces to apply this physics step.
#[derive(Resource, Default)]
pub struct ScriptedForces {
    forces: Vec<ScriptedForce>,
}

#[derive(Clone, Copy)]
struct ScriptedForce {
    body: Entity,
    /// The force, in world axes or (when `local`) in the attachment's axes.
    force: Vec3,
    local: bool,
    /// The attachment frame in the body's space.
    frame: Isometry3d,
    at_center_of_mass: bool,
}

// ── Reading the tree ────────────────────────────────────────────────────

fn num(g: &DataModel, id: InstanceId, prop: &str) -> f32 {
    g.get_prop(id, prop).and_then(|v| v.as_number()).unwrap_or(0.0) as f32
}

fn flag(g: &DataModel, id: InstanceId, prop: &str) -> bool {
    g.get_prop(id, prop).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn reference(g: &DataModel, id: InstanceId, prop: &str) -> Option<InstanceId> {
    match g.get_prop(id, prop) {
        Some(DmValue::Instance(other)) if g.exists(other) => Some(other),
        _ => None,
    }
}

fn enum_name(g: &DataModel, id: InstanceId, prop: &str) -> String {
    match g.get_prop(id, prop) {
        Some(DmValue::Enum(e)) => e.name,
        Some(DmValue::String(s)) => s,
        _ => String::new(),
    }
}

fn cframe(g: &DataModel, id: InstanceId, prop: &str) -> Isometry3d {
    let t = g.get_prop(id, prop).and_then(|v| v.as_cframe()).unwrap_or_default().to_transform();
    Isometry3d::new(t.translation, t.rotation)
}

/// An entity's world affine, composed from its own and its ancestors' Transforms
/// (correct the frame an entity spawns, before propagation has run).
fn world_affine(world: &World, e: Entity) -> Option<Affine3A> {
    let mut affine = world.get::<Transform>(e)?.compute_affine();
    let mut node = e;
    while let Some(parent) = world.get::<ChildOf>(node).map(|c| c.parent()) {
        if let Some(t) = world.get::<Transform>(parent) {
            affine = t.compute_affine() * affine;
        }
        node = parent;
    }
    Some(affine)
}

/// An entity's world pose (position and rotation; scale dropped).
fn world_pose(world: &World, e: Entity) -> Option<Isometry3d> {
    let (_, rotation, translation) = world_affine(world, e)?.to_scale_rotation_translation();
    Some(Isometry3d::new(translation, rotation))
}

/// The rigid body an entity moves with: itself, or its nearest ancestor with a body.
fn body_of(world: &World, e: Entity) -> Option<Entity> {
    let mut node = Some(e);
    while let Some(n) = node {
        if world.get::<RigidBody>(n).is_some() {
            return Some(n);
        }
        node = world.get::<ChildOf>(n).map(|c| c.parent());
    }
    None
}

/// A part's entity, when the part exists and is drawn.
fn part_entity(g: &DataModel, world: &World, part: InstanceId) -> Option<Entity> {
    let e = Entity::from_bits(g.entity_of(part)?);
    world.get_entity(e).ok().map(|_| e)
}

/// One end of a constraint: the body it moves with and the end's frame in that body.
struct End {
    part: InstanceId,
    body: Entity,
    frame: Isometry3d,
}

/// An Attachment end: its part-local CFrame times the part's pose, expressed in
/// the part's body.
fn attachment_end(g: &DataModel, world: &World, attachment: InstanceId) -> Option<End> {
    if g.class_of(attachment) != Some("Attachment") {
        return None;
    }
    let part = g.parent(attachment)?;
    let entity = part_entity(g, world, part)?;
    let body = body_of(world, entity)?;
    let att_world = world_pose(world, entity)? * cframe(g, attachment, "CFrame");
    let frame = world_pose(world, body)?.inverse() * att_world;
    Some(End { part, body, frame })
}

/// A part end, with the part's own pose (times `offset`) as the frame.
fn part_end(g: &DataModel, world: &World, part: InstanceId, offset: Isometry3d) -> Option<End> {
    let entity = part_entity(g, world, part)?;
    let body = body_of(world, entity)?;
    let frame = world_pose(world, body)?.inverse() * world_pose(world, entity)? * offset;
    Some(End { part, body, frame })
}

/// A WeldConstraint's `C1` when its `C0` is identity: the frame in Part1 that
/// sits where Part0 is, so the weld holds the pose it was made in.
fn held_pose_c1(part0: Isometry3d, part1: Isometry3d) -> Isometry3d {
    part1.inverse() * part0
}

/// A weld between a seat and a character (the SeatWeld a seat makes, or any
/// weld of the same two): a record for scripts, never a joint. The avatar is a
/// kinematic character, and a joint would drag the vehicle by its controller;
/// riding carries the character. A weld of anything else to a character (a hat,
/// a backpack) is an ordinary joint.
fn is_seat_weld(g: &DataModel, world: &World, p0: InstanceId, p1: InstanceId) -> bool {
    let seat = |p: InstanceId| matches!(g.class_of(p), Some("Seat" | "VehicleSeat"));
    let rider = |p: InstanceId| is_character_root(g, p) || is_avatar_part(g, world, p);
    (seat(p0) && rider(p1)) || (seat(p1) && rider(p0))
}

/// A character's root by the tree: a `HumanoidRootPart` in a Model holding a
/// Humanoid. It holds on a machine where the root is not bound to its avatar
/// yet (a Player receiving a replica's SeatWeld before the replica spawns).
fn is_character_root(g: &DataModel, part: InstanceId) -> bool {
    g.name_of(part) == Some("HumanoidRootPart")
        && g.parent(part).is_some_and(|model| g.children(model).iter().any(|c| g.class_of(*c) == Some("Humanoid")))
}

/// A part bound to an avatar.
fn is_avatar_part(g: &DataModel, world: &World, part: InstanceId) -> bool {
    part_entity(g, world, part).is_some_and(|e| world.get::<AvatarBody>(e).is_some())
}

// ── Motors ──────────────────────────────────────────────────────────────

fn velocity_model() -> MotorModel {
    MotorModel::AccelerationBased { stiffness: 0.0, damping: 1.0 }
}

fn servo_model() -> MotorModel {
    MotorModel::SpringDamper { frequency: SERVO_HZ, damping_ratio: 1.0 }
}

/// Moves `current` toward `target` by at most `step`.
fn approach(current: f32, target: f32, step: f32) -> f32 {
    if (target - current).abs() <= step { target } else { current + step * (target - current).signum() }
}

/// An angular actuator: `kind` is the ActuatorType name. Returns the motor and
/// the servo setpoint it used.
fn angular_motor(kind: &str, velocity: f32, motor_torque: f32, target: f32, speed: f32, servo_torque: f32, setpoint: f32, dt: f32) -> (AngularMotor, f32) {
    match kind {
        "Motor" if motor_torque > 0.0 => (
            AngularMotor::new(velocity_model()).with_target_velocity(velocity).with_max_torque(motor_torque),
            setpoint,
        ),
        "Servo" if servo_torque > 0.0 => {
            let s = if speed > 0.0 { approach(setpoint, target, speed * dt) } else { target };
            (AngularMotor::new(servo_model()).with_target_position(s).with_max_torque(servo_torque), s)
        }
        _ => (AngularMotor::new_disabled(velocity_model()), setpoint),
    }
}

/// A linear actuator, the same way.
fn linear_motor(kind: &str, velocity: f32, motor_force: f32, target: f32, speed: f32, servo_force: f32, setpoint: f32, dt: f32) -> (LinearMotor, f32) {
    match kind {
        "Motor" if motor_force > 0.0 => (
            LinearMotor::new(velocity_model()).with_target_velocity(velocity).with_max_force(motor_force),
            setpoint,
        ),
        "Servo" if servo_force > 0.0 => {
            let s = if speed > 0.0 { approach(setpoint, target, speed * dt) } else { target };
            (LinearMotor::new(servo_model()).with_target_position(s).with_max_force(servo_force), s)
        }
        _ => (LinearMotor::new_disabled(velocity_model()), setpoint),
    }
}

fn angle_limit(g: &DataModel, id: InstanceId, enabled: &str) -> Option<AngleLimit> {
    flag(g, id, enabled).then(|| {
        let (a, b) = (num(g, id, "LowerAngle").to_radians(), num(g, id, "UpperAngle").to_radians());
        AngleLimit::new(a.min(b), a.max(b))
    })
}

fn distance_limit(g: &DataModel, id: InstanceId) -> Option<(f32, f32)> {
    flag(g, id, "LimitsEnabled").then(|| {
        let (a, b) = (num(g, id, "LowerLimit"), num(g, id, "UpperLimit"));
        (a.min(b), a.max(b))
    })
}

// ── The frame's work ────────────────────────────────────────────────────

/// Once per frame after the tree is drawn: picks up constraints scripts made,
/// builds and rebuilds their joints, keeps their motors on the script's
/// values, and reports `CurrentAngle`, `CurrentPosition` and `CurrentLength`.
pub fn sync_scripted_joints(world: &mut World) {
    let Some(dm) = world.get_resource::<PlayDataModel>().map(|r| r.dm.clone()) else { return };
    let dt = world.resource::<Time>().delta_secs();
    let mut state = world.remove_resource::<ScriptedJoints>().unwrap_or_default();
    let session = std::sync::Arc::as_ptr(&dm) as usize;
    if state.session != session {
        // A new Play session: last session's joints went with its Stop.
        state = ScriptedJoints { session, ..default() };
    }
    let mut forces = Vec::new();
    {
        let mut g = dm.lock();
        read_events(&mut g, &mut state);

        let ids: Vec<InstanceId> = state.tracked.keys().copied().collect();
        for id in ids {
            if !g.exists(id) {
                if let Some(t) = state.tracked.remove(&id) {
                    tear_down(world, t.built);
                }
                continue;
            }
            let stale = state.tracked.get(&id).is_some_and(|t| t.stale || t.built.is_none());
            if stale {
                let built = state.tracked.get_mut(&id).and_then(|t| t.built.take());
                tear_down(world, built);
                let class = state.tracked[&id].class;
                let built = build(&mut g, world, id, class, &mut state.dependents);
                if let Some(t) = state.tracked.get_mut(&id) {
                    t.built = built;
                    t.stale = false;
                }
            }
            if let Some(t) = state.tracked.get_mut(&id) {
                update(&mut g, world, id, t, dt, &mut forces);
            }
        }
    }
    world.resource_mut::<ScriptedForces>().forces = forces;
    world.insert_resource(state);
}

/// The tree's events since last frame: new constraints, their writes, and
/// changes to the ends they hang from.
fn read_events(g: &mut DataModel, state: &mut ScriptedJoints) {
    let (events, cursor) = g.events_since(state.cursor);
    state.cursor = cursor;
    for event in events {
        match event {
            DmEvent::ChildAdded { child: id, .. } | DmEvent::DescendantAdded { descendant: id, .. } => {
                if state.tracked.contains_key(&id) {
                    mark(state, id);
                    continue;
                }
                let Some(class) = g.class_of(id).and_then(|c| CLASSES.iter().copied().find(|k| *k == c)) else {
                    // An Attachment or part a constraint waits on may have just arrived.
                    mark(state, id);
                    continue;
                };
                // A constraint with an entity of its own came from the Space's
                // files, and the loader's joint resolver owns it.
                if g.entity_of(id).is_some() {
                    continue;
                }
                g.watch_changes(id);
                state.tracked.insert(id, Tracked { class, stale: true, built: None, servo_angle: 0.0, servo_linear: 0.0 });
            }
            DmEvent::Changed { id, prop } => {
                if state.tracked.contains_key(&id) {
                    // Motor, servo, spring and force values are read every
                    // frame; anything else (ends, limits, Enabled) rebuilds.
                    let live = matches!(
                        prop.as_str(),
                        "AngularVelocity" | "MotorMaxTorque" | "MotorMaxAcceleration" | "TargetAngle" | "AngularSpeed"
                            | "ServoMaxTorque" | "Velocity" | "MotorMaxForce" | "TargetPosition" | "Speed"
                            | "ServoMaxForce" | "Stiffness" | "Damping" | "FreeLength" | "Force" | "CurrentAngle"
                            | "CurrentPosition" | "CurrentLength" | "Visible" | "Color"
                    );
                    if !live {
                        mark(state, id);
                    }
                } else if g.class_of(id) == Some("Attachment") && matches!(prop.as_str(), "CFrame" | "Position" | "Orientation") {
                    // An end moved on its part. Parts are never watched here:
                    // physics moves them every frame, and a joint's frames are
                    // relative to its bodies.
                    mark(state, id);
                }
            }
            DmEvent::AncestryChanged { id, .. } | DmEvent::Destroying { id } => mark(state, id),
            _ => {}
        }
    }
}

/// Flags a constraint, or every constraint that hangs from an end, for rebuilding.
fn mark(state: &mut ScriptedJoints, id: InstanceId) {
    if let Some(t) = state.tracked.get_mut(&id) {
        t.stale = true;
    }
    if let Some(deps) = state.dependents.get(&id) {
        for d in deps.clone() {
            if let Some(t) = state.tracked.get_mut(&d) {
                t.stale = true;
            }
        }
    }
}

/// Removes what a build spawned.
fn tear_down(world: &mut World, built: Option<Built>) {
    let Some(built) = built else { return };
    for e in built.entities {
        if let Ok(ec) = world.get_entity_mut(e) {
            ec.despawn();
        }
    }
    for e in built.no_sleep {
        if let Ok(mut ec) = world.get_entity_mut(e) {
            ec.remove::<SleepingDisabled>();
        }
    }
}

fn spawn_joint(world: &mut World, bundle: impl Bundle) -> Entity {
    world.spawn((Name::new("ScriptedJoint"), DataModelSpawned, bundle)).id()
}

/// Builds a constraint's joints, or nothing while it cannot act (disabled, an
/// end missing, both ends on one body, or a SeatWeld).
fn build(g: &mut DataModel, world: &mut World, id: InstanceId, class: &'static str, dependents: &mut HashMap<InstanceId, HashSet<InstanceId>>) -> Option<Built> {
    if !g.in_workspace(id) || g.get_prop(id, "Enabled").and_then(|v| v.as_bool()) == Some(false) {
        return None;
    }
    let by_parts = matches!(class, "NoCollisionConstraint" | "WeldConstraint" | "Weld" | "ManualWeld");
    let (end0, end1) = if by_parts {
        let (p0, p1) = (reference(g, id, "Part0")?, reference(g, id, "Part1")?);
        for p in [p0, p1] {
            dependents.entry(p).or_default().insert(id);
        }
        if class != "NoCollisionConstraint" && is_seat_weld(g, world, p0, p1) {
            // A record for scripts; riding carries the character. Checked on
            // every retry, so it never becomes a joint once the root is bound.
            return None;
        }
        let (c0, c1) = match class {
            "Weld" | "ManualWeld" => (cframe(g, id, "C0"), cframe(g, id, "C1")),
            // A WeldConstraint holds the pose it binds in: Part1 * C1 is where
            // Part0 is now. Identity frames would pull the two parts onto one
            // pose.
            "WeldConstraint" => {
                let pose = |p: InstanceId| part_entity(g, world, p).and_then(|e| world_pose(world, e));
                (Isometry3d::IDENTITY, held_pose_c1(pose(p0)?, pose(p1)?))
            }
            _ => (Isometry3d::IDENTITY, Isometry3d::IDENTITY),
        };
        (part_end(g, world, p0, c0)?, part_end(g, world, p1, c1)?)
    } else if class == "VectorForce" {
        let a0 = reference(g, id, "Attachment0")?;
        dependents.entry(a0).or_default().insert(id);
        g.watch_changes(a0);
        let end = attachment_end(g, world, a0)?;
        return Some(Built {
            entities: Vec::new(),
            primary: end.body,
            secondary: None,
            body0: end.body,
            body1: end.body,
            frame0: end.frame,
            frame1: end.frame,
            no_sleep: Vec::new(),
        });
    } else {
        let (a0, a1) = (reference(g, id, "Attachment0")?, reference(g, id, "Attachment1")?);
        for a in [a0, a1] {
            dependents.entry(a).or_default().insert(id);
            g.watch_changes(a);
            if let Some(p) = g.parent(a) {
                dependents.entry(p).or_default().insert(id);
            }
        }
        (attachment_end(g, world, a0)?, attachment_end(g, world, a1)?)
    };
    if end0.body == end1.body || end0.part == end1.part {
        return None;
    }
    let (b0, b1, f0, f1) = (end0.body, end1.body, end0.frame, end1.frame);

    let mut entities = Vec::new();
    let mut secondary = None;
    let primary = match class {
        "HingeConstraint" => {
            let joint = RevoluteJoint::new(b0, b1).with_local_frame1(f0).with_local_frame2(f1).with_hinge_axis(Vec3::X);
            spawn_joint(world, joint)
        }
        "PrismaticConstraint" => {
            let joint = PrismaticJoint::new(b0, b1).with_local_frame1(f0).with_local_frame2(f1).with_slider_axis(Vec3::X);
            spawn_joint(world, joint)
        }
        "CylindricalConstraint" => {
            // The carrier sits on Attachment0's axis, level with Attachment1.
            let pose0 = world_pose(world, b0)? * f0;
            let a1_world = world_pose(world, b1)?.transform_point(f1.translation);
            let axis = pose0.rotation * Vec3::X;
            let along = (Vec3::from(a1_world) - Vec3::from(pose0.translation)).dot(axis);
            let at = Vec3::from(pose0.translation) + axis * along;
            let (carrier_mass, carrier_inertia) = carrier_mass_properties(world, b0, b1);
            let carrier = world
                .spawn((
                    Name::new("ScriptedJointCarrier"),
                    DataModelSpawned,
                    RigidBody::Dynamic,
                    Transform::from_translation(at).with_rotation(pose0.rotation),
                    Position(at),
                    Rotation(pose0.rotation),
                    Mass(carrier_mass),
                    NoAutoMass,
                    AngularInertia::new(Vec3::splat(carrier_inertia)),
                    NoAutoAngularInertia,
                    CenterOfMass(Vec3::ZERO),
                    NoAutoCenterOfMass,
                ))
                .id();
            entities.push(carrier);
            let slide = spawn_joint(
                world,
                PrismaticJoint::new(b0, carrier)
                    .with_local_frame1(f0)
                    .with_local_frame2(Isometry3d::IDENTITY)
                    .with_slider_axis(Vec3::X),
            );
            let turn = spawn_joint(
                world,
                RevoluteJoint::new(carrier, b1)
                    .with_local_frame1(Isometry3d::IDENTITY)
                    .with_local_frame2(f1)
                    .with_hinge_axis(Vec3::X),
            );
            entities.push(turn);
            secondary = Some(turn);
            slide
        }
        "SpringConstraint" => {
            let spring = SpringJoint {
                body1: b0,
                body2: b1,
                local_anchor1: Vec3::from(f0.translation),
                local_anchor2: Vec3::from(f1.translation),
                free_length: num(g, id, "FreeLength"),
                stiffness: num(g, id, "Stiffness"),
                damping: num(g, id, "Damping"),
            };
            let joint = spawn_joint(world, spring);
            if flag(g, id, "LimitsEnabled") {
                let (a, b) = (num(g, id, "MinLength"), num(g, id, "MaxLength"));
                let limits = spawn_joint(
                    world,
                    DistanceJoint::new(b0, b1)
                        .with_local_anchor1(Vec3::from(f0.translation))
                        .with_local_anchor2(Vec3::from(f1.translation))
                        .with_limits(a.min(b).max(0.0), a.max(b)),
                );
                entities.push(limits);
            }
            joint
        }
        "NoCollisionConstraint" => spawn_joint(world, (DistanceJoint::new(b0, b1).with_limits(0.0, 1.0e4), JointCollisionDisabled)),
        // Welds: Part0 * C0 == Part1 * C1 (a WeldConstraint holds the pose it
        // was made in); welded parts never touch each other.
        _ => spawn_joint(world, (FixedJoint::new(b0, b1).with_local_frame1(f0).with_local_frame2(f1), JointCollisionDisabled)),
    };
    entities.push(primary);

    // A motor's target changes must reach bodies that would otherwise sleep.
    let mut no_sleep = Vec::new();
    if matches!(class, "HingeConstraint" | "PrismaticConstraint" | "CylindricalConstraint" | "SpringConstraint") {
        for b in [b0, b1] {
            if world.get::<SleepingDisabled>(b).is_none() {
                if let Ok(mut ec) = world.get_entity_mut(b) {
                    ec.insert(SleepingDisabled);
                    no_sleep.push(b);
                }
            }
        }
    }
    Some(Built { entities, primary, secondary, body0: b0, body1: b1, frame0: f0, frame1: f1, no_sleep })
}

/// Every frame: the script's motor, servo, spring and force values onto the
/// joints, and the joints' current state back into the tree.
fn update(g: &mut DataModel, world: &mut World, id: InstanceId, t: &mut Tracked, dt: f32, forces: &mut Vec<ScriptedForce>) {
    let Some(built) = t.built.as_ref() else { return };
    match t.class {
        "HingeConstraint" => {
            let kind = enum_name(g, id, "ActuatorType");
            let (motor, s) = angular_motor(
                &kind,
                num(g, id, "AngularVelocity"),
                num(g, id, "MotorMaxTorque"),
                num(g, id, "TargetAngle").to_radians(),
                num(g, id, "AngularSpeed"),
                num(g, id, "ServoMaxTorque"),
                t.servo_angle,
                dt,
            );
            t.servo_angle = s;
            let limit = angle_limit(g, id, "LimitsEnabled");
            if let Some(mut joint) = world.get_mut::<RevoluteJoint>(built.primary) {
                if joint.motor != motor {
                    joint.motor = motor;
                }
                if joint.angle_limit != limit {
                    joint.angle_limit = limit;
                }
            }
            let angle = hinge_angle(world, built);
            g.set_prop_from_engine(id, "CurrentAngle", DmValue::Number(angle.to_degrees() as f64));
        }
        "PrismaticConstraint" | "CylindricalConstraint" => {
            let kind = enum_name(g, id, "ActuatorType");
            let (motor, s) = linear_motor(
                &kind,
                num(g, id, "Velocity"),
                num(g, id, "MotorMaxForce"),
                num(g, id, "TargetPosition"),
                num(g, id, "Speed"),
                num(g, id, "ServoMaxForce"),
                t.servo_linear,
                dt,
            );
            t.servo_linear = s;
            let limits = distance_limit(g, id).map(|(a, b)| DistanceLimit::new(a, b));
            if let Some(mut joint) = world.get_mut::<PrismaticJoint>(built.primary) {
                if joint.motor != motor {
                    joint.motor = motor;
                }
                if joint.limits != limits {
                    joint.limits = limits;
                }
            }
            if let Some(turn) = built.secondary {
                let kind = enum_name(g, id, "AngularActuatorType");
                let (motor, s) = angular_motor(
                    &kind,
                    num(g, id, "AngularVelocity"),
                    num(g, id, "MotorMaxTorque"),
                    num(g, id, "TargetAngle").to_radians(),
                    num(g, id, "AngularSpeed"),
                    num(g, id, "ServoMaxTorque"),
                    t.servo_angle,
                    dt,
                );
                t.servo_angle = s;
                let limit = angle_limit(g, id, "AngularLimitsEnabled");
                if let Some(mut joint) = world.get_mut::<RevoluteJoint>(turn) {
                    if joint.motor != motor {
                        joint.motor = motor;
                    }
                    if joint.angle_limit != limit {
                        joint.angle_limit = limit;
                    }
                }
            }
            if let (Some(p0), Some(p1)) = (world_pose(world, built.body0), world_pose(world, built.body1)) {
                let a0 = p0 * built.frame0;
                let a1 = p1.transform_point(built.frame1.translation);
                let along = (Vec3::from(a1) - Vec3::from(a0.translation)).dot(a0.rotation * Vec3::X);
                g.set_prop_from_engine(id, "CurrentPosition", DmValue::Number(along as f64));
            }
        }
        "SpringConstraint" => {
            let (free_length, stiffness, damping) = (num(g, id, "FreeLength"), num(g, id, "Stiffness"), num(g, id, "Damping"));
            if let Some(mut spring) = world.get_mut::<SpringJoint>(built.primary) {
                if spring.free_length != free_length || spring.stiffness != stiffness || spring.damping != damping {
                    spring.free_length = free_length;
                    spring.stiffness = stiffness;
                    spring.damping = damping;
                }
            }
            if let (Some(p0), Some(p1)) = (world_pose(world, built.body0), world_pose(world, built.body1)) {
                let length = (Vec3::from(p1.transform_point(built.frame1.translation))
                    - Vec3::from(p0.transform_point(built.frame0.translation)))
                .length();
                g.set_prop_from_engine(id, "CurrentLength", DmValue::Number(length as f64));
            }
        }
        "VectorForce" => {
            let force = g.get_prop(id, "Force").and_then(|v| v.as_vector3()).map(|v| v.to_vec3()).unwrap_or(Vec3::ZERO);
            let relative = enum_name(g, id, "RelativeTo");
            forces.push(ScriptedForce {
                body: built.body0,
                force,
                local: relative != "World",
                frame: built.frame0,
                at_center_of_mass: flag(g, id, "ApplyAtCenterOfMass"),
            });
        }
        _ => {}
    }
}

/// A hinge's angle: Attachment1's frame turned about Attachment0's X axis, radians.
fn hinge_angle(world: &World, built: &Built) -> f32 {
    let (Some(p0), Some(p1)) = (world_pose(world, built.body0), world_pose(world, built.body1)) else { return 0.0 };
    let r0 = p0.rotation * built.frame0.rotation;
    let r1 = p1.rotation * built.frame1.rotation;
    let relative = r0.inverse() * r1;
    2.0 * relative.x.atan2(relative.w)
}

/// Every physics step of a Play session: each VectorForce onto its body.
pub fn apply_scripted_forces(session: Option<Res<PlayDataModel>>, mut scripted: ResMut<ScriptedForces>, mut bodies: Query<Forces>) {
    if session.is_none() {
        // Outside Play: last session's forces must never reach the restored parts.
        scripted.forces.clear();
        return;
    }
    for f in &scripted.forces {
        let Ok(mut body) = bodies.get_mut(f.body) else { continue };
        let rotation = body.rotation().0;
        let position = body.position().0;
        let force = if f.local { rotation * (f.frame.rotation * f.force) } else { f.force };
        if f.at_center_of_mass {
            body.apply_force(force);
        } else {
            body.apply_force_at_point(force, position + rotation * Vec3::from(f.frame.translation));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A headless physics world stepping one 60 Hz frame per update.
    fn physics_world() -> App {
        let mut app = App::new();
        bevy::tasks::IoTaskPool::get_or_init(Default::default);
        bevy::tasks::AsyncComputeTaskPool::get_or_init(Default::default);
        bevy::tasks::ComputeTaskPool::get_or_init(Default::default);
        app.add_plugins((
            bevy::time::TimePlugin,
            bevy::transform::TransformPlugin,
            bevy::asset::AssetPlugin::default(),
            bevy::diagnostic::DiagnosticsPlugin,
        ));
        // Avian takes these unconditionally; DefaultPlugins supplies them in
        // the apps (see common's avatar behaviour harness).
        app.init_resource::<avian3d::spatial_query::SpatialQueryDiagnostics>();
        app.init_resource::<avian3d::collider_tree::ColliderTreeDiagnostics>();
        app.init_resource::<avian3d::collision::CollisionDiagnostics>();
        app.init_resource::<avian3d::dynamics::solver::SolverDiagnostics>();
        app.init_asset::<Mesh>();
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)));
        app.add_plugins(PhysicsPlugins::default());
        app.insert_resource(Gravity(Vec3::NEG_Y * 9.81));
        app.add_plugins(ScriptedJointsPlugin);
        app
    }

    struct Car {
        chassis: Entity,
        axles: Vec<Entity>,
    }

    const WHEEL_RADIUS: f32 = 0.45;
    /// Chassis centre height when the springs hold it at rest.
    const RIDE_HEIGHT: f32 = 1.4;
    /// A 0.3 m cube at 900 kg/m^3.
    const STRUT_MASS: f32 = 900.0 * 0.3 * 0.3 * 0.3;

    /// SimChassis 4.0's car, from the joint types joints.rs spawns.
    fn spawn_car(app: &mut App) -> Car {
        let world = app.world_mut();
        world.spawn((RigidBody::Static, Collider::cuboid(400.0, 1.0, 400.0), Transform::from_xyz(0.0, -0.5, 0.0)));
        let mass = 1400.0_f32;
        let load = mass / 4.0;
        let stiffness = load * (2.0 * std::f32::consts::PI * 1.8).powi(2);
        let damping = 2.0 * 0.45 * (stiffness * load).sqrt();
        let sag = load * 9.81 / stiffness;
        // A flat chassis whose bottom clears the wheel tops.
        let half = Vec3::new(1.0, 0.3, 2.2);
        let drop = 0.1;
        let chassis = world
            .spawn((RigidBody::Dynamic, Collider::cuboid(2.0 * half.x, 2.0 * half.y, 2.0 * half.z), Mass(mass), Transform::from_xyz(0.0, RIDE_HEIGHT + drop, 0.0)))
            .id();
        let strut_y = WHEEL_RADIUS + drop;
        let rest = RIDE_HEIGHT - half.y - WHEEL_RADIUS;
        let mut axles = Vec::new();
        for (x, z) in [(-0.9, -1.5), (0.9, -1.5), (-0.9, 1.5), (0.9, 1.5)] {
            let corner = Vec3::new(x, -half.y, z);
            // SimChassis's strut: a 0.3 m Plastic (900 kg/m^3) cube, which
            // carries no collision, so its mass comes from its shape.
            let strut = world
                .spawn((
                    RigidBody::Dynamic,
                    Mass(STRUT_MASS),
                    NoAutoMass,
                    AngularInertia::new(Vec3::splat(STRUT_MASS * 0.3 * 0.3 / 6.0)),
                    NoAutoAngularInertia,
                    CenterOfMass(Vec3::ZERO),
                    NoAutoCenterOfMass,
                    Transform::from_xyz(x, strut_y, z),
                ))
                .id();
            let wheel = world
                .spawn((RigidBody::Dynamic, Collider::sphere(WHEEL_RADIUS), Mass(20.0), Friction::new(1.2), Transform::from_xyz(x, strut_y, z)))
                .id();
            let mut slider = PrismaticJoint::new(chassis, strut)
                .with_local_anchor1(corner)
                .with_local_anchor2(Vec3::ZERO)
                .with_slider_axis(Vec3::Y);
            slider.limits = Some(DistanceLimit::new(-(rest + 0.25), -(rest - 0.2)));
            world.spawn((slider, JointCollisionDisabled));
            world.spawn((
                SpringJoint {
                    body1: chassis,
                    body2: strut,
                    local_anchor1: corner,
                    local_anchor2: Vec3::ZERO,
                    free_length: rest + sag,
                    stiffness,
                    damping,
                },
                JointCollisionDisabled,
            ));
            let axle = world
                .spawn((
                    RevoluteJoint::new(strut, wheel)
                        .with_local_anchor1(Vec3::ZERO)
                        .with_local_anchor2(Vec3::ZERO)
                        .with_hinge_axis(Vec3::X),
                    JointCollisionDisabled,
                ))
                .id();
            axles.push(axle);
            // Nothing a motor drives may sleep.
            world.entity_mut(wheel).insert(SleepingDisabled);
            world.entity_mut(strut).insert(SleepingDisabled);
        }
        world.entity_mut(chassis).insert(SleepingDisabled);
        Car { chassis, axles }
    }

    fn run(app: &mut App, frames: usize) {
        for _ in 0..frames {
            app.update();
        }
    }

    fn chassis_pose(app: &App, car: &Car) -> (Vec3, Vec3, f32) {
        let world = app.world();
        let at = world.get::<Position>(car.chassis).map_or(Vec3::NAN, |p| p.0);
        let up = world.get::<Rotation>(car.chassis).map_or(Vec3::NAN, |r| r.0 * Vec3::Y);
        let speed = world.get::<LinearVelocity>(car.chassis).map_or(f32::NAN, |v| v.0.length());
        (at, up, speed)
    }

    fn set_motors(app: &mut App, car: &Car, velocity: f32, torque: f32) {
        for &axle in &car.axles {
            let mut joint = app.world_mut().get_mut::<RevoluteJoint>(axle).unwrap();
            joint.motor = AngularMotor::new(velocity_model()).with_target_velocity(velocity).with_max_torque(torque);
        }
    }

    /// Sits on its springs, drives on torque through its wheels, brakes.
    #[test]
    fn a_car_of_scripted_joints_sits_drives_and_brakes() {
        let mut app = physics_world();
        let car = spawn_car(&mut app);
        run(&mut app, 180);
        let (settled, up, speed) = chassis_pose(&app, &car);
        assert!((settled.y - RIDE_HEIGHT).abs() < 0.12, "rides on its springs at {RIDE_HEIGHT} m, not {settled:?}");
        assert!(up.y > 0.98, "upright, not {up:?}");
        assert!(speed < 0.2, "at rest, not {speed} m/s");

        // 20 rad/s on 0.45 m wheels is 9 m/s.
        set_motors(&mut app, &car, 20.0, 800.0);
        run(&mut app, 180);
        let (driven, up, speed) = chassis_pose(&app, &car);
        let travelled = driven - settled;
        assert!(travelled.z.abs() > 5.0, "drove along its length, travelled {travelled:?}");
        assert!(travelled.x.abs() < 1.5, "straight, travelled {travelled:?}");
        assert!(up.y > 0.95, "still upright, {up:?}");
        assert!(speed > 3.0, "moving at {speed} m/s");

        set_motors(&mut app, &car, 0.0, 3000.0);
        run(&mut app, 180);
        let (_, up, speed) = chassis_pose(&app, &car);
        assert!(speed < 1.0, "braked to {speed} m/s");
        assert!(up.y > 0.95, "upright after braking, {up:?}");
    }

    #[test]
    fn only_a_seat_to_character_weld_is_a_record() {
        let mut g = DataModel::new();
        let ws = g.get_service("Workspace").unwrap();
        let character = g.create("Model");
        g.set_parent(character, Some(ws)).unwrap();
        let humanoid = g.create("Humanoid");
        g.set_parent(humanoid, Some(character)).unwrap();
        let root = g.create("Part");
        g.rename(root, "HumanoidRootPart").unwrap();
        g.set_parent(root, Some(character)).unwrap();
        let seat = g.create("VehicleSeat");
        g.set_parent(seat, Some(ws)).unwrap();
        let hat = g.create("Part");
        g.set_parent(hat, Some(ws)).unwrap();
        // No entities at all: the tree alone decides, as on a Player whose
        // replica has not spawned yet.
        let world = World::new();
        assert!(is_seat_weld(&g, &world, seat, root));
        assert!(is_seat_weld(&g, &world, root, seat));
        assert!(!is_seat_weld(&g, &world, hat, root), "a hat welded to a character is a joint");
        assert!(!is_seat_weld(&g, &world, seat, hat), "a seat welded to its car is a joint");
    }

    #[test]
    fn a_weld_constraint_holds_its_parts_where_they_are() {
        let part0 = Isometry3d::new(Vec3::new(1.0, 2.0, 3.0), Quat::from_rotation_y(0.5));
        let part1 = Isometry3d::new(Vec3::new(-4.0, 0.0, 1.0), Quat::from_rotation_x(1.5));
        let c1 = held_pose_c1(part0, part1);
        // Part0 * C0 (identity) and Part1 * C1 meet, so the joint starts at rest.
        let meet = part1 * c1;
        assert!(Vec3::from(meet.translation).distance(Vec3::from(part0.translation)) < 1e-5);
        assert!(meet.rotation.angle_between(part0.rotation) < 1e-5);
    }

    #[test]
    fn a_motor_with_no_torque_is_off() {
        let (motor, _) = angular_motor("Motor", 10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0 / 60.0);
        assert!(!motor.enabled, "a cap of 0 would be unlimited in Avian");
        let (motor, _) = angular_motor("Motor", 10.0, 50.0, 0.0, 0.0, 0.0, 0.0, 1.0 / 60.0);
        assert!(motor.enabled && motor.max_torque == 50.0 && motor.target_velocity == 10.0);
    }

    #[test]
    fn a_servo_turns_at_its_speed() {
        let dt = 0.1;
        let (motor, s) = angular_motor("Servo", 0.0, 0.0, 1.0, 2.0, 100.0, 0.0, dt);
        assert!((s - 0.2).abs() < 1e-6, "2 rad/s for 0.1 s");
        assert!((motor.target_position - 0.2).abs() < 1e-6);
        let (_, s) = angular_motor("Servo", 0.0, 0.0, 1.0, 0.0, 100.0, 0.0, dt);
        assert_eq!(s, 1.0, "a speed of 0 goes straight to the target");
    }

    #[test]
    fn a_linear_servo_is_limited_the_same_way() {
        let (motor, s) = linear_motor("Servo", 0.0, 0.0, -1.0, 0.5, 10.0, 0.0, 0.2);
        assert!((s + 0.1).abs() < 1e-6);
        assert!(motor.enabled);
        let (motor, _) = linear_motor("None", 1.0, 10.0, 0.0, 0.0, 0.0, 0.0, 0.2);
        assert!(!motor.enabled);
    }
}
