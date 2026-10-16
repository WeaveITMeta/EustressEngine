//! # Weld groups as one rigid body
//!
//! Parts welded into a group of dynamic bodies move as ONE Avian body, as a
//! Roblox assembly does. The group's heaviest part keeps its body (the hub),
//! and every other part's collider is attached to it: the hub weighs, turns and
//! collides as the whole group, and nothing inside the group can bend, drift or
//! collide with itself. [`crate::joints`] plans the groups; this module merges
//! and splits them.
//!
//! A star of joints from the hub held a group to within millimetres and no
//! closer: a 390 kg deck carrying 840 kg of parts drifted 33 mm into a wall,
//! and every joint was one more body for the solver. Merged, the drift is zero
//! and the solver has one body.
//!
//! A merged part (an [`AssemblyMember`]):
//!
//! - keeps its `RigidBody`, disabled (`RigidBodyDisabled`), so what asks
//!   whether a part simulates still sees a dynamic part;
//! - has its collider attached by hand to the hub: `ColliderOf` and a
//!   `ColliderTransform` at its pose in the hub's frame, marked
//!   `ManualColliderTransform` (the workspace's Avian places and weighs such a
//!   collider), so a touch or a raycast still names the part itself;
//! - takes its `Position`, `Rotation` and velocities from the hub after every
//!   physics step ([`follow_hubs`]), and is drawn where the hub is drawn
//!   ([`draw_with_hubs`]), never eased on its own.
//!
//! A script's move, velocity or impulse on a member goes to the hub
//! ([`crate::apply`]). A joint the Space loaded (`engine::physics::joint_resolver`)
//! on a member is carried by the hub at the same place ([`CarriedJoint`]), and
//! a loaded weld inside a group is switched off while the group stands
//! ([`HeldWeld`]). All of it is undone when a part leaves its group (anchored,
//! or a weld removed) and when the session ends.

use std::collections::HashMap;

use avian3d::dynamics::integrator::{IntegrationSystems, VelocityIntegrationData};
use avian3d::dynamics::rigid_body::forces::AccumulatedLocalAcceleration;
use avian3d::prelude::*;
use bevy::ecs::component::Mutable;
use bevy::math::{Affine3A, Isometry3d};
use bevy::prelude::*;

use eustress_common::classes::{ClassName, Instance};

/// Keeps merged parts on their hubs: their physics pose and velocity after
/// every step, their drawn pose every frame.
pub struct AssembliesPlugin;

impl Plugin for AssembliesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedPostUpdate, (forward_velocities_to_hubs.in_set(PhysicsSystems::First), follow_hubs.after(PhysicsSystems::Writeback)))
            .add_systems(PostUpdate, draw_with_hubs.before(TransformSystems::Propagate))
            .add_observer(release_contacts_on_body_change)
            .add_systems(
                PhysicsSchedule,
                forward_to_hubs
                    .in_set(IntegrationSystems::UpdateVelocityIncrements)
                    .after(ForceSystems::ApplyConstantForces)
                    .before(avian3d::dynamics::integrator::pre_process_velocity_increments),
            );
    }
}

/// A part merged into its weld group's body, the `hub`, where it sits at
/// `in_hub` in the hub's frame. Its own body is disabled and its collider is
/// the hub's (see the module docs).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct AssemblyMember {
    pub hub: Entity,
    pub in_hub: Isometry3d,
    /// The velocities the hub last gave the part: any other velocity on it was
    /// put there since, by something pushing the part, and goes to the hub.
    followed: (Vec3, Vec3),
}

/// A joint the Space loaded, carried by a hub for the ends that were on a
/// merged part: each such end's part, and where it sits on the hub.
#[derive(Component, Clone, Copy, Debug)]
pub struct CarriedJoint {
    ends: [Option<(Entity, Isometry3d)>; 2],
}

/// A loaded weld between two parts of one merged group, switched off
/// (`JointDisabled`) while the group stands.
#[derive(Component, Clone, Copy, Debug)]
pub struct HeldWeld;

/// A part whose welds no longer hold: it fractured (`engine::physics::fracture_bridge`
/// hides it and disables its body, and its fragments fly on their own). The
/// weld groups plan without it, and a group it leaves keeps its body disabled.
#[derive(Component, Clone, Copy, Debug)]
pub struct WeldsReleased;

/// A merged part whose contacts [`release_contacts_on_body_change`] took out of
/// the contact graph: its `ColliderDisabled` is the groups' to remove.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ContactsReleased;

/// Avian points a merged part's collider back at the part's own body the moment
/// the part's `RigidBody` or parent is inserted (anchoring it, Stop, a script
/// reparenting it). Its contacts still name the hub and the hub's island, and
/// once that island is gone (the hub anchored too) removing them panicked in
/// `islands/mod.rs` ("Island ... does not exist"). So they come out of the
/// contact graph now, while the hub's island still stands; the weld groups split
/// the part next frame ([`split`]) and let its collider back in.
fn release_contacts_on_body_change(
    trigger: On<Insert, (RigidBody, ChildOf)>,
    members: Query<Has<ColliderDisabled>, With<AssemblyMember>>,
    mut commands: Commands,
) {
    if let Ok(false) = members.get(trigger.entity) {
        commands.entity(trigger.entity).try_insert((ColliderDisabled, ContactsReleased));
    }
}

/// The product of an entity's own and its ancestors' scales: the scale Avian
/// gives its collider.
fn world_scale(world: &World, e: Entity) -> Vec3 {
    let mut scale = Vec3::ONE;
    let mut node = Some(e);
    while let Some(n) = node {
        let Some(t) = world.get::<Transform>(n) else { break };
        scale *= t.scale;
        node = world.get::<ChildOf>(n).map(|c| c.parent());
    }
    scale
}

/// Merges `member` into `hub`'s body, at `in_hub` in the hub's frame. The part
/// moves there at once: a weld snaps its parts together, as in Roblox.
pub(crate) fn merge(world: &mut World, hub: Entity, member: Entity, in_hub: Isometry3d) {
    if member == hub || world.get_entity(member).is_err() || world.get_entity(hub).is_err() {
        return;
    }
    let scale = world_scale(world, member);
    // Out of the contact graph while it changes bodies: its contacts name the
    // body it had. A part a script took out of Workspace stays out.
    let off = world.get::<ColliderDisabled>(member).is_some();
    if !off {
        world.entity_mut(member).insert(ColliderDisabled);
        world.flush();
    }
    world.entity_mut(member).insert(RigidBodyDisabled);
    world.flush();
    // ColliderOf's insert hook sets the ColliderTransform from the two global
    // transforms, the hub's scale (its size) included, so the transform goes in after it.
    world.entity_mut(member).insert(ColliderOf { body: hub });
    world.entity_mut(member).insert((
        ColliderTransform { translation: Vec3::from(in_hub.translation), rotation: Rotation(in_hub.rotation), scale },
        ManualColliderTransform,
        AssemblyMember { hub, in_hub, followed: (Vec3::ZERO, Vec3::ZERO) },
    ));
    world.flush();
    if !off {
        world.entity_mut(member).remove::<ColliderDisabled>();
        world.flush();
    }
    // Drawn with the hub, never eased on its own.
    world
        .entity_mut(member)
        .remove::<(TranslationInterpolation, RotationInterpolation, crate::body_easing::MovedByScript)>()
        .insert((NoTranslationEasing, NoRotationEasing));
    follow(world, member);
}

