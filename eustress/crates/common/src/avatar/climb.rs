//! # Ledge detection and mantling
//!
//! Assassin's-Creed / Uncharted-style traversal: run at a ledge, grab it, pull
//! up. This module implements the **mechanic** — detection, state, and motion.
//! It deliberately does not pretend to implement the *animation*.
//!
//! ## Why the mechanic ships before the animation
//!
//! The shipped asset library is eight Mixamo clips: idle, walk, run, jump, per
//! sex. There is no hang, no shimmy, no mantle. A climb built on clips we do
//! not have would be a stub; a climb built on a procedural pose is a real,
//! playable mechanic that looks plain until clips exist.
//!
//! So the body is posed procedurally here (hands to the ledge, legs tucked)
//! and every phase carries a `clip_hint` naming the clip that should displace
//! the procedural pose once authored. Swapping one in is then a data change,
//! not a rewrite.
//!
//! **Honest limitation:** hand placement wants two-bone IK, and `ik.rs` is
//! disabled pending its world→local fix. Until that lands the arms reach
//! approximately, via direct shoulder/arm rotation, rather than planting
//! exactly on the ledge edge.
//!
//! ## Detection
//!
//! Two casts, both required:
//!
//! ```text
//!            ┌─────────  ← 2. down-cast finds the ledge SURFACE
//!            │              (must be flat enough to stand on)
//!   [char] ──┤           ← 1. forward-cast finds the WALL
//!            │
//! ```
//!
//! A wall with no walkable surface above it is a wall, not a ledge — that
//! distinction is the whole detector.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::spawn::{AvatarBody, AvatarIntent, AvatarLocomotion};
use super::{AvatarSystems, SpawnedByAvatarRuntime};

/// Ledges below this (relative to the feet) are handled by the step-up in the
/// locomotion controller instead.
const MIN_LEDGE_HEIGHT: f32 = 0.45;
/// Highest reach, as a multiple of body height. ~1.3x puts a 1.75 m character's
/// limit around 2.3 m, which is roughly a human's max mantle.
const MAX_LEDGE_REACH: f32 = 1.30;
/// How far ahead to look for a wall, as a multiple of capsule radius.
const WALL_PROBE_REACH: f32 = 2.2;
/// A ledge top steeper than this is a slope, not a surface you can pull onto.
const MAX_LEDGE_SLOPE_DEG: f32 = 40.0;
/// Seconds to hang before the pull-up begins.
///
/// Long enough that the hang reads as a beat rather than a frame of overlap.
/// At 0.12 s the grip had not finished blending in before the mantle started,
/// so the pose the player saw was a half-committed reach, never a hang.
const HANG_SETTLE: f32 = 0.25;
/// Seconds the pull-up takes.
///
/// 0.65 s to clear a ~2 m ledge is roughly 3 m/s of vertical body movement —
/// faster than a person can actually pull their own mass, so it read as the
/// character being teleported upward rather than climbing. This is a
/// deliberate readability choice over athletic plausibility: still brisker
/// than a real mantle, but slow enough that the eye tracks the motion.
const MANTLE_DURATION: f32 = 1.05;

/// What the avatar is doing with a ledge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClimbPhase {
    #[default]
    None,
    /// Hands on the edge, body hanging.
    Hanging,
    /// Pulling up and over.
    Mantling,
}

impl ClimbPhase {
    /// The clip that should drive this phase once one exists. Nothing consumes
    /// this yet — it is the seam for authored animation, kept next to the
    /// state it belongs to so the two cannot drift apart.
    pub const fn clip_hint(self) -> Option<&'static str> {
        match self {
            ClimbPhase::None => None,
            ClimbPhase::Hanging => Some("hang_idle"),
            ClimbPhase::Mantling => Some("mantle_up"),
        }
    }
}

/// Per-avatar climb state.
#[derive(Component, Debug, Clone, Default)]
pub struct AvatarClimb {
    pub phase: ClimbPhase,
    /// World point on the ledge edge the hands are holding.
    pub grab_point: Vec3,
    /// Outward normal of the wall being climbed (points at the character).
    pub wall_normal: Vec3,
    /// Where the body ends up once the mantle completes.
    pub top_point: Vec3,
    /// 0..1 through the current phase.
    pub t: f32,
    /// Position the mantle started from, for interpolation.
    start: Vec3,
}

