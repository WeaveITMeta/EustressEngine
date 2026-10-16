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
//! | `WeldConstraint`, `Weld`, `ManualWeld` | the weld group as one body ([`crate::assemblies`]), or a `FixedJoint` when the group has an anchored part (from the parts' poses, or from `C0` and `C1`) |
//! | `VectorForce` | a force applied every physics step ([`apply_scripted_forces`]) |
//!
//! Avian 0.7 facts the mapping respects: a motor's `max_torque`/`max_force` of
//! 0 means NO cap, so a constraint maximum of 0 disables the motor; the
//! acceleration-based model with damping 1 reaches its target speed within a
//! substep when the cap allows, so a Motor neither overshoots nor chatters.
//!
//! ## Weld groups
//!
//! Bodies welded together form a group, and a group of dynamic bodies is one
//! rigid body: its heaviest part (the hub, by its shape and authored density)
//! keeps its body, and every other part's collider is attached to it
//! ([`crate::assemblies`]), whatever shape the script's welds had. Every other
//! constraint or force that lands on a member (a hinge, a spring, a
//! `VectorForce`) acts on the hub at the same point in the world: [`body_of`]
//! answers the hub for a member. A chain of position-level joints between
//! bodies of very different mass does not stay rigid, and Avian turns every
//! joint correction into velocity: a 30 kg seat welded to 700 kg of chassis,
//! with the springs on the seat, drifted 18 cm at rest and threw a bumper off
//! at 89 m/s; a star of joints from the heaviest part still drifted 33 mm when
//! its parts outweighed it. The welds the Space loaded join the same groups. A
//! group with a static, kinematic or character body in it keeps one joint per
//! weld, since a static anchor is well conditioned already. A group re-forms
//! when one of its bodies is anchored or released, and a `WeldConstraint`
//! holds the offset it was made with: its `Part0`, `Part1` or `Enabled`
//! changing takes a new one and nothing else does, so a group that re-forms
//! never welds its parts where they have drifted to, and a plan that comes out
//! the same (a rider's SeatWeld coming or going) changes nothing.
//!
//! ## Constraints between anchored parts
//!
//! A constraint none of whose parts simulates (two anchored parts) waits, and is
//! built the frame one of them is released: Avian 0.7 links every joint into the
//! islands of its two bodies, only a dynamic or kinematic body has one, and a
//! joint between two bodies that have none panics the solver ("Neither body ...
//! is in an island"). A Roblox constraint between anchored parts does nothing
//! until a part moves either, so a script can weld and hinge a whole model while
//! it is anchored and release it at once.

use std::collections::{HashMap, HashSet};

use avian3d::dynamics::joints::EntityConstraint;
use avian3d::dynamics::rigid_body::RigidBodyQueryReadOnlyItem;
use avian3d::dynamics::solver::solver_body::{SolverBody, SolverBodyInertia};
use avian3d::dynamics::solver::xpbd::{
    prepare_xpbd_joint, solve_xpbd_joint, AngularConstraint, PositionConstraint,
    XpbdConstraint, XpbdConstraintSolverData, XpbdSolverSystems,
};
use avian3d::dynamics::solver::islands::BodyIslandNode;
use avian3d::dynamics::solver::joint_graph::JointGraphPlugin;
use avian3d::prelude::*;
use bevy::ecs::entity::{EntityMapper, MapEntities};
use bevy::math::{Affine3A, Isometry3d};
use bevy::prelude::*;

use eustress_common::avatar::spawn::AvatarBody;
use eustress_common::datamodel::{DataModel, DmEvent, DmValue, InstanceId};
use eustress_common::play_session::{DataModelSpawned, PlayDataModel, PlayRole, PlayScriptSet};

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
    let (w0, w1) = (weigh(world, b0), weigh(world, b1));
    let lighter = match (w0, w1) {
        (Some(a), Some(b)) => Some(a.0.min(b.0)),
        (a, b) => a.or(b).map(|w| w.0),
    };
    let turning = w1.map(|w| w.1).filter(|i| i.is_finite() && *i > 0.0);
    (lighter.filter(|m| m.is_finite()).map_or(CARRIER_MASS, |m| 0.1 * m).max(CARRIER_MASS), turning.unwrap_or(0.1).max(0.1))
}

/// A body's mass and its largest principal inertia: Avian's once it has
/// computed them, else its part's shape (a part spawned in this same pass has
/// no computed figures until the next physics step).
fn weigh(world: &World, b: Entity) -> Option<(f32, f32)> {
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
}

/// A body's mass for choosing a weld group's hub: its part's shape and authored
/// density when it has them, else Avian's figure. Avian's computed mass is
/// whatever the solver last settled on, and a part made dynamic a few frames ago,
/// or one whose collider scale has not reached Avian yet, reads lighter than a
/// part welded to it. A hub that is not the heaviest part leaves the group carried
/// by a body with neighbours touching it.
fn hub_weight(world: &World, body: Entity) -> f32 {
    let authored = world.get::<eustress_common::classes::BasePart>(body).map(|base| {
        let shape = world.get::<eustress_common::classes::Part>(body).map_or(eustress_common::classes::PartType::Block, |p| p.shape);
        crate::part_mass::shape_mass(shape, base.size, base.effective_density()).0
    });
    authored.filter(|m| m.is_finite() && *m > 0.0).or_else(|| weigh(world, body).map(|w| w.0)).unwrap_or(0.0)
}

/// Registers the scripted-constraint systems and the spring constraint.
pub struct ScriptedJointsPlugin;

impl Plugin for ScriptedJointsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptedJoints>()
            .init_resource::<ScriptedForces>()
            // A joint's non-colliding part (a strut, a knuckle) must weigh.
            .add_plugins(crate::part_mass::PartMassPlugin)
            // The parts it simulates are drawn between physics steps.
            .add_plugins(crate::body_easing::BodyEasingPlugin)
            // Weld groups merged into one body follow their hubs.
            .add_plugins(crate::assemblies::AssembliesPlugin)
            // `[JT]` lines when asked for (EUSTRESS_JOINT_TRACE).
            .add_plugins(crate::joint_trace::JointTracePlugin)
            .register_required_components::<SpringJoint, SpringJointSolverData>()
            .add_plugins(JointGraphPlugin::<SpringJoint>::default())
            .add_systems(PhysicsSchedule, prepare_xpbd_joint::<SpringJoint>.in_set(SolverSystems::PrepareJoints))
            .add_systems(SubstepSchedule, solve_xpbd_joint::<SpringJoint>.in_set(XpbdSolverSystems::SolveUserConstraints))
            .add_systems(
                Update,
                (note_body_changes, sync_scripted_joints).chain().in_set(PlayScriptSet::Apply).after(crate::apply::apply_frame),
            )
            .add_systems(FixedUpdate, apply_scripted_forces)
            // Outside the session's sets, which stop running with it.
            .add_systems(Update, release_assemblies_after_play);
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
    /// The session these belong to (its tree's session number, from 1); a new
    /// session starts clean.
    session: u64,
    cursor: u64,
    tracked: HashMap<InstanceId, Tracked>,
    /// An Attachment or part a tracked constraint depends on -> those constraints.
    dependents: HashMap<InstanceId, HashSet<InstanceId>>,
    /// The weld groups.
    assemblies: Assemblies,
    /// Bodies anchored, released or spawned since the last frame
    /// ([`note_body_changes`]): a weld group over one of them is planned again.
    changed_bodies: Vec<Entity>,
    /// A weld the Space loaded was bound, changed or removed, or a part's welds
    /// were released (it fractured), since the last frame.
    welds_changed: bool,
}

struct Tracked {
    class: &'static str,
    /// Needs building again (created, or a property, reference or end changed).
    stale: bool,
    built: Option<Built>,
    /// A servo's moving setpoint (radians, or metres for a linear servo).
    servo_angle: f32,
    servo_linear: f32,
    /// The two bodies a constraint between parts that do not simulate waits
    /// on: it is built once either of them does.
    waiting: Option<[Entity; 2]>,
    /// A WeldConstraint's offset (its `C1`: the frame in `Part1` where `Part0`
    /// sits), taken the first time both its parts were drawn.
    held: Option<Isometry3d>,
}

/// What building a constraint came to, when it came to anything.
enum Outcome {
    Built(Built),
    /// Neither end's body simulates yet.
    Waiting([Entity; 2]),
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

impl Built {
    /// A weld its merged group holds: no joint of its own.
    fn held_by_assembly() -> Built {
        Built {
            entities: Vec::new(),
            primary: Entity::PLACEHOLDER,
            secondary: None,
            body0: Entity::PLACEHOLDER,
            body1: Entity::PLACEHOLDER,
            frame0: Isometry3d::IDENTITY,
            frame1: Isometry3d::IDENTITY,
            no_sleep: Vec::new(),
        }
    }
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

// ── Weld assemblies ─────────────────────────────────────────────────────

pub use crate::assemblies::AssemblyMember;

/// A body some weld joins to another.
#[derive(Clone, Copy, Debug)]
struct WeldBody {
    entity: Entity,
    /// Only Dynamic bodies merge. A group with a static, kinematic or
    /// character body in it keeps one joint per weld.
    dynamic: bool,
    /// kg: the heaviest body of a group carries it.
    mass: f32,
}

/// A weld between two bodies.
#[derive(Clone, Copy, Debug)]
struct WeldEdge {
    a: Entity,
    b: Entity,
    /// Where `b` sits in `a`'s frame once the weld holds.
    b_in_a: Isometry3d,
}

/// A body merged into its group's hub, and where it sits on the hub.
#[derive(Clone, Copy, Debug)]
struct GroupMember {
    entity: Entity,
    in_hub: Isometry3d,
}

/// What one group of welded bodies becomes.
#[derive(Debug)]
enum GroupPlan {
    /// Every body is dynamic: one body, the hub, with every other body's
    /// collider on it ([`crate::assemblies`]), whatever shape the welds were
    /// written in. `edges` are the welds it stands for.
    Merged { hub: Entity, members: Vec<GroupMember>, edges: Vec<usize> },
    /// A static, kinematic or character body is among them: one joint per
    /// weld, as written. A static anchor is well conditioned already.
    PerEdge { edges: Vec<usize> },
}

/// Groups the welded bodies and decides what holds each group.
///
/// A rigid group built as a chain of position-level joints between bodies of
/// very different mass does not stay rigid: Avian turns every joint correction
/// into velocity, and a 30 kg seat welded to 700 kg of chassis, with the
/// springs on the seat, drifted 18 cm and threw a bumper off at 89 m/s. A star
/// of joints from the heaviest body held to millimetres; one body holds
/// exactly. The groups come out in a stable order, each hub the heaviest body
/// (on a tie, the same one of the tied bodies every time), which keeps its own
/// body and pose.
fn plan_groups(bodies: &[WeldBody], edges: &[WeldEdge]) -> Vec<GroupPlan> {
    let index: HashMap<Entity, usize> = bodies.iter().enumerate().map(|(i, b)| (b.entity, i)).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut parent: Vec<usize> = (0..bodies.len()).collect();
    let ends = |e: &WeldEdge| Some((*index.get(&e.a)?, *index.get(&e.b)?));
    for e in edges {
        let Some((a, b)) = ends(e) else { continue };
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent[ra.max(rb)] = ra.min(rb);
        }
    }
    let mut groups: std::collections::BTreeMap<usize, (Vec<usize>, Vec<usize>)> = std::collections::BTreeMap::new();
    for i in 0..bodies.len() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().0.push(i);
    }
    for (k, e) in edges.iter().enumerate() {
        if let Some((a, _)) = ends(e) {
            let root = find(&mut parent, a);
            groups.entry(root).or_default().1.push(k);
        }
    }