/// Takes `member` out of its group: its own body again, moving as it moved
/// with the hub. Eased again when `simulating` and dynamic.
pub(crate) fn split(world: &mut World, member: Entity, simulating: bool) {
    let Some(&AssemblyMember { hub, .. }) = world.get::<AssemblyMember>(member) else {
        return;
    };
    // The velocity the hub gives the part here, read before anything moves: a
    // hub anchored this frame has none, and the part must not keep the car's
    // speed into the joints that now hold it.
    let here = world.get::<Position>(member).map_or(Vec3::ZERO, |p| p.0);
    let (linear, spin) = velocity_on_hub(world, hub, here);
    let scale = world_scale(world, member);
    // Disabled by something else (a script took the part out of Workspace), or
    // by [`release_contacts_on_body_change`], which is the groups' own.
    let released = world.get::<ContactsReleased>(member).is_some();
    let off = world.get::<ColliderDisabled>(member).is_some() && !released;
    if !off && !released {
        world.entity_mut(member).insert(ColliderDisabled);
        world.flush();
    }
    world.entity_mut(member).remove::<(AssemblyMember, ManualColliderTransform, ContactsReleased)>();
    if world.get::<RigidBody>(member).is_some() {
        world.entity_mut(member).insert(ColliderOf { body: member });
    }
    // A body's own collider sits at its origin.
    world.entity_mut(member).insert(ColliderTransform { scale, ..default() });
    world.flush();
    // A fractured part stays switched off.
    let fractured = world.get::<WeldsReleased>(member).is_some();
    if !fractured {
        world.entity_mut(member).remove::<RigidBodyDisabled>();
        world.flush();
        let mut e = world.entity_mut(member);
        if let Some(mut lv) = e.get_mut::<LinearVelocity>() {
            lv.0 = linear;
        }
        if let Some(mut av) = e.get_mut::<AngularVelocity>() {
            av.0 = spin;
        }
    }
    if !off {
        world.entity_mut(member).remove::<ColliderDisabled>();
        world.flush();
    }
    world.entity_mut(member).remove::<(NoTranslationEasing, NoRotationEasing)>();
    if simulating && !fractured && world.get::<RigidBody>(member) == Some(&RigidBody::Dynamic) {
        world.entity_mut(member).insert((TranslationInterpolation, RotationInterpolation));
    }
}

/// The velocity a body moving with `hub` has at the world point `point`: the
/// hub's linear velocity plus its spin crossed with the lever from its centre of
/// mass. Also returns the spin.
fn velocity_on_hub(world: &World, hub: Entity, point: Vec3) -> (Vec3, Vec3) {
    let (Some(p), Some(r)) = (world.get::<Position>(hub), world.get::<Rotation>(hub)) else {
        return (Vec3::ZERO, Vec3::ZERO);
    };
    let v = world.get::<LinearVelocity>(hub).map_or(Vec3::ZERO, |v| v.0);
    let w = world.get::<AngularVelocity>(hub).map_or(Vec3::ZERO, |w| w.0);
    let com = p.0 + r.0 * world.get::<ComputedCenterOfMass>(hub).map_or(Vec3::ZERO, |c| c.0);
    (v + w.cross(point - com), w)
}

/// Puts `member` where its hub holds it, moving as the hub does, now.
fn follow(world: &mut World, member: Entity) {
    let Some(&AssemblyMember { hub, in_hub, .. }) = world.get::<AssemblyMember>(member) else { return };
    let (Some(p), Some(r)) = (world.get::<Position>(hub), world.get::<Rotation>(hub)) else { return };
    let hub_pose = Isometry3d::new(p.0, r.0);
    let at = hub_pose * in_hub;
    let (linear, w) = velocity_on_hub(world, hub, Vec3::from(at.translation));
    let mut e = world.entity_mut(member);
    if let Some(mut p) = e.get_mut::<Position>() {
        p.0 = Vec3::from(at.translation);
    }
    if let Some(mut r) = e.get_mut::<Rotation>() {
        r.0 = at.rotation;
    }
    if let Some(mut lv) = e.get_mut::<LinearVelocity>() {
        lv.0 = linear;
    }
    if let Some(mut av) = e.get_mut::<AngularVelocity>() {
        av.0 = w;
    }
    if let Some(mut m) = e.get_mut::<AssemblyMember>() {
        m.followed = (linear, w);
    }
}

/// The body a part moves with in its group: the hub for a merged part, else
/// the part itself. A script's velocity or impulse on a part goes here.
pub fn assembly_body(world: &World, part: Entity) -> Entity {
    world.get::<AssemblyMember>(part).map_or(part, |m| m.hub)
}

/// An entity's world affine, composed up its chain of Transforms.
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

/// A script moved a merged part to `pose`: its whole group goes with it, as a
/// Roblox assembly does. The hub is put where that leaves it (simulated and
/// drawn, and not eased back toward where it was), and every part of the group
/// stands on it again now, for a joint built or a pose read this frame.
pub(crate) fn move_group(world: &mut World, member: Entity, pose: Isometry3d) {
    let Some(&AssemblyMember { hub, in_hub, .. }) = world.get::<AssemblyMember>(member) else { return };
    let hub_pose = pose * in_hub.inverse();
    if let Some(mut p) = world.get_mut::<Position>(hub) {
        p.0 = Vec3::from(hub_pose.translation);
    }
    if let Some(mut r) = world.get_mut::<Rotation>(hub) {
        r.0 = hub_pose.rotation;
    }
    let parent = world.get::<ChildOf>(hub).map(|c| c.parent()).and_then(|p| world_affine(world, p)).unwrap_or(Affine3A::IDENTITY);
    let local = parent.inverse() * Affine3A::from_rotation_translation(hub_pose.rotation, Vec3::from(hub_pose.translation));
    let (_, rotation, translation) = local.to_scale_rotation_translation();
    if let Some(mut t) = world.get_mut::<Transform>(hub) {
        t.translation = translation;
        t.rotation = rotation;
    }
    if world.get::<TranslationInterpolation>(hub).is_some() {
        world.entity_mut(hub).insert((crate::body_easing::MovedByScript, NoTranslationEasing, NoRotationEasing));
    }
    let mut query = world.query::<(Entity, &AssemblyMember)>();
    let group: Vec<Entity> = query.iter(world).filter(|(_, m)| m.hub == hub).map(|(e, _)| e).collect();
    for part in group {
        follow(world, part);
    }
}