impl AvatarClimb {
    pub fn is_climbing(&self) -> bool {
        self.phase != ClimbPhase::None
    }
}

/// A ledge the detector found.
#[derive(Debug, Clone, Copy)]
pub struct LedgeHit {
    pub edge: Vec3,
    pub wall_normal: Vec3,
    pub top: Vec3,
}

pub(crate) struct AvatarClimbPlugin;

impl Plugin for AvatarClimbPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, attach_climb_state.in_set(AvatarSystems::Lifecycle))
            // Before locomotion: while climbing, the controller must not also
            // be driving the body, or the two fight over position.
            .add_systems(
                Update,
                drive_climb.in_set(AvatarSystems::Locomotion).before(super::locomotion::drive_locomotion),
            );
    }
}

fn attach_climb_state(
    mut commands: Commands,
    q: Query<Entity, (With<SpawnedByAvatarRuntime>, Without<AvatarClimb>)>,
) {
    for e in q.iter() {
        // `try_insert`: Lifecycle runs in the same frame a despawn can be
        // queued, and a plain `insert` on an entity that vanishes before the
        // buffers apply panics the whole schedule.
        commands.entity(e).try_insert(AvatarClimb::default());
    }
}

/// Look for a ledge in front of the character.
///
/// Returns `None` for a plain wall — a wall is only a ledge if there is a
/// surface on top of it flat enough to stand on.
pub fn detect_ledge(
    spatial: &SpatialQuery,
    origin: Vec3,
    forward: Vec3,
    body: &AvatarBody,
    exclude: Entity,
) -> Option<LedgeHit> {
    let m = &body.metrics;
    let filter = SpatialQueryFilter::default().with_excluded_entities([exclude]);

    let feet = origin.y - m.capsule_half_extent();
    let chest = feet + m.height_m * 0.55;
    let reach = m.capsule_radius * WALL_PROBE_REACH;

    // 1. Is there a wall ahead?
    let dir = Dir3::new(forward.with_y(0.0)).ok()?;
    let wall = spatial.cast_ray(Vec3::new(origin.x, chest, origin.z), dir, reach, true, &filter)?;

    let wall_point = Vec3::new(origin.x, chest, origin.z) + *dir * wall.distance;
    let wall_normal = Vec3::from(wall.normal);

    // 2. Is there a surface on top of it?
    //
    // Probe from above, slightly past the wall face, looking down. Starting
    // above max reach and casting down is what distinguishes a ledge from an
    // overhang: an overhang has no hit in the band we can actually reach.
    let max_h = feet + m.height_m * MAX_LEDGE_REACH;
    let probe_from = wall_point + *dir * (m.capsule_radius * 0.6);
    let probe_from = Vec3::new(probe_from.x, max_h + 0.35, probe_from.z);

    let down = spatial.cast_ray(probe_from, Dir3::NEG_Y, m.height_m * 1.4, true, &filter)?;
    let top = probe_from + Vec3::NEG_Y * down.distance;

    // Flat enough to stand on?
    let top_normal = Vec3::from(down.normal);
    if top_normal.angle_between(Vec3::Y).to_degrees() > MAX_LEDGE_SLOPE_DEG {
        return None;
    }

    // In the band we can actually reach?
    let height = top.y - feet;
    if height < MIN_LEDGE_HEIGHT || height > m.height_m * MAX_LEDGE_REACH {
        return None;
    }

    // Room to stand once up there?
    let stand_centre = top + Vec3::Y * (m.capsule_half_extent() + 0.02);
    let blocked = spatial
        .cast_shape(
            &Collider::capsule(m.capsule_radius * 0.9, m.capsule_cylinder_len * 0.9),
            stand_centre,
            Quat::IDENTITY,
            Dir3::Y,
            &ShapeCastConfig::from_max_distance(0.01),
            &filter,
        )
        .is_some();
    if blocked {
        return None;
    }

    Some(LedgeHit {
        edge: Vec3::new(wall_point.x, top.y, wall_point.z),
        wall_normal,
        top: stand_centre,
    })
}

