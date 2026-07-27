//! # Locomotion — the kinematic controller and the one `AvatarLocomotion` producer
//!
//! ## What was missing
//!
//! `SharedCharacterPlugin` produced a `MovementIntent` and then never applied
//! it. Its `character_movement_physics`, `character_jump`, `ground_check` and
//! `update_locomotion` were **empty function bodies commented out of system
//! registration** (`plugins/character_plugin.rs:172-186`, bodies at `418-443`).
//! Across the whole `common/src/plugins` tree, `LinearVelocity` appeared only
//! inside comments.
//!
//! The live behaviour was: input produced an intent, `update_character_facing`
//! rotated the mesh to face it, `camera_follow` orbited — and the avatar stayed
//! pinned at its spawn point, pivoting in place. No gravity, no collision, no
//! jump.
//!
//! Downstream, because nothing wrote `LocomotionController`,
//! `get_animation_state()` returned `Idle` forever, `current_state` never
//! differed from `target_state`, and the crossfade / blend-tree /
//! speed-scaling code was unreachable. Those were reported as animation bugs.
//! They were this.
//!
//! ## Ground probe: a sphere cast, not a ray
//!
//! A downward ray from the capsule centre reports airborne whenever it hangs
//! over a ledge edge or a stair nose, which produces a visible grounded/airborne
//! stutter and makes the animation state flicker. A sphere cast slightly
//! narrower than the capsule tracks the surface the capsule is actually resting
//! on.

use bevy::prelude::*;
use avian3d::prelude::*;

use super::spawn::{AvatarBody, AvatarIntent, AvatarLocomotion};
use super::{AvatarControl, AvatarSystems, SpawnedByAvatarRuntime};
use eustress_avatar_schema::GRAVITY_MPS2;

/// Grace period after leaving ground during which a jump still works.
const COYOTE_TIME: f32 = 0.12;
/// How long a jump press is remembered while airborne.
const JUMP_BUFFER: f32 = 0.10;
/// Steeper than this is a wall, not a floor.
const MAX_SLOPE_DEG: f32 = 50.0;
/// Ledges up to this height are stepped, not jumped.
const STEP_HEIGHT: f32 = 0.30;
/// Ground-probe distance below the capsule bottom.
const GROUND_PROBE: f32 = 0.35;
/// How far below the feet still counts as standing on something. Covers the
/// collision skin and a frame of settling without reporting airborne.
const GROUND_SNAP_TOLERANCE: f32 = 0.12;

/// Timers the controller keeps per avatar.
#[derive(Component, Debug, Default)]
pub struct AvatarTimers {
    pub since_grounded: f32,
    pub since_jump_press: f32,
    pub jump_consumed: bool,
}

pub(crate) struct AvatarLocomotionPlugin;

impl Plugin for AvatarLocomotionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (attach_timers, drive_locomotion).chain().in_set(AvatarSystems::Locomotion),
        );
    }
}

fn attach_timers(
    mut commands: Commands,
    q: Query<Entity, (With<SpawnedByAvatarRuntime>, Without<AvatarTimers>)>,
) {
    for e in q.iter() {
        commands.entity(e).insert(AvatarTimers::default());
    }
}