/// After every physics step: each merged part stands where its hub holds it
/// and moves as the hub does (a joint built against it, a script reading its
/// pose or velocity, and a part leaving the group all read these), and its
/// collider keeps the part's scale (a script may resize it). Its velocity
/// increments start the next step empty: Avian adds gravity to every dynamic
/// body's, disabled or not, and clears only the bodies it solves, so what
/// [`forward_to_hubs`] finds there next step is only what pushed the part.
#[allow(clippy::type_complexity)]
pub fn follow_hubs(
    mut members: Query<(
        Entity,
        &mut AssemblyMember,
        &mut Position,
        &mut Rotation,
        &mut LinearVelocity,
        &mut AngularVelocity,
        &mut ColliderTransform,
        &mut VelocityIntegrationData,
    )>,
    hubs: Query<(&Position, &Rotation, &LinearVelocity, &AngularVelocity, Option<&ComputedCenterOfMass>), Without<AssemblyMember>>,
    scales: Query<(&Transform, Option<&ChildOf>)>,
) {
    for (e, mut member, mut p, mut r, mut lv, mut av, mut ct, mut integration) in &mut members {
        integration.linear_increment = Vec3::ZERO;
        integration.angular_increment = Vec3::ZERO;
        let Ok((hp, hr, hv, hw, hcom)) = hubs.get(member.hub) else { continue };
        let at = Isometry3d::new(hp.0, hr.0) * member.in_hub;
        let com = hp.0 + hr.0 * hcom.map_or(Vec3::ZERO, |c| c.0);
        p.0 = Vec3::from(at.translation);
        r.0 = at.rotation;
        lv.0 = hv.0 + hw.0.cross(p.0 - com);
        av.0 = hw.0;
        member.followed = (lv.0, av.0);
        let mut scale = Vec3::ONE;
        let mut node = Some(e);
        while let Some(n) = node {
            let Ok((t, parent)) = scales.get(n) else { break };
            scale *= t.scale;
            node = parent.map(|c| c.parent());
        }
        if ct.scale != scale {
            ct.scale = scale;
        }
    }
}

/// Velocity something wrote on a merged part since the hub last moved it (a
/// mover, a script's body velocity) belongs to its group: before each physics
/// step it moves onto the hub as the same impulse at the part's centre of mass.
/// Before the step, since Avian copies each body's velocity into its solver
/// body as the step starts.
#[allow(clippy::type_complexity)]
pub fn forward_velocities_to_hubs(
    mut members: Query<(&AssemblyMember, &Position, &Rotation, &mut LinearVelocity, &mut AngularVelocity, &ComputedMass, &ComputedAngularInertia, &ComputedCenterOfMass)>,
    mut hubs: Query<(&Position, &Rotation, &mut LinearVelocity, &mut AngularVelocity, &ComputedMass, &ComputedAngularInertia, &ComputedCenterOfMass), Without<AssemblyMember>>,
) {
    for (member, p, r, mut lv, mut av, mass, inertia, com) in &mut members {
        let impulse = (lv.0 - member.followed.0) * mass.value();
        let spin = inertia.rotated(r.0).tensor().to_mat3() * (av.0 - member.followed.1);
        if impulse == Vec3::ZERO && spin == Vec3::ZERO {
            continue;
        }
        (lv.0, av.0) = member.followed;
        let Ok((hp, hr, mut hv, mut hw, hub_mass, hub_inertia, hub_com)) = hubs.get_mut(member.hub) else { continue };
        let lever = (p.0 + r.0 * com.0) - (hp.0 + hr.0 * hub_com.0);
        hv.0 += impulse * hub_mass.inverse();
        hw.0 += hub_inertia.rotated(hr.0).inverse_tensor().to_mat3() * (spin + lever.cross(impulse));
    }
}

/// Forces and torques something put on a merged part (a mover, a script, a
/// loaded `VectorForce`, buoyancy) belong to its group: each physics step,
/// before they are integrated, they move onto the hub as the same force and
/// torque at the part's centre of mass. The part's own body is disabled and
/// would integrate none of them.
#[allow(clippy::type_complexity)]
pub fn forward_to_hubs(
    mut members: Query<
        (&AssemblyMember, &Position, &Rotation, &mut VelocityIntegrationData, Option<&mut AccumulatedLocalAcceleration>, &ComputedMass, &ComputedAngularInertia, &ComputedCenterOfMass),
    >,
    mut hubs: Query<(&Position, &Rotation, &mut VelocityIntegrationData, &ComputedMass, &ComputedAngularInertia, &ComputedCenterOfMass), Without<AssemblyMember>>,
) {
    for (member, p, r, mut integration, local, mass, inertia, com) in &mut members {
        // The part's own figures: its disabled body keeps its own mass properties.
        let (m, inertia_world) = (mass.value(), inertia.rotated(r.0).tensor().to_mat3());
        let mut force = integration.linear_increment * m;
        let mut torque = inertia_world * integration.angular_increment;
        if let Some(mut local) = local {
            force += r.0 * (local.linear * m);
            torque += r.0 * (inertia.tensor().to_mat3() * local.angular);
            *local = AccumulatedLocalAcceleration::default();
        }
        integration.linear_increment = Vec3::ZERO;
        integration.angular_increment = Vec3::ZERO;
        if force == Vec3::ZERO && torque == Vec3::ZERO {
            continue;
        }
        let Ok((hp, hr, mut hub_integration, hub_mass, hub_inertia, hub_com)) = hubs.get_mut(member.hub) else { continue };
        let lever = (p.0 + r.0 * com.0) - (hp.0 + hr.0 * hub_com.0);
        hub_integration.linear_increment += force * hub_mass.inverse();
        hub_integration.angular_increment += hub_inertia.rotated(hr.0).inverse_tensor().to_mat3() * (torque + lever.cross(force));
    }
}

