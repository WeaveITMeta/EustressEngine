//! # Constraint joint body resolver — Wave F1d
//!
//! The constraint spawners in [`crate::spawners::constraints`] attach the
//! correct Avian joint (`FixedJoint` / `RevoluteJoint` / `DistanceJoint` /
//! `PrismaticJoint` / `SphericalJoint`) to a freshly-spawned constraint
//! entity, but with **placeholder bodies** — both `joint.body1` and
//! `joint.body2` are [`Entity::PLACEHOLDER`]. They cannot resolve the real
//! bodies at spawn time because [`ClassSpawner::spawn`] sees only
//! `Commands`, not a `World` query (see `spawners/constraints/mod.rs` §
//! "Entity ref resolution").
//!
//! This module is the promised downstream resolver. Each frame (in play
//! mode) it walks every joint that still carries a placeholder body and
//! patches `body1` / `body2` from the Eustress constraint component's
//! part / attachment references:
//!
//! - **Part-referenced constraints** (`WeldConstraint`, `Motor6D`,
//!   `HingeConstraint`, `DistanceConstraint`, `PrismaticConstraint`,
//!   `BallSocketConstraint`, `RopeConstraint`) store `part0` / `part1` as
//!   Eustress instance-ids (`Option<u32>`). We map id → `Entity` via
//!   `Query<(Entity, &Instance)>` and bind the joint directly to the part
//!   entity.
//! - **Attachment-referenced constraints** (`RodConstraint`,
//!   `CylindricalConstraint`, `TorsionSpringConstraint`,
//!   `UniversalConstraint`) store `attachment0` / `attachment1` as the
//!   Eustress instance-ids of `Attachment` entities. We resolve the
//!   attachment entity by id, then walk up its [`ChildOf`] chain to the
//!   first ancestor that is an Avian [`RigidBody`] — the part the joint
//!   must bind. This mirrors
//!   [`crate::physics::movers::resolve_target_body`].
//!
//! ## Self-limiting, idempotent, and safe
//!
//! - A joint is only inspected while it still holds a placeholder; once
//!   both ends are patched it is skipped on subsequent frames. No marker
//!   component is needed — the placeholder is its own "needs resolve" flag.
//! - Instance-id `0` is the unassigned sentinel (TOML-loaded parts spawn
//!   with `Instance.id == 0`). We never match against id `0`, so an
//!   unpopulated id can't mis-bind every id-0 entity to the first joint.
//!   When ids are unpopulated the resolver simply leaves the placeholder
//!   in place — strictly no worse than the pre-resolver behavior (Avian
//!   tolerates a placeholder body until the joint actually integrates and
//!   emits its own warning).
//! - We patch each side independently and only when resolution succeeds,
//!   so a half-resolvable joint still binds the side it can.
//!
//! ## Gating
//!
//! Systems run only `in_state(PlayModeState::Playing)`, matching the
//! mover runtime in [`crate::physics::movers`].

use bevy::prelude::*;

use avian3d::prelude::{
    DistanceJoint, FixedJoint, PrismaticJoint, RevoluteJoint, RigidBody, SphericalJoint,
};

use eustress_common::classes::{
    BallSocketConstraint, CylindricalConstraint, DistanceConstraint, HingeConstraint, Instance,
    Motor6D, PrismaticConstraint, RodConstraint, RopeConstraint, TorsionSpringConstraint,
    UniversalConstraint, WeldConstraint,
};

use crate::play_mode::PlayModeState;

// ─────────────────────────────────────────────────────────────────────────
// Shared resolution helpers
// ─────────────────────────────────────────────────────────────────────────

/// Resolve a Eustress instance-id to its Bevy `Entity` by scanning the
/// `Instance` query. Returns `None` for the unassigned sentinel (`0`) or
/// when no live entity carries that id. Linear scan — joint resolution is
/// a rare, transient event (only while a placeholder remains), not a
/// per-frame hot path once the scene settles.
fn entity_for_instance_id(id: u32, instances: &Query<(Entity, &Instance)>) -> Option<Entity> {
    if id == 0 {
        return None;
    }
    instances
        .iter()
        .find(|(_, inst)| inst.id == id)
        .map(|(e, _)| e)
}

/// Walk up the [`ChildOf`] chain from `start` (inclusive) and return the
/// first ancestor that is an Avian rigid body. Mirrors
/// [`crate::physics::movers::resolve_target_body`] — bounded to 8 hops to
/// defend against malformed cyclic hierarchies. Used by the
/// attachment-referenced constraints, whose stored ids point at the
/// `Attachment` entity rather than the part body.
fn rigid_body_ancestor(
    start: Entity,
    child_of: &Query<&ChildOf>,
    is_body: &impl Fn(Entity) -> bool,
) -> Option<Entity> {
    let mut current = start;
    for _ in 0..8 {
        if is_body(current) {
            return Some(current);
        }
        match child_of.get(current) {
            Ok(parent) => current = parent.0,
            Err(_) => return None,
        }
    }
    None
}

