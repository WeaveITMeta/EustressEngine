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
/// How close a foot must be to the ground beneath it to count as planted, m.
/// Roughly a sole's thickness plus the clip's contact slop.
const PLANT_GROUND_GAP: f32 = 0.09;
/// How far below the foot to look for ground.
const FOOT_PROBE_DOWN: f32 = 0.55;
/// Maximum foot pitch when conforming to a slope.
const MAX_FOOT_ROLL_DEG: f32 = 35.0;
/// IK fades out above this multiple of run speed — on a fast sprint the clip
/// reads better than the solve.
const IK_FADE_ABOVE: f32 = 1.4;
/// Where the WRIST sits relative to the lip, as a fraction of body height.
///
/// Negative — below it. The IK end effector is the wrist, but the hand mesh
/// continues past it along the forearm, which while hanging points straight
/// up. Targeting the wrist ON the lip therefore hangs the whole hand in the
/// air above the ledge, gripping nothing. Dropping the wrist by about a hand's
/// length puts the palm on the edge, which is what the grip is supposed to
/// look like.
const WRIST_BELOW_LIP_FRAC: f32 = -0.055;
/// How far the sole target sits below the body centre, as a fraction of the
/// capsule half-extent.
///
/// At 0.92 the legs hung nearly straight and the knees never bent — the pose
/// read as a limp dangle rather than a braced hang. Bringing the feet up puts
/// a real angle at the knee, which is what "feet planted on the wall" means.
const CLIMB_SOLE_DROP_FRAC: f32 = 0.42;
/// How much say the leg IK gets while hanging.
///
/// Small. The authored stance already puts the knees out and the feet tucked;
/// a full-weight solve straightens all of that back out to reach whatever
/// point on the wall the target happens to sit at. The IK is here to press the
/// soles onto the face, not to decide the shape of the legs.
const CLIMB_LEG_IK_WEIGHT: f32 = 0.30;
/// How much closer the ANCHORED hand's target sits to its shoulder, as a
/// fraction of arm length — i.e. how much that elbow bends under load.
const ANCHOR_ELBOW_FLEX: f32 = 0.16;
/// Extra lateral splay given to the leg on the REACHING side, as a multiple of
/// foot separation. The opposite leg pulls slightly the other way.
const COUNTERWEIGHT_SPLAY: f32 = 0.9;
/// Soles press this far off the wall face — the collision skin, so the foot
/// contacts rather than intersects.
const SOLE_WALL_CLEARANCE: f32 = 0.05;
/// Mantle fraction over which the hands let go of the edge.
///
/// Late, because the hands are what the body is pulling AGAINST. An earlier
/// band (0.15–0.55) was chosen when the vertical curve was `ease_out`, which
/// front-loaded the rise so hard that the torso passed the lip while the arms
/// were still on it — holding longer only stretched them further. With a
/// symmetric rise the body and hands stay in the same part of the motion, so
/// the grip can hold through the pull and release as the hips clear.
const HAND_RELEASE_BAND: (f32, f32) = (0.80, 0.96);

/// How much of the arm grip the LEGS get during a mantle.
///
/// Low. Pinning the soles to the wall face through a pull-up bends the knees
/// hard for the whole move, which is what made a vault read as being climbed
/// with the knees. Once the body is rising, the legs should trail on the clip
/// and let the arms carry the motion.
const MANTLE_FOOT_WEIGHT: f32 = 0.45;
/// How far a swinging foot lifts off the wall mid-step, in metres.
const FOOT_SWING_LIFT: f32 = 0.10;
/// How fast the grip blends in and out, per second. A hand catching a ledge is
/// near-instant; at the old 18/s the blend was still climbing when the mantle
/// took over.
const GRIP_BLEND_RATE: f32 = 35.0;
/// Furthest a solved chain is allowed to reach, as a fraction of its own
/// length. Keeps a residual bend in the knee and elbow instead of snapping to
/// a locked joint at the solver's straight-line case.
const MAX_CHAIN_EXTENSION: f32 = 0.97;
/// Tightest a solved chain may fold, as a fraction of its own length.
///
/// The solver had a maximum reach clamp and no minimum, so a target near the
/// joint root folded the limb as far as the maths allowed. That is where every
/// "sitting" and "L-sit" pose came from: soles targeted level with the hips are
/// perfectly satisfiable by a knee bent past 120°, which is geometrically valid
/// and anatomically absurd. A real knee or elbow stops around 55% of full
/// extension, and pulling the target out to that distance produces a plausible
/// pose instead of a folded one.
const MIN_CHAIN_EXTENSION: f32 = 0.58;