/// The draw's working state, kept from frame to frame so its capacity is
/// reused: each member's place this frame, worked out from the Transforms as
/// they stand before any is written, and the world affine of its parent once
/// every member is in place.
#[derive(Default)]
pub struct DrawPlan {
    /// A member's pose this frame and its parent's world affine; `None`: it has
    /// no place this frame (its hub has no Transform) and stays as it is.
    placed: HashMap<Entity, Option<(Isometry3d, Affine3A)>>,
    /// Members whose place is being worked out, outermost first.
    working: Vec<Entity>,
    /// The members, in the order their places are worked out.
    order: Vec<Entity>,
    /// Each member's place, written once all are worked out.
    places: Vec<(Entity, Isometry3d, Affine3A)>,
}

impl DrawPlan {
    /// Empties the plan for a new frame, keeping its capacity.
    fn clear(&mut self) {
        self.placed.clear();
        self.working.clear();
        self.order.clear();
        self.places.clear();
    }

    /// Member `e`'s pose this frame (its hub's pose times its place on the hub)
    /// and its parent's world affine. `None` for an entity that is not a member,
    /// a member whose hub has no Transform, and a member met again while its own
    /// place is being worked out (a cycle, below).
    fn place(
        &mut self,
        e: Entity,
        transforms: &Query<&mut Transform>,
        parents: &Query<&ChildOf>,
        members: &Query<(Entity, &AssemblyMember)>,
    ) -> Option<(Isometry3d, Affine3A)> {
        let (_, &member) = members.get(e).ok()?;
        if let Some(placed) = self.placed.get(&e) {
            return *placed;
        }
        if self.working.contains(&e) {
            // A cycle: `e`'s place hangs on itself (its hub hangs under it, or
            // under a member whose place hangs on it). The cycle is cut here:
            // `e` counts at its Transform as it stands, and still gets its own
            // place when its own turn ends. Members are met in entity order, so
            // where the cut falls does not hang on the query's order.
            return None;
        }
        self.working.push(e);
        let placed = self.world(member.hub, transforms, parents, members).map(|hub| {
            let (_, rotation, translation) = hub.to_scale_rotation_translation();
            let at = Isometry3d::new(translation, rotation) * member.in_hub;
            let parent = parents.get(e).ok().and_then(|c| self.world(c.parent(), transforms, parents, members)).unwrap_or(Affine3A::IDENTITY);
            (at, parent)
        });
        self.working.pop();
        self.placed.insert(e, placed);
        placed
    }

    /// `e`'s world affine once the draw is written, composed up its chain as the
    /// Transforms compose (an ancestor with no Transform adds nothing), with every
    /// member in the chain at its new place. `None` when `e` has no Transform.
    fn world(
        &mut self,
        e: Entity,
        transforms: &Query<&mut Transform>,
        parents: &Query<&ChildOf>,
        members: &Query<(Entity, &AssemblyMember)>,
    ) -> Option<Affine3A> {
        let mut affine = self.local(e, transforms, parents, members)?;
        let mut node = e;
        while let Ok(parent) = parents.get(node).map(|c| c.parent()) {
            if let Some(local) = self.local(parent, transforms, parents, members) {
                affine = local * affine;
            }
            node = parent;
        }
        Some(affine)
    }

    /// `e`'s Transform as an affine once the draw is written: a member's new
    /// translation and rotation with its own scale, which the draw keeps (a
    /// part's scale is its size, and its children inherit it), anything else's
    /// as it stands.
    fn local(
        &mut self,
        e: Entity,
        transforms: &Query<&mut Transform>,
        parents: &Query<&ChildOf>,
        members: &Query<(Entity, &AssemblyMember)>,
    ) -> Option<Affine3A> {
        let t = *transforms.get(e).ok()?;
        Some(match self.place(e, transforms, parents, members) {
            Some((at, parent)) => {
                let (translation, rotation) = local_place(at, parent);
                Affine3A::from_scale_rotation_translation(t.scale, rotation, translation)
            }
            None => t.compute_affine(),
        })
    }
}

/// A member's new translation and rotation in its parent's frame, from where
/// it is drawn and its parent's world affine.
fn local_place(at: Isometry3d, parent: Affine3A) -> (Vec3, Quat) {
    let local = parent.inverse() * Affine3A::from_rotation_translation(at.rotation, Vec3::from(at.translation));
    let (_, rotation, translation) = local.to_scale_rotation_translation();
    (translation, rotation)
}

/// Every frame, before transforms propagate: each merged part is drawn where
/// its hub is drawn (eased between physics steps), at its place on the hub.
/// Every place is worked out before any is written, from the Transforms as
/// they stand, so a part whose ECS parent is another merged part is placed in
/// that part's frame as it is drawn this frame, whichever of the two the query
/// meets first.
pub fn draw_with_hubs(
    members: Query<(Entity, &AssemblyMember)>,
    mut transforms: Query<&mut Transform>,
    parents: Query<&ChildOf>,
    mut plan: Local<DrawPlan>,
) {
    let plan = &mut *plan;
    plan.clear();
    plan.order.extend(members.iter().map(|(e, _)| e));
    // In entity order, so where a cycle is cut does not hang on the query's order.
    plan.order.sort_unstable_by_key(|e| e.to_bits());
    for i in 0..plan.order.len() {
        let e = plan.order[i];
        if let Some((at, parent)) = plan.place(e, &transforms, &parents, &members) {
            plan.places.push((e, at, parent));
        }
    }
    for &(e, at, parent) in &plan.places {
        let (translation, rotation) = local_place(at, parent);
        if let Ok(mut t) = transforms.get_mut(e) {
            if t.translation != translation || t.rotation != rotation {
                t.translation = translation;
                t.rotation = rotation;
            }
        }
    }
}

/// The world pose a simulated part is drawn at this frame: its own `Transform`
/// (eased between physics steps) composed up its ancestors or, for a part merged
/// into a weld group, its hub's pose times its place on the hub, which is the pose
/// [`draw_with_hubs`] draws it at. As there, only the translation and rotation
/// count, so a hub's size (its scale) does not enter. The engine's pull keeps it
/// for the part's `DrawnCFrame`.
pub fn drawn_pose(
    part: Entity,
    transforms: &Query<&Transform>,
    parents: &Query<&ChildOf>,
    members: &Query<&AssemblyMember>,
) -> Option<Isometry3d> {
    let anchor = members.get(part).map_or(part, |m| m.hub);
    let (_, rotation, translation) = chain_affine(anchor, transforms, parents)?.to_scale_rotation_translation();
    let pose = Isometry3d::new(translation, rotation);
    Some(match members.get(part) {
        Ok(m) => pose * m.in_hub,
        Err(_) => pose,
    })
}