#[allow(clippy::too_many_arguments)]
fn drive_climb(
    time: Res<Time>,
    spatial: SpatialQuery,
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &mut LinearVelocity,
            &mut AvatarClimb,
            &AvatarIntent,
            &AvatarLocomotion,
            &AvatarBody,
        ),
        With<SpawnedByAvatarRuntime>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for (entity, mut tf, mut vel, mut climb, intent, loco, body) in q.iter_mut() {
        match climb.phase {
            ClimbPhase::None => {
                // Only reach for a ledge while airborne and moving into a
                // surface — grabbing while walking along the ground would
                // hijack ordinary movement.
                if loco.grounded || intent.direction.length_squared() < 1e-4 {
                    continue;
                }
                let forward = intent.direction.normalize_or_zero();
                let Some(hit) = detect_ledge(&spatial, tf.translation, forward, body, entity) else {
                    continue;
                };

                climb.phase = ClimbPhase::Hanging;
                climb.grab_point = hit.edge;
                climb.wall_normal = hit.wall_normal;
                climb.top_point = hit.top;
                climb.t = 0.0;
                climb.start = tf.translation;
                vel.0 = Vec3::ZERO;

                info!("🧗 ledge grabbed at {:?}", hit.edge);
            }

            ClimbPhase::Hanging => {
                // Hold still against the wall, hands at the edge.
                vel.0 = Vec3::ZERO;
                let hang = climb.grab_point
                    + climb.wall_normal * (body.metrics.capsule_radius * 0.9)
                    - Vec3::Y * hang_drop(body);
                tf.translation = tf.translation.lerp(hang, (14.0 * dt).min(1.0));

                // Face the wall.
                let into_wall = -climb.wall_normal.with_y(0.0).normalize_or(Vec3::NEG_Z);
                let yaw = (-into_wall.x).atan2(-into_wall.z);
                tf.rotation = tf.rotation.slerp(Quat::from_rotation_y(yaw), (10.0 * dt).min(1.0));

                climb.t += dt;

                // Drop off deliberately.
                if intent.crouch {
                    climb.phase = ClimbPhase::None;
                    continue;
                }
                // Pull up automatically once settled, or immediately on jump.
                if climb.t >= HANG_SETTLE || intent.jump_pressed {
                    climb.phase = ClimbPhase::Mantling;
                    climb.t = 0.0;
                    climb.start = tf.translation;
                }
            }

            ClimbPhase::Mantling => {
                vel.0 = Vec3::ZERO;
                climb.t += dt / MANTLE_DURATION;

                // Up first, then forward. A straight lerp to the top clips the
                // character through the ledge corner; going vertical before
                // horizontal traces the shape of the obstacle.
                let k = climb.t.clamp(0.0, 1.0);
                let up_phase = (k / 0.6).clamp(0.0, 1.0);
                let fwd_phase = ((k - 0.4) / 0.6).clamp(0.0, 1.0);

                let lifted = Vec3::new(
                    climb.start.x,
                    climb.start.y + (climb.top_point.y - climb.start.y) * ease_out(up_phase),
                    climb.start.z,
                );
                tf.translation = lifted.lerp(
                    Vec3::new(climb.top_point.x, lifted.y.max(climb.top_point.y), climb.top_point.z),
                    ease_in_out(fwd_phase),
                );

                if k >= 1.0 {
                    tf.translation = climb.top_point;
                    climb.phase = ClimbPhase::None;
                    climb.t = 0.0;
                }
            }
        }
    }
}

/// How far the body's centre sits below the ledge lip while hanging.
///
/// Derived from the arm, not picked by feel. The hands are pinned to the lip,
/// so the drop *is* what decides how extended the arms end up: too small a
/// drop and the IK has to fold the elbows out sideways to absorb the slack,
/// which reads as a bodybuilder flex rather than a hang. Targeting ~85% of arm
/// extension keeps the elbows soft without stretching the chain to its limit,
/// where the solver clamps and the arms lock rigid.
///
/// On a ledge barely taller than the character this legitimately leaves the
/// feet near the ground — that is what hanging off a chest-high wall looks
/// like, not a bug to clamp away.
pub(crate) fn hang_drop(body: &AvatarBody) -> f32 {
    let m = &body.metrics;
    let arm_len = m.height_m * ARM_SPAN_FRAC;
    let shoulder_above_centre = m.height_m * SHOULDER_ABOVE_CENTRE_FRAC;
    arm_len * HANG_ARM_EXTENSION + shoulder_above_centre
}