    let mut plans = Vec::new();
    for (_, (members, group_edges)) in groups {
        if members.len() < 2 || group_edges.is_empty() {
            continue;
        }
        if members.iter().any(|&i| !bodies[i].dynamic) {
            plans.push(GroupPlan::PerEdge { edges: group_edges });
            continue;
        }
        let hub = members
            .iter()
            .copied()
            .max_by(|&x, &y| bodies[x].mass.total_cmp(&bodies[y].mass).then(bodies[y].entity.to_bits().cmp(&bodies[x].entity.to_bits())))
            .expect("a group has bodies");
        // Each body's pose in the hub's frame, along the first weld path to it.
        let mut next_to: HashMap<usize, Vec<(usize, Isometry3d)>> = HashMap::new();
        for &k in &group_edges {
            let e = &edges[k];
            if let Some((a, b)) = ends(e) {
                next_to.entry(a).or_default().push((b, e.b_in_a));
                next_to.entry(b).or_default().push((a, e.b_in_a.inverse()));
            }
        }
        let mut in_hub: HashMap<usize, Isometry3d> = HashMap::from([(hub, Isometry3d::IDENTITY)]);
        let mut queue = std::collections::VecDeque::from([hub]);
        while let Some(i) = queue.pop_front() {
            let here = in_hub[&i];
            for &(j, j_in_i) in next_to.get(&i).into_iter().flatten() {
                if !in_hub.contains_key(&j) {
                    in_hub.insert(j, here * j_in_i);
                    queue.push_back(j);
                }
            }
        }
        let merged = members
            .iter()
            .filter(|&&i| i != hub)
            .filter_map(|&i| Some(GroupMember { entity: bodies[i].entity, in_hub: *in_hub.get(&i)? }))
            .collect();
        plans.push(GroupPlan::Merged { hub: bodies[hub].entity, members: merged, edges: group_edges });
    }
    plans
}

/// A weld as the groups know it: a script's (its instance), or one the Space
/// loaded (its joint, which `engine::physics::joint_resolver` built).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum WeldId {
    Script(InstanceId),
    Loaded(Entity),
}

/// The weld groups of a session, and what holds each weld.
#[derive(Default)]
struct Assemblies {
    /// Merged part -> its hub and its place on the hub, as the
    /// [`AssemblyMember`] markers say.
    members: HashMap<Entity, (Entity, Isometry3d)>,
    /// Scripted welds a merged group stands for: they build no joint of their own.
    merged: HashSet<InstanceId>,
    /// Scripted welds that keep a joint of their own: their group has a static,
    /// kinematic or character body in it.
    per_edge: HashSet<InstanceId>,
    /// What the groups were planned from: each weld, the bodies at its ends,
    /// whether each simulates, and the offset it holds.
    signature: HashSet<WeldKey>,
    /// Every body the groups were planned over.
    bodies: HashSet<Entity>,
    /// A weld is waiting for a part that is not drawn yet.
    retry: bool,
}

/// One weld as the planner saw it. Planning from the same keys gives the same
/// groups, so they are left standing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct WeldKey {
    weld: WeldId,
    a: Entity,
    b: Entity,
    a_dynamic: bool,
    b_dynamic: bool,
    /// `b_in_a`, bit for bit.
    offset: [u32; 7],
}

fn offset_bits(offset: Isometry3d) -> [u32; 7] {
    let (t, r) = (Vec3::from(offset.translation), offset.rotation);
    [t.x, t.y, t.z, r.x, r.y, r.z, r.w].map(f32::to_bits)
}

fn is_weld(class: &str) -> bool {
    matches!(class, "WeldConstraint" | "Weld" | "ManualWeld")
}

/// A body as the planner sees it. A merged part keeps its dynamic body
/// (disabled), so it plans as dynamic.
fn weld_body(world: &World, body: Entity) -> WeldBody {
    WeldBody { entity: body, dynamic: matches!(world.get::<RigidBody>(body), Some(RigidBody::Dynamic)), mass: hub_weight(world, body) }
}

/// A weld's two frames: `Part0 * C0 == Part1 * C1` once it holds. A
/// WeldConstraint holds the pose it was made in: Part1 * C1 is where Part0 was
/// when both parts were first drawn (`held`, taken then and kept until its
/// parts or `Enabled` change). Identity frames would pull the two parts onto
/// one pose.
fn weld_frames(
    g: &DataModel,
    world: &World,
    id: InstanceId,
    class: &str,
    p0: InstanceId,
    p1: InstanceId,
    held: &mut Option<Isometry3d>,
) -> Option<(Isometry3d, Isometry3d)> {
    Some(match class {
        "Weld" | "ManualWeld" => (cframe(g, id, "C0"), cframe(g, id, "C1")),
        "WeldConstraint" => {
            if held.is_none() {
                let pose = |p: InstanceId| part_entity(g, world, p).and_then(|e| world_pose(world, e));
                *held = Some(held_pose_c1(pose(p0)?, pose(p1)?));
            }
            (Isometry3d::IDENTITY, (*held)?)
        }
        _ => (Isometry3d::IDENTITY, Isometry3d::IDENTITY),
    })
}

/// What a weld joins.
enum WeldEnds {
    /// Two bodies, and where the second sits in the first once the weld holds.
    Edge { a: Entity, b: Entity, b_in_a: Isometry3d },
    /// Never a joint: disabled, an end unset, a SeatWeld, or both ends on one body.
    Skip,
    /// An end's part is not drawn yet.
    Wait,
}

fn weld_ends(
    g: &DataModel,
    world: &World,
    id: InstanceId,
    class: &str,
    dependents: &mut HashMap<InstanceId, HashSet<InstanceId>>,
    held: &mut Option<Isometry3d>,
) -> WeldEnds {
    if !g.in_workspace(id) || g.get_prop(id, "Enabled").and_then(|v| v.as_bool()) == Some(false) {
        return WeldEnds::Skip;
    }
    let (Some(p0), Some(p1)) = (reference(g, id, "Part0"), reference(g, id, "Part1")) else { return WeldEnds::Skip };
    for p in [p0, p1] {
        dependents.entry(p).or_default().insert(id);
    }
    if is_seat_weld(g, world, p0, p1) {
        return WeldEnds::Skip;
    }
    let (Some(e0), Some(e1)) = (part_entity(g, world, p0), part_entity(g, world, p1)) else { return WeldEnds::Wait };
    // A part that fractured holds by no weld.
    if [e0, e1].iter().any(|&e| world.get::<crate::assemblies::WeldsReleased>(e).is_some()) {
        return WeldEnds::Skip;
    }
    let (Some(a), Some(b)) = (raw_body_of(world, e0), raw_body_of(world, e1)) else { return WeldEnds::Wait };
    if a == b {
        return WeldEnds::Skip;
    }
    let Some((c0, c1)) = weld_frames(g, world, id, class, p0, p1, held) else { return WeldEnds::Wait };
    // Each part in its body's frame: exactly identity for a part that is its
    // own body (every part is, in Play), so one weld always plans to the same bits.
    let in_body = |part: Entity, body: Entity| -> Option<Isometry3d> {
        if part == body {
            Some(Isometry3d::IDENTITY)
        } else {
            Some(world_pose(world, body)?.inverse() * world_pose(world, part)?)
        }
    };
    let (Some(rel0), Some(rel1)) = (in_body(e0, a), in_body(e1, b)) else { return WeldEnds::Wait };
    // Part1 sits at Part0 * C0 * C1^-1, and each body moves with its part.
    WeldEnds::Edge { a, b, b_in_a: rel0 * c0 * c1.inverse() * rel1.inverse() }
}

/// Splits every merged group and gives the loaded joints back to their parts.
fn release_assemblies(world: &mut World, assemblies: &mut Assemblies, simulating: bool) {
    crate::assemblies::return_loaded_joints(world);
    for (member, _) in assemblies.members.drain() {
        crate::assemblies::split(world, member, simulating);
    }
}