/// An entity's world affine, from its own and its ancestors' Transforms as they
/// stand now.
fn chain_affine(e: Entity, transforms: &Query<&Transform>, parents: &Query<&ChildOf>) -> Option<Affine3A> {
    let mut affine = transforms.get(e).ok()?.compute_affine();
    let mut node = e;
    while let Ok(parent) = parents.get(node).map(|c| c.parent()) {
        if let Ok(t) = transforms.get(parent) {
            affine = t.compute_affine() * affine;
        }
        node = parent;
    }
    Some(affine)
}

// ── Joints the Space loaded ─────────────────────────────────────────────

/// A loaded weld's two bodies and where the second sits in the first's frame,
/// for the weld groups: `WeldConstraint` and `Weld` joints the loader bound
/// (both ends real) and nothing else switched off.
pub(crate) fn loaded_welds(world: &mut World) -> Vec<(Entity, Entity, Entity, Isometry3d)> {
    let mut query = world.query::<(Entity, &Instance, &FixedJoint, Has<JointDisabled>, Has<HeldWeld>)>();
    let mut welds = Vec::new();
    for (e, instance, joint, disabled, held) in query.iter(world) {
        if !matches!(instance.class_name, ClassName::WeldConstraint | ClassName::Weld) || (disabled && !held) {
            continue;
        }
        if joint.body1 == Entity::PLACEHOLDER || joint.body2 == Entity::PLACEHOLDER || joint.body1 == joint.body2 {
            continue;
        }
        welds.push((e, joint.body1, joint.body2, joint.frame1, joint.frame2));
    }
    // A weld to a part that is gone, or that fractured, holds nothing.
    welds.retain(|&(_, a, b, _, _)| {
        [a, b].iter().all(|&body| world.get_entity(body).is_ok() && world.get::<WeldsReleased>(body).is_none())
    });
    welds
        .into_iter()
        .filter_map(|(e, a, b, frame1, frame2)| {
            // body1 * frame1 == body2 * frame2 once the weld holds. A frame given
            // in world space (not prepared yet) is read from where the parts are.
            let b_in_a = match (frame1.get_local_isometry(), frame2.get_local_isometry()) {
                (Some(f1), Some(f2)) => f1 * f2.inverse(),
                _ => {
                    let pose = |body: Entity| Some(Isometry3d::new(world.get::<Position>(body)?.0, world.get::<Rotation>(body)?.0));
                    pose(a)?.inverse() * pose(b)?
                }
            };
            Some((e, a, b, b_in_a))
        })
        .collect()
}

/// The part a hub carries an end for, and where it sits on the hub.
type Carry = Option<(Entity, Isometry3d)>;

/// A local joint frame on a member, as the same frame on its hub (and back,
/// with `in_hub` inverted). A frame still given in world space is left to Avian,
/// which reads it against whichever body the joint has when it prepares.
fn moved_frame(frame: &mut JointFrame, in_hub: Isometry3d) {
    if let JointAnchor::Local(anchor) = frame.anchor {
        frame.anchor = JointAnchor::Local(Vec3::from(in_hub.transform_point(anchor)));
    }
    if let JointBasis::Local(basis) = frame.basis {
        frame.basis = JointBasis::Local(in_hub.rotation * basis);
    }
}

fn moved_anchor(anchor: &mut JointAnchor, in_hub: Isometry3d) {
    if let JointAnchor::Local(point) = *anchor {
        *anchor = JointAnchor::Local(Vec3::from(in_hub.transform_point(point)));
    }
}

fn fixed_ends(j: &mut FixedJoint) -> [(&mut Entity, &mut JointFrame); 2] {
    [(&mut j.body1, &mut j.frame1), (&mut j.body2, &mut j.frame2)]
}

fn revolute_ends(j: &mut RevoluteJoint) -> [(&mut Entity, &mut JointFrame); 2] {
    [(&mut j.body1, &mut j.frame1), (&mut j.body2, &mut j.frame2)]
}

fn prismatic_ends(j: &mut PrismaticJoint) -> [(&mut Entity, &mut JointFrame); 2] {
    [(&mut j.body1, &mut j.frame1), (&mut j.body2, &mut j.frame2)]
}

fn spherical_ends(j: &mut SphericalJoint) -> [(&mut Entity, &mut JointFrame); 2] {
    [(&mut j.body1, &mut j.frame1), (&mut j.body2, &mut j.frame2)]
}

/// Moves a framed joint's ends off merged parts onto their hubs; returns what it moved.
fn carry_framed<J: Component<Mutability = Mutable>>(
    world: &mut World,
    joint: Entity,
    members: &HashMap<Entity, (Entity, Isometry3d)>,
    ends: for<'a> fn(&'a mut J) -> [(&'a mut Entity, &'a mut JointFrame); 2],
) -> [Carry; 2] {
    let mut carried = [None, None];
    let Some(mut j) = world.get_mut::<J>(joint) else { return carried };
    for (i, (body, frame)) in ends(&mut j).into_iter().enumerate() {
        if let Some(&(hub, in_hub)) = members.get(&*body) {
            carried[i] = Some((*body, in_hub));
            *body = hub;
            moved_frame(frame, in_hub);
        }
    }
    carried
}

/// Puts a framed joint's carried ends back on their parts.
fn return_framed<J: Component<Mutability = Mutable>>(
    world: &mut World,
    joint: Entity,
    carried: [Carry; 2],
    ends: for<'a> fn(&'a mut J) -> [(&'a mut Entity, &'a mut JointFrame); 2],
) {
    let Some(mut j) = world.get_mut::<J>(joint) else { return };
    for ((body, frame), carry) in ends(&mut j).into_iter().zip(carried) {
        if let Some((part, in_hub)) = carry {
            *body = part;
            moved_frame(frame, in_hub.inverse());
        }
    }
}