/// The single system that moves an avatar and the single producer of
/// [`AvatarLocomotion`].
///
/// Being one system rather than four is deliberate: ground state, velocity and
/// the gait signal are mutually dependent within a frame, and splitting them is
/// what let the old code ship a `LocomotionController` that nothing wrote.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_locomotion(
    time: Res<Time>,
    spatial: SpatialQuery,
    move_and_slide: MoveAndSlide,
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &mut LinearVelocity,
            &mut AvatarLocomotion,
            &mut AvatarIntent,
            &mut AvatarTimers,
            &AvatarBody,
            &Collider,
            Option<&super::climb::AvatarClimb>,
        ),
        With<SpawnedByAvatarRuntime>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for (entity, mut tf, mut vel, mut loco, mut intent, mut timers, body, collider, climb) in q.iter_mut() {
        // A climb owns the body outright. Letting the controller integrate at
        // the same time makes the two fight over position and the character
        // jitters off the ledge.
        if climb.map(|c| c.is_climbing()).unwrap_or(false) {
            loco.grounded = false;
            loco.planar_speed = 0.0;
            loco.speed_norm = 0.0;
            intent.jump_pressed = false;
            continue;
        }
        let m = &body.metrics;
        let motion = &body.motion;

        let filter = SpatialQueryFilter::default().with_excluded_entities([entity]);

        // ── Ground probe ────────────────────────────────────────────────────
        //
        // Cast from the capsule CENTRE, not from the bottom hemisphere.
        //
        // The previous origin placed the probe sphere's lower pole exactly on
        // the capsule's bottom — i.e. already touching the floor when standing.
        // A shape cast that begins in contact reports no hit, so `grounded` was
        // false for the entire session: the air blend sat at 0.914, the jump
        // clip played permanently (with its root motion drifting the mesh and
        // snapping back on loop), and walk/run were inaudible underneath a
        // ground blend pinned near zero. One bad origin, every symptom.
        let probe_radius = m.capsule_radius * 0.9;
        let probe_origin = tf.translation;
        // Gap from the sphere's lower pole down to the capsule's bottom.
        let gap_to_feet = m.capsule_half_extent() - probe_radius;
        let probe = Collider::sphere(probe_radius);

        let hit = move_and_slide_ground_probe(
            &spatial,
            &probe,
            probe_origin,
            gap_to_feet + GROUND_PROBE,
            &filter,
        )
        // Ground only counts if the surface is within reach of the feet;
        // a hit further away means the character is genuinely airborne.
        .filter(|(_, dist)| *dist <= gap_to_feet + GROUND_SNAP_TOLERANCE);

        let was_grounded = loco.grounded;
        let (grounded, normal) = match hit {
            Some((n, _)) if n.angle_between(Vec3::Y).to_degrees() <= MAX_SLOPE_DEG => (true, n),
            // A hit on a too-steep face is a wall: not ground, but also not
            // free air. Keep the normal for slide projection.
            Some((n, _)) => (false, n),
            None => (false, Vec3::Y),
        };

        loco.ground_normal = normal;

        // Landing edge: latch impact strength before vertical velocity resets.
        if grounded && !was_grounded {
            loco.land_impact = (-vel.0.y / 8.0).clamp(0.0, 1.0);
        } else if !grounded {
            loco.land_impact *= 1.0 - (6.0 * dt).min(1.0);
        }

        loco.grounded = grounded;
        timers.since_grounded = if grounded { 0.0 } else { timers.since_grounded + dt };
        loco.air_time = timers.since_grounded;

        // ── Jump buffering ──────────────────────────────────────────────────
        if intent.jump_pressed {
            timers.since_jump_press = 0.0;
            timers.jump_consumed = false;
            // Edge-consumed here so no other system can double-read it.
            intent.jump_pressed = false;
        } else {
            timers.since_jump_press += dt;
        }

        // ── Horizontal target velocity ──────────────────────────────────────
        let control_scale = match body.control {
            AvatarControl::LocalPlayer => 1.0,
            AvatarControl::Remote => 1.0,
        };

        let mut dir = intent.direction;
        dir.y = 0.0;
        let dir = if dir.length_squared() > 1e-6 { dir.normalize() } else { Vec3::ZERO };

        let target_speed = if intent.sprint {
            motion.run_speed * motion.sprint_multiplier
        } else if dir != Vec3::ZERO {
            motion.walk_speed
        } else {
            0.0
        } * control_scale;

        let target = dir * target_speed;

        // Ground is responsive, air is not. Exponential smoothing rather than
        // a per-frame lerp constant, so behaviour is framerate-independent —
        // the old blend used a hardcoded 0.016 and changed with framerate.
        let rate = if grounded { 14.0 } else { 2.5 };
        let alpha = 1.0 - (-rate * dt).exp();
        let mut v = vel.0;
        v.x += (target.x - v.x) * alpha;
        v.z += (target.z - v.z) * alpha;

        // ── Vertical ────────────────────────────────────────────────────────
        let can_jump = (grounded || timers.since_grounded <= COYOTE_TIME)
            && timers.since_jump_press <= JUMP_BUFFER
            && !timers.jump_consumed;

        if can_jump {
            v.y = motion.jump_velocity();
            timers.jump_consumed = true;
            timers.since_grounded = COYOTE_TIME + 1.0; // consume coyote window
        } else if grounded && v.y <= 0.0 {
            // Small downward bias keeps the capsule seated on slopes instead
            // of skipping down them.
            v.y = -2.0;
        } else {
            v.y -= GRAVITY_MPS2 * dt;
            // Terminal velocity, so a long fall cannot tunnel.
            v.y = v.y.max(-55.0);
        }

        // ── Move and slide ──────────────────────────────────────────────────
        let out = move_and_slide.move_and_slide(
            collider,
            tf.translation,
            tf.rotation,
            v,
            time.delta(),
            &MoveAndSlideConfig::default(),
            &filter,
            |_hit| MoveAndSlideHitResponse::Accept,
        );

        let mut new_pos = out.position;
        let mut new_vel = out.projected_velocity;

        // ── Step-up ─────────────────────────────────────────────────────────
        // If we were grounded, wanted to move, and barely did, try again from
        // one step-height higher and drop back down.
        if grounded && dir != Vec3::ZERO {
            let planar_moved = (new_pos - tf.translation).with_y(0.0).length();
            let planar_wanted = v.with_y(0.0).length() * dt;
            if planar_wanted > 1e-3 && planar_moved < planar_wanted * 0.35 {
                if let Some(stepped) = try_step_up(
                    &move_and_slide,
                    &spatial,
                    collider,
                    tf.translation,
                    tf.rotation,
                    v.with_y(0.0),
                    time.delta(),
                    &filter,
                    m.capsule_half_extent(),
                ) {
                    new_pos = stepped;
                    new_vel = v;
                }
            }
        }

        tf.translation = new_pos;
        vel.0 = new_vel;

        // ── The gait signal ─────────────────────────────────────────────────
        let planar = Vec3::new(new_vel.x, 0.0, new_vel.z);
        loco.planar_speed = planar.length();
        loco.speed_norm =
            if motion.run_speed > 1e-3 { (loco.planar_speed / motion.run_speed).min(2.0) } else { 0.0 };
        loco.vertical_velocity = new_vel.y;
    }
}