/// Makes the weld groups, when a weld has changed (a script's, or one the
/// Space loaded), one was waiting on a part, or a grouped body was anchored or
/// released: groups the welded bodies, merges each group of dynamic bodies
/// into its hub and splits what left a group, and has every constraint built
/// again against the new groups.
fn sync_assemblies(g: &DataModel, world: &mut World, state: &mut ScriptedJoints) {
    let mut welds: Vec<(InstanceId, &'static str)> = state.tracked.iter().filter(|(_, t)| is_weld(t.class)).map(|(id, t)| (*id, t.class)).collect();
    let changed = welds.iter().any(|(id, _)| state.tracked[id].stale);
    // A grouped body anchored or released: its group is planned again (a model
    // welded while anchored becomes one assembly once released, and keeps one
    // joint per weld while anchored).
    let bodies_changed = state.changed_bodies.drain(..).any(|e| state.assemblies.bodies.contains(&e));
    let welds_changed = std::mem::take(&mut state.welds_changed);
    // A merged part whose collider Avian pointed back at its own body (it was
    // anchored, reparented, or the session stopped): split and merged again.
    let disturbed: Vec<Entity> =
        state.assemblies.members.keys().copied().filter(|e| world.get::<crate::assemblies::ContactsReleased>(*e).is_some()).collect();
    if !changed && !state.assemblies.retry && !bodies_changed && !welds_changed && disturbed.is_empty() {
        return;
    }
    welds.sort();
    let mut bodies: Vec<WeldBody> = Vec::new();
    let mut seen: HashSet<Entity> = HashSet::new();
    let mut edges: Vec<WeldEdge> = Vec::new();
    let mut ends: Vec<(WeldId, Entity, Entity)> = Vec::new();
    let mut signature: HashSet<WeldKey> = HashSet::new();
    let mut retry = false;
    let mut add = |world: &World, weld: WeldId, a: Entity, b: Entity, b_in_a: Isometry3d| {
        for body in [a, b] {
            if seen.insert(body) {
                bodies.push(weld_body(world, body));
            }
        }
        let dynamic = |body: Entity| bodies.iter().any(|w| w.entity == body && w.dynamic);
        signature.insert(WeldKey { weld, a, b, a_dynamic: dynamic(a), b_dynamic: dynamic(b), offset: offset_bits(b_in_a) });
        edges.push(WeldEdge { a, b, b_in_a });
        ends.push((weld, a, b));
    };
    for (id, class) in welds {
        if !g.exists(id) {
            continue;
        }
        let Some(tracked) = state.tracked.get_mut(&id) else { continue };
        match weld_ends(g, world, id, class, &mut state.dependents, &mut tracked.held) {
            WeldEnds::Edge { a, b, b_in_a } => add(world, WeldId::Script(id), a, b, b_in_a),
            WeldEnds::Skip => {}
            WeldEnds::Wait => retry = true,
        }
    }
    // The Space's own welds join the same groups.
    for (joint, a, b, b_in_a) in crate::assemblies::loaded_welds(world) {
        add(world, WeldId::Loaded(joint), a, b, b_in_a);
    }
    state.assemblies.retry = retry;
    // The same welds between the same bodies, each simulating as it was and
    // holding the same offset: the groups stand, and a weld they hold keeps
    // the joint it has. A rider's SeatWeld coming or going, or a part spawned
    // elsewhere, re-welds nothing.
    if signature == state.assemblies.signature && disturbed.is_empty() {
        for key in &signature {
            if let WeldId::Script(id) = key.weld {
                if let Some(t) = state.tracked.get_mut(&id) {
                    if t.built.is_some() {
                        t.stale = false;
                    }
                }
            }
        }
        return;
    }

    let script = |k: &usize| match ends[*k].0 {
        WeldId::Script(id) => Some(id),
        WeldId::Loaded(_) => None,
    };
    let mut members: HashMap<Entity, (Entity, Isometry3d)> = HashMap::new();
    let (mut merged, mut per_edge) = (HashSet::new(), HashSet::new());
    for plan in plan_groups(&bodies, &edges) {
        match plan {
            GroupPlan::Merged { hub, members: group, edges: held } => {
                let name = |body: Entity| world.get::<Name>(body).map_or_else(|| format!("{body:?}"), |n| n.as_str().to_string());
                info!(
                    "weld group: {} ({:.0} kg) is one body with {} welded parts",
                    name(hub),
                    bodies.iter().find(|b| b.entity == hub).map_or(0.0, |b| b.mass),
                    group.len()
                );
                members.extend(group.iter().map(|m| (m.entity, (hub, m.in_hub))));
                merged.extend(held.iter().filter_map(script));
            }
            GroupPlan::PerEdge { edges: group } => per_edge.extend(group.iter().filter_map(script)),
        }
    }
    let old = std::mem::take(&mut state.assemblies);
    let same = |x: &(Entity, Isometry3d), y: &(Entity, Isometry3d)| x.0 == y.0 && offset_bits(x.1) == offset_bits(y.1);
    let moved = |e: &Entity, was: &(Entity, Isometry3d), now: Option<&(Entity, Isometry3d)>| now.map_or(true, |now| !same(was, now)) || disturbed.contains(e);
    let leaving: Vec<Entity> = old.members.iter().filter(|(e, was)| moved(e, was, members.get(*e))).map(|(e, _)| *e).collect();
    let joining: Vec<(Entity, Entity, Isometry3d)> = members
        .iter()
        .filter(|(e, now)| old.members.get(*e).map_or(true, |was| moved(e, was, Some(now))))
        .map(|(e, (hub, in_hub))| (*e, *hub, *in_hub))
        .collect();
    if !leaving.is_empty() || !joining.is_empty() || merged != old.merged || per_edge != old.per_edge {
        // Every scripted joint is built again against the new groups: one
        // standing may hang from a body about to merge or split.
        for t in state.tracked.values_mut() {
            tear_down(world, t.built.take());
            t.stale = true;
        }
        let simulating = world.get_resource::<PlayRole>().map_or(true, |r| *r == PlayRole::Authority);
        crate::assemblies::return_loaded_joints(world);
        let (left, joined) = (leaving.clone(), joining.len());
        for member in leaving {
            crate::assemblies::split(world, member, simulating);
        }
        for (member, hub, in_hub) in joining {
            crate::assemblies::merge(world, hub, member, in_hub);
        }
        crate::assemblies::carry_loaded_joints(world, &members);
        let welded: Vec<Entity> = seen.iter().copied().collect();
        crate::joint_trace::plan(world, &members, &welded, &left, joined);
    }
    state.assemblies = Assemblies { members, merged, per_edge, signature, bodies: seen, retry };
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

/// An entity's world pose (position and rotation; scale dropped). A body's is
/// its physics pose, `Position` and `Rotation`: its `Transform` is drawn eased
/// between physics steps ([`crate::body_easing`]), so composing Transforms
/// would build a joint against where the body is drawn, up to a step behind.
/// Avian sets a body's pose from its Transforms the moment it is spawned, and
/// a script's move sets it too ([`crate::apply`]).
fn world_pose(world: &World, e: Entity) -> Option<Isometry3d> {
    if world.get::<RigidBody>(e).is_some() {
        if let (Some(p), Some(r)) = (world.get::<Position>(e), world.get::<Rotation>(e)) {
            return Some(Isometry3d::new(p.0, r.0));
        }
    }
    let (_, rotation, translation) = world_affine(world, e)?.to_scale_rotation_translation();
    Some(Isometry3d::new(translation, rotation))
}

/// The rigid body an entity moves with: itself, or its nearest ancestor with a body.
fn raw_body_of(world: &World, e: Entity) -> Option<Entity> {
    let mut node = Some(e);
    while let Some(n) = node {
        if world.get::<RigidBody>(n).is_some() {
            return Some(n);
        }
        node = world.get::<ChildOf>(n).map(|c| c.parent());
    }
    None
}

/// The body a constraint on `e` acts on: the one it moves with, or the hub of
/// its weld group when that body is a member of one ([`AssemblyMember`]).
fn body_of(world: &World, e: Entity) -> Option<Entity> {
    let body = raw_body_of(world, e)?;
    match world.get::<AssemblyMember>(body) {
        Some(member) if world.get::<RigidBody>(member.hub).is_some() => Some(member.hub),
        _ => Some(body),
    }
}

/// Whether Avian has `body` in a simulation island: a dynamic or kinematic
/// body that is not disabled. A joint needs at least one of its two bodies in
/// one, or Avian 0.7 panics linking it ("Neither body ... is in an island").
fn simulates(world: &World, body: Entity) -> bool {
    world.get::<BodyIslandNode>(body).is_some()
}

/// Bodies anchored, released or spawned this frame, for the weld groups to
/// look at: `RigidBody` is (re)inserted whenever a part's `Anchored` is written
/// or a part is drawn. And the welds: a joint the loader bound, changed or
/// removed (a removal is any joint's, and only costs a look), and a part whose
/// welds were released (it fractured). A body despawned counts as changed:
/// a group whose hub is destroyed re-forms around the parts left.
pub fn note_body_changes(
    session: Option<Res<PlayDataModel>>,
    changed: Query<Entity, Changed<RigidBody>>,
    mut despawned: RemovedComponents<RigidBody>,
    loaded: Query<(), (With<eustress_common::classes::Instance>, Changed<FixedJoint>)>,
    released: Query<(), Added<crate::assemblies::WeldsReleased>>,
    mut removed: RemovedComponents<FixedJoint>,
    mut state: ResMut<ScriptedJoints>,
) {
    let gone = removed.read().count() > 0;
    let despawned: Vec<Entity> = despawned.read().collect();
    if session.is_some() {
        state.changed_bodies.extend(changed.iter().chain(despawned));
        if gone || !loaded.is_empty() || !released.is_empty() {
            state.welds_changed = true;
        }
    }
}

/// When the session ends (Stop, or a Player leaving it), every merged group
/// splits and the loaded joints go back to their parts: Edit, or the next
/// session, finds every part its own body.
pub fn release_assemblies_after_play(world: &mut World) {
    if world.contains_resource::<PlayDataModel>() {
        return;
    }
    let Some(mut state) = world.get_resource_mut::<ScriptedJoints>() else { return };
    if state.assemblies.members.is_empty() && state.assemblies.signature.is_empty() {
        return;
    }
    let mut assemblies = std::mem::take(&mut state.assemblies);
    release_assemblies(world, &mut assemblies, false);
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

/// A part end, with the part's own pose (times `offset`) as the frame, on the
/// body the part moves with: its own, or its merged group's hub. A weld that
/// gets a joint of its own is never in a merged group; a NoCollisionConstraint
/// on a merged part keeps the whole group from colliding with the other part.
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
    // A body drawn or released this frame joins its island through Avian's
    // queued commands; applied here, before any joint is made against it.
    world.flush();
    let dt = world.resource::<Time>().delta_secs();
    let mut state = world.remove_resource::<ScriptedJoints>().unwrap_or_default();
    let session = eustress_common::datamodel::session_of(&dm);
    if state.session != session {
        // A new Play session: last session's joints went with its Stop.
        let simulating = world.get_resource::<PlayRole>().map_or(true, |r| *r == PlayRole::Authority);
        release_assemblies(world, &mut state.assemblies, simulating);
        state = ScriptedJoints { session, ..default() };
        crate::joint_trace::start_session(world);
    }
    let mut forces = Vec::new();
    {
        let mut g = dm.lock();
        read_events(&mut g, &mut state);
        sync_assemblies(&g, world, &mut state);

        let ids: Vec<InstanceId> = state.tracked.keys().copied().collect();
        for id in ids {
            if !g.exists(id) {
                if let Some(t) = state.tracked.remove(&id) {
                    tear_down(world, t.built);
                }
                continue;
            }
            // Built again when changed; while unbuilt, every frame (an end may
            // just have been drawn), unless it waits on two bodies that still
            // do not simulate.
            let due = state.tracked.get(&id).is_some_and(|t| {
                t.stale
                    || (t.built.is_none()
                        && t.waiting.map_or(true, |bodies| bodies.iter().any(|b| simulates(world, *b) || world.get_entity(*b).is_err())))
            });
            if due {
                let built = state.tracked.get_mut(&id).and_then(|t| t.built.take());
                tear_down(world, built);
                let class = state.tracked[&id].class;
                let mut held = state.tracked[&id].held;
                let outcome = build(&mut g, world, id, class, &mut state.dependents, &state.assemblies, &mut held);
                // The joint trace, when asked for; a weld its group holds builds nothing.
                let (ends, waiting) = match &outcome {
                    Some(Outcome::Built(b)) if b.body0 != Entity::PLACEHOLDER => (Some((b.body0, b.body1, b.frame0, b.frame1)), None),
                    Some(Outcome::Waiting(bodies)) => (None, Some(*bodies)),
                    _ => (None, None),
                };
                let held_by_group = matches!(&outcome, Some(Outcome::Built(b)) if b.body0 == Entity::PLACEHOLDER);
                if !held_by_group && world.get_resource::<crate::joint_trace::JointTrace>().is_some_and(|t| t.on) {
                    let what = format!("{} {id:?}", g.name_of(id).unwrap_or_default());
                    crate::joint_trace::built(world, &what, class, ends, waiting);
                }
                if let Some(t) = state.tracked.get_mut(&id) {
                    t.held = held;
                    t.stale = false;
                    (t.built, t.waiting) = match outcome {
                        Some(Outcome::Built(built)) => (Some(built), None),
                        Some(Outcome::Waiting(bodies)) => (None, Some(bodies)),
                        None => (None, None),
                    };
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
                state.tracked.insert(id, Tracked { class, stale: true, built: None, servo_angle: 0.0, servo_linear: 0.0, waiting: None, held: None });
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
                    // A WeldConstraint takes a new offset when its parts or
                    // Enabled change, as in Roblox, and only then.
                    if matches!(prop.as_str(), "Part0" | "Part1" | "Enabled") {
                        if let Some(t) = state.tracked.get_mut(&id) {
                            t.held = None;
                        }
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
/// end missing, both ends on one body, or a SeatWeld), or waits while neither
/// end's body simulates.
fn build(
    g: &mut DataModel,
    world: &mut World,
    id: InstanceId,
    class: &'static str,
    dependents: &mut HashMap<InstanceId, HashSet<InstanceId>>,
    assemblies: &Assemblies,
    held: &mut Option<Isometry3d>,
) -> Option<Outcome> {
    if !g.in_workspace(id) || g.get_prop(id, "Enabled").and_then(|v| v.as_bool()) == Some(false) {
        return None;
    }
    let by_parts = matches!(class, "NoCollisionConstraint" | "WeldConstraint" | "Weld" | "ManualWeld");
    let (end0, end1) = if by_parts {
        let (p0, p1) = (reference(g, id, "Part0")?, reference(g, id, "Part1")?);
        for p in [p0, p1] {
            dependents.entry(p).or_default().insert(id);
        }
        if is_weld(class) {
            // The weld groups decided this one: a merged group holds it, or it
            // keeps a joint of its own, or it is no joint at all.
            if assemblies.merged.contains(&id) {
                return Some(Outcome::Built(Built::held_by_assembly()));
            }
            if !assemblies.per_edge.contains(&id) {
                return None;
            }
        }
        if class != "NoCollisionConstraint" && is_seat_weld(g, world, p0, p1) {
            // A record for scripts; riding carries the character. Checked on
            // every retry, so it never becomes a joint once the root is bound.
            return None;
        }
        let (c0, c1) = weld_frames(g, world, id, class, p0, p1, held)?;
        (part_end(g, world, p0, c0)?, part_end(g, world, p1, c1)?)
    } else if class == "VectorForce" {
        let a0 = reference(g, id, "Attachment0")?;
        dependents.entry(a0).or_default().insert(id);
        g.watch_changes(a0);
        let end = attachment_end(g, world, a0)?;
        return Some(Outcome::Built(Built {
            entities: Vec::new(),
            primary: end.body,
            secondary: None,
            body0: end.body,
            body1: end.body,
            frame0: end.frame,
            frame1: end.frame,
            no_sleep: Vec::new(),
        }));
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
    // Two parts neither of which simulates (anchored parts): no joint yet. It
    // is built from the same attachments and offsets the frame one is released.
    if !simulates(world, b0) && !simulates(world, b1) {
        return Some(Outcome::Waiting([b0, b1]));
    }

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
    Some(Outcome::Built(Built { entities, primary, secondary, body0: b0, body1: b1, frame0: f0, frame1: f1, no_sleep }))
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

    /// A car of scripted joints: sprung struts, axle motors, welded to a chassis.
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
            // A strut: a 0.3 m Plastic (900 kg/m^3) cube, which
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

    // ── Weld groups ─────────────────────────────────────────────────────

    fn some_entities(n: usize) -> Vec<Entity> {
        let mut world = World::new();
        (0..n).map(|_| world.spawn_empty().id()).collect()
    }

    fn weld_body_of(entity: Entity, mass: f32) -> WeldBody {
        WeldBody { entity, dynamic: true, mass }
    }

    fn at(x: f32, y: f32, z: f32) -> Isometry3d {
        Isometry3d::from_translation(Vec3::new(x, y, z))
    }

    fn same_pose(a: Isometry3d, b: Isometry3d) -> bool {
        Vec3::from(a.translation).distance(Vec3::from(b.translation)) < 1e-5 && a.rotation.angle_between(b.rotation) < 1e-5
    }

    /// A seat welded to a spine and a bumper, as a script writes it: every weld
    /// on the seat. The heavy spine carries the group.
    #[test]
    fn a_weld_group_is_carried_by_its_heaviest_body_whatever_shape_the_welds_have() {
        let e = some_entities(3);
        let (seat, spine, bumper) = (e[0], e[1], e[2]);
        let bodies = [weld_body_of(seat, 30.0), weld_body_of(spine, 771.0), weld_body_of(bumper, 20.0)];
        let spine_in_seat = at(0.0, -0.58, 0.2);
        let bumper_in_seat = Isometry3d::new(Vec3::new(0.0, -0.58, -2.05), Quat::from_rotation_y(0.2));
        let edges = [
            WeldEdge { a: seat, b: spine, b_in_a: spine_in_seat },
            WeldEdge { a: seat, b: bumper, b_in_a: bumper_in_seat },
        ];
        let plans = plan_groups(&bodies, &edges);
        assert_eq!(plans.len(), 1);
        let GroupPlan::Merged { hub, members, edges: held } = &plans[0] else { panic!("one body, not {:?}", plans[0]) };
        assert_eq!(*hub, spine);
        assert_eq!(held.len(), 2);
        assert_eq!(members.len(), 2);
        let member = |entity: Entity| members.iter().find(|m| m.entity == entity).expect("a member");
        assert!(same_pose(member(seat).in_hub, spine_in_seat.inverse()), "the seat sits where the spine sees it");
        assert!(same_pose(member(bumper).in_hub, spine_in_seat.inverse() * bumper_in_seat), "the bumper through the seat's frame");
    }

    #[test]
    fn welds_in_a_chain_compose_into_the_hubs_frame() {
        let e = some_entities(3);
        let bodies = [weld_body_of(e[0], 100.0), weld_body_of(e[1], 10.0), weld_body_of(e[2], 1.0)];
        let ab = Isometry3d::new(Vec3::new(1.0, 0.0, 0.0), Quat::from_rotation_z(0.5));
        let bc = Isometry3d::new(Vec3::new(0.0, 2.0, 0.0), Quat::from_rotation_x(0.25));
        let edges = [WeldEdge { a: e[0], b: e[1], b_in_a: ab }, WeldEdge { a: e[1], b: e[2], b_in_a: bc }];
        let plans = plan_groups(&bodies, &edges);
        let GroupPlan::Merged { hub, members, .. } = &plans[0] else { panic!("one body") };
        assert_eq!(*hub, e[0]);
        let c = members.iter().find(|m| m.entity == e[2]).unwrap();
        assert!(same_pose(c.in_hub, ab * bc));
    }

    #[test]
    fn a_group_with_a_static_body_keeps_one_joint_per_weld() {
        let e = some_entities(3);
        let anchored = WeldBody { dynamic: false, ..weld_body_of(e[0], 0.0) };
        let bodies = [anchored, weld_body_of(e[1], 50.0), weld_body_of(e[2], 5.0)];
        let edges = [
            WeldEdge { a: e[0], b: e[1], b_in_a: at(0.0, 1.0, 0.0) },
            WeldEdge { a: e[1], b: e[2], b_in_a: at(0.0, 1.0, 0.0) },
        ];
        let plans = plan_groups(&bodies, &edges);
        assert_eq!(plans.len(), 1);
        assert!(matches!(&plans[0], GroupPlan::PerEdge { edges } if edges.len() == 2), "{:?}", plans[0]);
    }

    #[test]
    fn groups_that_share_no_body_stay_apart() {
        let e = some_entities(4);
        let bodies = [weld_body_of(e[0], 5.0), weld_body_of(e[1], 9.0), weld_body_of(e[2], 7.0), weld_body_of(e[3], 3.0)];
        let edges = [
            WeldEdge { a: e[0], b: e[1], b_in_a: at(1.0, 0.0, 0.0) },
            WeldEdge { a: e[2], b: e[3], b_in_a: at(1.0, 0.0, 0.0) },
        ];
        let hubs: Vec<Entity> = plan_groups(&bodies, &edges)
            .iter()
            .map(|p| match p {
                GroupPlan::Merged { hub, .. } => *hub,
                other => panic!("one body, not {other:?}"),
            })
            .collect();
        assert_eq!(hubs, vec![e[1], e[2]]);
    }

    #[test]
    fn a_weld_between_nothing_known_makes_no_group() {
        let e = some_entities(2);
        assert!(plan_groups(&[], &[WeldEdge { a: e[0], b: e[1], b_in_a: Isometry3d::IDENTITY }]).is_empty());
        assert!(plan_groups(&[weld_body_of(e[0], 1.0)], &[]).is_empty());
    }

    /// The Chassis Playground Coupe: (name, centre, size, kg). A 771 kg spine
    /// and a 390 kg deck, the 30 kg seat, and light parts around them.
    const COUPE: &[(&str, [f32; 3], [f32; 3], f32)] = &[
        ("BumperFront", [0.0, 0.62, 2.25], [1.9, 0.35, 0.2], 20.0),
        ("BumperRear", [0.0, 0.62, -2.25], [1.9, 0.35, 0.2], 20.0),
        ("Deck", [0.0, 0.975, 0.0], [1.9, 0.25, 4.3], 390.0),
        ("Hood", [0.0, 1.25, 1.45], [1.8, 0.3, 1.3], 14.0),
        ("RollHoop", [0.0, 1.55, -0.75], [1.6, 0.9, 0.1], 5.0),
        ("Spine", [0.0, 0.62, 0.0], [0.9, 0.3, 4.0], 771.0),
        ("Trunk", [0.0, 1.25, -1.75], [1.8, 0.3, 0.9], 10.0),
        ("VehicleSeat", [0.0, 1.2, -0.2], [0.6, 0.2, 0.6], 30.0),
    ];
    const COUPE_SPINE_Y: f32 = 0.62;

    struct Coupe {
        /// The body the group planned as its hub.
        hub: Entity,
        /// Every welded part and where the hub should hold it.
        parts: Vec<(Entity, Isometry3d)>,
        /// The rear axles, which the throttle drives.
        axles: Vec<Entity>,
    }

    fn pose_of(app: &App, e: Entity) -> Isometry3d {
        let w = app.world();
        Isometry3d::new(w.get::<Position>(e).unwrap().0, w.get::<Rotation>(e).unwrap().0)
    }

    /// A pose frame whose X axis is `axis` and Y axis `secondary`.
    fn frame_rotation(axis: Vec3, secondary: Vec3) -> Quat {
        let x = axis.normalize();
        let z = x.cross(secondary).normalize();
        Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z))
    }

    /// The Coupe the way a script builds it: every part welded to the 30 kg
    /// seat, and the suspension hung on the seat. The weld group is one body,
    /// the spine's, and the suspension follows the seat there (`body_of`).
    fn spawn_welded_coupe(app: &mut App) -> Coupe {
        const R: f32 = 0.33;
        let world = app.world_mut();
        world.spawn((RigidBody::Static, Collider::cuboid(600.0, 1.0, 600.0), Transform::from_xyz(0.0, -0.5, 0.0), Friction::new(1.0)));
        let mut bodies = Vec::new();
        let mut seat = None;
        for (name, p, s, kg) in COUPE {
            let id = world
                .spawn((
                    RigidBody::Dynamic,
                    Collider::cuboid(s[0], s[1], s[2]),
                    ColliderDensity(kg / (s[0] * s[1] * s[2])),
                    Friction::new(1.0),
                    Transform::from_xyz(p[0], p[1], p[2]),
                    SleepingDisabled,
                ))
                .id();
            if *name == "VehicleSeat" {
                seat = Some(id);
            }
            bodies.push(id);
        }
        let seat = seat.unwrap();
        // Four 35 kg ball wheels and, for each, a 19.2 kg strut (and a knuckle at
        // the front) that carries no collision and weighs through an explicit Mass.
        let carrier = |world: &mut World, at: Vec3| {
            let s = 0.4_f32;
            world
                .spawn((
                    RigidBody::Dynamic,
                    Collider::cuboid(s, s, s),
                    Sensor,
                    Mass(19.2),
                    AngularInertia::new(Vec3::splat(19.2 * 2.0 * s * s / 12.0)),
                    CenterOfMass(Vec3::ZERO),
                    Transform::from_translation(at),
                    SleepingDisabled,
                ))
                .id()
        };
        let mut corners = Vec::new();
        for (x, z, front) in [(0.85, 1.3, true), (-0.85, 1.3, true), (0.85, -1.3, false), (-0.85, -1.3, false)] {
            let at = Vec3::new(x, R + 0.005, z);
            let wheel_volume = 4.0 / 3.0 * std::f32::consts::PI * R * R * R;
            let wheel = world
                .spawn((RigidBody::Dynamic, Collider::sphere(R), ColliderDensity(35.0 / wheel_volume), Friction::new(1.0), Transform::from_translation(at), SleepingDisabled))
                .id();
            let strut = carrier(world, at);
            let knuckle = front.then(|| carrier(world, at));
            corners.push((at, wheel, strut, knuckle, front));
        }
        run(app, 3);

        // The script's welds: every part to the seat. The bodies have their masses by now.
        let weld_bodies: Vec<WeldBody> = bodies.iter().map(|&b| weld_body(app.world(), b)).collect();
        let weld_edges: Vec<WeldEdge> = bodies
            .iter()
            .filter(|&&b| b != seat)
            .map(|&b| WeldEdge { a: seat, b, b_in_a: pose_of(app, seat).inverse() * pose_of(app, b) })
            .collect();
        let plans = plan_groups(&weld_bodies, &weld_edges);
        let GroupPlan::Merged { hub, members, .. } = plans.into_iter().next().expect("one group") else { panic!("one body") };
        let mut parts = Vec::new();
        for member in &members {
            parts.push((member.entity, member.in_hub));
            crate::assemblies::merge(app.world_mut(), hub, member.entity, member.in_hub);
        }
        // Everything the script hung on the seat acts on the hub.
        let on_hub = |app: &App, at: Vec3, rotation: Quat| pose_of(app, hub).inverse() * Isometry3d::new(at, rotation);
        let up = frame_rotation(Vec3::Y, Vec3::Z);
        let (load, g) = (1260.0 / 4.0, 9.81_f32);
        let stiffness = load * (2.0 * std::f32::consts::PI * 1.8).powi(2);
        let damping = 2.0 * 0.45 * (stiffness * load).sqrt();
        let sag = load * g / stiffness;
        let mut axles = Vec::new();
        for (at, wheel, strut, knuckle, front) in corners {
            let top = on_hub(app, at + Vec3::Y * 0.6, up);
            let bottom = pose_of(app, strut).inverse() * Isometry3d::new(at, up);
            let mut slide = PrismaticJoint::new(hub, strut)
                .with_local_frame1(on_hub(app, at, up))
                .with_local_frame2(bottom)
                .with_slider_axis(Vec3::X);
            slide.limits = Some(DistanceLimit::new(-0.4 * R, 0.33 * R));
            app.world_mut().spawn((slide, JointCollisionDisabled));
            app.world_mut().spawn((
                SpringJoint {
                    body1: hub,
                    body2: strut,
                    local_anchor1: Vec3::from(top.translation),
                    local_anchor2: Vec3::from(bottom.translation),
                    free_length: 0.6 + sag,
                    stiffness,
                    damping,
                },
                JointCollisionDisabled,
            ));
            let mut carried = strut;
            if let Some(knuckle) = knuckle {
                let (a, b) = (pose_of(app, strut).inverse() * Isometry3d::new(at, up), pose_of(app, knuckle).inverse() * Isometry3d::new(at, up));
                let mut steer = RevoluteJoint::new(strut, knuckle).with_local_frame1(a).with_local_frame2(b).with_hinge_axis(Vec3::X);
                steer.motor = AngularMotor::new(servo_model()).with_target_position(0.0).with_max_torque(1.0e6);
                app.world_mut().spawn((steer, JointCollisionDisabled));
                carried = knuckle;
            }
            let along_x = frame_rotation(Vec3::X, Vec3::Y);
            let (a, b) = (pose_of(app, carried).inverse() * Isometry3d::new(at, along_x), pose_of(app, wheel).inverse() * Isometry3d::new(at, along_x));
            let axle = RevoluteJoint::new(carried, wheel).with_local_frame1(a).with_local_frame2(b).with_hinge_axis(Vec3::X);
            let axle = app.world_mut().spawn((axle, JointCollisionDisabled)).id();
            if !front {
                axles.push(axle);
            }
        }
        Coupe { hub, parts, axles }
    }

    /// How far the worst welded part has strayed from where its hub holds it.
    fn coupe_drift(app: &App, coupe: &Coupe) -> f32 {
        let hub = pose_of(app, coupe.hub);
        coupe.parts.iter().map(|(part, in_hub)| Vec3::from((hub * *in_hub).translation).distance(Vec3::from(pose_of(app, *part).translation))).fold(0.0, f32::max)
    }

    /// The Chassis Playground Coupe exploded in Play on build 14: a 30 kg seat
    /// at the hub of seven welds, with the suspension on the seat, drifted 18 cm
    /// at rest and threw a bumper off at 89 m/s within two seconds of throttle.
    /// Welded as the script writes it and merged by the weld groups, it is one
    /// 1260 kg body on the spine, settles on its springs, stays together, and
    /// drives.
    #[test]
    fn a_coupe_welded_through_its_seat_settles_stays_together_and_drives() {
        let mut app = physics_world();
        let coupe = spawn_welded_coupe(&mut app);
        run(&mut app, 2);
        let spine_mass = app.world().get::<ComputedMass>(coupe.hub).map_or(0.0, |m| m.value());
        assert!((spine_mass - 1260.0).abs() < 1.0, "the spine's body weighs the whole 1260 kg group, not {spine_mass} kg");
        for (part, _) in &coupe.parts {
            assert_eq!(app.world().get::<ColliderOf>(*part).map(|c| c.body), Some(coupe.hub), "every part's collider is on the spine's body");
        }
        run(&mut app, 180);
        let ride = pose_of(&app, coupe.hub).translation.y - COUPE_SPINE_Y;
        assert!(ride.abs() < 0.05, "rides at its design height, {ride:+.3} m off");
        assert!(coupe_drift(&app, &coupe) < 0.005, "welds hold at rest, {:.1} mm", coupe_drift(&app, &coupe) * 1000.0);
        let rest_speed = app.world().get::<LinearVelocity>(coupe.hub).unwrap().0.length();
        assert!(rest_speed < 0.05, "at rest, not {rest_speed} m/s");

        // 5 m/s^2 asked of the rear wheels: 1250 N m each on 0.33 m wheels.
        for &axle in &coupe.axles {
            let mut joint = app.world_mut().get_mut::<RevoluteJoint>(axle).unwrap();
            joint.motor = AngularMotor::new(velocity_model()).with_target_velocity(55.0 / 0.33).with_max_torque(5.0 * 1515.0 * 0.33 / 2.0);
        }
        let mut worst_drift = 0.0_f32;
        for _ in 0..300 {
            app.update();
            worst_drift = worst_drift.max(coupe_drift(&app, &coupe));
            let hub_speed = app.world().get::<LinearVelocity>(coupe.hub).unwrap().0.length();
            for (part, _) in &coupe.parts {
                let speed = app.world().get::<LinearVelocity>(*part).unwrap().0.length();
                assert!(speed < hub_speed + 5.0, "a part flew off at {speed} m/s with the hub at {hub_speed}");
            }
        }
        let speed = app.world().get::<LinearVelocity>(coupe.hub).unwrap().0.length();
        assert!(speed > 15.0, "five seconds of throttle reached {speed} m/s");
        assert!(worst_drift < 0.01, "welds held while driving, {:.1} mm", worst_drift * 1000.0);
    }

    // ── Collision-disabled joints and Avian's islands ───────────────────

    /// Two touching cubes settle into one island, `join` puts a joint with
    /// collisions off between them, and a third cube lands on the stack. Stock
    /// Avian 0.7.0 cut the pair's contacts out of its contact graph when the
    /// marker was added but not out of the island they were linked in, so the
    /// landing unwrapped a missing contact in `islands/mod.rs` and panicked: a
    /// welded Coupe crashed the engine that way a second after it was built. The
    /// workspace carries a patched Avian (`.patches/avian3d-0.7.0`); these tests
    /// fail on the stock crate, and a panic here fails them.
    fn a_stack_joined_with_collisions_off(join: impl FnOnce(&mut App, Entity, Entity)) {
        let mut app = physics_world();
        app.world_mut().spawn((RigidBody::Static, Collider::cuboid(50.0, 1.0, 50.0), Transform::from_xyz(0.0, -0.5, 0.0)));
        let cube = |app: &mut App, y: f32| app.world_mut().spawn((RigidBody::Dynamic, Collider::cuboid(1.0, 1.0, 1.0), Transform::from_xyz(0.0, y, 0.0))).id();
        let (a, b) = (cube(&mut app, 0.5), cube(&mut app, 1.5));
        run(&mut app, 30);
        join(&mut app, a, b);
        run(&mut app, 5);
        let c = cube(&mut app, 4.0);
        run(&mut app, 180);
        // With collisions off between them the two cubes can overlap: a weld pulls
        // their origins together and a slack joint lets the upper one sink into the
        // lower. Either way both end on the ground, and the third cube, whose
        // landing stock Avian panicked on, comes to rest on top of them.
        let height = |e: Entity| Vec3::from(pose_of(&app, e).translation).y;
        assert!((height(a) - 0.5).abs() < 0.05 && (height(b) - 0.5).abs() < 0.05, "both rest on the ground, a at {} and b at {}", height(a), height(b));
        assert!((height(c) - 1.5).abs() < 0.2, "the third cube rests on them, at {}", height(c));
    }

    #[test]
    fn a_weld_between_touching_bodies_leaves_no_stale_contact_in_an_island() {
        a_stack_joined_with_collisions_off(|app, a, b| {
            app.world_mut().spawn((FixedJoint::new(a, b), JointCollisionDisabled));
        });
    }

    #[test]
    fn a_no_collision_joint_between_touching_bodies_leaves_no_stale_contact_in_an_island() {
        a_stack_joined_with_collisions_off(|app, a, b| {
            app.world_mut().spawn((DistanceJoint::new(a, b).with_limits(0.0, 1.0e4), JointCollisionDisabled));
        });
    }

    #[test]
    fn the_marker_added_to_an_existing_joint_leaves_no_stale_contact_in_an_island() {
        a_stack_joined_with_collisions_off(|app, a, b| {
            let joint = app.world_mut().spawn(FixedJoint::new(a, b)).id();
            app.update();
            app.world_mut().entity_mut(joint).insert(JointCollisionDisabled);
        });
    }

    /// A weld group's hub comes from its parts' shapes and authored densities:
    /// the solver's own mass reads the same for a part made dynamic a moment ago.
    #[test]
    fn a_groups_hub_is_chosen_from_the_parts_authored_mass_not_the_solvers_current_figure() {
        use eustress_common::classes::{BasePart, Part, PartType};
        let mut world = World::new();
        let part = |world: &mut World, size: Vec3| {
            world
                .spawn((RigidBody::Dynamic, BasePart { size, ..Default::default() }, Part { shape: PartType::Block }, ComputedMass::new(1.0)))
                .id()
        };
        let (small, big) = (part(&mut world, Vec3::splat(1.0)), part(&mut world, Vec3::splat(4.0)));
        assert!(hub_weight(&world, big) > 10.0 * hub_weight(&world, small), "the big part weighs more, whatever the solver read");
        let bodies = [weld_body(&world, small), weld_body(&world, big)];
        let edges = [WeldEdge { a: small, b: big, b_in_a: Isometry3d::IDENTITY }];
        let GroupPlan::Merged { hub, .. } = &plan_groups(&bodies, &edges)[0] else { panic!("one body") };
        assert_eq!(*hub, big);
    }

    // ── Constraints read from the tree ──────────────────────────────────

    /// A Play session's tree over a physics world: [`ScriptedJointsPlugin`]
    /// reads it every frame, as in Studio's Play. Parts are drawn by hand (no
    /// apply step): each is a 1 m cube bound to its instance.
    struct Session {
        app: App,
        tree: eustress_common::datamodel::SharedDataModel,
        ws: InstanceId,
    }

    impl Session {
        fn new() -> Session {
            let mut app = physics_world();
            app.world_mut().spawn((RigidBody::Static, Collider::cuboid(100.0, 1.0, 100.0), Transform::from_xyz(0.0, -0.5, 0.0)));
            let tree = eustress_common::datamodel::new_shared();
            let ws = tree.lock().get_service("Workspace").expect("a Workspace");
            app.insert_resource(PlayDataModel { dm: tree.clone() });
            Session { app, tree, ws }
        }

        fn part(&mut self, at: Vec3, body: RigidBody) -> (InstanceId, Entity) {
            let e = self.app.world_mut().spawn((body, Collider::cuboid(1.0, 1.0, 1.0), Transform::from_translation(at))).id();
            let mut g = self.tree.lock();
            let id = g.create("Part");
            g.set_prop(id, "CFrame", DmValue::CFrame(eustress_common::scripting::CFrame::new(at.x as f64, at.y as f64, at.z as f64))).unwrap();
            g.set_parent(id, Some(self.ws)).unwrap();
            g.bind_entity(id, e.to_bits());
            (id, e)
        }

        /// A constraint of `class` in Workspace with these instance references.
        fn constraint(&mut self, class: &str, refs: &[(&str, InstanceId)]) -> InstanceId {
            let mut g = self.tree.lock();
            let c = g.create(class);
            for (prop, target) in refs {
                g.set_prop(c, prop, DmValue::Instance(*target)).unwrap();
            }
            g.set_parent(c, Some(self.ws)).unwrap();
            c
        }

        /// An Attachment on `part` at `offset` from its centre, X along `axis`.
        fn attachment(&mut self, part: InstanceId, offset: Vec3) -> InstanceId {
            let mut g = self.tree.lock();
            let a = g.create("Attachment");
            g.set_prop(a, "CFrame", DmValue::CFrame(eustress_common::scripting::CFrame::new(offset.x as f64, offset.y as f64, offset.z as f64))).unwrap();
            g.set_parent(a, Some(part)).unwrap();
            a
        }

        /// Four times as dense: the hub of any group it is in.
        fn heavy(&mut self, e: Entity) {
            self.app.world_mut().entity_mut(e).insert(ColliderDensity(4.0));
        }

        fn release(&mut self, e: Entity) {
            self.app.world_mut().entity_mut(e).insert(RigidBody::Dynamic);
        }

        fn anchor(&mut self, e: Entity) {
            self.app.world_mut().entity_mut(e).insert(RigidBody::Static);
        }

        fn run(&mut self, frames: usize) {
            run(&mut self.app, frames);
        }

        fn count<T: Component>(&mut self) -> usize {
            let world = self.app.world_mut();
            world.query::<&T>().iter(world).count()
        }

        fn at(&self, e: Entity) -> Vec3 {
            self.app.world().get::<Position>(e).unwrap().0
        }
    }

    /// Avian 0.7 panics linking a joint whose two bodies have no island (two
    /// anchored parts). The weld waits, and holds the part once it is released.
    #[test]
    fn a_weld_between_two_anchored_parts_waits_and_holds_once_one_is_released() {
        let mut s = Session::new();
        let (base, _) = s.part(Vec3::new(0.0, 3.0, 0.0), RigidBody::Static);
        let (lid, lid_e) = s.part(Vec3::new(0.0, 4.0, 0.0), RigidBody::Static);
        s.constraint("WeldConstraint", &[("Part0", base), ("Part1", lid)]);
        s.run(5);
        assert_eq!(s.count::<FixedJoint>(), 0, "no joint between two anchored parts");
        s.release(lid_e);
        s.run(60);
        assert_eq!(s.count::<FixedJoint>(), 1, "the weld once the lid simulates");
        assert!(s.at(lid_e).distance(Vec3::new(0.0, 4.0, 0.0)) < 0.01, "the lid stays welded where it was, at {}", s.at(lid_e));
    }

    /// The same for an attachment constraint: a hinge between two anchored parts
    /// waits, and a released arm then swings on it without leaving it.
    #[test]
    fn a_hinge_between_two_anchored_parts_waits_and_swings_once_one_is_released() {
        let mut s = Session::new();
        let (post, _) = s.part(Vec3::new(0.0, 3.0, 0.0), RigidBody::Static);
        let (arm, arm_e) = s.part(Vec3::new(1.0, 3.0, 0.0), RigidBody::Static);
        let (a0, a1) = (s.attachment(post, Vec3::new(0.5, 0.0, 0.0)), s.attachment(arm, Vec3::new(-0.5, 0.0, 0.0)));
        s.constraint("HingeConstraint", &[("Attachment0", a0), ("Attachment1", a1)]);
        s.run(5);
        assert_eq!(s.count::<RevoluteJoint>(), 0);
        s.release(arm_e);
        s.run(90);
        assert_eq!(s.count::<RevoluteJoint>(), 1);
        let pivot = Vec3::new(0.5, 3.0, 0.0);
        assert!((s.at(arm_e).distance(pivot) - 0.5).abs() < 0.02, "the arm hangs half a metre from the pivot, at {}", s.at(arm_e));
        assert!(s.at(arm_e).y < 3.0, "and has swung down, to {}", s.at(arm_e));
    }

    /// A model welded while anchored is one assembly once released: the group
    /// is planned again when its bodies change, and merges into one body.
    #[test]
    fn a_model_welded_while_anchored_is_one_assembly_once_released() {
        let mut s = Session::new();
        let (a, a_e) = s.part(Vec3::new(0.0, 3.0, 0.0), RigidBody::Static);
        let (b, b_e) = s.part(Vec3::new(1.0, 3.0, 0.0), RigidBody::Static);
        let (c, c_e) = s.part(Vec3::new(2.0, 3.0, 0.0), RigidBody::Static);
        s.constraint("WeldConstraint", &[("Part0", a), ("Part1", b)]);
        s.constraint("WeldConstraint", &[("Part0", b), ("Part1", c)]);
        s.run(5);
        assert_eq!(s.count::<FixedJoint>(), 0);
        for e in [a_e, b_e, c_e] {
            s.release(e);
        }
        s.run(120);
        assert_eq!(s.count::<AssemblyMember>(), 2, "two members on one hub");
        let (pa, pb, pc) = (s.at(a_e), s.at(b_e), s.at(c_e));
        assert!(pa.y < 1.0, "the model fell to the ground, at {pa}");
        assert!((pb - pa).distance(Vec3::X) < 0.01 && (pc - pa).distance(2.0 * Vec3::X) < 0.01, "and stayed one rigid piece: {pa} {pb} {pc}");
    }

    /// A welded group anchored whole (SimChassis anchors a runaway car) and then
    /// planned again (a weld changes) waits instead of panicking, and is one
    /// assembly again once released.
    #[test]
    fn a_welded_group_anchored_whole_and_planned_again_does_not_panic() {
        let mut s = Session::new();
        let (a, a_e) = s.part(Vec3::new(0.0, 0.5, 0.0), RigidBody::Dynamic);
        let (b, b_e) = s.part(Vec3::new(1.0, 0.5, 0.0), RigidBody::Dynamic);
        s.constraint("WeldConstraint", &[("Part0", a), ("Part1", b)]);
        s.run(10);
        assert_eq!(s.count::<AssemblyMember>(), 1, "one body");
        s.anchor(a_e);
        s.anchor(b_e);
        s.run(5);
        s.constraint("WeldConstraint", &[("Part0", b), ("Part1", a)]);
        s.run(5);
        assert_eq!(s.count::<FixedJoint>(), 0, "no joint between the anchored pair");
        assert_eq!(s.count::<AssemblyMember>(), 0, "and no merged part");
        s.release(a_e);
        s.release(b_e);
        s.run(10);
        assert_eq!(s.count::<AssemblyMember>(), 1, "one assembly again");
    }

    /// Anchoring a moving group's hub, as the script's Anchored write does (in
    /// Update, before the joints read it): a part it frees must not keep the speed
    /// it carried. It takes the hub's velocity at its place, which is zero once the
    /// hub is anchored, so it is at rest the moment it is freed. Stock behaviour
    /// left it moving at the car's speed into the new joints, which absorbed that
    /// speed in one physics step.
    #[test]
    fn a_part_freed_by_its_hub_being_anchored_takes_the_hubs_velocity() {
        #[derive(Resource, Default)]
        struct Anchor(Option<Entity>);
        fn anchor_in_update(world: &mut World) {
            let Some(hub) = world.resource_mut::<Anchor>().0.take() else {
                return;
            };
            world.entity_mut(hub).insert(RigidBody::Static);
            if let Some(mut v) = world.get_mut::<LinearVelocity>(hub) {
                v.0 = Vec3::ZERO;
            }
            if let Some(mut w) = world.get_mut::<AngularVelocity>(hub) {
                w.0 = Vec3::ZERO;
            }
        }
        let mut s = Session::new();
        let (a, a_e) = s.part(Vec3::new(0.0, 3.0, 0.0), RigidBody::Dynamic);
        let (b, b_e) = s.part(Vec3::new(1.0, 3.0, 0.0), RigidBody::Dynamic);
        s.heavy(a_e);
        s.app.init_resource::<Anchor>().add_systems(Update, anchor_in_update.before(note_body_changes));
        s.run(2);
        for e in [a_e, b_e] {
            s.app.world_mut().get_mut::<LinearVelocity>(e).unwrap().0 = Vec3::X * 8.0;
        }
        s.constraint("WeldConstraint", &[("Part0", a), ("Part1", b)]);
        s.run(3);
        assert_eq!(s.app.world().get::<AssemblyMember>(b_e).map(|m| m.hub), Some(a_e), "b is merged into a");
        s.app.world_mut().resource_mut::<Anchor>().0 = Some(a_e);
        s.run(1);
        assert_eq!(s.count::<AssemblyMember>(), 0, "the group split");
        let v = s.app.world().get::<LinearVelocity>(b_e).unwrap().0;
        assert!(v.length() < 1e-3, "the freed part is at rest, not moving at {v:?}");
    }

    /// A weld the groups do not use (a SeatWeld, a weld of a part to itself)
    /// coming and going leaves the groups as they are: the merged part is never
    /// split and merged again.
    #[test]
    fn a_weld_that_changes_no_group_rebuilds_nothing() {
        let mut s = Session::new();
        let (a, a_e) = s.part(Vec3::new(0.0, 0.5, 0.0), RigidBody::Dynamic);
        let (b, b_e) = s.part(Vec3::new(1.0, 0.5, 0.0), RigidBody::Dynamic);
        s.heavy(a_e);
        s.run(2);
        s.constraint("WeldConstraint", &[("Part0", a), ("Part1", b)]);
        s.run(10);
        let merged_at = |s: &Session| s.app.world().entity(b_e).get_change_ticks::<AssemblyMember>().map(|t| t.added);
        let first = merged_at(&s);
        assert!(first.is_some(), "the pair is one body");
        let idle = s.constraint("WeldConstraint", &[("Part0", a), ("Part1", a)]);
        s.run(3);
        s.tree.lock().destroy(idle);
        s.run(3);
        assert_eq!(merged_at(&s), first, "the same merge, never split and made again");
    }

    fn mass_of(s: &Session, e: Entity) -> f32 {
        s.app.world().get::<ComputedMass>(e).map_or(f32::NAN, |m| m.value())
    }

    fn pose_in(s: &Session, e: Entity) -> Isometry3d {
        let w = s.app.world();
        Isometry3d::new(w.get::<Position>(e).unwrap().0, w.get::<Rotation>(e).unwrap().0)
    }

    /// Two welded cubes are one body: the heavier (the hub) weighs both, the
    /// other's collider is on it and its own body is off, and the pair lands on
    /// the ground with the second exactly where the weld put it.
    #[test]
    fn a_welded_pair_is_one_body_that_weighs_both() {
        let mut s = Session::new();
        let (a, a_e) = s.part(Vec3::new(0.0, 3.0, 0.0), RigidBody::Dynamic);
        let (b, b_e) = s.part(Vec3::new(1.0, 3.5, 0.0), RigidBody::Dynamic);
        s.heavy(a_e);
        s.run(3);
        let both = mass_of(&s, a_e) + mass_of(&s, b_e);
        s.constraint("WeldConstraint", &[("Part0", a), ("Part1", b)]);
        s.run(120);
        let w = s.app.world();
        assert_eq!(w.get::<AssemblyMember>(b_e).map(|m| m.hub), Some(a_e), "b is merged into a");
        assert_eq!(w.get::<ColliderOf>(b_e).map(|c| c.body), Some(a_e));
        assert!(w.get::<RigidBodyDisabled>(b_e).is_some(), "b's own body is off");
        assert!((mass_of(&s, a_e) - both).abs() < 1e-3 * both, "a weighs both, {} of {both}", mass_of(&s, a_e));
        assert!(s.at(a_e).y < 1.6, "the pair fell and rests, a at {}", s.at(a_e));
        let held = pose_in(&s, a_e).inverse() * pose_in(&s, b_e);
        assert!(Vec3::from(held.translation).distance(Vec3::new(1.0, 0.5, 0.0)) < 1e-4, "b is where the weld put it: {held:?}");
    }

    /// Anchoring a merged part splits its group: the part is its own (static)
    /// body again, and its weld to the dynamic part becomes a joint.
    #[test]
    fn anchoring_a_merged_part_splits_its_group() {
        let mut s = Session::new();
        let (a, a_e) = s.part(Vec3::new(0.0, 0.5, 0.0), RigidBody::Dynamic);
        let (b, b_e) = s.part(Vec3::new(1.0, 0.5, 0.0), RigidBody::Dynamic);
        s.heavy(a_e);
        s.run(2);
        let alone = mass_of(&s, a_e);
        s.constraint("WeldConstraint", &[("Part0", a), ("Part1", b)]);
        s.run(10);
        assert_eq!(s.app.world().get::<AssemblyMember>(b_e).map(|m| m.hub), Some(a_e));
        s.anchor(b_e);
        s.run(10);
        assert_eq!(s.count::<AssemblyMember>(), 0, "the group split");
        assert_eq!(s.count::<FixedJoint>(), 1, "the weld is a joint to the anchored part");
        let w = s.app.world();
        assert!(w.get::<RigidBodyDisabled>(b_e).is_none() && w.get::<ManualColliderTransform>(b_e).is_none());
        assert_eq!(w.get::<ColliderOf>(b_e).map(|c| c.body), Some(b_e), "b's collider is its own body's again");
        assert!((mass_of(&s, a_e) - alone).abs() < 1e-3 * alone, "a weighs itself alone again, {} of {alone}", mass_of(&s, a_e));
    }

    /// The Space's own weld (the loader's joint, with its instance) joins the
    /// groups like a script's: switched off while its pair is one body, and on
    /// again, with every part its own body, once the session ends.
    #[test]
    fn a_loaded_weld_is_one_body_in_play_and_a_joint_again_after() {
        let mut s = Session::new();
        let (_, a_e) = s.part(Vec3::new(0.0, 0.5, 0.0), RigidBody::Dynamic);
        let (_, b_e) = s.part(Vec3::new(1.0, 0.5, 0.0), RigidBody::Dynamic);
        s.heavy(a_e);
        s.run(2);
        let weld = s
            .app
            .world_mut()
            .spawn((
                eustress_common::classes::Instance {
                    name: "Weld".into(),
                    class_name: eustress_common::classes::ClassName::WeldConstraint,
                    archivable: true,
                    id: 0,
                    uuid: String::new(),
                    ai: false,
                },
                FixedJoint::new(a_e, b_e).with_local_frame1(Isometry3d::from_translation(Vec3::X)).with_local_frame2(Isometry3d::IDENTITY),
                JointCollisionDisabled,
            ))
            .id();
        s.run(10);
        assert_eq!(s.app.world().get::<AssemblyMember>(b_e).map(|m| m.hub), Some(a_e), "the loaded weld's pair is one body");
        assert!(s.app.world().get::<JointDisabled>(weld).is_some(), "and its joint is off while it is");
        s.app.world_mut().remove_resource::<PlayDataModel>();
        s.run(2);
        let w = s.app.world();
        assert!(w.get::<JointDisabled>(weld).is_none(), "the joint is on again after the session");
        assert!(w.get::<AssemblyMember>(b_e).is_none() && w.get::<RigidBodyDisabled>(b_e).is_none(), "b is its own body again");
    }

    /// A loaded hinge on a merged part is carried by the hub at the same place
    /// in the world, and handed back to the part after the session.
    #[test]
    fn a_loaded_hinge_on_a_merged_part_is_carried_by_its_hub() {
        let mut s = Session::new();
        let (a, a_e) = s.part(Vec3::new(0.0, 0.5, 0.0), RigidBody::Dynamic);
        let (b, b_e) = s.part(Vec3::new(1.0, 0.5, 0.0), RigidBody::Dynamic);
        let (_, wheel) = s.part(Vec3::new(2.0, 0.5, 0.0), RigidBody::Dynamic);
        s.heavy(a_e);
        let on_b = Isometry3d::from_translation(Vec3::new(0.5, 0.0, 0.0));
        let hinge = s
            .app
            .world_mut()
            .spawn((
                eustress_common::classes::Instance {
                    name: "Hinge".into(),
                    class_name: eustress_common::classes::ClassName::HingeConstraint,
                    archivable: true,
                    id: 0,
                    uuid: String::new(),
                    ai: false,
                },
                RevoluteJoint::new(b_e, wheel).with_local_frame1(on_b).with_local_frame2(Isometry3d::from_translation(Vec3::new(-0.5, 0.0, 0.0))),
                JointCollisionDisabled,
            ))
            .id();
        s.run(2);
        s.constraint("WeldConstraint", &[("Part0", a), ("Part1", b)]);
        s.run(1);
        let w = s.app.world();
        let joint = w.get::<RevoluteJoint>(hinge).unwrap();
        assert_eq!(joint.body1, a_e, "the hub carries the hinge");
        let carried = pose_in(&s, a_e) * joint.local_frame1().unwrap();
        let meant = pose_in(&s, b_e) * on_b;
        assert!(Vec3::from(carried.translation).distance(Vec3::from(meant.translation)) < 1e-4, "at the same place: {carried:?} {meant:?}");
        s.app.world_mut().remove_resource::<PlayDataModel>();
        s.run(1);
        let joint = s.app.world().get::<RevoluteJoint>(hinge).unwrap();
        assert_eq!(joint.body1, b_e, "the part has its hinge back");
        assert!(Vec3::from(joint.local_frame1().unwrap().translation).distance(Vec3::new(0.5, 0.0, 0.0)) < 1e-4, "at its own frame");
    }

    /// The joint trace, switched on, follows a pair merging and falling: it
    /// watches the steps after the plan and names the merged part as merged.
    #[test]
    fn the_joint_trace_follows_a_merging_pair() {
        let mut s = Session::new();
        let (a, a_e) = s.part(Vec3::new(0.0, 3.0, 0.0), RigidBody::Dynamic);
        let (b, b_e) = s.part(Vec3::new(1.0, 3.0, 0.0), RigidBody::Dynamic);
        s.heavy(a_e);
        s.run(2);
        s.app.world_mut().resource_mut::<crate::joint_trace::JointTrace>().on = true;
        s.constraint("WeldConstraint", &[("Part0", a), ("Part1", b)]);
        s.run(30);
        let described = crate::joint_trace::describe(s.app.world(), b_e);
        assert!(described.contains("merged into") && described.contains("body off"), "{described}");
        assert!(crate::joint_trace::joints_on(s.app.world_mut(), &[a_e, b_e]).is_empty(), "a merged pair has no joint");
    }

    /// Something pushing a merged part (a mover, a loaded `VectorForce`) pushes
    /// its group: a constant force on the light part lifts the whole pair, and a
    /// velocity written on it moves the pair. Gravity is not counted twice.
    #[test]
    fn a_force_or_velocity_on_a_merged_part_moves_its_group() {
        let mut s = Session::new();
        let (a, a_e) = s.part(Vec3::new(0.0, 5.0, 0.0), RigidBody::Dynamic);
        let (b, b_e) = s.part(Vec3::new(1.0, 5.0, 0.0), RigidBody::Dynamic);
        s.heavy(a_e);
        s.run(2);
        let both = mass_of(&s, a_e) + mass_of(&s, b_e);
        s.constraint("WeldConstraint", &[("Part0", a), ("Part1", b)]);
        s.run(2);
        assert_eq!(s.app.world().get::<AssemblyMember>(b_e).map(|m| m.hub), Some(a_e));
        // Twice the pair's weight, up, on the light part: the pair rises at 1 g.
        s.app.world_mut().entity_mut(b_e).insert(ConstantForce(Vec3::Y * 2.0 * both * 9.81));
        let v0 = s.app.world().get::<LinearVelocity>(a_e).unwrap().0.y;
        s.run(30);
        let v1 = s.app.world().get::<LinearVelocity>(a_e).unwrap().0.y;
        let rise = (v1 - v0) / 0.5;
        assert!((rise - 9.81).abs() < 0.5, "the pair rises at {rise} m/s^2, 9.81 meant");
        s.app.world_mut().entity_mut(b_e).remove::<ConstantForce>();
        // A sideways velocity written on the light part: the pair takes its share.
        s.run(1);
        let hub_before = s.app.world().get::<LinearVelocity>(a_e).unwrap().0.x;
        let light = mass_of(&s, b_e);
        s.app.world_mut().get_mut::<LinearVelocity>(b_e).unwrap().0.x += 10.0;
        s.run(1);
        let gained = s.app.world().get::<LinearVelocity>(a_e).unwrap().0.x - hub_before;
        assert!((gained - 10.0 * light / both).abs() < 0.05 * 10.0 * light / both + 1e-3, "the pair gained {gained} m/s sideways, {} meant", 10.0 * light / both);
    }

    /// A hub destroyed (a script's `Destroy`) leaves its merged parts standing:
    /// stock Avian despawned every collider attached to a body with it. The
    /// part that is left is its own body again and falls.
    #[test]
    fn despawning_a_hub_leaves_its_merged_parts() {
        let mut s = Session::new();
        let (a, a_e) = s.part(Vec3::new(0.0, 3.0, 0.0), RigidBody::Dynamic);
        let (b, b_e) = s.part(Vec3::new(1.0, 3.0, 0.0), RigidBody::Dynamic);
        s.heavy(a_e);
        s.run(2);
        s.constraint("WeldConstraint", &[("Part0", a), ("Part1", b)]);
        s.run(3);
        assert_eq!(s.app.world().get::<AssemblyMember>(b_e).map(|m| m.hub), Some(a_e));
        s.tree.lock().unbind_entity(a);
        s.app.world_mut().entity_mut(a_e).despawn();
        s.run(60);
        let w = s.app.world();
        assert!(w.get_entity(b_e).is_ok(), "the merged part outlives its hub");
        assert!(w.get::<AssemblyMember>(b_e).is_none() && w.get::<RigidBodyDisabled>(b_e).is_none(), "and is its own body again");
        assert!(s.at(b_e).y < 1.0, "which fell to the ground, at {}", s.at(b_e));
    }

    /// A WeldConstraint keeps the offset it was made with: re-planned later (a
    /// part spawned, a weld added), it never re-welds its parts where they have
    /// drifted to. Its parts changing takes a new one.
    #[test]
    fn a_weld_constraint_keeps_the_offset_it_was_made_with() {
        let mut world = World::new();
        let (p0, p1) = (world.spawn(Transform::from_xyz(0.0, 0.0, 0.0)).id(), world.spawn(Transform::from_xyz(1.0, 0.0, 0.0)).id());
        let mut g = DataModel::new();
        let ws = g.get_service("Workspace").unwrap();
        let (i0, i1) = (g.create("Part"), g.create("Part"));
        for (i, e) in [(i0, p0), (i1, p1)] {
            g.set_parent(i, Some(ws)).unwrap();
            g.bind_entity(i, e.to_bits());
        }
        let weld = g.create("WeldConstraint");
        let mut held = None;
        let (_, made) = weld_frames(&g, &world, weld, "WeldConstraint", i0, i1, &mut held).unwrap();
        world.get_mut::<Transform>(p1).unwrap().translation.x = 1.3;
        let (_, later) = weld_frames(&g, &world, weld, "WeldConstraint", i0, i1, &mut held).unwrap();
        assert!(same_pose(made, later), "the offset it was made with, not the drifted one");
        held = None;
        let (_, rebound) = weld_frames(&g, &world, weld, "WeldConstraint", i0, i1, &mut held).unwrap();
        assert!((Vec3::from(rebound.translation).x + 1.3).abs() < 1e-5, "a new one once its parts change: {rebound:?}");
    }

    // ── The vendored Avian's contacts on fast-spinning bodies ───────────

    /// A ball on a flat road, rolling or spun: the lowest its centre gets after
    /// the first few frames, and its speed after five seconds.
    fn roll(velocity: Vec3, spin: Vec3) -> (f32, f32) {
        let mut app = physics_world();
        app.world_mut().spawn((RigidBody::Static, Collider::cuboid(400.0, 2.0, 400.0), Transform::from_xyz(0.0, -1.0, 0.0)));
        let ball = app
            .world_mut()
            .spawn((
                RigidBody::Dynamic,
                Collider::sphere(0.33),
                Friction::new(1.4),
                Transform::from_xyz(0.0, 0.331, 0.0),
                LinearVelocity(velocity),
                AngularVelocity(spin),
            ))
            .id();
        let mut lowest = f32::MAX;
        for frame in 0..300 {
            app.update();
            if frame > 10 {
                lowest = lowest.min(pose_of(&app, ball).translation.y);
            }
        }
        (lowest, app.world().get::<LinearVelocity>(ball).unwrap().0.length())
    }

    /// Stock Avian 0.7.0 turned each contact point with its body when it
    /// estimated the separation within a step: a ball rolling at 30 m/s turns
    /// 26 degrees a substep, read as lifted off the road, lost its normal force
    /// for the rest of the step, sank 1.3 cm and slowed. The vendored copy
    /// (`.patches/avian3d-0.7.0/PATCH.md`) counts a contact point's turn only
    /// where it takes the contact deeper.
    #[test]
    fn a_ball_rolling_at_speed_stays_on_the_road() {
        let (lowest, speed) = roll(Vec3::new(0.0, 0.0, 30.0), Vec3::new(30.0 / 0.33, 0.0, 0.0));
        assert!(lowest > 0.33 - 0.002, "the ball sank into the road: its centre went down to {lowest}");
        assert!(speed > 29.9, "rolling on a flat road keeps its speed, not {speed}");
    }

    /// A wheel spinning on the spot (200 rad/s) grips the road and turns its
    /// spin into speed without sinking; stock 0.7.0 let it sink 5 cm.
    #[test]
    fn a_ball_spun_on_the_road_grips_without_sinking() {
        let (lowest, speed) = roll(Vec3::ZERO, Vec3::new(200.0, 0.0, 0.0));
        assert!(lowest > 0.33 - 0.003, "the spinning ball sank into the road, its centre to {lowest}");
        assert!(speed > 15.0, "friction turned the spin into speed, {speed} m/s");
    }

    /// A ground 4 km across, as a Space's baseplate can be.
    fn wide_ground(app: &mut App) {
        app.world_mut().spawn((
            RigidBody::Static,
            Collider::cuboid(4000.0, 2.0, 4000.0),
            Transform::from_xyz(0.0, -1.0, 0.0),
            Friction::new(1.0),
        ));
    }

    /// A 35 kg cylinder wheel rolled on its tread at 30 m/s across a wide
    /// ground. Stock 0.7.0 sank it 2.6 cm (the contact turn above); and with
    /// that fixed, parry's GJK, measuring a sub-millimetre gap from the ground's
    /// corners 2 km away, read normals up to 32 degrees off vertical and threw
    /// the wheel 6 m into the air. The vendored copy meets a large cuboid's face
    /// as a half-space.
    #[test]
    fn a_cylinder_wheel_rolling_on_a_wide_ground_neither_sinks_nor_hops() {
        let mut app = physics_world();
        wide_ground(&mut app);
        let r = 0.33;
        let wheel = app
            .world_mut()
            .spawn((
                RigidBody::Dynamic,
                Collider::cylinder(r, 0.25),
                ColliderDensity(35.0 / (std::f32::consts::PI * r * r * 0.25)),
                Friction::new(1.4),
                Transform::from_xyz(0.0, r, 0.0).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                LinearVelocity(Vec3::new(0.0, 0.0, 30.0)),
                AngularVelocity(Vec3::new(30.0 / r, 0.0, 0.0)),
            ))
            .id();
        let (mut lowest, mut highest) = (f32::MAX, f32::MIN);
        for frame in 0..300 {
            app.update();
            if frame > 10 {
                let y = pose_of(&app, wheel).translation.y;
                lowest = lowest.min(y);
                highest = highest.max(y);
            }
        }
        let speed = app.world().get::<LinearVelocity>(wheel).unwrap().0.length();
        assert!(lowest > r - 0.002, "the wheel sank into the ground, its centre to {lowest}");
        assert!(highest < r + 0.002, "the wheel hopped off the ground, its centre up to {highest}");
        assert!(speed > 29.9, "rolling on a flat ground keeps its speed, not {speed}");
    }

    /// A wedge (a convex hull) slid at 20 m/s across a wide ground stops dead
    /// ahead, where friction says. On stock 0.7.0 the same noisy normals turned
    /// it 12.7 m aside.
    #[test]
    fn a_wedge_slides_straight_across_a_wide_ground() {
        let mut app = physics_world();
        wide_ground(&mut app);
        let corners = vec![
            Vec3::new(-1.0, -0.5, -1.5),
            Vec3::new(1.0, -0.5, -1.5),
            Vec3::new(-1.0, -0.5, 1.5),
            Vec3::new(1.0, -0.5, 1.5),
            Vec3::new(-1.0, 0.5, 1.5),
            Vec3::new(1.0, 0.5, 1.5),
        ];
        let wedge = app
            .world_mut()
            .spawn((
                RigidBody::Dynamic,
                Collider::convex_hull(corners).unwrap(),
                Friction::new(0.3),
                Transform::from_xyz(0.0, 0.5, 0.0),
                LinearVelocity(Vec3::new(0.0, 0.0, 20.0)),
            ))
            .id();
        for _ in 0..600 {
            app.update();
        }
        let at = Vec3::from(pose_of(&app, wedge).translation);
        assert!(at.x.abs() < 0.01, "a wedge slid straight ahead went {} m aside", at.x);
        // Friction 0.65 (0.3 and 1.0 averaged) stops 20 m/s in 400 / (2 * 0.65 * 9.81) m.
        let stop = 400.0 / (2.0 * 0.65 * 9.81);
        assert!((at.z - stop).abs() < 0.5, "it slid {} m, not the {stop} m friction allows", at.z);
    }

    /// The energy of a free spin, from its body-frame inertia per kilogram.
    fn spin_energy(app: &App, body: Entity, inertia_per_kg: Vec3) -> f32 {
        let w = app.world();
        let local = w.get::<Rotation>(body).unwrap().0.inverse() * w.get::<AngularVelocity>(body).unwrap().0;
        0.5 * (inertia_per_kg * local * local).element_sum()
    }

    /// A free wheel spun about its axle (its axis of most inertia) with a slight
    /// wobble keeps spinning about the axle. Stock 0.7.0's gyroscopic step kept
    /// the angular momentum's length but not the energy, and the wheel ended
    /// tumbling at 150 rad/s with 90 percent more energy.
    #[test]
    fn a_free_wheel_keeps_spinning_about_its_axle() {
        let mut app = physics_world();
        app.insert_resource(Gravity(Vec3::ZERO));
        let r = 0.33;
        let wheel = app
            .world_mut()
            .spawn((
                RigidBody::Dynamic,
                Collider::cylinder(r, 0.25),
                ColliderDensity(35.0 / (std::f32::consts::PI * r * r * 0.25)),
                AngularDamping(0.0),
                Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                AngularVelocity(Vec3::new(30.0 / r, 0.0, 0.5)),
            ))
            .id();
        for _ in 0..300 {
            app.update();
        }
        let w = app.world();
        let axle = w.get::<Rotation>(wheel).unwrap().0 * Vec3::Y;
        let spin = w.get::<AngularVelocity>(wheel).unwrap().0;
        let about_axle = spin.dot(axle).abs();
        let off_axle = (spin - axle * spin.dot(axle)).length();
        assert!((about_axle - 30.0 / r).abs() < 0.3, "the spin about the axle went from {} to {about_axle} rad/s", 30.0 / r);
        assert!(off_axle < 1.0, "the wheel tumbles: {off_axle} rad/s off its axle");
    }

    /// A 2 by 0.5 by 1 m box spun about its axis of most inertia stays on it with
    /// its energy. Stock 0.7.0 gained 18 percent and flipped six times in 5 s.
    #[test]
    fn a_box_spun_about_its_axis_of_most_inertia_stays_on_it() {
        let mut app = physics_world();
        app.insert_resource(Gravity(Vec3::ZERO));
        let inertia_per_kg = Vec3::new(0.25 + 1.0, 4.0 + 1.0, 4.0 + 0.25) / 12.0;
        let block = app
            .world_mut()
            .spawn((
                RigidBody::Dynamic,
                Collider::cuboid(2.0, 0.5, 1.0),
                AngularDamping(0.0),
                Transform::default(),
                AngularVelocity(Vec3::new(2.0, 100.0, 2.0)),
            ))
            .id();
        app.update();
        let start = spin_energy(&app, block, inertia_per_kg);
        for _ in 0..300 {
            app.update();
            let w = app.world();
            let local = w.get::<Rotation>(block).unwrap().0.inverse() * w.get::<AngularVelocity>(block).unwrap().0;
            assert!(local.y > 99.0, "the box left its axis of most inertia: {local} rad/s in its own frame");
        }
        let end = spin_energy(&app, block, inertia_per_kg);
        assert!((end / start - 1.0).abs() < 0.01, "the spin's energy went from {start} to {end}");
    }
}