/// Switches off the loaded welds inside merged groups and has the hubs carry
/// every other loaded joint on a merged part. `members`: merged part -> (hub,
/// its place on the hub). Only joints with an `Instance` are the Space's; the
/// scripted joints are built against the groups as they stand.
pub(crate) fn carry_loaded_joints(world: &mut World, members: &HashMap<Entity, (Entity, Isometry3d)>) {
    if members.is_empty() {
        return;
    }
    let group_of = |body: Entity| members.get(&body).map_or(body, |m| m.0);
    let mut query = world.query_filtered::<Entity, With<Instance>>();
    let loaded: Vec<Entity> = query.iter(world).collect();
    for joint in loaded {
        // A weld between two parts of one group: the group holds them.
        if let Some(fixed) = world.get::<FixedJoint>(joint) {
            let weld = world.get::<Instance>(joint).is_some_and(|i| matches!(i.class_name, ClassName::WeldConstraint | ClassName::Weld));
            let (a, b) = (fixed.body1, fixed.body2);
            if weld && (members.contains_key(&a) || members.contains_key(&b)) && group_of(a) == group_of(b) {
                if world.get::<JointDisabled>(joint).is_none() {
                    world.entity_mut(joint).insert((JointDisabled, HeldWeld));
                }
                continue;
            }
        }
        let carried = if world.get::<FixedJoint>(joint).is_some() {
            carry_framed(world, joint, members, fixed_ends)
        } else if world.get::<RevoluteJoint>(joint).is_some() {
            carry_framed(world, joint, members, revolute_ends)
        } else if world.get::<PrismaticJoint>(joint).is_some() {
            carry_framed(world, joint, members, prismatic_ends)
        } else if world.get::<SphericalJoint>(joint).is_some() {
            carry_framed(world, joint, members, spherical_ends)
        } else if let Some(mut j) = world.get_mut::<DistanceJoint>(joint) {
            let mut carried = [None, None];
            let j = &mut *j;
            for (i, (body, anchor)) in [(&mut j.body1, &mut j.anchor1), (&mut j.body2, &mut j.anchor2)].into_iter().enumerate() {
                if let Some(&(hub, in_hub)) = members.get(&*body) {
                    carried[i] = Some((*body, in_hub));
                    *body = hub;
                    moved_anchor(anchor, in_hub);
                }
            }
            carried
        } else {
            [None, None]
        };
        if carried.iter().any(Option::is_some) {
            world.entity_mut(joint).insert(CarriedJoint { ends: carried });
        }
    }
}

