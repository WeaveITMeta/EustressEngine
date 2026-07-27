//! # Two-bone IK, foot planting, and ground adaptation
//!
//! ## Why rate-matching is not enough
//!
//! `anim.rs` scales playback rate to ground speed, which reduces skating but
//! can never eliminate it — the clamp leaks, and on slopes or stairs the
//! authored foot path is simply wrong. A **foot lock** can eliminate it: once
//! a foot is planted, its world position is latched and the IK target holds it
//! there regardless of what the clip does.
//!
//! ## Why this does its own forward kinematics
//!
//! These systems run in [`AvatarSystems::PostAnim`], which is
//! `.after(AnimationSystems).before(TransformSystems::Propagate)` — the only
//! window where animated local transforms exist but have not yet been baked
//! into `GlobalTransform`. Reading `GlobalTransform` here would therefore read
//! **last frame's** pose, and IK against a stale pose oscillates.
//!
//! So [`world_of`] composes the local chain up to the avatar root. Depth is
//! ~8 for a Mixamo rig, so this is cheap and, unlike `GlobalTransform`,
//! current.
//!
//! The old code's foot IK never ran at all: `update_foot_ik` was written but
//! never registered, its ground raycast was an unconditional `None` under a
//! `#[cfg]` that was enabled anyway, and it approximated feet at `±0.15 m`
//! from the character centre rather than reading foot bones.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::climb::{AvatarClimb, ClimbPhase};
use super::rig::{AvatarRig, HumanoidBone};
use super::spawn::{AvatarBody, AvatarLocomotion};
use super::{AvatarSystems, SpawnedByAvatarRuntime};

/// Below this the foot is considered planted.
const PLANT_SPEED_FRAC: f32 = 0.15;
/// How far below the foot to look for ground.
const FOOT_PROBE_DOWN: f32 = 0.55;
/// Maximum foot pitch when conforming to a slope.
const MAX_FOOT_ROLL_DEG: f32 = 35.0;
/// IK fades out above this multiple of run speed — on a fast sprint the clip
/// reads better than the solve.
const IK_FADE_ABOVE: f32 = 1.4;
/// How far past the ledge lip the palms sit, so fingers wrap the edge rather
/// than floating on the face.
const HAND_GRIP_LIFT: f32 = 0.03;
/// Soles press this far off the wall face — the collision skin, so the foot
/// contacts rather than intersects.
const SOLE_WALL_CLEARANCE: f32 = 0.05;
/// Mantle fraction over which the hands let go of the edge.
///
/// Measured, not guessed: the body clears 1.96 m in 0.65 s, so with a ~0.54 m
/// arm span the grip stays solvable only for the first ~15% of the pull.
/// Holding past that leaves the arms stretched straight at a point they cannot
/// reach, which reads as the hands being dragged behind the body.
const HAND_RELEASE_BAND: (f32, f32) = (0.15, 0.55);
/// How fast the grip blends in and out, per second. A hand catching a ledge is
/// near-instant; at the old 18/s the blend was still climbing when the mantle
/// took over.
const GRIP_BLEND_RATE: f32 = 35.0;
/// Furthest a solved chain is allowed to reach, as a fraction of its own
/// length. Keeps a residual bend in the knee and elbow instead of snapping to
/// a locked joint at the solver's straight-line case.
const MAX_CHAIN_EXTENSION: f32 = 0.97;

/// Per-foot lock state.
#[derive(Debug, Clone, Copy, Default)]
pub struct FootLock {
    pub locked: bool,
    pub world_pos: Vec3,
    pub normal: Vec3,
    /// 0..1, ramped so unlocking does not pop.
    pub weight: f32,
}

/// Foot-IK state for one avatar.
#[derive(Component, Debug, Clone, Default)]
pub struct AvatarFootIk {
    pub left: FootLock,
    pub right: FootLock,
    /// Critically damped pelvis drop so the hips follow the lower foot.
    pub pelvis_offset: f32,
    pub pelvis_velocity: f32,