/// Sphere-cast downward; return the surface normal and distance.
fn move_and_slide_ground_probe(
    spatial: &SpatialQuery,
    probe: &Collider,
    origin: Vec3,
    max_dist: f32,
    filter: &SpatialQueryFilter,
) -> Option<(Vec3, f32)> {
    let hit = spatial.cast_shape(
        probe,
        origin,
        Quat::IDENTITY,
        Dir3::NEG_Y,
        &ShapeCastConfig::from_max_distance(max_dist),
        filter,
    )?;
    Some((Vec3::from(hit.normal1), hit.distance))
}

/// Lift, move, and drop — a position correction, never an impulse.
#[allow(clippy::too_many_arguments)]
fn try_step_up(
    move_and_slide: &MoveAndSlide,
    spatial: &SpatialQuery,
    collider: &Collider,
    from: Vec3,
    rot: Quat,
    planar_vel: Vec3,
    dt: std::time::Duration,
    filter: &SpatialQueryFilter,
    half_extent: f32,
) -> Option<Vec3> {
    // 1. Is there headroom to lift into?
    let lifted = from + Vec3::Y * STEP_HEIGHT;
    if spatial
        .cast_shape(
            collider,
            from,
            rot,
            Dir3::Y,
            &ShapeCastConfig::from_max_distance(STEP_HEIGHT),
            filter,
        )
        .is_some()
    {
        return None;
    }

    // 2. Move horizontally at the raised height.
    let moved = move_and_slide.move_and_slide(
        collider,
        lifted,
        rot,
        planar_vel,
        dt,
        &MoveAndSlideConfig::default(),
        filter,
        |_| MoveAndSlideHitResponse::Accept,
    );

    // Did the lift actually buy us anything?
    if (moved.position - lifted).with_y(0.0).length() < 1e-3 {
        return None;
    }

    // 3. Drop back onto the step.
    let drop = spatial.cast_shape(
        collider,
        moved.position,
        rot,
        Dir3::NEG_Y,
        &ShapeCastConfig::from_max_distance(STEP_HEIGHT + 0.05),
        filter,
    )?;

    let landed = moved.position - Vec3::Y * drop.distance;
    // Reject a "step" that is really a fall.
    if landed.y < from.y - 0.01 {
        return None;
    }
    let _ = half_extent;
    Some(landed)
}