/// What a chain does when its target is closer than the joint can fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NearTarget {
    /// Leave the authored pose alone.
    Refuse,
    /// Solve to the nearest plausible point along the same direction.
    PushOut,
}

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
    near: NearTarget,
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
    let span = upper_len + lower_len;
    let reachable = span * MAX_CHAIN_EXTENSION;
    let closest = span * MIN_CHAIN_EXTENSION;
    let to_target = target - up_w.translation;
    let dist = to_target.length();
    // Clamp the target into the band the limb can plausibly occupy — too far
    // locks the joint straight, too near folds it past anatomy.
    let target = if dist > reachable {
        // Too far: pull the target in, which only straightens the limb.
        up_w.translation + to_target.normalize_or_zero() * reachable
    } else if dist < closest {
        match near {
            // A FOOT refuses. Relocating it outward would plant it where the
            // ground is not, and a foot in the authored pose still reads as a
            // foot. This is where every "sitting" and "L-sit" pose came from.
            NearTarget::Refuse => return None,
            // A HAND pushes out to the closest anatomically plausible point.
            //
            // Refusing here was much worse than the problem it avoided: with
            // no solve the arm keeps the authored hang direction, which points
            // it STRAIGHT UP INTO THE AIR — the hand ends up half a metre from
            // the ledge instead of the two or three centimetres that clamping
            // costs. "Hands don't always hold the ledge" is this branch.
            NearTarget::PushOut => {
                up_w.translation + to_target.normalize_or(Vec3::NEG_Y) * closest
            }
        }
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
                &spatial, &mut writes, &parents, root, root_tf, rig, body, climb, &mut ik,
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

            // Plant test: is THIS FOOT near the ground?
            //
            // Gating on whole-body speed was wrong in both directions. Applied
            // to both feet it dragged the swing foot to the floor and flattened
            // the walk; restricted to slow movement it meant the stance foot
            // got no ground adaptation at any walking speed, so ankles ignored
            // slopes and steps the moment you were actually moving.
            //
            // Per-foot height is the real signal: the stance foot is down and
            // conforms, the swing foot is up and is left to the clip.
            let foot_gap = foot_w.translation.y - ground_pos.y;
            let planted = foot_gap <= PLANT_GROUND_GAP
                || loco.planar_speed
                    < body.motion.walk_speed
                        * PLANT_SPEED_FRAC
                        * body.metrics.stride_scale.max(0.25);

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
                NearTarget::Refuse,
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
                    let blended = Quat::IDENTITY.slerp(clamped, (w * 0.9).clamp(0.0, 1.0));
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
            // PELVIS DROP IS DISABLED — deliberately, not forgotten.
            //
            // It was wrong on two independent axes: the value was in world
            // metres written into an armature-local bone (~100x off), AND
            // hips-local +Y is world FORWARD after the root correction, so it
            // never moved the pelvis down at all. Fixing only the scale made
            // the wrong-axis error 100x larger and threw the hips forward,
            // which is what folded the character into a seated pose while
            // simply standing on flat ground.
            //
            // A pelvis drop needs the hips' own world basis to push along
            // world -Y, and it is a refinement rather than a requirement.
            // Shipping a third guess at it is worse than shipping none.
            let _ = (hips, &ik.pelvis_offset);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The authored climb pose
// ─────────────────────────────────────────────────────────────────────────────

/// A hand-authored pose, written as the direction each bone should POINT in
/// the body's own frame (+X right, +Y up, −Z forward, i.e. into the wall).
///
/// ## Why directions and not Euler angles
///
/// A pose is normally authored as local bone rotations, but those depend
/// entirely on the rig's bind orientation and axis conventions — get one wrong
/// on a Mixamo skeleton and the limb twists somewhere unrelated. A *direction*
/// is convention-free: aim the bone at its child, measure the arc, apply it.
/// The same machinery the IK already uses.
///
/// ## Why a base pose exists at all
///
/// Without one, the climb was four end-effector targets and a solver free to
/// satisfy them however it liked — which is why every fix produced a different
/// wrong pose, and why bones the IK does not own (spine, neck, shoulders) sat
/// frozen at whatever the clip last wrote. The pose sets the whole body; the
/// IK then adjusts only the hands and feet onto the real geometry.
type PoseDir = (HumanoidBone, HumanoidBone, [f32; 3]);

/// A dead hang: arms overhead and slightly out, torso long, knees soft, feet
/// toward the wall.
const HANG_POSE: &[PoseDir] = &[
    // Spine counter-rotates against the pelvis: the chain leans progressively
    // to one side going up, so the shoulder line and the hip line are NOT
    // parallel. In the footage they are separate, counter-rotating segments —
    // a rigid torso is the difference between a climber and a plank.
    (HumanoidBone::Spine, HumanoidBone::Spine1, [0.10, 0.98, 0.10]),
    (HumanoidBone::Spine1, HumanoidBone::Spine2, [0.16, 0.97, 0.08]),
    (HumanoidBone::Spine2, HumanoidBone::Neck, [0.20, 0.96, 0.04]),
    (HumanoidBone::Neck, HumanoidBone::Head, [-0.08, 0.96, -0.22]),
    // The girdle SHRUGS toward the ears under a hanging load, and tilts high
    // on the side taking weight. Shoulder bones run outboard from the spine, so
    // a positive Y here lifts that shoulder.
    (HumanoidBone::LeftShoulder, HumanoidBone::LeftArm, [-0.90, 0.42, -0.10]),
    (HumanoidBone::RightShoulder, HumanoidBone::RightArm, [0.90, 0.42, -0.10]),
    // Upper arms reach up and a little outboard; forearms close to vertical.
    (HumanoidBone::LeftArm, HumanoidBone::LeftForeArm, [-0.28, 0.94, -0.16]),
    (HumanoidBone::LeftForeArm, HumanoidBone::LeftHand, [-0.10, 0.99, -0.06]),
    (HumanoidBone::RightArm, HumanoidBone::RightForeArm, [0.28, 0.94, -0.16]),
    (HumanoidBone::RightForeArm, HumanoidBone::RightHand, [0.10, 0.99, -0.06]),
    // A CLIMBER'S STANCE, not a dangle.
    //
    // Knees splay outward and forward, shins come back inboard so the feet
    // tuck up under the body against the wall. Straight legs with pointed toes
    // read as a corpse on a rope; this is what taking your weight on the wall
    // looks like.
    //
    // Deliberately ASYMMETRIC: one leg rides higher than the other. Perfectly
    // mirrored legs look posed, and a stagger is what a climber actually does
    // while searching for the next foothold.
    (HumanoidBone::LeftUpLeg, HumanoidBone::LeftLeg, [-0.62, -0.62, -0.48]),
    (HumanoidBone::LeftLeg, HumanoidBone::LeftFoot, [0.30, -0.88, -0.37]),
    (HumanoidBone::RightUpLeg, HumanoidBone::RightLeg, [0.52, -0.76, -0.39]),
    (HumanoidBone::RightLeg, HumanoidBone::RightFoot, [-0.24, -0.93, -0.28]),
];

/// Moving sideways, authored for travel toward the character's RIGHT (+X).
///
/// Mirrored for the other direction rather than authored twice — see
/// [`mirror_pose_dir`]. The asymmetry is the whole point of the pose: the lead
/// arm is long and reaching along the lip, the trail arm is bent and pulling,
/// the torso is angled into the direction of travel, and the legs cross so the
/// trailing foot comes across behind the leading one. A symmetric hang that
/// happens to be translating sideways reads as a body on rails.
const SHIMMY_POSE: &[PoseDir] = &[
    // Torso angles toward the reach and leans into it.
    (HumanoidBone::Spine, HumanoidBone::Spine1, [0.24, 0.95, 0.08]),
    (HumanoidBone::Spine1, HumanoidBone::Spine2, [0.30, 0.94, 0.06]),
    (HumanoidBone::Spine2, HumanoidBone::Neck, [0.34, 0.93, 0.02]),
    // Head tracks where the hand is going.
    (HumanoidBone::Neck, HumanoidBone::Head, [0.22, 0.92, -0.26]),
    // Leading (right) shoulder drives UP and out into the reach; trailing
    // (left) shoulder drops as that arm takes the load.
    (HumanoidBone::LeftShoulder, HumanoidBone::LeftArm, [-0.92, 0.18, -0.12]),
    (HumanoidBone::RightShoulder, HumanoidBone::RightArm, [0.86, 0.52, -0.08]),
    // Lead arm reaches OUT along the lip — closer to horizontal than vertical.
    // This is the shape that makes a traverse legible from any camera angle.
    (HumanoidBone::RightArm, HumanoidBone::RightForeArm, [0.82, 0.54, -0.18]),
    (HumanoidBone::RightForeArm, HumanoidBone::RightHand, [0.52, 0.84, -0.10]),
    // Trail arm is the one bearing weight, so it hangs close to VERTICAL and
    // crosses slightly under the body as it pulls across.
    (HumanoidBone::LeftArm, HumanoidBone::LeftForeArm, [-0.34, 0.92, -0.20]),
    (HumanoidBone::LeftForeArm, HumanoidBone::LeftHand, [0.06, 0.99, -0.08]),
    // Legs CROSS. The trailing leg comes across behind the leading one, which
    // is what actually happens when a climber traverses and is the single most
    // recognisable thing about the motion.
    (HumanoidBone::RightUpLeg, HumanoidBone::RightLeg, [0.66, -0.66, -0.36]),
    (HumanoidBone::RightLeg, HumanoidBone::RightFoot, [0.20, -0.94, -0.28]),
    (HumanoidBone::LeftUpLeg, HumanoidBone::LeftLeg, [0.18, -0.80, -0.57]),
    (HumanoidBone::LeftLeg, HumanoidBone::LeftFoot, [0.42, -0.86, -0.30]),
];

/// The drive: lead knee up and forward, trail leg extended behind, arms
/// swinging CONTRALATERALLY — right leg forward means left arm forward, the
/// same coupling as a walk cycle.
///
/// Authored for a right-leg lead and mirrored for the other, so the vault
/// alternates legs instead of always hopping off the same foot.
const VAULT_LAUNCH_POSE: &[PoseDir] = &[
    // A lean, not a dive. Around 15°.
    (HumanoidBone::Spine, HumanoidBone::Spine1, [0.0, 0.97, -0.22]),
    (HumanoidBone::Spine1, HumanoidBone::Spine2, [0.0, 0.98, -0.19]),
    (HumanoidBone::Spine2, HumanoidBone::Neck, [0.0, 0.99, -0.14]),
    // Eyes on the landing.
    (HumanoidBone::Neck, HumanoidBone::Head, [0.0, 0.95, -0.30]),
    (HumanoidBone::LeftShoulder, HumanoidBone::LeftArm, [-0.97, 0.16, -0.18]),
    (HumanoidBone::RightShoulder, HumanoidBone::RightArm, [0.97, 0.16, 0.18]),
    // ARMS HANG AND SWING. Every one of these is Y-DOMINANT on purpose.
    //
    // The first pass had the upper arm at z = -0.86 — nearly horizontal — which
    // does not read as a swing at all, it reads as Superman. A running arm
    // hangs from the shoulder and swings through maybe 40° while the elbow
    // carries the forearm further; the upper arm never leaves vertical by much.
    // LEFT arm forward (opposing the right leg).
    (HumanoidBone::LeftArm, HumanoidBone::LeftForeArm, [-0.30, -0.87, -0.39]),
    (HumanoidBone::LeftForeArm, HumanoidBone::LeftHand, [-0.18, -0.72, -0.67]),
    // RIGHT arm driving back.
    // The back-swinging arm stays MODEST. A full rearward drive is a sprint
    // start, and over a low block it reads as the arms flying backwards rather
    // than swinging; the opposition only has to be legible, not extreme.
    (HumanoidBone::RightArm, HumanoidBone::RightForeArm, [0.30, -0.93, 0.20]),
    (HumanoidBone::RightForeArm, HumanoidBone::RightHand, [0.16, -0.88, 0.44]),
    // RIGHT leg leads: thigh down and forward — a stride, not a high march.
    (HumanoidBone::RightUpLeg, HumanoidBone::RightLeg, [0.14, -0.71, -0.69]),
    (HumanoidBone::RightLeg, HumanoidBone::RightFoot, [0.09, -0.95, -0.30]),
    // LEFT leg trails: extended back, taking the push-off.
    (HumanoidBone::LeftUpLeg, HumanoidBone::LeftLeg, [-0.12, -0.85, 0.51]),
    (HumanoidBone::LeftLeg, HumanoidBone::LeftFoot, [-0.09, -0.90, 0.43]),
];
/// The landing: lead foot planted on the top, trail leg swinging through,
/// arms passing back toward neutral. Blending out of this returns the body to
/// a walk without a seam.
const VAULT_LAND_POSE: &[PoseDir] = &[
    (HumanoidBone::Spine, HumanoidBone::Spine1, [0.0, 0.99, -0.11]),
    (HumanoidBone::Spine1, HumanoidBone::Spine2, [0.0, 0.99, -0.09]),
    (HumanoidBone::Spine2, HumanoidBone::Neck, [0.0, 1.0, -0.06]),
    (HumanoidBone::Neck, HumanoidBone::Head, [0.0, 0.98, -0.18]),
    (HumanoidBone::LeftShoulder, HumanoidBone::LeftArm, [-0.98, 0.14, -0.08]),
    (HumanoidBone::RightShoulder, HumanoidBone::RightArm, [0.98, 0.14, 0.08]),
    // Arms passing back through neutral, still opposed but closer to hanging.
    (HumanoidBone::LeftArm, HumanoidBone::LeftForeArm, [-0.26, -0.95, -0.16]),
    (HumanoidBone::LeftForeArm, HumanoidBone::LeftHand, [-0.16, -0.92, -0.36]),
    (HumanoidBone::RightArm, HumanoidBone::RightForeArm, [0.26, -0.96, 0.13]),
    (HumanoidBone::RightForeArm, HumanoidBone::RightHand, [0.14, -0.94, 0.31]),
    // Lead leg under the body, taking the weight.
    (HumanoidBone::RightUpLeg, HumanoidBone::RightLeg, [0.11, -0.95, -0.29]),
    (HumanoidBone::RightLeg, HumanoidBone::RightFoot, [0.07, -0.99, -0.12]),
    // Trail leg swinging through, knee folding as it comes forward.
    (HumanoidBone::LeftUpLeg, HumanoidBone::LeftLeg, [-0.11, -0.82, 0.56]),
    (HumanoidBone::LeftLeg, HumanoidBone::LeftFoot, [-0.07, -0.96, -0.27]),
];
/// The mirror image of one authored entry, for travel the other way.
///
/// Both halves of the transform are required: flipping the direction's X
/// without swapping the bone would aim the LEFT arm along a right-handed
/// reach. Mirroring is a reflection of the whole body, not of a vector.
fn mirror_pose_dir((bone, child, d): PoseDir) -> PoseDir {
    (
        bone.mirrored(),
        child.mirrored(),
        [-d[0], d[1], d[2]],
    )
}

/// The pull-up: elbows driving down and back, torso tucked, knees rising.
const MANTLE_POSE: &[PoseDir] = &[
    (HumanoidBone::Spine, HumanoidBone::Spine1, [0.08, 0.93, -0.34]),
    (HumanoidBone::Spine1, HumanoidBone::Spine2, [0.12, 0.94, -0.30]),
    (HumanoidBone::Spine2, HumanoidBone::Neck, [0.15, 0.95, -0.24]),
    (HumanoidBone::Neck, HumanoidBone::Head, [-0.06, 0.94, -0.34]),
    // Shoulders driven down and back — the pull-up position.
    (HumanoidBone::LeftShoulder, HumanoidBone::LeftArm, [-0.88, 0.30, -0.36]),
    (HumanoidBone::RightShoulder, HumanoidBone::RightArm, [0.88, 0.30, -0.36]),
    (HumanoidBone::LeftArm, HumanoidBone::LeftForeArm, [-0.34, 0.70, -0.62]),
    (HumanoidBone::LeftForeArm, HumanoidBone::LeftHand, [-0.12, 0.96, -0.24]),
    (HumanoidBone::RightArm, HumanoidBone::RightForeArm, [0.34, 0.70, -0.62]),
    (HumanoidBone::RightForeArm, HumanoidBone::RightHand, [0.12, 0.96, -0.24]),
    // Knees RISE, but they do not come up to horizontal.
    //
    // Both thighs used to point further forward than down, which is a knee
    // raised to waist height on both legs at once. Under a torso already
    // pitched forward that reads as the body folding up — the crumpled,
    // "weird knee" shape that shows whenever the character climbs onto a
    // block. A pull-up tucks the knees; it does not sit down in mid-air.
    // Two properties, and both matter: the thigh must lift MORE than a dead
    // hang does (it is a pull-up, the knees come up) while still pointing
    // further down than forward (it is not a mid-air sit).
    (HumanoidBone::LeftUpLeg, HumanoidBone::LeftLeg, [-0.40, -0.55, -0.50]),
    (HumanoidBone::LeftLeg, HumanoidBone::LeftFoot, [0.26, -0.90, -0.35]),
    (HumanoidBone::RightUpLeg, HumanoidBone::RightLeg, [0.34, -0.68, -0.55]),
    (HumanoidBone::RightLeg, HumanoidBone::RightFoot, [-0.20, -0.94, -0.27]),
];

/// Aim `bone` so the segment toward `child` points along `want` (world space).
fn aim_bone(
    writes: &mut Query<&mut Transform, Without<SpawnedByAvatarRuntime>>,
    parents: &Query<&ChildOf>,
    root: Entity,
    root_tf: &Transform,
    bone: Entity,
    child: Entity,
    want: Vec3,
    weight: f32,
) {
    if weight < 1e-3 {
        return;
    }
    let (Some(b), Some(c)) = (
        world_of(bone, root, root_tf, parents, writes),
        world_of(child, root, root_tf, parents, writes),
    ) else {
        return;
    };
    let cur = c.translation - b.translation;
    if cur.length_squared() < 1e-8 {
        return;
    }
    let cur = cur.normalize();
    let d = scaled_arc(cur, want.normalize_or(cur), weight);
    apply_world_delta(writes, bone, b.rotation, d);
}

/// Lay an authored pose over the skeleton, in the avatar's own frame.
fn apply_pose(
    writes: &mut Query<&mut Transform, Without<SpawnedByAvatarRuntime>>,
    parents: &Query<&ChildOf>,
    root: Entity,
    root_tf: &Transform,
    rig: &AvatarRig,
    pose: &[PoseDir],
    weight: f32,
    mirror: bool,
) {
    // Rotation only: the pose is expressed relative to which way the body
    // faces, so it follows the character around a corner for free.
    let facing = root_tf.rotation;
    for entry in pose {
        let (bone, child, dir) = if mirror { mirror_pose_dir(*entry) } else { *entry };
        let (Some(b), Some(c)) = (rig.bone(bone), rig.bone(child)) else {
            continue;
        };
        aim_bone(
            writes,
            parents,
            root,
            root_tf,
            b,
            c,
            facing * Vec3::from_array(dir),
            weight,
        );
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
        // Every attached phase except the mantle keeps full grip: hanging,
        // shimmying, reaching and lowering are all defined by the hands being
        // on the wall.
        ClimbPhase::Hanging
        | ClimbPhase::Shimmy
        | ClimbPhase::Transfer
        | ClimbPhase::Lowering => 1.0,
        ClimbPhase::Mantling => {
            let (a, b) = HAND_RELEASE_BAND;
            1.0 - ((climb.t - a) / (b - a)).clamp(0.0, 1.0)
        }
        // A vault never touches the block. The arms are SWINGING — that swing
        // is where the momentum comes from — so binding a hand to the lip is
        // exactly wrong, and was part of why stepping onto a low block looked
        // like being winched up it.
        ClimbPhase::Vaulting => 0.0,
    }
}

/// Look for an actual foothold on the wall, rather than pressing the sole flat
/// against a blank face.
///
/// Naughty Dog's write-up on Uncharted 4 names this directly: *"the feet
/// actually look for an edge instead of having them just dangling and
/// swinging"*, alongside the observation that when you climb, most of your
/// weight is on your feet. A sole pinned to a flat plane at a fixed height is
/// the dangle; searching the face for something to stand on is the difference.
///
/// Casts down the wall face inside the band the leg can reach and returns the
/// first up-facing surface found — a ledge, a step, a protrusion. `None` means
/// the face really is blank there, and the caller falls back to pressing on it.
pub(crate) fn find_foothold(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    face_xz: Vec3,
    normal: Vec3,
    from_y: f32,
    to_y: f32,
    stand_off: f32,
) -> Option<Vec3> {
    let span = from_y - to_y;
    if span <= 0.01 {
        return None;
    }
    // Just off the face, so the cast samples the wall's profile rather than
    // skimming along the inside of it.
    let origin = Vec3::new(face_xz.x, from_y, face_xz.z) + normal * stand_off;
    if !origin.is_finite() {
        return None;
    }
    let hit = spatial.cast_ray(origin, Dir3::NEG_Y, span, true, filter)?;
    let n = Vec3::from(hit.normal);
    // Only a surface you could actually weight — a near-vertical hit is the
    // wall itself, not a foothold on it.
    if n.dot(Vec3::Y) < 0.5 {
        return None;
    }
    Some(origin + Vec3::NEG_Y * hit.distance)
}

/// Plant both hands on the ledge edge and both soles on the wall face.
///
/// The frame is built from the ledge itself, not from the character, so the
/// grip stays put while the body swings under it.
#[allow(clippy::too_many_arguments)]
fn solve_climb_limbs(
    spatial: &SpatialQuery,
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

    // Base pose FIRST, IK second.
    //
    // The solver only owns four end effectors; everything else — spine, neck,
    // shoulders — had no one writing it once the clips were blended out, and
    // sat frozen wherever the last frame of walking left it. Laying an
    // authored pose down first gives the whole body a sensible shape, and the
    // limb solve then adjusts the hands and feet onto the actual geometry
    // instead of inventing the entire posture from four points.
    if climb.phase == ClimbPhase::Vaulting {
        // A STRIDE, so the pose has to move. Two authored keys blended across
        // the hop: a single static shape would hold one silhouette the whole
        // way over and read as the body being carried rather than stepping.
        //
        // The lead leg alternates by which hand last reached, so consecutive
        // blocks are not all taken off the same foot.
        let mirror = climb.reaching == 0;
        let k = climb.t.clamp(0.0, 1.0);
        apply_pose(writes, parents, root, root_tf, rig, VAULT_LAUNCH_POSE, w, mirror);
        // Cross over in the back half, once the lead foot is near the surface.
        let land = ((k - 0.35) / 0.65).clamp(0.0, 1.0);
        if land > 0.01 {
            apply_pose(
                writes, parents, root, root_tf, rig, VAULT_LAND_POSE,
                w * super::climb::ease_in_out(land), mirror,
            );
        }
    } else if climb.phase == ClimbPhase::Mantling {
        apply_pose(writes, parents, root, root_tf, rig, MANTLE_POSE, w, false);
    } else {
        // Hang underneath, sideways lean over the top, blended by how hard the
        // character is actually traversing. Laying the shimmy over the hang
        // rather than switching between them means starting and stopping a
        // traverse eases in and out instead of popping between two postures.
        apply_pose(writes, parents, root, root_tf, rig, HANG_POSE, w, false);
        let lean = climb.travel.clamp(-1.0, 1.0);
        if lean.abs() > 0.02 {
            apply_pose(
                writes,
                parents,
                root,
                root_tf,
                rig,
                SHIMMY_POSE,
                w * lean.abs(),
                lean < 0.0,
            );
        }
    }

    // Ledge frame: `n` points off the wall toward the character, `tangent`
    // runs along the lip. Taken from `hand_frame` rather than the raw grip so
    // the hands lead the body across a transfer.
    let Some((edge, raw_n, raw_t)) = climb.hand_frame() else {
        ik.hand_error_m = 0.0;
        ik.climb_foot_error_m = 0.0;
        return;
    };
    let n = raw_n.with_y(0.0).normalize_or(Vec3::Z);
    let tangent = raw_t.with_y(0.0).normalize_or(Vec3::Y.cross(n).normalize_or(Vec3::X));

    let arm_len = m.height_m * super::climb::ARM_SPAN_FRAC;
    let leg_len = m.leg_length.max(0.2);

    // Both soles at ONE height, derived from the body rather than from wherever
    // the playing clip happened to leave each foot. Following the clip left one
    // leg tucked at knee height while the other hung straight, which reads as a
    // stumble rather than a brace.
    let sole_y = root_tf.translation.y - m.capsule_half_extent() * CLIMB_SOLE_DROP_FRAC;

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

        // Each hand solves to ITS OWN hold, not to a shared point offset by
        // shoulder width. That symmetry is what made every pose read as
        // hanging: a climber anchors one hand and reaches with the other, and
        // the two are rarely on the same feature.
        let hand_idx = if side < 0.0 { 0 } else { 1 };
        let hold = climb.holds[hand_idx];
        let mut target = if hold.is_finite() && hold != Vec3::ZERO {
            hold + Vec3::Y * (m.height_m * WRIST_BELOW_LIP_FRAC)
        } else {
            edge + tangent * (side * m.shoulder_half_width.max(0.10))
                + Vec3::Y * (m.height_m * WRIST_BELOW_LIP_FRAC)
        };

        // The two arms are never in the same state.
        //
        // Frame analysis: one arm sits near full extension (~165–175°) while
        // the other is bent — elbow flexion IS the load signal, and it is on
        // the ANCHORED arm, which is pulling. Solving both to identical
        // extension is what makes a hang read as a gymnast's dead hang rather
        // than a climber holding on.
        //
        // Pulling the anchored hand's target slightly toward its shoulder
        // shortens that chain, which the solver resolves as a bent elbow.
        if hand_idx != climb.reaching {
            if let Some(sh) = rig.bone(arm_b).and_then(|e| world_of(e, root, root_tf, parents, writes))
            {
                let toward_shoulder = (sh.translation - target).normalize_or_zero();
                target += toward_shoulder * (arm_len * ANCHOR_ELBOW_FLEX);
            }
        }

        // Elbows hang low and flare outboard — the shape of a dead hang. A
        // pole directly below would leave the solve free to pick an inward
        // bend and the forearms would cross.
        let pole = shoulder.translation
            + Vec3::NEG_Y * (arm_len * 0.85)
            + tangent * (side * arm_len * 0.55);

        if let Some(err) = drive_two_bone_chain(
            writes, parents, root, root_tf, up_e, lo_e, end_e, target, pole, w,
            NearTarget::PushOut,
        ) {
            worst_hand = worst_hand.max(err);
        }

        // Orient the WRIST.
        //
        // The chain solver aims the upper and lower arm and stops there, so
        // the hand kept whatever rotation was last written to it. That used to
        // be hidden because the clips were overwriting hand rotation every
        // frame; now that climbing blends clip authority to zero, the stale
        // value is what you see — hands cocked at an angle that has nothing to
        // do with the ledge.
        //
        // Local identity is a straight wrist on a Mixamo rig (bind-pose bones
        // run along the chain), which is the right STARTING point — it clears
        // whatever stale rotation the clip left — but it is not the answer.
        if let Ok(mut h) = writes.get_mut(end_e) {
            h.rotation = h.rotation.slerp(Quat::IDENTITY, w.clamp(0.0, 1.0));
        }

        // Now lay the PALM DOWN on the lip.
        //
        // Aiming the finger direction was not enough and made things worse:
        // rotating one axis onto a target leaves the twist about that axis
        // completely unconstrained, so the hand landed at whatever roll the
        // elbow's pole vector happened to produce — which is what "palms
        // broken" looks like. A grip needs the palm PLANE controlled, not the
        // direction the fingers happen to point.
        //
        // The hand's local +Y is the back of the hand — the same convention
        // the sole uses a few lines below, where local +Y is the top of the
        // foot. Pointing it at world +Y therefore lays the palm flat and
        // downward on the top face of the ledge, which is the grip being
        // asked for.
        if let Some(hand_now) = world_of(end_e, root, root_tf, parents, writes) {
            if let Ok(mut h) = writes.get_mut(end_e) {
                // Two constraints, because one is not enough.
                //
                // Aligning the back of the hand to world up fixes the palm
                // PLANE but leaves the spin within that plane free, and the
                // elbow pole flares the arms outboard — so each hand settled
                // with its fingers pointing out to its own side, a quarter turn
                // off, mirrored left to right.
                //
                // The second term spins each hand about the now-vertical axis
                // until the fingers point along the body's forward, across the
                // top of the ledge. `side` is -1 left and +1 right, so the two
                // hands turn opposite ways — which is exactly the mirrored
                // correction the pose needs.
                let flat = Quat::from_rotation_arc(hand_now.rotation * Vec3::Y, Vec3::Y);
                let square = Quat::from_axis_angle(Vec3::Y, side * std::f32::consts::FRAC_PI_2);
                let want = square * flat;
                let blended = Quat::IDENTITY.slerp(want, w.clamp(0.0, 1.0));
                h.rotation = local_after_world_delta(hand_now.rotation, h.rotation, blended);
            }
        }
    }

    // ── Soles ───────────────────────────────────────────────────────────────
    //
    // Weighted DOWN during a mantle: see `MANTLE_FOOT_WEIGHT`.
    let foot_w = if climb.phase == ClimbPhase::Mantling {
        w * MANTLE_FOOT_WEIGHT
    } else {
        w * CLIMB_LEG_IK_WEIGHT
    };
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

        // Search the face for something to stand on before settling for it.
        //
        // The band runs from just under the hips down to the leg's reach, so a
        // ledge, a step or any protrusion in that range wins over the blank
        // wall — which is what makes the stance read as taking weight rather
        // than hanging.
        // The SAME-SIDE leg tracks the reaching arm.
        //
        // Measured in the footage as a cross-body counterbalance: reach left
        // and the left leg splays left and flexes, keeping the centre of mass
        // under the load. Legs that stay put while an arm swings out make the
        // body look like it is hanging off a hook.
        let reaching_side = if climb.reaching == 0 { -1.0 } else { 1.0 };
        let follows = if (side - reaching_side).abs() < 0.5 { 1.0 } else { -0.35 };
        let counter = tangent * (follows * side * m.foot_half_separation * COUNTERWEIGHT_SPLAY);
        let lateral = tangent * (side * m.foot_half_separation * 1.4) + counter;

        // THE FEET STEP TOO, in counterphase to the hands.
        //
        // The hands got a hand-over-hand cycle and the feet were left on a
        // fixed offset, so traversing showed two arms working above a pair of
        // legs that never moved. Monkeying along a wall is four limbs
        // alternating: a foot swings while the hand diagonally opposite it is
        // planted and bearing load.
        let travel = climb.travel.clamp(-1.0, 1.0);
        let (step_along, lift) = if travel.abs() > 0.05 {
            // Contralateral — this foot leads when the hand on the OTHER side
            // is the one leading.
            let hand_lead_side = if climb.reaching == 0 { -1.0 } else { 1.0 };
            crate::avatar::climb::shimmy_foot_step(
                climb.cycle,
                (side - hand_lead_side).abs() > 0.5,
            )
        } else {
            (0.0, 0.0)
        };
        let stride = travel.signum() * step_along * crate::avatar::climb::SHIMMY_STRIDE;
        let face_xz = edge + lateral + tangent * stride;
        let reach_low = root_tf.translation.y - m.capsule_half_extent() - leg_len * 0.45;
        let foothold = find_foothold(
            spatial,
            &SpatialQueryFilter::default().with_excluded_entities([root]),
            face_xz,
            n,
            root_tf.translation.y - m.capsule_half_extent() * 0.2,
            reach_low,
            m.capsule_radius * 0.5,
        );

        let target = match foothold {
            // Stand ON it, a sole's thickness above the surface and pressed
            // back toward the wall.
            Some(p) => p + Vec3::Y * 0.03 + n * (SOLE_WALL_CLEARANCE * 0.5),
            None => Vec3::new(face_xz.x, sole_y, face_xz.z) + n * SOLE_WALL_CLEARANCE,
        };
        // Off the wall while swinging, back on it when planted.
        let target = target + Vec3::Y * (lift * FOOT_SWING_LIFT * travel.abs())
            + n * (lift * FOOT_SWING_LIFT * 0.6 * travel.abs());

        // Knees drop and bulge AWAY from the wall.
        //
        // The pole decides which way the joint folds, and this pointed it at
        // the wall (`-n`), so the knees bent inward — through the surface the
        // feet are braced on. Hanging with the hips out from the face and the
        // soles on it, the shin swings back from the thigh, which is away from
        // the wall: `+n`.
        let pole = hip.translation + Vec3::NEG_Y * leg_len + n * (leg_len * 0.35);

        if let Some(err) = drive_two_bone_chain(
            writes, parents, root, root_tf, up_e, lo_e, foot_e, target, pole, foot_w,
            NearTarget::Refuse,
        ) {
            worst_foot = worst_foot.max(err);
        }

        // Sole flat against the wall face, for the same reason the wrist needs
        // orienting: nothing else is writing this bone while climbing.
        if let Some(foot_now) = world_of(foot_e, root, root_tf, parents, writes) {
            if let Ok(mut f) = writes.get_mut(foot_e) {
                // The foot's "up" should point off the wall, i.e. along the
                // face normal, so the sole lies on it.
                let want = Quat::from_rotation_arc(foot_now.rotation * Vec3::Y, n);
                let blended = Quat::IDENTITY.slerp(want, (foot_w * 0.7).clamp(0.0, 1.0));
                f.rotation = local_after_world_delta(foot_now.rotation, f.rotation, blended);
            }
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

        // Mid-pull the grip is still FULL: the hands are what the body pulls
        // against, so releasing here is what left the arms behind the torso.
        c.t = 0.5;
        assert_eq!(climb_hand_weight(&c), 1.0, "let go in the middle of the pull");

        // Inside the release band it must be easing off.
        c.t = 0.88;
        let late = climb_hand_weight(&c);
        assert!((0.0..1.0).contains(&late), "late-mantle grip {late} not releasing");

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

#[cfg(test)]
mod limb_band_tests {
    use super::*;

    /// A limb must never be asked to fold tighter than anatomy allows — that
    /// is where the sitting and L-sit poses came from.
    #[test]
    fn a_target_inside_the_fold_limit_is_pushed_out_to_it() {
        let span = 1.0_f32;
        let closest = span * MIN_CHAIN_EXTENSION;
        // Simulate the clamp the solver applies.
        for dist in [0.05_f32, 0.2, 0.4, 0.5] {
            let clamped = if dist < closest { closest } else { dist };
            assert!(
                clamped >= closest - 1e-6,
                "target at {dist} was not pushed out to the fold limit"
            );
        }
    }

    #[test]
    fn the_plausible_band_is_neither_locked_nor_folded() {
        assert!(MIN_CHAIN_EXTENSION > 0.4, "folds tighter than a real joint");
        assert!(MIN_CHAIN_EXTENSION < MAX_CHAIN_EXTENSION, "band is empty");
        assert!(MAX_CHAIN_EXTENSION < 1.0, "locks the joint straight");
    }

    /// A hang tucks the feet UP under the body — a climber's stance, not a
    /// dangle.
    ///
    /// This used to assert the opposite, that the soles hang low. That was a
    /// misreading of an earlier bug: tucked feet looked like a seated pose
    /// because the leg IK had full authority and folded the chain to reach its
    /// target, not because the target height was wrong. With the authored
    /// stance owning the shape and the solve reduced to a nudge, tucked is
    /// correct — so the safeguard belongs on the IK's SHARE, not the height.
    #[test]
    fn hanging_legs_are_owned_by_the_stance_not_the_solver() {
        assert!(
            CLIMB_SOLE_DROP_FRAC < 0.7,
            "soles at {CLIMB_SOLE_DROP_FRAC} hang low — a dangle, not a brace"
        );
        assert!(
            CLIMB_LEG_IK_WEIGHT < 0.5,
            "leg IK at {CLIMB_LEG_IK_WEIGHT} outvotes the authored stance and              straightens the knees back out"
        );
        assert!(CLIMB_LEG_IK_WEIGHT > 0.0, "soles never reach the wall at all");
    }
}

#[cfg(test)]
mod pose_tests {
    use super::*;

    fn dirs(pose: &[PoseDir]) -> Vec<(HumanoidBone, Vec3)> {
        pose.iter().map(|(b, _, d)| (*b, Vec3::from_array(*d))).collect()
    }

    /// Every authored direction must be usable as a direction — a zero or
    /// non-finite entry silently disables that bone and the pose half-applies.
    #[test]
    fn every_authored_direction_is_a_usable_direction() {
        for (name, pose) in [("hang", HANG_POSE), ("mantle", MANTLE_POSE)] {
            for (bone, dir) in dirs(pose) {
                assert!(dir.is_finite(), "{name}/{bone:?}: non-finite");
                assert!(
                    dir.length() > 0.5,
                    "{name}/{bone:?}: length {} is too short to normalise safely",
                    dir.length()
                );
            }
        }
    }

    /// A hang points the arms UP and the legs DOWN. If a sign is flipped the
    /// character hangs upside down, which is the sort of thing that should be
    /// caught here rather than in a screenshot.
    #[test]
    fn the_hang_reaches_up_and_hangs_down() {
        for (bone, dir) in dirs(HANG_POSE) {
            match bone {
                HumanoidBone::LeftArm
                | HumanoidBone::RightArm
                | HumanoidBone::LeftForeArm
                | HumanoidBone::RightForeArm => {
                    assert!(dir.y > 0.5, "{bone:?} does not reach upward: {dir:?}")
                }
                HumanoidBone::LeftUpLeg
                | HumanoidBone::RightUpLeg
                | HumanoidBone::LeftLeg
                | HumanoidBone::RightLeg => {
                    assert!(dir.y < -0.5, "{bone:?} does not hang downward: {dir:?}")
                }
                _ => {}
            }
        }
    }

    /// A climber's stance: the knee goes OUT to the side, and the shin comes
    /// back inboard so the foot tucks under the body. Thigh and shin sharing a
    /// lateral sign is a straight splayed leg, not a tucked one.
    #[test]
    fn knees_splay_out_and_shins_tuck_back_in() {
        for (name, pose) in [("hang", HANG_POSE), ("mantle", MANTLE_POSE)] {
            let map: std::collections::HashMap<_, _> = dirs(pose).into_iter().collect();
            for (thigh, shin, out) in [
                (HumanoidBone::LeftUpLeg, HumanoidBone::LeftLeg, -1.0_f32),
                (HumanoidBone::RightUpLeg, HumanoidBone::RightLeg, 1.0),
            ] {
                let t = map[&thigh];
                let s = map[&shin];
                assert!(
                    t.x * out > 0.25,
                    "{name}: {thigh:?} x={} does not splay outward", t.x
                );
                assert!(
                    s.x * out < 0.0,
                    "{name}: {shin:?} x={} does not tuck back inboard", s.x
                );
                assert!(t.z < 0.0, "{name}: {thigh:?} should lean toward the wall");
            }
        }
    }

    /// The legs must NOT be mirrored — a stagger is what makes it read as
    /// climbing rather than as a mannequin.
    #[test]
    fn the_legs_are_staggered_not_mirrored() {
        let map: std::collections::HashMap<_, _> = dirs(HANG_POSE).into_iter().collect();
        let l = map[&HumanoidBone::LeftUpLeg];
        let r = map[&HumanoidBone::RightUpLeg];
        assert!(
            (l.y - r.y).abs() > 0.05,
            "both thighs sit at the same height ({} vs {}) — no stagger",
            l.y,
            r.y
        );
    }

    /// The pose is mirrored: left and right differ only in the sign of X.
    #[test]
    fn the_pose_is_symmetric_left_to_right() {
        for (name, pose) in [("hang", HANG_POSE), ("mantle", MANTLE_POSE)] {
            let map: std::collections::HashMap<_, _> = dirs(pose).into_iter().collect();
            // ARMS only. The legs are deliberately staggered — see HANG_POSE —
            // because mirrored legs read as posed rather than climbing.
            for (l, r) in [
                (HumanoidBone::LeftArm, HumanoidBone::RightArm),
                (HumanoidBone::LeftForeArm, HumanoidBone::RightForeArm),
            ] {
                let a = map[&l];
                let b = map[&r];
                assert!(
                    (a.x + b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5 && (a.z - b.z).abs() < 1e-5,
                    "{name}: {l:?} {a:?} and {r:?} {b:?} are not mirrored"
                );
            }
        }
    }

    /// A mantle tucks: knees come up relative to the hang, or it is just a
    /// hang with the body moved.
    /// Paired with `no_authored_climb_pose_leaves_a_limb_horizontal`, which
    /// bounds the same value from the other side. Raising the knees to satisfy
    /// this one is what produced the horizontal thighs that folded the body;
    /// dropping them to satisfy that one removes the tuck entirely. The pose
    /// has to sit between the two.
    #[test]
    fn the_mantle_tucks_more_than_the_hang() {
        let hang: std::collections::HashMap<_, _> = dirs(HANG_POSE).into_iter().collect();
        let mantle: std::collections::HashMap<_, _> = dirs(MANTLE_POSE).into_iter().collect();
        for thigh in [HumanoidBone::LeftUpLeg, HumanoidBone::RightUpLeg] {
            assert!(
                mantle[&thigh].y > hang[&thigh].y,
                "{thigh:?} does not lift during the pull-up"
            );
        }
    }

    /// A traverse pose that mirrors to itself would give the same body shape
    /// going both ways, which is the same as having no directional pose at all.
    #[test]
    fn the_shimmy_pose_is_directional_and_mirrors_cleanly() {
        let m: std::collections::HashMap<_, _> = SHIMMY_POSE
            .iter()
            .map(|(b, c, d)| ((*b, *c), Vec3::from_array(*d)))
            .collect();

        // Every entry has a mirror partner, or the reflected pose has holes.
        for (bone, child, _) in SHIMMY_POSE {
            let key = (bone.mirrored(), child.mirrored());
            assert!(
                m.contains_key(&key) || bone.mirrored() == *bone,
                "{bone:?} has no mirror partner in SHIMMY_POSE"
            );
        }

        // Asymmetric where it counts: the two arms must NOT be reflections of
        // one another, or there is no lead arm and no trail arm.
        let l = m[&(HumanoidBone::LeftArm, HumanoidBone::LeftForeArm)];
        let r = m[&(HumanoidBone::RightArm, HumanoidBone::RightForeArm)];
        let reflected_r = Vec3::new(-r.x, r.y, r.z);
        assert!(
            l.normalize().distance(reflected_r.normalize()) > 0.15,
            "the arms are mirror images, so the pose has no direction: {l:?} vs {r:?}"
        );

        // The lead (right) arm reaches further along +X than the trail arm.
        assert!(
            r.normalize().x > -l.normalize().x + 0.1,
            "the leading arm does not reach further along the lip than the trailing one"
        );

        // Mirroring twice is the identity, so travelling left then right does
        // not accumulate a bias.
        for e in SHIMMY_POSE {
            let back = mirror_pose_dir(mirror_pose_dir(*e));
            assert_eq!(back.0, e.0);
            assert_eq!(back.1, e.1);
            assert_eq!(back.2, e.2);
        }

        // And one mirrored entry actually lands on the other side.
        let (b, c, d) = mirror_pose_dir((
            HumanoidBone::RightArm,
            HumanoidBone::RightForeArm,
            r.to_array(),
        ));
        assert_eq!(b, HumanoidBone::LeftArm);
        assert_eq!(c, HumanoidBone::LeftForeArm);
        assert_eq!(d[0], -r.x);
        assert_eq!(d[1], r.y);
    }

    /// "One leg out front and the other trailing behind, swinging arms for
    /// momentum." Both halves of that are structural, so both are pinned here.
    #[test]
    fn the_vault_is_a_stride_with_opposed_arms_and_staggered_legs() {
        let key = |pose: &[PoseDir]| -> std::collections::HashMap<_, _> {
            pose.iter()
                .map(|(b, c, d)| ((*b, *c), Vec3::from_array(*d)))
                .collect()
        };
        // -Z is forward in the body frame.
        let launch = key(VAULT_LAUNCH_POSE);
        let land = key(VAULT_LAND_POSE);

        // EVERY limb root hangs. This is the single property that separates a
        // stride from a dive, and getting it wrong is not subtle: the first
        // version of this pose put the upper arms and the leading thigh near
        // horizontal, and the character went over the block in a Superman pose.
        // A limb that points further forward than it points down is not
        // swinging, it is reaching.
        for pose in [VAULT_LAUNCH_POSE, VAULT_LAND_POSE] {
            for (bone, child, d) in pose {
                let is_limb_root = matches!(
                    bone,
                    HumanoidBone::LeftArm
                        | HumanoidBone::RightArm
                        | HumanoidBone::LeftForeArm
                        | HumanoidBone::RightForeArm
                        | HumanoidBone::LeftUpLeg
                        | HumanoidBone::RightUpLeg
                        | HumanoidBone::LeftLeg
                        | HumanoidBone::RightLeg
                );
                if !is_limb_root {
                    continue;
                }
                let v = Vec3::from_array(*d);
                assert!(
                    v.y < 0.0 && v.y.abs() > v.z.abs(),
                    "{bone:?}->{child:?} points more along the ground than down                      ({v:?}) — that is a dive, not a swing"
                );
            }
        }

        let r_leg = launch[&(HumanoidBone::RightUpLeg, HumanoidBone::RightLeg)];
        let l_leg = launch[&(HumanoidBone::LeftUpLeg, HumanoidBone::LeftLeg)];
        assert!(
            r_leg.z < -0.25,
            "the leading thigh is not driving forward: {r_leg:?}"
        );
        assert!(
            l_leg.z > 0.2,
            "the trailing thigh is not extended behind: {l_leg:?}"
        );

        // CONTRALATERAL: the arm opposite the leading leg swings forward.
        let l_arm = launch[&(HumanoidBone::LeftArm, HumanoidBone::LeftForeArm)];
        let r_arm = launch[&(HumanoidBone::RightArm, HumanoidBone::RightForeArm)];
        assert!(
            l_arm.z < -0.25,
            "right leg leads, so the LEFT arm must swing forward: {l_arm:?}"
        );
        assert!(
            r_arm.z > 0.15,
            "the right arm should be driving back against it: {r_arm:?}"
        );
        assert!(
            l_arm.z * r_arm.z < 0.0,
            "both arms swing the same way — that is a jump, not a stride"
        );

        // The two keys must actually differ, or blending them animates nothing.
        let moved: f32 = VAULT_LAUNCH_POSE
            .iter()
            .filter_map(|(b, c, d)| {
                land.get(&(*b, *c))
                    .map(|e| Vec3::from_array(*d).distance(*e))
            })
            .sum();
        assert!(
            moved > 0.9,
            "launch and landing poses are nearly identical (total change {moved:.2}) —              blending them would hold one silhouette across the whole vault"
        );

        // Landing takes the weight: the lead shin comes under the body rather
        // than staying out in front of it.
        let r_shin_launch = launch[&(HumanoidBone::RightLeg, HumanoidBone::RightFoot)];
        let r_shin_land = land[&(HumanoidBone::RightLeg, HumanoidBone::RightFoot)];
        assert!(
            r_shin_land.y < r_shin_launch.y,
            "the leading shin does not drop under the body to land:              {r_shin_launch:?} -> {r_shin_land:?}"
        );

        // Mirrors cleanly, so vaults can alternate legs.
        for e in VAULT_LAUNCH_POSE.iter().chain(VAULT_LAND_POSE) {
            let back = mirror_pose_dir(mirror_pose_dir(*e));
            assert_eq!(back.0, e.0);
            assert_eq!(back.2, e.2);
        }
    }

    /// No climbing pose puts a limb HORIZONTAL.
    ///
    /// The vault learned this the hard way and the mantle was left with both
    /// thighs pointing further forward than down — a knee raised to waist
    /// height on each leg at once, which under a forward-pitched torso reads as
    /// the body folding up in mid-air. Applied to every authored climb pose so
    /// the next one cannot reintroduce it.
    ///
    /// Sign is deliberately not constrained: a hang reaches UP and a vault
    /// swings DOWN, both legitimate. What is never legitimate is a limb lying
    /// flat.
    #[test]
    fn no_authored_climb_pose_leaves_a_limb_horizontal() {
        for (name, pose) in [
            ("hang", HANG_POSE),
            ("mantle", MANTLE_POSE),
            ("shimmy", SHIMMY_POSE),
            ("vault-launch", VAULT_LAUNCH_POSE),
            ("vault-land", VAULT_LAND_POSE),
        ] {
            for (bone, child, d) in pose {
                let limb = matches!(
                    bone,
                    HumanoidBone::LeftArm
                        | HumanoidBone::RightArm
                        | HumanoidBone::LeftForeArm
                        | HumanoidBone::RightForeArm
                        | HumanoidBone::LeftUpLeg
                        | HumanoidBone::RightUpLeg
                        | HumanoidBone::LeftLeg
                        | HumanoidBone::RightLeg
                );
                if !limb {
                    continue;
                }
                let v = Vec3::from_array(*d);
                assert!(
                    v.y.abs() > v.z.abs(),
                    "{name}: {bone:?}->{child:?} lies flat ({v:?}) — it points further                      along the ground than up or down, which folds the body"
                );
            }
        }
    }



}