/// Undoes [`carry_loaded_joints`]: every carried end back on its part, every
/// held weld on again.
pub(crate) fn return_loaded_joints(world: &mut World) {
    let mut held = world.query_filtered::<Entity, With<HeldWeld>>();
    for joint in held.iter(world).collect::<Vec<_>>() {
        world.entity_mut(joint).remove::<(JointDisabled, HeldWeld)>();
    }
    let mut carried = world.query::<(Entity, &CarriedJoint)>();
    for (joint, carry) in carried.iter(world).map(|(e, c)| (e, *c)).collect::<Vec<_>>() {
        if world.get::<FixedJoint>(joint).is_some() {
            return_framed(world, joint, carry.ends, fixed_ends);
        } else if world.get::<RevoluteJoint>(joint).is_some() {
            return_framed(world, joint, carry.ends, revolute_ends);
        } else if world.get::<PrismaticJoint>(joint).is_some() {
            return_framed(world, joint, carry.ends, prismatic_ends);
        } else if world.get::<SphericalJoint>(joint).is_some() {
            return_framed(world, joint, carry.ends, spherical_ends);
        } else if let Some(mut j) = world.get_mut::<DistanceJoint>(joint) {
            let j = &mut *j;
            for ((body, anchor), end) in [(&mut j.body1, &mut j.anchor1), (&mut j.body2, &mut j.anchor2)].into_iter().zip(carry.ends) {
                if let Some((part, in_hub)) = end {
                    *body = part;
                    moved_anchor(anchor, in_hub.inverse());
                }
            }
        }
        world.entity_mut(joint).remove::<CarriedJoint>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// Two poses agree to within 1e-5: their translations, and where their
    /// rotations take each axis. `Quat::angle_between` is no measure of that:
    /// it is `acos_approx(|dot|) * 2`, which gives about 6.9e-4 rad for two
    /// equal rotations whose dot product rounds one ULP below 1.
    fn same(a: Isometry3d, b: Isometry3d) -> bool {
        Vec3::from(a.translation).distance(Vec3::from(b.translation)) < 1e-5
            && [Vec3::X, Vec3::Y, Vec3::Z].into_iter().all(|v| (a.rotation * v).distance(b.rotation * v) < 1e-5)
    }

    /// The pose `drawn_pose` gives `part`, read from `world` as a system would.
    fn drawn(world: &mut World, part: Entity) -> Option<Isometry3d> {
        world
            .run_system_once(move |transforms: Query<&Transform>, parents: Query<&ChildOf>, members: Query<&AssemblyMember>| {
                drawn_pose(part, &transforms, &parents, &members)
            })
            .expect("the system runs")
    }

    /// A merged member is drawn at its hub's pose times its place on the hub. The
    /// hub's pose comes from its own Transform and its folder's. The member's own
    /// Transform is stale until the draw runs, so it must not count. The hub's size
    /// (its scale) does not enter the pose, as in the draw.
    #[test]
    fn a_merged_members_drawn_pose_is_its_hubs_pose_times_its_place() {
        let mut world = World::new();
        let folder_t = Transform::from_xyz(5.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.5));
        let folder = world.spawn(folder_t).id();
        let hub_t = Transform::from_xyz(1.0, 2.0, 3.0).with_rotation(Quat::from_rotation_z(0.3)).with_scale(Vec3::new(4.0, 1.0, 2.0));
        let hub = world.spawn((hub_t, ChildOf(folder))).id();
        let in_hub = Isometry3d::new(Vec3::new(0.5, -0.2, 0.1), Quat::from_rotation_x(0.2));
        let member = world
            .spawn((Transform::from_xyz(-9.0, 9.0, 9.0), ChildOf(folder), AssemblyMember { hub, in_hub, followed: (Vec3::ZERO, Vec3::ZERO) }))
            .id();
        let (_, hub_rot, hub_pos) = (folder_t.compute_affine() * hub_t.compute_affine()).to_scale_rotation_translation();
        let expected = Isometry3d::new(hub_pos, hub_rot) * in_hub;
        let got = drawn(&mut world, member).expect("the member has a Transform");
        assert!(same(got, expected), "drawn at {got:?}, expected {expected:?}");
    }

    /// A part that is not merged is drawn where its own Transform puts it, under
    /// its folder.
    #[test]
    fn an_eased_part_is_drawn_where_its_transform_is() {
        let mut world = World::new();
        let folder_t = Transform::from_xyz(5.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.5));
        let folder = world.spawn(folder_t).id();
        let part_t = Transform::from_xyz(1.0, 2.0, 3.0).with_rotation(Quat::from_rotation_x(0.7));
        let part = world.spawn((part_t, ChildOf(folder))).id();
        let (_, rot, pos) = (folder_t.compute_affine() * part_t.compute_affine()).to_scale_rotation_translation();
        let got = drawn(&mut world, part).expect("the part has a Transform");
        assert!(same(got, Isometry3d::new(pos, rot)), "drawn at {got:?}");
    }

    /// What the pull reads is what the draw then draws: `drawn_pose`, read in
    /// `Update` as the engine's pull reads it, before the draw, is where the real
    /// `draw_with_hubs` and transform propagation put the merged member, as its
    /// `GlobalTransform` says once the frame is drawn.
    #[test]
    fn drawn_pose_matches_what_draw_with_hubs_draws() {
        #[derive(Resource, Default)]
        struct Pulled(Option<Isometry3d>);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::transform::TransformPlugin));
        app.add_systems(PostUpdate, draw_with_hubs.before(TransformSystems::Propagate));
        app.init_resource::<Pulled>();
        let folder_t = Transform::from_xyz(5.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.5));
        let folder = app.world_mut().spawn(folder_t).id();
        let hub_t = Transform::from_xyz(1.0, 2.0, 3.0).with_rotation(Quat::from_rotation_z(0.3)).with_scale(Vec3::new(4.0, 1.0, 2.0));
        let hub = app.world_mut().spawn((hub_t, ChildOf(folder))).id();
        let in_hub = Isometry3d::new(Vec3::new(0.5, -0.2, 0.1), Quat::from_rotation_x(0.2));
        let member = app
            .world_mut()
            .spawn((Transform::from_xyz(-9.0, 9.0, 9.0), ChildOf(folder), AssemblyMember { hub, in_hub, followed: (Vec3::ZERO, Vec3::ZERO) }))
            .id();
        app.add_systems(
            Update,
            move |transforms: Query<&Transform>, parents: Query<&ChildOf>, members: Query<&AssemblyMember>, mut pulled: ResMut<Pulled>| {
                pulled.0 = drawn_pose(member, &transforms, &parents, &members);
            },
        );
        app.update();
        let pulled = app.world().resource::<Pulled>().0.expect("the member has a Transform");
        let (_, rotation, translation) = app.world().get::<GlobalTransform>(member).expect("drawn").to_scale_rotation_translation();
        let drawn_at = Isometry3d::new(translation, rotation);
        assert!(same(pulled, drawn_at), "the pull reads {pulled:?}, the draw puts it at {drawn_at:?}");
    }

    /// A merged part P under a folder, and a merged part C whose ECS parent is
    /// P, each on its own hub under the same folder. P starts away from its
    /// place, so the draw moves it this frame, and its size is a uniform scale
    /// of 2, which C inherits as any child does. With `c_first`, C comes first
    /// both in the query (each member holds a child, so the two share one
    /// table, whose rows keep the order they arrived in) and in entity order
    /// (the order the draw works them out in). Returns the app, P and C, and
    /// where each must be drawn: its hub's pose times its place on the hub.
    fn nested_members(c_first: bool) -> (App, Entity, Entity, Isometry3d, Isometry3d) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::transform::TransformPlugin));
        app.add_systems(PostUpdate, draw_with_hubs.before(TransformSystems::Propagate));
        let world = app.world_mut();
        let folder_t = Transform::from_xyz(5.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.5));
        let folder = world.spawn(folder_t).id();
        let hub_p_t = Transform::from_xyz(1.0, 2.0, 3.0).with_rotation(Quat::from_rotation_z(0.3)).with_scale(Vec3::new(4.0, 1.0, 2.0));
        let hub_p = world.spawn((hub_p_t, ChildOf(folder))).id();
        let hub_c_t = Transform::from_xyz(-2.0, 1.0, 0.5).with_rotation(Quat::from_rotation_x(-0.4));
        let hub_c = world.spawn((hub_c_t, ChildOf(folder))).id();
        let in_p = Isometry3d::new(Vec3::new(0.5, -0.2, 0.1), Quat::from_rotation_x(0.2));
        let in_c = Isometry3d::new(Vec3::new(-0.3, 0.4, 0.2), Quat::from_rotation_y(-0.6));
        let member = |hub, in_hub| AssemblyMember { hub, in_hub, followed: (Vec3::ZERO, Vec3::ZERO) };
        let mut ids = [world.spawn_empty().id(), world.spawn_empty().id()];
        ids.sort_by_key(|e| e.to_bits());
        let (p, c) = if c_first { (ids[1], ids[0]) } else { (ids[0], ids[1]) };
        let p_parts = (Transform::from_xyz(-9.0, 9.0, 9.0).with_scale(Vec3::splat(2.0)), ChildOf(folder), member(hub_p, in_p));
        let c_parts = (Transform::from_xyz(3.0, -3.0, 1.0), ChildOf(p), member(hub_c, in_c));
        if c_first {
            world.entity_mut(c).insert(c_parts);
            world.spawn(ChildOf(c));
            world.entity_mut(p).insert(p_parts);
        } else {
            world.entity_mut(p).insert(p_parts);
            world.entity_mut(c).insert(c_parts);
            world.spawn(ChildOf(c));
        }
        let order = world
            .run_system_once(|q: Query<Entity, With<AssemblyMember>>| q.iter().collect::<Vec<Entity>>())
            .expect("the system runs");
        assert_eq!(order, if c_first { vec![c, p] } else { vec![p, c] }, "the order the query meets the members in");
        assert_eq!(c.to_bits() < p.to_bits(), c_first, "the order the draw works them out in");
        let pose = |hub_t: Transform, in_hub: Isometry3d| {
            let (_, rotation, translation) = (folder_t.compute_affine() * hub_t.compute_affine()).to_scale_rotation_translation();
            Isometry3d::new(translation, rotation) * in_hub
        };
        (app, p, c, pose(hub_p_t, in_p), pose(hub_c_t, in_c))
    }

    /// `e`'s pose as drawn: its GlobalTransform's translation and rotation.
    fn global_pose(app: &App, e: Entity) -> Isometry3d {
        let (_, rotation, translation) = app.world().get::<GlobalTransform>(e).expect("drawn").to_scale_rotation_translation();
        Isometry3d::new(translation, rotation)
    }

    /// A merged part whose ECS parent is another merged part, met by the draw
    /// before that parent, is drawn where its hub holds it: placed in its
    /// parent's frame as the parent is drawn this frame, not as it stood.
    #[test]
    fn a_member_met_before_the_member_it_hangs_from_is_drawn_on_its_hub() {
        let (mut app, p, c, p_at, c_at) = nested_members(true);
        app.update();
        let (p_drawn, c_drawn) = (global_pose(&app, p), global_pose(&app, c));
        assert!(same(p_drawn, p_at), "P is drawn at {p_drawn:?}, its hub holds it at {p_at:?}");
        assert!(same(c_drawn, c_at), "C is drawn at {c_drawn:?}, its hub holds it at {c_at:?}");
    }

    /// The same, with the draw meeting the parent first.
    #[test]
    fn a_member_met_after_the_member_it_hangs_from_is_drawn_on_its_hub() {
        let (mut app, p, c, p_at, c_at) = nested_members(false);
        app.update();
        let (p_drawn, c_drawn) = (global_pose(&app, p), global_pose(&app, c));
        assert!(same(p_drawn, p_at), "P is drawn at {p_drawn:?}, its hub holds it at {p_at:?}");
        assert!(same(c_drawn, c_at), "C is drawn at {c_drawn:?}, its hub holds it at {c_at:?}");
    }

    /// Two merged parts whose hubs each hang under the other: each one's place
    /// hangs on where the other is drawn. Both are still drawn this frame. The
    /// draw's search starts from the member with the lowest entity bits,
    /// whatever order the query meets them in, and the cycle is cut at the
    /// first member the search meets again.
    #[test]
    fn members_whose_hubs_hang_under_each_other_are_both_drawn() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::transform::TransformPlugin));
        app.add_systems(PostUpdate, draw_with_hubs.before(TransformSystems::Propagate));
        let world = app.world_mut();
        let folder_t = Transform::from_xyz(5.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(0.5));
        let folder = world.spawn(folder_t).id();
        // The members first, so each hub can hang under the other member; `a`
        // has the lower entity bits.
        let mut ids = [world.spawn_empty().id(), world.spawn_empty().id()];
        ids.sort_by_key(|e| e.to_bits());
        let [a, b] = ids;
        let hub_a_t = Transform::from_xyz(0.4, 0.1, -0.2).with_rotation(Quat::from_rotation_z(0.1));
        let hub_b_t = Transform::from_xyz(-0.3, 0.2, 0.1).with_rotation(Quat::from_rotation_x(0.15));
        let hub_a = world.spawn((hub_a_t, ChildOf(b))).id();
        let hub_b = world.spawn((hub_b_t, ChildOf(a))).id();
        let in_a = Isometry3d::new(Vec3::new(0.5, 0.0, 0.0), Quat::from_rotation_y(0.2));
        let in_b = Isometry3d::new(Vec3::new(0.0, 0.5, 0.0), Quat::from_rotation_x(-0.1));
        let a_start = Transform::from_xyz(1.0, 2.0, 3.0);
        let b_start = Transform::from_xyz(-1.0, 1.0, 0.5).with_rotation(Quat::from_rotation_y(0.3));
        let member = |hub, in_hub| AssemblyMember { hub, in_hub, followed: (Vec3::ZERO, Vec3::ZERO) };
        // `b` takes its parts first, so the query meets `b` before `a`.
        world.entity_mut(b).insert((b_start, ChildOf(folder), member(hub_b, in_b)));
        world.entity_mut(a).insert((a_start, ChildOf(folder), member(hub_a, in_a)));
        let order = world
            .run_system_once(|q: Query<Entity, With<AssemblyMember>>| q.iter().collect::<Vec<Entity>>())
            .expect("the system runs");
        assert_eq!(order, vec![b, a], "the query meets b before a");
        app.update();
        let w = app.world();
        for (e, start) in [(a, a_start), (b, b_start)] {
            let t = w.get::<Transform>(e).expect("a member");
            assert!(t.translation != start.translation || t.rotation != start.rotation, "{e:?} is drawn this frame");
        }
        let pose_of = |(_, rotation, translation): (Vec3, Quat, Vec3)| Isometry3d::new(translation, rotation);
        let drawn = |e: Entity| pose_of(w.get::<GlobalTransform>(e).expect("drawn").to_scale_rotation_translation());
        // Where the hub held the member before this frame's draw: the hub hangs
        // under the other member, at that member's start Transform.
        let held = |other: Transform, hub_t: Transform| {
            pose_of((folder_t.compute_affine() * other.compute_affine() * hub_t.compute_affine()).to_scale_rotation_translation())
        };
        // How far apart two poses are: the translations, and each axis turned.
        let gap = |p: Isometry3d, q: Isometry3d| {
            let axes = [Vec3::X, Vec3::Y, Vec3::Z].map(|v| (p.rotation * v).distance(q.rotation * v));
            (Vec3::from(p.translation).distance(Vec3::from(q.translation)), axes)
        };
        // Tolerance: a cycle has no order that places each member against where
        // the other is drawn this frame. The cut member counts at its Transform as
        // it stood, so the other is placed against its hub as the hub stood before
        // this frame's draw moved it, and lags its hub by the hub's motion this
        // frame: the gap between where the hub held it before the draw and where
        // the hub holds it now. No member may be off by more than that step (plus
        // 1e-5 for rounding); one skipped, or drawn anywhere else, is.
        for (e, hub, hub_t, in_hub, other_start) in [(a, hub_a, hub_a_t, in_a, b_start), (b, hub_b, hub_b_t, in_b, a_start)] {
            let got = drawn(e);
            assert!(got.translation.is_finite() && got.rotation.is_finite(), "{e:?} is drawn at {got:?}");
            let intended = drawn(hub) * in_hub;
            let before = held(other_start, hub_t) * in_hub;
            let (moved, turned) = gap(got, intended);
            let (step, step_turned) = gap(before, intended);
            assert!(
                moved <= step + 1e-5 && turned.iter().zip(step_turned).all(|(t, s)| *t <= s + 1e-5),
                "{e:?} is drawn at {got:?}, its hub holds it at {intended:?} and held it at {before:?} before the draw"
            );
        }
        // Here the search starts from `a` (the lower entity bits, though the query
        // meets `b` first), goes through `b`, and meets `a` again first, so the
        // cycle is cut at `a`. Both members hang from the folder, so each is
        // drawn exactly at the place worked out for it.
        // `b` is placed against `a` as `a` stood: exactly at the cut value, the
        // folder times `a`'s start Transform times its hub's, times its place on
        // the hub. Its lag from its hub, which moved with `a`, is the cut.
        let b_at = held(a_start, hub_b_t) * in_b;
        let got = drawn(b);
        assert!(same(got, b_at), "b is drawn at {got:?}, its place at the cut is {b_at:?}");
        // `a` is placed against `b` as `b` is drawn: `b`'s place found above came
        // first, inside `a`'s search, so `a`'s hub stands at `b`'s new pose (its
        // scale is 1) times the hub's Transform, and `a` at that times its place
        // on the hub. The draw composes the same, so `a` is exact too.
        let b_world = Affine3A::from_rotation_translation(b_at.rotation, Vec3::from(b_at.translation));
        let a_at = pose_of((b_world * hub_a_t.compute_affine()).to_scale_rotation_translation()) * in_a;
        let got = drawn(a);
        assert!(same(got, a_at), "a is drawn at {got:?}, its place against b as drawn is {a_at:?}");
    }
}
