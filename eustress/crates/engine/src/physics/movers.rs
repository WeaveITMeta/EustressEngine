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
//! Roblox's align movers are critically-damped PD controllers whose
//! Responsiveness is mass-normalised: a heavy part and a light one move
//! alike. We reproduce that with accelerations:
//!
//! ```text
//! linear  = P * (target_pos - pos) - D * linear_velocity - gravity   (mass × it within max_force)
//! angular = P * angle_error_axis   - D * angular_velocity            (inertia × it within max_torque)
//! ```
//!
//! `P` is derived from `responsiveness` (Roblox's stiffness knob) and `D`
//! for critical damping. The gravity term holds a part at its goal rather
//! than sagging below it. When `rigidity_enabled` is set the gains are as
//! stiff as a controller sampled once per fixed step stays stable (Roblox
//! treats rigid mode as an effectively infinitely-stiff constraint).
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
    AngularVelocity as AvAngularVelocity, ComputedAngularInertia, ComputedMass, Forces, Gravity, GravityScale,
    LinearVelocity as AvLinearVelocity, ReadRigidBodyForces, RigidBody, WriteRigidBodyForces,
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
/// An acceleration, `P·(target − pos) − D·velocity − gravity`, as Roblox's
/// Responsiveness: a part moves the same way whatever it weighs, and the
/// gravity term lets a held part sit at its goal instead of sagging g/P
/// below it. `max_force` caps it as a force: mass × acceleration. Pose and
/// velocity are read from the [`Forces`] item (Avian's physics-space
/// `Position` / `LinearVelocity`), not a separate `&Transform`; combining
/// `&LinearVelocity` with `Forces` would be a borrow conflict.
pub fn apply_align_position_movers(
    movers: Query<(&AlignPosition, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut forces_q: Query<(Forces, &ComputedMass, Option<&GravityScale>)>,
    gravity: Option<Res<Gravity>>,
    fixed: Res<Time<Fixed>>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    let gravity = gravity.map_or(Vec3::ZERO, |g| g.0);
    let step = fixed.timestep().as_secs_f32();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok((mut forces, mass, gravity_scale)) = forces_q.get_mut(body) else {
            continue;
        };
        let inverse_mass = mass.inverse();
        if !(inverse_mass > 0.0) {
            // Infinite mass: nothing a mover does can move it.
            continue;
        }

        let (p_gain, d_gain) = pd_gains(mover.responsiveness, mover.rigidity_enabled, step);
        let pos = forces.position().0;
        let vel = forces.linear_velocity();
        let error = mover.position - pos;
        let weight = gravity * gravity_scale.map_or(1.0, |s| s.0);
        // PD toward the target. `max_velocity` is treated as a soft
        // damping target: once the body is already moving at/over the
        // ceiling toward the goal, the derivative term dominates and the
        // proportional pull no longer accelerates it further. A precise
        // velocity governor is follow-up work; `max_force` is the hard cap.
        let mut acceleration = error * p_gain - vel * d_gain - weight;
        acceleration = clamp_magnitude(acceleration, mover.max_force * inverse_mass);
        if acceleration.is_finite() {
            forces.apply_linear_acceleration(acceleration);
        }
    }
}