    // ── Climb-limb diagnostics ──────────────────────────────────────────────
    //
    // These exist so "the hands are on the ledge" is a measurement rather than
    // a claim. `hand_error_m` is the distance from the SOLVED hand to the
    // target it was asked to reach; if the solver is not running, or is
    // running against a stale pose, this does not fall.
    /// Blend weight currently applied to the arm chains, 0..1.
    pub hand_weight: f32,
    /// Worst of the two hands' distance-to-target after the solve, metres.
    pub hand_error_m: f32,
    /// Worst of the two feet's distance-to-target after the solve, metres.
    pub climb_foot_error_m: f32,
}

pub(crate) struct AvatarIkPlugin;

impl Plugin for AvatarIkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, attach_ik_state.in_set(AvatarSystems::Lifecycle))
            .add_systems(
                PostUpdate,
                solve_limb_ik
                    .in_set(AvatarSystems::PostAnim)
                    // The procedural life layer also writes bone locals. IK is
                    // the harder constraint — a planted foot or a gripped ledge
                    // must win over a breathing sway — so it composes last.
                    .after(super::procedural::apply_life_to_bones),
            );
    }
}

fn attach_ik_state(
    mut commands: Commands,
    q: Query<Entity, (With<SpawnedByAvatarRuntime>, With<AvatarRig>, Without<AvatarFootIk>)>,
) {
    for e in q.iter() {
        commands.entity(e).try_insert(AvatarFootIk::default());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The solver
// ─────────────────────────────────────────────────────────────────────────────

/// Analytic two-bone IK by the law of cosines.
///
/// Returns `(upper_dir, lower_dir)` — unit vectors the upper and lower bone
/// should point along, in the same space as the inputs.
///
/// `pole` biases the joint so the knee bends forward instead of wandering to
/// an arbitrary solution on the circle of valid positions.
///
/// Unreachable targets are clamped to full extension rather than producing NaN
/// — a NaN here propagates into a bone rotation and corrupts the whole pose.
pub fn solve_two_bone(
    root: Vec3,
    target: Vec3,
    upper_len: f32,
    lower_len: f32,
    pole: Vec3,
) -> (Vec3, Vec3) {
    let total = upper_len + lower_len;
    let to_target = target - root;
    let dist = to_target.length();

    // Degenerate: target on top of the root.
    if dist < 1e-5 || upper_len <= 1e-5 || lower_len <= 1e-5 {
        let d = Vec3::NEG_Y;
        return (d, d);
    }

    let dir = to_target / dist;

    // Beyond reach → straight line. Also covers the "sum of two sides"
    // degenerate case where the cosine would exceed 1.
    if dist >= total * 0.999 {
        return (dir, dir);
    }

    // Below the difference of the two bones the triangle is also impossible.
    let min_reach = (upper_len - lower_len).abs();
    let dist = dist.max(min_reach * 1.001);

    // Angle at the root between the chain direction and the upper bone.
    let cos_root =
        ((upper_len * upper_len + dist * dist - lower_len * lower_len) / (2.0 * upper_len * dist))
            .clamp(-1.0, 1.0);
    let root_angle = cos_root.acos();

    // Bend plane: defined by the chain direction and the pole vector.
    let pole_dir = pole - root;
    let mut axis = dir.cross(pole_dir);
    if axis.length_squared() < 1e-8 {
        // Pole is colinear with the chain — pick any stable perpendicular.
        axis = dir.cross(Vec3::Z);
        if axis.length_squared() < 1e-8 {
            axis = dir.cross(Vec3::X);
        }
    }
    let axis = axis.normalize();

    // Sign matters: with `axis = dir × pole_dir`, a POSITIVE rotation carries
    // the joint toward the pole. Negating it bends the knee backwards, which
    // the `knee_bends_toward_the_pole_not_away` test catches.
    let upper_dir = Quat::from_axis_angle(axis, root_angle) * dir;
    let joint = root + upper_dir * upper_len;
    let lower_dir = (target - joint).normalize_or(dir);

    (upper_dir, lower_dir)
}

/// World transform of `e`, composed from local transforms up to `root`.
///
/// Used instead of `GlobalTransform` because in `PostAnim` the global
/// transforms are one frame stale.
///
/// Reads through the same `&mut Transform` query the solver writes with —
/// a second, unfiltered `Query<&Transform>` would overlap it and Bevy rejects
/// that at schedule build (B0001).
fn world_of(
    e: Entity,
    root: Entity,
    root_tf: &Transform,
    parents: &Query<&ChildOf>,
    locals: &Query<&mut Transform, Without<SpawnedByAvatarRuntime>>,
) -> Option<Transform> {
    let mut chain = Vec::with_capacity(10);
    let mut cur = e;
    let mut guard = 0;
    while cur != root {
        chain.push(cur);
        let Ok(p) = parents.get(cur) else { return None };
        cur = p.parent();
        guard += 1;
        if guard > 32 {
            return None;
        }
    }

    let mut acc = *root_tf;
    for node in chain.iter().rev() {
        let Ok(l) = locals.get(*node) else { return None };
        acc = acc.mul_transform(*l);
    }
    Some(acc)
}

/// Re-express a **world-space** rotation delta as the local rotation that
/// produces it, given the bone's own current world and local rotations.
///
/// This is the fix the whole module was blocked on. A bone's world rotation is
/// `W = P · L` for parent world `P`. Asking for `W' = Δ · W` gives
/// `L' = P⁻¹ · Δ · P · L`, and substituting `P = W · L⁻¹` collapses that to
/// `L' = L · (W⁻¹ · Δ · W)` — expressible from the bone alone, with no
/// separate parent lookup.
///
/// The old code wrote `L' = Δ · L`, which is the same thing only when `P` is
/// identity. Inside a skeleton it never is, so every solved bone was rotated
/// about the wrong axis by the parent's orientation.
pub(crate) fn local_after_world_delta(bone_world: Quat, bone_local: Quat, delta: Quat) -> Quat {
    (bone_local * (bone_world.inverse() * delta * bone_world)).normalize()
}

/// `from → to` rotation, blended toward identity by `w`.
fn scaled_arc(from: Vec3, to: Vec3, w: f32) -> Quat {
    Quat::IDENTITY.slerp(Quat::from_rotation_arc(from, to), w.clamp(0.0, 1.0))
}

fn apply_world_delta(
    writes: &mut Query<&mut Transform, Without<SpawnedByAvatarRuntime>>,
    bone: Entity,
    bone_world: Quat,
    delta: Quat,
) {
    if let Ok(mut t) = writes.get_mut(bone) {
        t.rotation = local_after_world_delta(bone_world, t.rotation, delta);
    }
}

/// Aim a two-bone chain (`up_e` → `lo_e` → `end_e`) at a world `target`.
///
/// Returns how far the end effector finished from the target, in metres — the
/// only honest way to know the solve landed. Shared by legs and arms: a
/// shoulder/elbow/hand chain and a hip/knee/foot chain are the same problem.
#[allow(clippy::too_many_arguments)]
fn drive_two_bone_chain(
    writes: &mut Query<&mut Transform, Without<SpawnedByAvatarRuntime>>,
    parents: &Query<&ChildOf>,
    root: Entity,
    root_tf: &Transform,
    up_e: Entity,
    lo_e: Entity,
    end_e: Entity,
    target: Vec3,
    pole: Vec3,
    weight: f32,
) -> Option<f32> {
    if weight < 1e-3 {
        return None;
    }

    let up_w = world_of(up_e, root, root_tf, parents, writes)?;
    let lo_w = world_of(lo_e, root, root_tf, parents, writes)?;
    let end_w = world_of(end_e, root, root_tf, parents, writes)?;

    // Segment lengths measured from the live pose, so a rescaled or
    // non-Mixamo rig needs no table of constants.
    let upper_len = (lo_w.translation - up_w.translation).length();
    let lower_len = (end_w.translation - lo_w.translation).length();
    if upper_len < 1e-4 || lower_len < 1e-4 {
        return None;
    }

    // Never drive the chain to full extension. `solve_two_bone` returns a
    // straight line at >=99.9% reach, which on a leg means a locked knee — a
    // pose no walk cycle ever contains and the single loudest tell that a
    // character is being posed by IK rather than animated.
    let reachable = (upper_len + lower_len) * MAX_CHAIN_EXTENSION;
    let to_target = target - up_w.translation;
    let target = if to_target.length() > reachable {
        up_w.translation + to_target.normalize_or_zero() * reachable
    } else {
        target
    };

    let (want_upper, want_lower) =
        solve_two_bone(up_w.translation, target, upper_len, lower_len, pole);

    let cur_upper = (lo_w.translation - up_w.translation).normalize_or(Vec3::NEG_Y);
    let cur_lower = (end_w.translation - lo_w.translation).normalize_or(Vec3::NEG_Y);

    let d_up = scaled_arc(cur_upper, want_upper, weight);
    apply_world_delta(writes, up_e, up_w.rotation, d_up);

    // The lower bone is a CHILD of the upper, so it has already been carried
    // along by `d_up`. Aiming it from its pre-rotation direction would
    // double-count that rotation and the chain would overshoot every frame.
    let carried_dir = d_up * cur_lower;
    let carried_rot = d_up * lo_w.rotation;
    let d_lo = scaled_arc(carried_dir, want_lower, weight);
    apply_world_delta(writes, lo_e, carried_rot, d_lo);

    // Where the end effector actually finished. At weight < 1 this is
    // deliberately short of the target; that is a blend, not an error.
    let joint = up_w.translation + (d_up * cur_upper) * upper_len;
    let reached = joint + (d_lo * carried_dir) * lower_len;
    Some((reached - target).length())
}

#[allow(clippy::too_many_arguments)]
fn solve_limb_ik(
    time: Res<Time>,
    spatial: SpatialQuery,
    parents: Query<&ChildOf>,
    // ONE Transform access for bones, disjoint from the avatar-root query
    // below by `Without`/`With`. Used for both reads (via `.get`) and writes.
    mut writes: Query<&mut Transform, Without<SpawnedByAvatarRuntime>>,
    mut q: Query<
        (
            Entity,
            &Transform,
            &AvatarRig,
            &AvatarLocomotion,
            &AvatarBody,
            &AvatarClimb,
            &mut AvatarFootIk,
        ),
        With<SpawnedByAvatarRuntime>,
    >,
) {
    let dt = time.delta_secs().max(1e-5);

    for (root, root_tf, rig, loco, body, climb, mut ik) in q.iter_mut() {
        if !rig.is_healthy() {
            continue;
        }

        // ── Climbing: the limbs answer to the ledge, not to the ground ─────
        let hand_target_w = climb_hand_weight(climb);
        ik.hand_weight += (hand_target_w - ik.hand_weight) * (1.0 - (-GRIP_BLEND_RATE * dt).exp());

        if ik.hand_weight > 1e-3 {
            solve_climb_limbs(
                &mut writes, &parents, root, root_tf, rig, body, climb, &mut ik,
            );
            // The ground solver must not also be running: there is no ground
            // under a hanging character, and a half-faded foot lock left over
            // from the run-up would drag the legs back down.
            ik.left = FootLock::default();
            ik.right = FootLock::default();
            continue;
        }

        ik.hand_error_m = 0.0;
        ik.climb_foot_error_m = 0.0;

        // Fade IK out at speed and in the air.
        let speed_fade =
            1.0 - ((loco.speed_norm - IK_FADE_ABOVE) / 0.6).clamp(0.0, 1.0);

        let filter = SpatialQueryFilter::default().with_excluded_entities([root]);

        let legs = [
            (HumanoidBone::LeftUpLeg, HumanoidBone::LeftLeg, HumanoidBone::LeftFoot, true),
            (HumanoidBone::RightUpLeg, HumanoidBone::RightLeg, HumanoidBone::RightFoot, false),
        ];

        let mut lowest_delta = 0.0_f32;

        for (up_b, lo_b, foot_b, is_left) in legs {
            let (Some(up_e), Some(lo_e), Some(foot_e)) =
                (rig.bone(up_b), rig.bone(lo_b), rig.bone(foot_b))
            else {
                continue;
            };

            let (Some(hip_w), Some(foot_w)) = (
                world_of(up_e, root, root_tf, &parents, &writes),
                world_of(foot_e, root, root_tf, &parents, &writes),
            ) else {
                continue;
            };

            // Ground under the animated foot.
            let origin = foot_w.translation + Vec3::Y * 0.30;
            let hit = spatial.cast_ray(origin, Dir3::NEG_Y, FOOT_PROBE_DOWN + 0.30, true, &filter);

            let lock = if is_left { &mut ik.left } else { &mut ik.right };

            let Some(hit) = hit else {
                lock.locked = false;
                lock.weight = (lock.weight - dt / 0.12).max(0.0);
                continue;
            };

            let ground_pos = origin + Vec3::NEG_Y * hit.distance;
            let ground_n = Vec3::from(hit.normal);

            // Plant test: the foot is slow relative to this body's stride.
            let planted = loco.planar_speed
                < body.motion.walk_speed * PLANT_SPEED_FRAC * body.metrics.stride_scale.max(0.25);

            if planted && !lock.locked {
                lock.locked = true;
                lock.world_pos = ground_pos;
                lock.normal = ground_n;
            } else if !planted {
                lock.locked = false;
            }

            // ONLY the planted foot is corrected.
            //
            // This used to pull the swing foot down to `ground_pos` as well —
            // the ground directly beneath it — every frame of the stride. That
            // pins the ankle to the floor through the whole gait cycle, so the
            // knee never gets to bend and the walk reads stiff and skated. The
            // authored clip already knows where the swing foot goes; IK has no
            // business there.
            //
            // The cost is that ground adaptation now only applies while the
            // foot is actually planted, which is the correct scope for it
            // anyway: a foot in mid-air has no ground to adapt to.
            let target = lock.world_pos;
            let normal = lock.normal;

            let want = if loco.grounded && lock.locked { speed_fade } else { 0.0 };
            lock.weight += (want - lock.weight) * (1.0 - (-10.0 * dt).exp());
            if lock.weight < 1e-3 {
                continue;
            }

            lowest_delta = lowest_delta.min(target.y - foot_w.translation.y);

            // Pole in front of the hip so the knee always bends forward.
            let fwd = root_tf.rotation * Vec3::NEG_Z;
            let pole = hip_w.translation + fwd * (0.35 * body.metrics.leg_length.max(0.1));

            let w = lock.weight;
            drive_two_bone_chain(
                &mut writes, &parents, root, root_tf, up_e, lo_e, foot_e, target, pole, w,
            );

            // Conform the foot to the surface, clamped.
            //
            // Recomposed AFTER the chain solve: the leg has just moved, so the
            // pre-solve foot orientation is the wrong frame to conjugate
            // through. This is the same world→local trap the chain solve had.
            if let Some(foot_now) = world_of(foot_e, root, root_tf, &parents, &writes) {
                if let Ok(mut ft) = writes.get_mut(foot_e) {
                    let want = Quat::from_rotation_arc(Vec3::Y, normal.normalize_or(Vec3::Y));
                    let (axis, angle) = want.to_axis_angle();
                    let clamped = Quat::from_axis_angle(
                        axis,
                        angle.clamp(
                            -MAX_FOOT_ROLL_DEG.to_radians(),
                            MAX_FOOT_ROLL_DEG.to_radians(),
                        ),
                    );
                    let blended = Quat::IDENTITY.slerp(clamped, (w * 0.6).clamp(0.0, 1.0));
                    ft.rotation = local_after_world_delta(foot_now.rotation, ft.rotation, blended);
                }
            }
        }

        // ── Pelvis levelling ───────────────────────────────────────────────
        //
        // Drop the hips toward the LOWER foot so a leg on a step still
        // reaches. Critically damped (zeta = 1) so it settles without
        // overshoot.
        let want = lowest_delta.min(0.0).max(-0.35);
        let omega = 12.0;
        let accel =
            omega * omega * (want - ik.pelvis_offset) - 2.0 * omega * ik.pelvis_velocity;
        ik.pelvis_velocity += accel * dt;
        ik.pelvis_offset += ik.pelvis_velocity * dt;

        if let Some(hips) = rig.bone(HumanoidBone::Hips) {
            if let Ok(mut h) = writes.get_mut(hips) {
                h.translation.y += ik.pelvis_offset;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Climb: hands on the ledge, soles on the wall
// ─────────────────────────────────────────────────────────────────────────────

/// How strongly the arms are bound to the ledge in the current climb phase.
///
/// Hanging holds full grip. The mantle keeps the grip through the pull — the
/// hands are what the body is pulling *against* — and lets go over the back
/// half, so the arms are free to swing onto the top surface instead of staying
/// welded to an edge that is now behind and below.
pub(crate) fn climb_hand_weight(climb: &AvatarClimb) -> f32 {
    match climb.phase {
        ClimbPhase::None => 0.0,
        ClimbPhase::Hanging => 1.0,
        ClimbPhase::Mantling => {
            let (a, b) = HAND_RELEASE_BAND;
            1.0 - ((climb.t - a) / (b - a)).clamp(0.0, 1.0)
        }
    }
}

/// Plant both hands on the ledge edge and both soles on the wall face.
///
/// The frame is built from the ledge itself, not from the character, so the
/// grip stays put while the body swings under it.
#[allow(clippy::too_many_arguments)]
fn solve_climb_limbs(
    writes: &mut Query<&mut Transform, Without<SpawnedByAvatarRuntime>>,
    parents: &Query<&ChildOf>,
    root: Entity,
    root_tf: &Transform,
    rig: &AvatarRig,
    body: &AvatarBody,
    climb: &AvatarClimb,
    ik: &mut AvatarFootIk,
) {
    let m = &body.metrics;
    let w = ik.hand_weight;

    // Ledge frame: `n` points off the wall toward the character, `tangent`
    // runs along the lip.
    let n = climb.wall_normal.with_y(0.0).normalize_or(Vec3::Z);
    let tangent = Vec3::Y.cross(n).normalize_or(Vec3::X);
    let edge = climb.grab_point;

    let arm_len = m.height_m * super::climb::ARM_SPAN_FRAC;
    let leg_len = m.leg_length.max(0.2);

    // Both soles at ONE height, derived from the body rather than from wherever
    // the playing clip happened to leave each foot. Following the clip left one
    // leg tucked at knee height while the other hung straight, which reads as a
    // stumble rather than a brace.
    let sole_y = root_tf.translation.y - m.capsule_half_extent() * 0.92;

    // ── Hands ───────────────────────────────────────────────────────────────
    let mut worst_hand = 0.0_f32;
    for (side, arm_b, fore_b, hand_b) in [
        (-1.0_f32, HumanoidBone::LeftArm, HumanoidBone::LeftForeArm, HumanoidBone::LeftHand),
        (1.0, HumanoidBone::RightArm, HumanoidBone::RightForeArm, HumanoidBone::RightHand),
    ] {
        let (Some(up_e), Some(lo_e), Some(end_e)) =
            (rig.bone(arm_b), rig.bone(fore_b), rig.bone(hand_b))
        else {
            continue;
        };
        let Some(shoulder) = world_of(up_e, root, root_tf, parents, writes) else {
            continue;
        };

        let target = edge
            + tangent * (side * m.shoulder_half_width.max(0.10))
            + Vec3::Y * HAND_GRIP_LIFT;

        // Elbows hang low and flare outboard — the shape of a dead hang. A
        // pole directly below would leave the solve free to pick an inward
        // bend and the forearms would cross.
        let pole = shoulder.translation
            + Vec3::NEG_Y * (arm_len * 0.85)
            + tangent * (side * arm_len * 0.55);

        if let Some(err) = drive_two_bone_chain(
            writes, parents, root, root_tf, up_e, lo_e, end_e, target, pole, w,
        ) {
            worst_hand = worst_hand.max(err);
        }
    }

    // ── Soles ───────────────────────────────────────────────────────────────
    //
    // The wall is vertical, so its face at any height shares the edge's x/z.
    let mut worst_foot = 0.0_f32;
    for (side, up_b, lo_b, foot_b) in [
        (-1.0_f32, HumanoidBone::LeftUpLeg, HumanoidBone::LeftLeg, HumanoidBone::LeftFoot),
        (1.0, HumanoidBone::RightUpLeg, HumanoidBone::RightLeg, HumanoidBone::RightFoot),
    ] {
        let (Some(up_e), Some(lo_e), Some(foot_e)) =
            (rig.bone(up_b), rig.bone(lo_b), rig.bone(foot_b))
        else {
            continue;
        };
        let Some(hip) = world_of(up_e, root, root_tf, parents, writes) else {
            continue;
        };

        let target = Vec3::new(edge.x, sole_y, edge.z)
            + n * SOLE_WALL_CLEARANCE
            + tangent * (side * m.foot_half_separation);

        // Knees drop and tuck slightly toward the wall. A pole pushed hard at
        // the wall would bend the knee straight through it.
        let pole = hip.translation + Vec3::NEG_Y * leg_len - n * (leg_len * 0.25);

        if let Some(err) = drive_two_bone_chain(
            writes, parents, root, root_tf, up_e, lo_e, foot_e, target, pole, w,
        ) {
            worst_foot = worst_foot.max(err);
        }
    }

    ik.hand_error_m = worst_hand;
    ik.climb_foot_error_m = worst_foot;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reach(root: Vec3, target: Vec3, ul: f32, ll: f32, pole: Vec3) -> Vec3 {
        let (u, l) = solve_two_bone(root, target, ul, ll, pole);
        root + u * ul + l * ll
    }

    #[test]
    fn reachable_targets_are_reached() {
        let root = Vec3::new(0.0, 1.0, 0.0);
        let pole = Vec3::new(0.0, 1.0, 1.0);
        for d in [0.2_f32, 0.5, 0.8, 0.95] {
            let target = root + Vec3::new(0.0, -d, 0.0);
            let got = reach(root, target, 0.5, 0.5, pole);
            assert!(
                (got - target).length() < 1e-3,
                "target {target:?} reached {got:?} (dist {d})"
            );
        }
    }

    #[test]
    fn unreachable_targets_extend_straight_without_nan() {
        let root = Vec3::ZERO;
        let target = Vec3::new(0.0, -50.0, 0.0);
        let (u, l) = solve_two_bone(root, target, 0.5, 0.5, Vec3::Z);
        assert!(u.is_finite() && l.is_finite());
        // Fully extended: both segments colinear.
        assert!(u.dot(l) > 0.999, "not straight: {u:?} {l:?}");
    }

    #[test]
    fn no_nan_across_ten_thousand_randomised_targets() {
        // Deterministic LCG — no rand dependency, and reproducible on failure.
        let mut s: u32 = 0x1234_5678;
        let mut next = || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (s >> 8) as f32 / 16_777_216.0 * 2.0 - 1.0
        };

        for i in 0..10_000 {
            let root = Vec3::new(next(), next(), next()) * 2.0;
            let target = Vec3::new(next(), next(), next()) * 3.0;
            let pole = Vec3::new(next(), next(), next()) * 2.0;
            let ul = (next().abs() + 0.05) * 1.5;
            let ll = (next().abs() + 0.05) * 1.5;

            let (u, l) = solve_two_bone(root, target, ul, ll, pole);
            assert!(
                u.is_finite() && l.is_finite(),
                "iteration {i}: NaN from root {root:?} target {target:?} lens {ul}/{ll}"
            );
            assert!(
                (u.length() - 1.0).abs() < 1e-3 && (l.length() - 1.0).abs() < 1e-3,
                "iteration {i}: non-unit dirs {u:?} {l:?}"
            );
        }
    }

    #[test]
    fn degenerate_inputs_do_not_panic_or_nan() {
        for (ul, ll) in [(0.0_f32, 0.5_f32), (0.5, 0.0), (0.0, 0.0)] {
            let (u, l) = solve_two_bone(Vec3::ZERO, Vec3::NEG_Y, ul, ll, Vec3::Z);
            assert!(u.is_finite() && l.is_finite());
        }
        // Target exactly on the root.
        let (u, l) = solve_two_bone(Vec3::ONE, Vec3::ONE, 0.5, 0.5, Vec3::Z);
        assert!(u.is_finite() && l.is_finite());
    }

    /// The bug this module was disabled for. A world-space delta applied to a
    /// bone under a rotated parent must still produce that world rotation.
    #[test]
    fn world_delta_survives_a_rotated_parent() {
        let parent = Quat::from_rotation_y(1.1) * Quat::from_rotation_x(0.4);
        let local = Quat::from_rotation_z(0.7) * Quat::from_rotation_x(-0.2);
        let world = parent * local;
        let delta = Quat::from_axis_angle(Vec3::new(0.3, 0.8, -0.5).normalize(), 0.62);

        let new_local = local_after_world_delta(world, local, delta);
        let new_world = parent * new_local;

        assert!(
            new_world.angle_between(delta * world) < 1e-4,
            "conjugation lost the rotation: got {new_world:?}, want {:?}",
            delta * world
        );
    }

    /// The old `delta * local` form is only right for an unrotated parent —
    /// this pins that it really was wrong, so the fix cannot be reverted as a
    /// no-op simplification.
    #[test]
    fn naive_world_delta_is_wrong_under_a_rotated_parent() {
        let parent = Quat::from_rotation_y(1.1);
        let local = Quat::from_rotation_z(0.7);
        let world = parent * local;
        let delta = Quat::from_rotation_x(0.6);

        let naive_world = parent * (delta * local);
        assert!(
            naive_world.angle_between(delta * world).to_degrees() > 5.0,
            "the naive form happened to agree — this test proves nothing"
        );
    }

    #[test]
    fn identity_parent_makes_both_forms_agree() {
        let local = Quat::from_rotation_z(0.4);
        let delta = Quat::from_rotation_x(0.25);
        let got = local_after_world_delta(local, local, delta);
        assert!(got.angle_between(delta * local) < 1e-5);
    }

    #[test]
    fn a_zero_weight_solve_is_the_identity() {
        let q = scaled_arc(Vec3::Y, Vec3::X, 0.0);
        assert!(q.angle_between(Quat::IDENTITY) < 1e-6);
    }

    #[test]
    fn grip_holds_through_the_pull_then_releases() {
        use super::super::climb::{AvatarClimb, ClimbPhase};

        let mut c = AvatarClimb::default();
        c.phase = ClimbPhase::Hanging;
        assert_eq!(climb_hand_weight(&c), 1.0, "a hang with no grip is a fall");

        c.phase = ClimbPhase::Mantling;
        c.t = 0.0;
        assert_eq!(climb_hand_weight(&c), 1.0, "let go before the pull started");

        c.t = 0.5;
        let mid = climb_hand_weight(&c);
        assert!((0.0..1.0).contains(&mid), "mid-mantle grip {mid} not releasing");

        c.t = 1.0;
        assert_eq!(climb_hand_weight(&c), 0.0, "still welded to the edge on top");

        c.phase = ClimbPhase::None;
        assert_eq!(climb_hand_weight(&c), 0.0);
    }

    /// A hand target on a ledge is normally *within* reach, so the chain must
    /// actually converge — not clamp to full extension like the unreachable
    /// case does.
    #[test]
    fn an_arm_reaches_a_ledge_within_its_span() {
        let shoulder = Vec3::new(0.0, 1.4, 0.0);
        let upper = 0.28;
        let lower = 0.26;
        // Edge up and forward, comfortably inside the 0.54 m span.
        let target = shoulder + Vec3::new(0.0, 0.34, -0.22);
        let pole = shoulder + Vec3::NEG_Y * 0.4 + Vec3::X * 0.3;
        let got = reach(shoulder, target, upper, lower, pole);
        assert!(
            (got - target).length() < 1e-3,
            "arm fell short of the ledge: {got:?} vs {target:?}"
        );
    }

    #[test]
    fn knee_bends_toward_the_pole_not_away() {
        // Root above, target below, pole in +Z → knee must move +Z.
        let root = Vec3::new(0.0, 1.0, 0.0);
        let target = Vec3::new(0.0, 0.2, 0.0);
        let pole = root + Vec3::Z;
        let (u, _) = solve_two_bone(root, target, 0.5, 0.5, pole);
        let knee = root + u * 0.5;
        assert!(knee.z > root.z, "knee bent away from pole: {knee:?}");
    }
}