/// Arm span (shoulder to wrist) as a fraction of standing height.
pub(crate) const ARM_SPAN_FRAC: f32 = 0.30;
/// Shoulder height above the body centre, as a fraction of standing height.
pub(crate) const SHOULDER_ABOVE_CENTRE_FRAC: f32 = 0.33;
/// Fraction of full arm extension to hang at.
const HANG_ARM_EXTENSION: f32 = 0.85;

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t)
}

fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_is_bounded_and_monotonic() {
        let mut prev_o = -1.0;
        let mut prev_io = -1.0;
        let mut t = 0.0;
        while t <= 1.0 {
            let o = ease_out(t);
            let io = ease_in_out(t);
            assert!((0.0..=1.0).contains(&o), "ease_out({t}) = {o}");
            assert!((0.0..=1.0).contains(&io), "ease_in_out({t}) = {io}");
            assert!(o >= prev_o - 1e-6, "ease_out not monotonic at {t}");
            assert!(io >= prev_io - 1e-6, "ease_in_out not monotonic at {t}");
            prev_o = o;
            prev_io = io;
            t += 0.02;
        }
        assert!((ease_out(1.0) - 1.0).abs() < 1e-6);
        assert!((ease_in_out(1.0) - 1.0).abs() < 1e-6);
        assert!(ease_in_out(0.0).abs() < 1e-6);
    }

    /// The vertical move must lead the horizontal one, or the character cuts
    /// the corner and clips through the ledge.
    #[test]
    fn mantle_goes_up_before_it_goes_forward() {
        let mut k = 0.0_f32;
        while k <= 1.0 {
            let up = ease_out((k / 0.6).clamp(0.0, 1.0));
            let fwd = ease_in_out(((k - 0.4) / 0.6).clamp(0.0, 1.0));
            assert!(up >= fwd - 1e-6, "at k={k} forward ({fwd}) outran up ({up})");
            k += 0.02;
        }
    }

    #[test]
    fn every_active_phase_names_the_clip_that_should_replace_it() {
        assert!(ClimbPhase::None.clip_hint().is_none());
        for p in [ClimbPhase::Hanging, ClimbPhase::Mantling] {
            assert!(p.clip_hint().is_some(), "{p:?} has no authored-clip seam");
        }
    }

    /// The first hang that rendered had horizontal upper arms and elbows
    /// flared sideways — a flex, not a hang — because the body hung too high
    /// and the IK had to fold the arms to absorb the slack. The drop is what
    /// controls that, so it is pinned here.
    #[test]
    fn the_hang_drop_leaves_the_arms_extended_but_not_locked() {
        for height in [1.5_f32, 1.75, 2.0] {
            let arm = height * ARM_SPAN_FRAC;
            let shoulder = height * SHOULDER_ABOVE_CENTRE_FRAC;
            let drop = arm * HANG_ARM_EXTENSION + shoulder;

            // How much of the arm the shoulder-to-lip gap consumes.
            let frac = (drop - shoulder) / arm;
            assert!(
                (0.70..0.95).contains(&frac),
                "height {height}: arm extension {frac} — folded elbows below 0.7, \
                 locked at the solver's clamp above 0.95"
            );
            assert!(drop > height * 0.4, "height {height}: drop {drop} barely moves the body");
        }
    }

    #[test]
    fn reach_band_excludes_what_step_up_already_handles() {
        // A 0.3 m kerb is the step-up's job; a 1.5 m ledge is a climb.
        assert!(MIN_LEDGE_HEIGHT > 0.30, "overlaps the controller's step height");
        assert!(MAX_LEDGE_REACH > 1.0 && MAX_LEDGE_REACH < 1.6, "implausible human reach");
    }
}