/// `AlignOrientation` mover → PD-drive the parent body toward a target
/// orientation.
///
/// An angular acceleration, `P·angle_error_axis − D·angular_velocity`,
/// whose torque (world inertia × it) stays within `max_torque`. The
/// orientation error is the shortest-arc rotation from the current to the
/// target orientation, expressed as an axis-angle vector (axis × angle),
/// the standard small-rotation target.
pub fn apply_align_orientation_movers(
    movers: Query<(&AlignOrientation, &ChildOf)>,
    bodies: Query<(), With<RigidBody>>,
    child_of: Query<&ChildOf>,
    mut forces_q: Query<(Forces, &ComputedAngularInertia)>,
    fixed: Res<Time<Fixed>>,
) {
    let is_body = |e: Entity| bodies.get(e).is_ok();
    let step = fixed.timestep().as_secs_f32();
    for (mover, parent) in &movers {
        let Some(body) = resolve_target_body(parent.0, &child_of, &is_body) else {
            continue;
        };
        let Ok((mut forces, inertia)) = forces_q.get_mut(body) else {
            continue;
        };

        let (p_gain, d_gain) = pd_gains(mover.responsiveness, mover.rigidity_enabled, step);
        let rot = forces.rotation().0;
        let ang = forces.angular_velocity();
        let error_axis = orientation_error(rot, mover.cframe.rotation);
        // An angular acceleration, mass-normalised as AlignPosition's, and
        // capped as a torque: the world inertia times it within `max_torque`.
        let mut acceleration = error_axis * p_gain - ang * d_gain;
        let torque = inertia.rotated(rot).value().mul_vec3(acceleration).length();
        if mover.max_torque.is_finite() && mover.max_torque > 0.0 && torque > mover.max_torque {
            acceleration *= mover.max_torque / torque;
        }
        if acceleration.is_finite() {
            forces.apply_angular_acceleration(acceleration);
        }
    }
}

