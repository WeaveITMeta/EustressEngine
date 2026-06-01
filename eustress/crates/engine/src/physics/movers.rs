//! # Mover runtime systems — Wave 6.B
//!
//! The *spawners* in [`crate::spawners::constraints`] place each mover's
//! configuration component (the Phase-0 struct) on a child entity of the
//! `BasePart` it drives. **This module is the runtime half**: a set of
//! Bevy systems that read those config components every physics frame and
//! push the corresponding force / velocity / torque onto the parent
//! body's Avian rigid body.
//!
//! ## Roblox semantics mirrored here
//!
//! In Roblox a mover (`VectorForce`, `AlignPosition`, `LinearVelocity`,
//! the legacy `Body*` objects, …) is parented to the `BasePart` it acts
//! on (directly, or via an `Attachment` whose parent is the part). Each
//! system therefore resolves its **target body** by walking up the
//! [`ChildOf`] chain from the mover entity until it finds an entity that
//! carries an Avian [`RigidBody`] (equivalently: a body the solver owns,
//! detected here by the presence of a [`Forces`]/velocity component). The
//! immediate parent is the common case; the walk handles the
//! mover-under-attachment-under-part nesting too.
//!
//! ## Force vs velocity application
//!
//! - **Force movers** (`VectorForce`, `Torque`, `AlignPosition`,
//!   `AlignOrientation`, `BodyForce`, `BodyThrust`, `BodyPosition`,
//!   `BodyGyro`) accumulate into Avian's per-substep [`Forces`] via
//!   `apply_force` / `apply_local_force` / `apply_torque`. These are
//!   cleared each step, so re-applying every frame yields a continuous
//!   force — exactly the Roblox mover model.
//! - **Velocity movers** (`LinearVelocity`, `AngularVelocity`,
//!   `BodyVelocity`, `BodyAngularVelocity`) write the target's Avian
//!   [`LinearVelocity`](avian3d::prelude::LinearVelocity) /
//!   [`AngularVelocity`](avian3d::prelude::AngularVelocity) directly.
//!   The `max_force` / `max_torque` ceiling is honoured by blending
//!   toward the target proportionally to the ceiling rather than snapping
//!   (an unbounded mover sets the velocity outright).
//!
//! ## PD controllers (`AlignPosition` / `AlignOrientation`)
//!
//! Roblox's align movers are critically-damped PD controllers. We
//! reproduce that:
//!
//! ```text
//! force  = P * (target_pos - pos) - D * linear_velocity      (clamped to max_force)
//! torque = P * angle_error_axis   - D * angular_velocity      (clamped to max_torque)
//! ```
//!
//! `P` is derived from `responsiveness` (Roblox's stiffness knob) and `D`
//! from a critical-damping estimate. When `rigidity_enabled` is set the
//! gains go very high (Roblox treats rigid mode as an effectively
//! infinitely-stiff constraint) — we cap `P`/`D` at a large finite value
//! and let the `max_force`/`max_torque` clamp keep it stable.
//!
//! ## Avian / Eustress name collision
//!
//! Avian and `eustress_common::classes` BOTH export `LinearVelocity` and
//! `AngularVelocity`. Throughout this module the Eustress *config*
//! components are referred to by their `eustress_common::classes::` path
//! (re-exported here under `cfg`-prefixed aliases) and the Avian *runtime*
//! components keep their bare prelude names.
//!
//! ## Gating
//!
//! Every system runs only `in_state(PlayModeState::Playing)` — the same
//! gate the simulation/electrochemistry plugins use. Movers do nothing in
//! Edit mode.

use bevy::prelude::*;

use avian3d::prelude::{
    AngularVelocity as AvAngularVelocity, Forces, LinearVelocity as AvLinearVelocity,
    ReadRigidBodyForces, RigidBody, WriteRigidBodyForces,
};