/// Resolve a part-referenced target id directly to its entity.
fn resolve_part(id: Option<u32>, instances: &Query<(Entity, &Instance)>) -> Option<Entity> {
    entity_for_instance_id(id?, instances)
}

/// Resolve an attachment-referenced target id: map id → attachment
/// entity, then climb to the owning rigid body.
fn resolve_attachment(
    id: Option<u32>,
    instances: &Query<(Entity, &Instance)>,
    child_of: &Query<&ChildOf>,
    is_body: &impl Fn(Entity) -> bool,
) -> Option<Entity> {
    let attachment = entity_for_instance_id(id?, instances)?;
    rigid_body_ancestor(attachment, child_of, is_body)
}

// ─────────────────────────────────────────────────────────────────────────
// Per-(constraint, joint) resolver systems
// ─────────────────────────────────────────────────────────────────────────

/// Generate a resolver system for a part-referenced constraint.
///
/// `$sys`   — system fn name
/// `$cons`  — Eustress constraint component (provides `part0` / `part1`)
/// `$joint` — Avian joint component (provides `body1` / `body2`)
macro_rules! part_resolver_system {
    ($sys:ident, $cons:ty, $joint:ty) => {
        fn $sys(
            mut joints: Query<(&$cons, &mut $joint)>,
            instances: Query<(Entity, &Instance)>,
        ) {
            for (cons, mut joint) in joints.iter_mut() {
                if joint.body1 == Entity::PLACEHOLDER {
                    if let Some(e) = resolve_part(cons.part0, &instances) {
                        joint.body1 = e;
                    }
                }
                if joint.body2 == Entity::PLACEHOLDER {
                    if let Some(e) = resolve_part(cons.part1, &instances) {
                        joint.body2 = e;
                    }
                }
            }
        }
    };
}

/// Generate a resolver system for an attachment-referenced constraint.
///
/// `$sys`   — system fn name
/// `$cons`  — Eustress constraint component (provides `attachment0` / `attachment1`)
/// `$joint` — Avian joint component (provides `body1` / `body2`)
macro_rules! attachment_resolver_system {
    ($sys:ident, $cons:ty, $joint:ty) => {
        fn $sys(
            mut joints: Query<(&$cons, &mut $joint)>,
            instances: Query<(Entity, &Instance)>,
            child_of: Query<&ChildOf>,
            bodies: Query<(), With<RigidBody>>,
        ) {
            let is_body = |e: Entity| bodies.get(e).is_ok();
            for (cons, mut joint) in joints.iter_mut() {
                if joint.body1 == Entity::PLACEHOLDER {
                    if let Some(e) =
                        resolve_attachment(cons.attachment0, &instances, &child_of, &is_body)
                    {
                        joint.body1 = e;
                    }
                }
                if joint.body2 == Entity::PLACEHOLDER {
                    if let Some(e) =
                        resolve_attachment(cons.attachment1, &instances, &child_of, &is_body)
                    {
                        joint.body2 = e;
                    }
                }
            }
        }
    };
}

// Part-referenced constraints → joint type.
part_resolver_system!(resolve_weld_bodies, WeldConstraint, FixedJoint);
part_resolver_system!(resolve_motor6d_bodies, Motor6D, RevoluteJoint);
part_resolver_system!(resolve_hinge_bodies, HingeConstraint, RevoluteJoint);
part_resolver_system!(resolve_distance_bodies, DistanceConstraint, DistanceJoint);
part_resolver_system!(resolve_prismatic_bodies, PrismaticConstraint, PrismaticJoint);
part_resolver_system!(resolve_ball_socket_bodies, BallSocketConstraint, SphericalJoint);
part_resolver_system!(resolve_rope_bodies, RopeConstraint, DistanceJoint);

// Attachment-referenced constraints → joint type.
attachment_resolver_system!(resolve_rod_bodies, RodConstraint, DistanceJoint);
attachment_resolver_system!(resolve_cylindrical_bodies, CylindricalConstraint, PrismaticJoint);
attachment_resolver_system!(resolve_torsion_spring_bodies, TorsionSpringConstraint, RevoluteJoint);
attachment_resolver_system!(resolve_universal_bodies, UniversalConstraint, SphericalJoint);

// ─────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────

/// Registers the constraint joint body resolver systems. Gated to
/// `PlayModeState::Playing`, matching [`crate::physics::movers::MoversPlugin`].
///
/// Mount via `app.add_plugins(JointResolverPlugin)` next to `MoversPlugin`.
pub struct JointResolverPlugin;

impl Plugin for JointResolverPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                // Part-referenced.
                resolve_weld_bodies,
                resolve_motor6d_bodies,
                resolve_hinge_bodies,
                resolve_distance_bodies,
                resolve_prismatic_bodies,
                resolve_ball_socket_bodies,
                resolve_rope_bodies,
                // Attachment-referenced.
                resolve_rod_bodies,
                resolve_cylindrical_bodies,
                resolve_torsion_spring_bodies,
                resolve_universal_bodies,
            )
                .run_if(in_state(PlayModeState::Playing)),
        );
    }
}