/// Derive `(P, D)` PD gains, as accelerations per unit error, from a
/// Roblox-style `responsiveness` knob.
///
/// Roblox's responsiveness ranges roughly 5..200; higher = stiffer. We
/// map it directly to the proportional gain and pick the derivative gain
/// for critical damping (`D = 2·√P`). Rigidity mode is as stiff as a
/// controller sampled once per fixed step (`step`, seconds) stays stable:
/// critically damped at ω = 0.5 / step, so it settles in a few steps
/// without ringing. A far higher gain rings and then diverges.
#[inline]
fn pd_gains(responsiveness: f32, rigidity_enabled: bool, step: f32) -> (f32, f32) {
    if rigidity_enabled {
        let omega = 0.5 / step.max(1.0e-4);
        return (omega * omega, 2.0 * omega);
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

#[cfg(test)]
mod tests {
    use super::*;
    use avian3d::prelude::{Collider, ColliderDensity, PhysicsPlugins, Position, Rotation};

    /// A headless physics world stepping one 60 Hz frame per update, the two
    /// align movers running every fixed step, as in Play.
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
        // the apps.
        app.init_resource::<avian3d::spatial_query::SpatialQueryDiagnostics>();
        app.init_resource::<avian3d::collider_tree::ColliderTreeDiagnostics>();
        app.init_resource::<avian3d::collision::CollisionDiagnostics>();
        app.init_resource::<avian3d::dynamics::solver::SolverDiagnostics>();
        app.init_asset::<Mesh>();
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)));
        app.add_plugins(PhysicsPlugins::default());
        app.insert_resource(Gravity(Vec3::NEG_Y * 9.81));
        app.add_systems(FixedUpdate, (apply_align_position_movers, apply_align_orientation_movers));
        app
    }

    /// A 4 × 1 × 2 m box at `density`: 19,200 kg of concrete at 2400, 8 kg at 1.
    fn a_box(app: &mut App, density: f32, at: Vec3) -> Entity {
        app.world_mut()
            .spawn((RigidBody::Dynamic, Collider::cuboid(4.0, 1.0, 2.0), ColliderDensity(density), Transform::from_translation(at)))
            .id()
    }

    fn hold_at(app: &mut App, body: Entity, goal: Vec3, max_force: f32, rigid: bool) {
        app.world_mut().spawn((
            AlignPosition { position: goal, max_force, responsiveness: 10.0, rigidity_enabled: rigid, ..Default::default() },
            ChildOf(body),
        ));
    }

    fn at(app: &App, body: Entity) -> Vec3 {
        app.world().get::<Position>(body).map_or(Vec3::NAN, |p| p.0)
    }

    const HEAVY: f32 = 2400.0;
    const LIGHT: f32 = 1.0;

    /// A 19 t concrete part and an 8 kg one, each told to rise 1 m, move
    /// alike and come to rest at the goal: Roblox's mass-normalised
    /// Responsiveness, with gravity held off.
    #[test]
    fn a_heavy_and_a_light_part_move_alike_and_hold() {
        let mut app = physics_world();
        let heavy = a_box(&mut app, HEAVY, Vec3::new(-5.0, 5.0, 0.0));
        let light = a_box(&mut app, LIGHT, Vec3::new(5.0, 5.0, 0.0));
        hold_at(&mut app, heavy, Vec3::new(-5.0, 6.0, 0.0), 1.0e7, false);
        hold_at(&mut app, light, Vec3::new(5.0, 6.0, 0.0), 1.0e7, false);
        for frame in 0..240 {
            app.update();
            let (h, l) = (at(&app, heavy).y, at(&app, light).y);
            assert!((h - l).abs() < 0.01, "frame {frame}: heavy at {h}, light at {l}");
        }
        for body in [heavy, light] {
            assert!((at(&app, body).y - 6.0).abs() < 0.02, "held at {:?}", at(&app, body));
        }
    }

    /// `max_force` caps it as a force: the default 100 kN holds the 8 kg part
    /// but cannot hold up 19 t (188 kN of weight).
    #[test]
    fn max_force_caps_what_a_heavy_part_gets() {
        let mut app = physics_world();
        let heavy = a_box(&mut app, HEAVY, Vec3::new(-5.0, 5.0, 0.0));
        let light = a_box(&mut app, LIGHT, Vec3::new(5.0, 5.0, 0.0));
        let default_force = AlignPosition::default().max_force;
        hold_at(&mut app, heavy, Vec3::new(-5.0, 5.0, 0.0), default_force, false);
        hold_at(&mut app, light, Vec3::new(5.0, 5.0, 0.0), default_force, false);
        for _ in 0..120 {
            app.update();
        }
        assert!(at(&app, heavy).y < 4.0, "19 t held by {default_force} N: {:?}", at(&app, heavy));
        assert!((at(&app, light).y - 5.0).abs() < 0.02, "8 kg: {:?}", at(&app, light));
    }

    /// Rigidity snaps a 19 t part to its goal within a second without
    /// ringing past it, where a gain of 1e6 per step rang and diverged.
    #[test]
    fn rigid_mode_snaps_a_heavy_part_without_ringing() {
        let mut app = physics_world();
        let heavy = a_box(&mut app, HEAVY, Vec3::new(0.0, 5.0, 0.0));
        hold_at(&mut app, heavy, Vec3::new(0.0, 6.0, 0.0), 1.0e8, true);
        let mut highest = f32::MIN;
        for _ in 0..60 {
            app.update();
            highest = highest.max(at(&app, heavy).y);
        }
        assert!((at(&app, heavy).y - 6.0).abs() < 0.01, "at {:?} after a second", at(&app, heavy));
        assert!(highest < 6.05, "rang to {highest}");
    }

    /// AlignOrientation turns a heavy and a light part alike, to the goal.
    #[test]
    fn a_heavy_and_a_light_part_turn_alike() {
        let mut app = physics_world();
        let turned = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let bodies = [(HEAVY, -5.0), (LIGHT, 5.0)].map(|(density, x)| {
            let body = a_box(&mut app, density, Vec3::new(x, 5.0, 0.0));
            app.world_mut().get_mut::<Transform>(body).unwrap().rotation = turned;
            // Held in place, so only the turn is measured.
            hold_at(&mut app, body, Vec3::new(x, 5.0, 0.0), 1.0e9, false);
            app.world_mut().spawn((
                AlignOrientation { cframe: Transform::IDENTITY, max_torque: 1.0e9, responsiveness: 10.0, ..Default::default() },
                ChildOf(body),
            ));
            body
        });
        let angle = |app: &App, body: Entity| {
            app.world().get::<Rotation>(body).map_or(f32::NAN, |r| r.0.angle_between(Quat::IDENTITY))
        };
        for frame in 0..240 {
            app.update();
            let (h, l) = (angle(&app, bodies[0]), angle(&app, bodies[1]));
            assert!((h - l).abs() < 0.01, "frame {frame}: heavy at {h} rad, light at {l} rad");
        }
        for body in bodies {
            assert!(angle(&app, body) < 0.02, "left at {} rad", angle(&app, body));
        }
    }
}