use eustress_common::classes::{
    AlignOrientation, AlignPosition, AngularVelocity as CfgAngularVelocity, BodyAngularVelocity,
    BodyGyro, BodyPosition, BodyThrust, LinearVelocity as CfgLinearVelocity, Torque, VectorForce,
};
// Legacy `BodyVelocity` / `BodyForce` predate the Wave 6.B structs and
// still live in the services module (per the Phase-0 contract), as does
// the shared `ForceRelativeTo` enum the force movers read.
use eustress_common::services::physics::{BodyForce, BodyVelocity, ForceRelativeTo};

use crate::play_mode::PlayModeState;

// ─────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────

/// Maximum gain used when a PD mover is in "rigidity" mode. Roblox treats
/// rigid alignment as an effectively infinite-stiffness constraint; a
/// large finite value plus the `max_force`/`max_torque` clamp keeps the
/// XPBD solver stable while still snapping hard to the target.
const RIGID_GAIN: f32 = 1.0e6;

/// Walk up the [`ChildOf`] chain from `start` and return the first
/// ancestor (including `start` itself) that is an Avian rigid body.
///
/// "Is a rigid body" is tested via `body_filter`, a closure the caller
/// backs with a `Query<(), With<RigidBody>>` (or similar) lookup. The
/// walk is bounded to a small depth to defend against malformed cyclic
/// hierarchies.
fn resolve_target_body(
    start: Entity,
    child_of: &Query<&ChildOf>,
    is_body: &impl Fn(Entity) -> bool,
) -> Option<Entity> {
    // Roblox movers are usually a direct child of the part, sometimes a
    // grandchild (mover → attachment → part). 8 hops is generous.
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

/// Resolve a string-valued `relative_to` (Phase-0 movers store this as a
/// `String`: `"World"` or `"Attachment0"`) to a world-space direction.
/// Anything other than world-frame rotates the vector into the body's
/// frame (the attachment frame is approximated by the body frame until a
/// per-attachment offset query is threaded in).
#[inline]
fn world_dir_str(vec: Vec3, relative_to: &str, body_rot: Quat) -> Vec3 {
    if relative_to.eq_ignore_ascii_case("world") {
        vec
    } else {
        // "Attachment0" / "Attachment1" / anything local → body frame.
        body_rot * vec
    }
}

/// Blend a body's current velocity toward `target` subject to a
/// per-axis-magnitude ceiling. `max` of `0` or non-finite means
/// "unbounded" → set the velocity outright (the Roblox default for an
/// uncapped mover). Otherwise step toward the target by at most `max`
/// (interpreted as a max delta-velocity this frame, the discrete analogue
/// of Roblox's max-force ceiling).
#[inline]
fn approach_velocity(current: Vec3, target: Vec3, max: f32) -> Vec3 {
    if !max.is_finite() || max <= 0.0 {
        return target;
    }
    let delta = target - current;
    let dist = delta.length();
    if dist <= max || dist <= f32::EPSILON {
        target
    } else {
        current + delta / dist * max
    }
}

/// Clamp a force/torque vector to a maximum magnitude. `0`/non-finite ⇒
/// no clamp.
#[inline]
fn clamp_magnitude(v: Vec3, max: f32) -> Vec3 {
    if !max.is_finite() || max <= 0.0 {
        return v;
    }
    let len = v.length();
    if len > max && len > f32::EPSILON {
        v / len * max
    } else {
        v
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Velocity movers
// ─────────────────────────────────────────────────────────────────────────

/// `LinearVelocity` mover → set the parent body's Avian linear velocity.
///
/// The target is selected by `velocity_constraint_mode`:
/// - `"Line"`  → `line_velocity * line_direction`,
/// - `"Plane"` → `plane_velocity` lifted to 3D in the XZ plane,
/// - `"Vector"` (default) → `vector_velocity` directly.
///
/// `relative_to == "Attachment0"` rotates the target into the body frame.
/// The `max_force` field bounds how fast the body is allowed to converge
/// per frame.
pub fn apply_linear_velocity_movers(
    movers: Query<(&CfgLinearVelocity, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut body_q: Query<(&mut AvLinearVelocity, &Transform)>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok((mut vel, xf)) = body_q.get_mut(body) else {
            continue;
        };

        let local_target = match mover.velocity_constraint_mode.as_str() {
            "Line" => mover.line_direction.normalize_or_zero() * mover.line_velocity,
            "Plane" => Vec3::new(mover.plane_velocity.x, 0.0, mover.plane_velocity.y),
            // "Vector" and any unrecognised mode.
            _ => mover.vector_velocity,
        };
        let target = world_dir_str(local_target, &mover.relative_to, xf.rotation);
        vel.0 = approach_velocity(vel.0, target, mover.max_force);
    }
}

/// `AngularVelocity` mover → set the parent body's Avian angular velocity.
///
/// `relative_to == "Attachment0"` rotates the target spin into the body
/// frame. `max_torque` bounds the per-frame convergence.
pub fn apply_angular_velocity_movers(
    movers: Query<(&CfgAngularVelocity, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut body_q: Query<(&mut AvAngularVelocity, &Transform)>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok((mut ang, xf)) = body_q.get_mut(body) else {
            continue;
        };
        let target = world_dir_str(mover.angular_velocity, &mover.relative_to, xf.rotation);
        ang.0 = approach_velocity(ang.0, target, mover.max_torque);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Force movers
// ─────────────────────────────────────────────────────────────────────────

/// `VectorForce` mover → apply a continuous force each frame.
///
/// `relative_to == "Attachment0"` applies the force in the body's local
/// frame (`apply_local_force`); `"World"` applies it in world space
/// (`apply_force`). `apply_at_center_of_mass` is honoured implicitly —
/// Avian's `apply_force`/`apply_local_force` apply at the center of mass;
/// off-center application (which would induce torque) is follow-up work
/// once an attachment-offset query is threaded in.
pub fn apply_vector_force_movers(
    movers: Query<(&VectorForce, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut forces_q: Query<Forces>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok(mut forces) = forces_q.get_mut(body) else {
            continue;
        };
        if mover.relative_to.eq_ignore_ascii_case("world") {
            forces.apply_force(mover.force);
        } else {
            forces.apply_local_force(mover.force);
        }
    }
}

/// `Torque` mover → apply a continuous torque each frame.
///
/// Avian's `apply_torque` is world-space; for `relative_to ==
/// "Attachment0"` we rotate the torque into world space via the body's
/// current rotation (read from the [`Forces`] item — adding `&Transform`
/// to the same query would conflict with the
/// `Write<LinearVelocity>`/`Write<AngularVelocity>` access `Forces`
/// already holds).
pub fn apply_torque_movers(
    movers: Query<(&Torque, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut forces_q: Query<Forces>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok(mut forces) = forces_q.get_mut(body) else {
            continue;
        };
        let body_rot = forces.rotation().0;
        let world_torque = world_dir_str(mover.torque, &mover.relative_to, body_rot);
        forces.apply_torque(world_torque);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// PD controllers — AlignPosition / AlignOrientation
// ─────────────────────────────────────────────────────────────────────────

/// `AlignPosition` mover → PD-drive the parent body toward a target
/// world position.
///
/// `force = P·(target − pos) − D·velocity`, clamped to `max_force`. Pose
/// and velocity are read from the [`Forces`] item (Avian's physics-space
/// `Position` / `LinearVelocity`), not a separate `&Transform` —
/// combining `&LinearVelocity` with `Forces` would be a borrow conflict.
pub fn apply_align_position_movers(
    movers: Query<(&AlignPosition, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut forces_q: Query<Forces>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok(mut forces) = forces_q.get_mut(body) else {
            continue;
        };

        let (p_gain, d_gain) = pd_gains(mover.responsiveness, mover.rigidity_enabled);
        let pos = forces.position().0;
        let vel = forces.linear_velocity();
        let error = mover.position - pos;
        // PD force toward the target. `max_velocity` is treated as a soft
        // damping target: once the body is already moving at/over the
        // ceiling toward the goal, the derivative term dominates and the
        // proportional pull no longer accelerates it further. A precise
        // velocity governor is follow-up work; `max_force` is the hard cap.
        let mut force = error * p_gain - vel * d_gain;
        force = clamp_magnitude(force, mover.max_force);
        if force.is_finite() {
            forces.apply_force(force);
        }
    }
}

/// `AlignOrientation` mover → PD-drive the parent body toward a target
/// orientation.
///
/// `torque = P·angle_error_axis − D·angular_velocity`, clamped to
/// `max_torque`. The orientation error is the shortest-arc rotation from
/// the current to the target orientation, expressed as an axis-angle
/// vector (axis × angle), the standard small-rotation torque target.
pub fn apply_align_orientation_movers(
    movers: Query<(&AlignOrientation, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut forces_q: Query<Forces>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok(mut forces) = forces_q.get_mut(body) else {
            continue;
        };

        let (p_gain, d_gain) = pd_gains(mover.responsiveness, mover.rigidity_enabled);
        let rot = forces.rotation().0;
        let ang = forces.angular_velocity();
        let error_axis = orientation_error(rot, mover.cframe.rotation);
        let mut torque = error_axis * p_gain - ang * d_gain;
        torque = clamp_magnitude(torque, mover.max_torque);
        if torque.is_finite() {
            forces.apply_torque(torque);
        }
    }
}

/// Derive `(P, D)` PD gains from a Roblox-style `responsiveness` knob.
///
/// Roblox's responsiveness ranges roughly 5..200; higher = stiffer. We
/// map it directly to the proportional gain and pick the derivative gain
/// for approximate critical damping (`D ≈ 2·√P`). Rigidity mode pins both
/// gains very high so the body snaps to target (the `max_*` clamp keeps
/// it stable).
#[inline]
fn pd_gains(responsiveness: f32, rigidity_enabled: bool) -> (f32, f32) {
    if rigidity_enabled {
        return (RIGID_GAIN, 2.0 * RIGID_GAIN.sqrt());
    }
    let p = responsiveness.max(0.0);
    let d = 2.0 * p.sqrt();
    (p, d)
}

/// Shortest-arc orientation error from `current` to `target`, as an
/// axis-angle vector (`axis * angle`, angle in `(-π, π]`). Suitable as a
/// proportional torque target.
#[inline]
fn orientation_error(current: Quat, target: Quat) -> Vec3 {
    // Relative rotation that takes `current` onto `target`.
    let mut delta = target * current.inverse();
    // Pick the shorter of the two equivalent quaternions.
    if delta.w < 0.0 {
        delta = Quat::from_xyzw(-delta.x, -delta.y, -delta.z, -delta.w);
    }
    let (axis, angle) = delta.to_axis_angle();
    if angle.abs() <= f32::EPSILON {
        Vec3::ZERO
    } else {
        axis * angle
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Legacy Body* movers — map onto the same math
// ─────────────────────────────────────────────────────────────────────────

/// `BodyVelocity` (legacy) ≈ `LinearVelocity`. Drives the body toward
/// `velocity`, bounded by `max_force` (largest component as the per-frame
/// delta ceiling).
pub fn apply_body_velocity_movers(
    movers: Query<(&BodyVelocity, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut vel_q: Query<&mut AvLinearVelocity>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok(mut vel) = vel_q.get_mut(body) else {
            continue;
        };
        let max = mover.max_force.max_element();
        vel.0 = approach_velocity(vel.0, mover.velocity, max);
    }
}

/// `BodyAngularVelocity` (legacy) ≈ `AngularVelocity`.
pub fn apply_body_angular_velocity_movers(
    movers: Query<(&BodyAngularVelocity, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut ang_q: Query<&mut AvAngularVelocity>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok(mut ang) = ang_q.get_mut(body) else {
            continue;
        };
        let max = mover.max_torque.max_element();
        ang.0 = approach_velocity(ang.0, mover.angular_velocity, max);
    }
}

/// `BodyForce` (legacy) ≈ world-space `VectorForce`.
pub fn apply_body_force_movers(
    movers: Query<(&BodyForce, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut forces_q: Query<Forces>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok(mut forces) = forces_q.get_mut(body) else {
            continue;
        };
        match mover.relative_to {
            ForceRelativeTo::Part => forces.apply_local_force(mover.force),
            ForceRelativeTo::World => forces.apply_force(mover.force),
        }
    }
}

/// `BodyThrust` (legacy) ≈ local-space `VectorForce` (Roblox applies
/// `BodyThrust.Force` in the part's local frame, optionally offset by
/// `Location`). Offset-induced torque is follow-up work — applied at the
/// center of mass for now.
pub fn apply_body_thrust_movers(
    movers: Query<(&BodyThrust, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut forces_q: Query<Forces>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok(mut forces) = forces_q.get_mut(body) else {
            continue;
        };
        forces.apply_local_force(mover.force);
    }
}

/// `BodyPosition` (legacy) ≈ `AlignPosition`. PD-drives the body toward
/// `position` using the legacy `p` (proportional) and `d` (derivative)
/// gains directly, clamped to `max_force` (largest component).
pub fn apply_body_position_movers(
    movers: Query<(&BodyPosition, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut forces_q: Query<Forces>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok(mut forces) = forces_q.get_mut(body) else {
            continue;
        };
        let pos = forces.position().0;
        let vel = forces.linear_velocity();
        let error = mover.position - pos;
        let mut force = error * mover.p - vel * mover.d;
        force = clamp_magnitude(force, mover.max_force.max_element());
        if force.is_finite() {
            forces.apply_force(force);
        }
    }
}

/// `BodyGyro` (legacy) ≈ `AlignOrientation`. PD-drives the body toward
/// `cframe`'s orientation using the legacy `p`/`d` gains, clamped to
/// `max_torque` (largest component).
pub fn apply_body_gyro_movers(
    movers: Query<(&BodyGyro, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut forces_q: Query<Forces>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok(mut forces) = forces_q.get_mut(body) else {
            continue;
        };
        let rot = forces.rotation().0;
        let ang = forces.angular_velocity();
        let error_axis = orientation_error(rot, mover.cframe.rotation);
        let mut torque = error_axis * mover.p - ang * mover.d;
        torque = clamp_magnitude(torque, mover.max_torque.max_element());
        if torque.is_finite() {
            forces.apply_torque(torque);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────

/// Bevy plugin registering every mover runtime system.
///
/// All systems are gated to `in_state(PlayModeState::Playing)` and run in
/// [`FixedUpdate`] — the schedule Avian integrates forces on, and the one
/// Avian's own force tests use. Forces accumulated here are consumed by
/// the solver in the same frame and cleared afterward, so a mover applies
/// a *continuous* effect by re-running every fixed step.
///
/// Mount order: add this plugin after Avian's `PhysicsPlugins` and after
/// the play-mode state has been initialised (both are already up by the
/// time `SlintUiPlugin` adds its child plugins). The plugin only adds
/// systems — it inserts no resources and initialises no state — so it has
/// no ordering requirement beyond `PlayModeState` existing.
pub struct MoversPlugin;

impl Plugin for MoversPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (
                // Velocity setters first so force movers see the updated
                // velocity within the same step where relevant.
                apply_linear_velocity_movers,
                apply_angular_velocity_movers,
                apply_body_velocity_movers,
                apply_body_angular_velocity_movers,
                // Continuous force / torque movers.
                apply_vector_force_movers,
                apply_torque_movers,
                apply_body_force_movers,
                apply_body_thrust_movers,
                // PD controllers.
                apply_align_position_movers,
                apply_align_orientation_movers,
                apply_body_position_movers,
                apply_body_gyro_movers,
            )
                .run_if(in_state(PlayModeState::Playing)),
        );
    }
}
