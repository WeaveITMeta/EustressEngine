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
//!
//! ## Gravity is the live value
//!
//! The body is kinematic, so Avian's gravity never acts on it; the controller
//! integrates gravity itself, from Avian's `Gravity` as it stands that frame,
//! which `Workspace.gravity` drives. A Space set to the Moon, or a script that
//! changes gravity mid-game, moves the player the way it moves every falling
//! part. A jump follows Roblox's rules, so an imported game tuned for
//! Roblox's gravity jumps as it was tuned: a launch speed (JumpPower under
//! UseJumpPower) takes off at that speed, and a height (JumpHeight) takes off
//! at the speed that peaks there under that gravity. Weightless, a height
//! needs no take-off, so only a launch speed leaves the ground.

use bevy::prelude::*;
use avian3d::prelude::*;

use super::spawn::{AvatarBody, AvatarIntent, AvatarLocomotion};
use super::{AvatarControl, AvatarSystems, SpawnedByAvatarRuntime};
use crate::units::STANDARD_GRAVITY_F32;

/// Grace period after leaving ground during which a jump still works.
const COYOTE_TIME: f32 = 0.12;
/// How long a jump press is remembered while airborne.
const JUMP_BUFFER: f32 = 0.10;
/// Steeper than this is a wall, not a floor.
const MAX_SLOPE_DEG: f32 = 50.0;
/// Ledges up to this height are stepped, not jumped.
const STEP_HEIGHT: f32 = 0.30;
/// How far a step-up carries the body forward before dropping it back down.
///
/// Must exceed the capsule radius, or the body lands back on the surface it
/// started from because it still overhangs the edge.
const STEP_FORWARD_CLEARANCE: f32 = 0.42;
/// Ground-probe distance below the capsule bottom.
const GROUND_PROBE: f32 = 0.35;
/// How far below the feet still counts as standing on something. Covers the
/// collision skin and a frame of settling without reporting airborne.
const GROUND_SNAP_TOLERANCE: f32 = 0.12;

/// How long the body may be airborne and motionless before it is treated as
/// wedged, seconds.
const STUCK_GRACE: f32 = 0.30;
/// Speed below which an airborne body counts as not moving, m/s, at standard
/// gravity. The controller scales it by the live gravity.
///
/// A jump spends `2 · speed / g` seconds below a given speed around its apex,
/// so a threshold fixed for Earth would hold a standing jump on the Moon under
/// it for the whole grace period. Scaled, every apex passes through it in
/// about 0.05 s, and a weightless body, which nothing presses into a wedge, is
/// never counted as one. Being a speed rather than a distance per frame, it
/// means the same at every frame rate.
const STUCK_SPEED: f32 = 0.24;

/// The downward pull the controller integrates, m/s², from the live gravity.
///
/// The controller stands on a world-up floor: it probes for ground along −Y
/// and measures slopes against +Y. So it takes gravity's downward component
/// and ignores the rest, and a gravity pointing up leaves the body weightless
/// rather than pinning it to a ceiling it cannot stand on. With no `Gravity`
/// resource it is standard gravity.
pub fn downward_gravity(gravity: Option<&Gravity>) -> f32 {
    gravity.map_or(STANDARD_GRAVITY_F32, |g| (-g.0.y).max(0.0))
}

/// Is there room for the body at `pos`, counting only the colliders `solid`
/// accepts?
///
/// Uses the OVERLAP query, not a shape cast. A zero-distance cast does not
/// reliably report a starting penetration — it reported solid rock as free
/// space, and the wedge recovery duly teleported the body inside a wall.
/// `shape_intersections` asks the question directly.
///
/// The climb passes its surface rules as `solid`, so a trigger volume the
/// body is standing in never reads as the body being buried in a wall.
pub(crate) fn capsule_fits_where(
    spatial: &SpatialQuery,
    collider: &Collider,
    pos: Vec3,
    rot: Quat,
    filter: &SpatialQueryFilter,
    solid: &dyn Fn(Entity) -> bool,
) -> bool {
    if !pos.is_finite() {
        return false;
    }
    let mut clear = true;
    spatial.shape_intersections_callback(collider, pos, rot, filter, |e| {
        clear = !solid(e);
        // Keep looking only while nothing solid has been found.
        clear
    });
    clear
}

/// Find somewhere near `from` the body actually fits, counting only the
/// colliders `solid` accepts.
///
/// Wedging between two colliders is a dead end the controller cannot escape on
/// its own: collide-and-slide blocks every horizontal direction, gravity is
/// cancelled by the contact, and the ground probe finds no floor — so the
/// character hangs in the air playing a fall that never lands, with no input
/// that helps. Nothing in the controller detected or recovered from it.
///
/// Tries straight up first (the way out of a V-shaped wedge), then progressively
/// wider offsets. Returns `None` if the body is buried too deeply to rescue,
/// which is preferable to teleporting it somewhere arbitrary.
pub(crate) fn unwedge_where(
    spatial: &SpatialQuery,
    collider: &Collider,
    from: Vec3,
    rot: Quat,
    filter: &SpatialQueryFilter,
    radius: f32,
    solid: &dyn Fn(Entity) -> bool,
) -> Option<Vec3> {
    const RINGS: [f32; 5] = [0.6, 1.2, 2.0, 3.2, 5.0];
    for scale in RINGS {
        let step = radius * scale;
        // Up first: a wedge is usually narrower below than above.
        let candidates = [
            Vec3::Y * step * 1.5,
            Vec3::new(step, step * 0.5, 0.0),
            Vec3::new(-step, step * 0.5, 0.0),
            Vec3::new(0.0, step * 0.5, step),
            Vec3::new(0.0, step * 0.5, -step),
            Vec3::new(step, step * 0.5, step),
            Vec3::new(-step, step * 0.5, step),
            Vec3::new(step, step * 0.5, -step),
            Vec3::new(-step, step * 0.5, -step),
        ];
        for off in candidates {
            let p = from + off;
            if capsule_fits_where(spatial, collider, p, rot, filter, solid) {
                return Some(p);
            }
        }
    }
    None
}

/// Bias that keeps a grounded capsule seated against the surface rather than
/// skimming a collision skin above it, m/s.
const GROUND_STICK: f32 = 2.0;

/// The seating velocity for a given ground normal.
///
/// Pressed along **−normal**, not world-down. A world-down bias is only
/// perpendicular to the ground when the ground is flat; on any slope it has a
/// component *along* the face, and collide-and-slide faithfully turns that
/// component into motion. The result was a character that slid downhill at a
/// constant rate while standing still — the bias meant to hold it down was
/// what pushed it along.
///
/// Pressing along the normal has zero tangential component by construction, so
/// it seats the capsule without ever moving it.
pub fn ground_seat(ground_normal: Vec3) -> Vec3 {
    -ground_normal.normalize_or(Vec3::Y) * GROUND_STICK
}

/// Project a horizontal movement direction onto the ground plane.
///
/// Returns a unit vector along the surface, so speed is preserved *along the
/// slope* rather than being the horizontal projection of it. Falls back to the
/// input when airborne, when there is no meaningful slope, or when the
/// projection degenerates (facing straight into a wall).
pub fn slope_project(dir: Vec3, ground_normal: Vec3, grounded: bool) -> Vec3 {
    if !grounded || dir.length_squared() < 1e-6 {
        return dir;
    }
    let n = ground_normal.normalize_or(Vec3::Y);
    // Flat ground: nothing to project, and normalising a near-identical vector
    // only invites float noise.
    if n.dot(Vec3::Y) > 0.9999 {
        return dir;
    }
    let projected = dir - n * dir.dot(n);
    projected.try_normalize().unwrap_or(dir)
}

/// Touchdown speed that saturates `land_impact`, m/s.
///
/// 18 m/s is a ~16.5 m fall. The old value of 8 saturated after 3.3 m, which
/// collapsed roughly 85% of the achievable range into a single value and made
/// every landing above waist height look identical.
pub const LAND_IMPACT_FULL_MPS: f32 = 18.0;

/// Timers the controller keeps per avatar.
///
/// `Default` is hand-written because the derived one spelled "grounded, and
/// jump was pressed this instant": `since_grounded = 0.0` satisfies the coyote
/// clause, `since_jump_press = 0.0` satisfies the buffer clause, and
/// `jump_consumed = false` satisfies the third — so `can_jump` was true on the
/// first frame the component existed and every avatar jumped on spawn with no
/// input. Both fields below independently prevent that.
#[derive(Component, Debug)]
pub struct AvatarTimers {
    pub since_grounded: f32,
    pub since_jump_press: f32,
    pub jump_consumed: bool,
    /// Seconds spent airborne while gravity is being applied but the body is
    /// not actually moving — the signature of being wedged.
    pub stuck_time: f32,
    /// Whether a non-finite move on this avatar has been reported yet.
    pub non_finite_reported: bool,
    /// Whether this stuck spell has been reported; cleared once the body
    /// moves or lands again.
    pub stuck_reported: bool,
    /// How long a jump pressed while seated has been waiting.
    pub seated_jump_age: f32,
    /// The collider the avatar last stood on, for the ground line in the log.
    /// Kept through jumps, so only a NEW surface is reported.
    pub last_ground: Option<Entity>,
}

impl Default for AvatarTimers {
    fn default() -> Self {
        Self {
            since_grounded: 0.0,
            // Already past the buffer window; a real press resets it to 0.
            since_jump_press: f32::MAX,
            // Nothing to consume until a real press clears this.
            jump_consumed: true,
            stuck_time: 0.0,
            non_finite_reported: false,
            stuck_reported: false,
            seated_jump_age: 0.0,
            last_ground: None,
        }
    }
}

/// How long a jump pressed while seated stays pending, seconds: a joined
/// player's press reaches the host a round trip later.
const SEATED_JUMP_HOLD: f32 = 0.5;

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
    gravity: Option<Res<Gravity>>,
    spatial: SpatialQuery,
    move_and_slide: MoveAndSlide,
    names: Query<&Name>,
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
            Option<&super::landing::AvatarLanding>,
            Option<&super::abilities::AvatarAbilities>,
            Option<&super::seat::AvatarSeated>,
        ),
        With<SpawnedByAvatarRuntime>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let g = downward_gravity(gravity.as_deref());
    // Below this much movement in a frame, an airborne body has stopped.
    let stuck_below = STUCK_SPEED * (g / STANDARD_GRAVITY_F32) * dt;
    // Solid colliders only, the set `move_and_slide` itself moves against. A
    // sensor (a trigger volume, a touch zone) is walked through, so it is
    // never ground, never a wall, and never a step.
    let solid = |e: Entity| move_and_slide.colliders.contains(e);

    for (entity, mut tf, mut vel, mut loco, mut intent, mut timers, body, collider, climb, landing, abilities, seated)
        in q.iter_mut()
    {
        let abilities = abilities.copied().unwrap_or_default();
        // A seat owns the body outright too: it rides the seat
        // (`seat::ride_seats`). A jump pressed while seated stays pending for
        // `SEATED_JUMP_HOLD`, long enough to reach the host and be spent on
        // getting up; an older one expires, so a press that meant something
        // else in the seat (a handbrake) never fires when the rider leaves
        // another way.
        loco.seated = seated.is_some();
        if loco.seated {
            loco.grounded = true;
            loco.planar_speed = 0.0;
            loco.speed_norm = 0.0;
            loco.vertical_velocity = 0.0;
            loco.air_time = 0.0;
            vel.0 = Vec3::ZERO;
            if intent.jump_pressed {
                timers.seated_jump_age += dt;
                if timers.seated_jump_age > SEATED_JUMP_HOLD {
                    intent.jump_pressed = false;
                }
            }
            if !intent.jump_pressed {
                timers.seated_jump_age = 0.0;
            }
            continue;
        }
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
            &solid,
        )
        // Ground only counts if the surface is within reach of the feet;
        // a hit further away means the character is genuinely airborne.
        .filter(|(_, dist, _)| *dist <= gap_to_feet + GROUND_SNAP_TOLERANCE);

        let was_grounded = loco.grounded;
        let (grounded, normal) = match hit {
            Some((n, _, _)) if n.angle_between(Vec3::Y).to_degrees() <= MAX_SLOPE_DEG => (true, n),
            // A hit on a too-steep face is a wall: not ground, but also not
            // free air. Keep the normal for slide projection.
            Some((n, _, _)) => (false, n),
            None => (false, Vec3::Y),
        };

        loco.ground_normal = normal;

        // Each new surface the avatar stands on is named once, so a character
        // held up by something unseen has an answer in the log: the collider,
        // and its gap to the feet (below zero, the feet are inside it).
        if let (true, Some((_, dist, ground))) = (grounded, hit) {
            if timers.last_ground != Some(ground) {
                timers.last_ground = Some(ground);
                let name = names.get(ground).map_or("unnamed", |n| n.as_str());
                tracing::info!(
                    "avatar: standing on {name} ({ground}), gap to the feet {:.3} m",
                    dist - gap_to_feet
                );
            }
        }

        // Landing edge: latch impact strength before vertical velocity resets.
        //
        // The decay must run on EVERY non-landing frame, not only airborne
        // ones. With `else if !grounded` there was no steady-grounded branch,
        // so `land_impact` held its latched value from touchdown until the
        // avatar next left the ground. `procedural.rs` re-latches `land_flex`
        // from it whenever it is the larger value and zeroes `land_flex_vel`
        // doing so, which defeated the critically damped release below it —
        // the avatar stayed in a permanent half-crouch with dropped hips after
        // any landing at all.
        loco.just_landed = grounded && !was_grounded;
        if loco.just_landed {
            loco.land_speed_mps = (-vel.0.y).max(0.0);
            loco.land_impact = (loco.land_speed_mps / LAND_IMPACT_FULL_MPS).clamp(0.0, 1.0);
        } else {
            loco.land_impact *= 1.0 - (6.0 * dt).min(1.0);
        }

        loco.grounded = grounded;
        timers.since_grounded = if grounded { 0.0 } else { timers.since_grounded + dt };
        loco.air_time = timers.since_grounded;

        // ── Jump buffering ──────────────────────────────────────────────────
        // With jumping switched off a press is dropped, not buffered, so it
        // cannot fire later when jumping comes back on.
        if !abilities.jump {
            intent.jump_pressed = false;
            timers.jump_consumed = true;
        }
        if intent.jump_pressed {
            timers.since_jump_press = 0.0;
            timers.jump_consumed = false;
            // Edge-consumed here so no other system can double-read it.
            intent.jump_pressed = false;
        } else {
            timers.since_jump_press += dt;
        }

        // ── Horizontal target velocity ──────────────────────────────────────
        //
        // Replaces a two-arm match on `AvatarControl` that returned 1.0 in
        // both arms — dead code that read as if remote avatars were scaled.
        // The landing state is a real authority scale: a roll takes the body
        // outright, a stumble only slows it.
        let control_scale = landing.map(|l| l.control_scale()).unwrap_or(1.0);
        let roll_owns_body = landing.map(|l| l.phase.owns_body()).unwrap_or(false);

        let mut dir = intent.direction;
        dir.y = 0.0;
        let dir = if dir.length_squared() > 1e-6 { dir.normalize() } else { Vec3::ZERO };

        // A sprint stops at a person's top speed; a walk is never capped.
        let target_speed = if intent.sprint && abilities.sprint {
            motion.capped_run_and_sprint().1
        } else if dir != Vec3::ZERO {
            motion.walk_speed
        } else {
            0.0
        } * control_scale;

        // Walk ALONG the ground, not horizontally through it.
        //
        // `ground_normal` was computed every frame and read by nothing. With a
        // purely horizontal target, collide-and-slide has to deflect the whole
        // motion off the slope face, which costs `cos θ` of speed going up and,
        // going down, walks the capsule straight off the surface into the air.
        // Projecting onto the ground plane keeps the full speed along the
        // surface and produces the vertical rate a descent actually needs.
        let move_dir = slope_project(dir, loco.ground_normal, grounded);
        let target = move_dir * target_speed;

        // Ground is responsive, air is not. Exponential smoothing rather than
        // a per-frame lerp constant, so behaviour is framerate-independent —
        // the old blend used a hardcoded 0.016 and changed with framerate.
        let rate = if grounded { 14.0 } else { 2.5 };
        let alpha = 1.0 - (-rate * dt).exp();
        let mut v = vel.0;
        // While a roll owns the body it is setting horizontal velocity itself.
        // Retargeting here would decay the roll toward the input target at
        // 14/s and the character would stop mid-tumble.
        if !roll_owns_body {
            v.x += (target.x - v.x) * alpha;
            v.z += (target.z - v.z) * alpha;
        }

        // ── Vertical ────────────────────────────────────────────────────────
        let can_jump = (grounded || timers.since_grounded <= COYOTE_TIME)
            && timers.since_jump_press <= JUMP_BUFFER
            && !timers.jump_consumed;

        if can_jump {
            v.y = motion.jump_velocity_under(g);
            timers.jump_consumed = true;
            timers.since_grounded = COYOTE_TIME + 1.0; // consume coyote window
        } else if grounded && v.y <= 0.0 {
            // Ride the surface downhill.
            //
            // A fixed -2.0 only sticks on the flat. Descending a 45° ramp at
            // sprint speed needs ~5.7 m/s downward just to stay in contact, so
            // the capsule left the surface every step, fell, landed, and left
            // again — which is what "problems with sloped inclines" feels
            // like. The slope-projected target already carries the correct
            // descent rate; the constant is now only the seating bias on top
            // of it.
            //
            // Clamped at 0 for ascent: climbing is left to collide-and-slide,
            // because forcing `v.y` upward here would fight the jump on the
            // frame after take-off, while the character still reads grounded.
            if dir == Vec3::ZERO {
                // Standing still on ground means EXACTLY zero velocity.
                //
                // Every velocity-based seating bias is wrong on a slope, in one
                // direction or the other. A world-down press gets projected
                // along the face by collide-and-slide and slides you downhill;
                // a press along −normal has a horizontal component pointing
                // into the hill and drives you *up* it. Both were shipped, in
                // that order.
                //
                // Contact is a position problem, not a velocity one — the
                // ground snap below pulls the capsule onto the surface without
                // ever giving it momentum along the surface.
                v = Vec3::ZERO;
            } else {
                v.y = target.y.min(0.0);
            }
        } else {
            v.y -= g * dt;
            // Terminal velocity, so a long fall cannot tunnel.
            v.y = v.y.max(-55.0);
        }

        // ── Move and slide ──────────────────────────────────────────────────
        // ── Running into a wall should STOP you, not redirect you ───────────
        //
        // Collide-and-slide projects the whole movement onto the wall plane and
        // KEEPS its magnitude, so a run that is almost square into a face turns
        // into a fast glide along it and carries you to the far edge. Nothing
        // about that is contact; it is a frictionless rail.
        //
        // Damped BEFORE the move, from an explicit cast. The first attempt read
        // normals from `move_and_slide`'s hit callback and measured a head-on
        // value of exactly zero on every frame — the wall contact never reached
        // it — so the damping silently never ran. A cast we issue ourselves is
        // one we can reason about.
        //
        // What survives is scaled by cos² of the impact angle: running ALONG a
        // wall is untouched, running INTO one keeps almost nothing.
        //
        // The component INTO the face is dropped outright, and that is the part
        // that matters on a ramp. Keeping it left collide-and-slide with a
        // horizontal push against a steep face, which it faithfully projects
        // ONTO that face — and the projection of a horizontal push onto a 60°
        // slope points UP it. Damping only the sideways glide therefore fixed
        // walls and did nothing at all for ramps: the character still slid
        // smoothly up a surface far too steep to stand on.
        //
        // Moving AWAY from the surface is always allowed, so nothing traps the
        // body against it.
        let planar = v.with_y(0.0);
        // Kept for the step-up test below, which must ask what the player WANTED
        // rather than what survived this damping — otherwise zeroing the push
        // into a riser makes every real step look like no input at all.
        let desired_planar = planar;
        let mut wall_ahead: Option<Vec3> = None;
        if planar.length_squared() > 1e-4 {
            let d = planar.normalize();
            if let Ok(dir3) = Dir3::new(d) {
                let look = planar.length() * dt + m.capsule_radius * 0.6;
                let mut cfg = ShapeCastConfig::from_max_distance(look);
                // Already touching the wall is the normal case here, and a cast
                // that refuses to report the surface it starts against would
                // only ever fire on the first frame of contact.
                cfg.ignore_origin_penetration = true;
                if let Some(hit) =
                    spatial.cast_shape_predicate(collider, tf.translation, tf.rotation, dir3, &cfg, &filter, &solid)
                {
                    let n = Vec3::from(hit.normal1).with_y(0.0).normalize_or_zero();
                    // Only a surface too steep to walk up is a wall.
                    let steep = Vec3::from(hit.normal1).angle_between(Vec3::Y).to_degrees()
                        > MAX_SLOPE_DEG;
                    if steep && n.length_squared() > 0.5 {
                        wall_ahead = Some(n);
                        let head_on = d.dot(n).abs().clamp(0.0, 1.0);
                        let retain = (1.0 - head_on * head_on).clamp(0.0, 1.0);
                        let dn = planar.dot(n);
                        // Outward (dn > 0) survives untouched; inward is gone.
                        let outward = dn.max(0.0) * n;
                        let tangential = planar - dn * n;
                        let kept = outward + tangential * retain;
                        v.x = kept.x;
                        v.z = kept.z;
                    }
                }
            }
        }

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

        // A collide-and-slide that begins inside geometry (a body snapped
        // into overlap, as a replica can be) can hand back a non-finite
        // result. Whatever is written below is permanent: the velocity feeds
        // the next frame, the gait signal and, through the animation, every
        // bone. Stay put for this frame instead.
        let mut new_pos = if out.position.is_finite() { out.position } else { tf.translation };
        let mut new_vel = if out.projected_velocity.is_finite() { out.projected_velocity } else { Vec3::ZERO };

        // ── Running into a wall should STOP you ─────────────────────────────
        //
        // Collide-and-slide projects the whole movement onto the wall plane and
        // keeps its magnitude, so running square into a face converts nearly
        // all of your speed into sideways glide and carries you along it to the
        // far edge. Nothing about that is contact: it is a frictionless rail.
        //
        // What survives is scaled by cos² of the deflection, which is exactly
        // the tangential fraction of the original motion. Running ALONG a wall
        // is untouched (the deflection is near zero, so cos² is near one);
        // running INTO one keeps almost nothing.
        //
        // The normal component is deliberately left alone — that is
        // depenetration, and damping it would leave the body inside the wall.
        // ── Step-up ─────────────────────────────────────────────────────────
        // If we were grounded, wanted to move, and barely did, try again from
        // one step-height higher and drop back down.
        if grounded && dir != Vec3::ZERO {
            let planar_moved = (new_pos - tf.translation).with_y(0.0).length();
            let planar_wanted = desired_planar.length() * dt;
            // 0.35 demanded the character be almost completely stopped before
            // a step was attempted. A small riser only costs you part of a
            // frame's motion, so the trigger never fired and low steps read as
            // invisible walls you scuff against. 0.80 catches a partial block.
            // Never step up a surface you could simply WALK up.
            //
            // A ramp partially blocks horizontal motion — you climb it, so you
            // do not travel as far as you asked — which looks exactly like
            // being stopped by a step. Step-up then fired and teleported the
            // body 0.42 m up and forward, so slopes were "climbed" instantly
            // instead of walked. Only an obstruction too steep to stand on is
            // a step.
            let blocked_by_wall = spatial
                .cast_ray(
                    tf.translation - Vec3::Y * (m.capsule_half_extent() - STEP_HEIGHT * 0.5),
                    Dir3::new(dir).unwrap_or(Dir3::NEG_Z),
                    m.capsule_radius + 0.25,
                    true,
                    &filter,
                )
                .map(|h| {
                    Vec3::from(h.normal).angle_between(Vec3::Y).to_degrees() > MAX_SLOPE_DEG
                })
                .unwrap_or(false);

            // A STEP is something you can get on top of. A wall is not.
            //
            // The obstruction test only sampled near the feet, where a 6 m wall
            // and a 0.2 m riser look identical — so running into a wall fired
            // step-up every frame, and each firing restored the full undamped
            // velocity below, feeding the very glide the damping above exists
            // to stop. If the obstruction is still there a step-height up, it
            // is not a step.
            let taller_than_a_step = spatial
                .cast_ray(
                    tf.translation - Vec3::Y * (m.capsule_half_extent() - STEP_HEIGHT * 1.6),
                    Dir3::new(dir).unwrap_or(Dir3::NEG_Z),
                    m.capsule_radius + 0.25,
                    true,
                    &filter,
                )
                .is_some();

            if blocked_by_wall
                && !taller_than_a_step
                && planar_wanted > 1e-3
                && planar_moved < planar_wanted * 0.80
            {
                if let Some(stepped) = try_step_up(
                    &move_and_slide,
                    &spatial,
                    collider,
                    tf.translation,
                    tf.rotation,
                    desired_planar,
                    time.delta(),
                    &filter,
                    m.capsule_half_extent(),
                ) {
                    new_pos = stepped;
                    new_vel = v;
                }
            }
        }

        // ── Step-down ───────────────────────────────────────────────────────
        //
        // The missing half of step-up. Walking DOWN a small riser or off a
        // slope crest left the capsule airborne for a few frames until gravity
        // caught it — which flips `grounded` false, drives the air blend, and
        // plays the jump clip on every stair tread and every ramp lip.
        //
        // If we were on the ground, are still moving, and did not jump, pull
        // the body back down to a surface within one step height instead of
        // letting it launch.
        if grounded && !can_jump && new_vel.y <= 0.0 && new_pos.is_finite() {
            let probe_radius = m.capsule_radius * 0.9;
            let gap = m.capsule_half_extent() - probe_radius;
            if let Some((n, dist, _)) = move_and_slide_ground_probe(
                &spatial,
                &Collider::sphere(probe_radius),
                new_pos,
                gap + STEP_HEIGHT,
                &filter,
                &solid,
            ) {
                let drop = dist - gap;
                // Only snap to something we could have walked down, and only
                // to a surface flat enough to stand on — a steep face below is
                // a fall, not a step.
                // Only a REAL step, never a hair's gap.
                //
                // Snapping every sub-centimetre gap walks the body down into a
                // sloped surface each frame; the solver pushes it back out
                // along the normal, and the net effect is a slow downhill
                // creep — 1.2 m in 3 s at 30°, with no input. The ground probe
                // already tolerates a gap this size without reporting
                // airborne, so there is nothing to gain by closing it.
                if drop > 0.02
                    && drop <= STEP_HEIGHT
                    && n.angle_between(Vec3::Y).to_degrees() <= MAX_SLOPE_DEG
                {
                    // Scaled by how flat the surface is. A vertical sphere cast
                    // measures the gap along -Y, but on a slope the capsule
                    // contacts tangentially, so moving down by the full gap
                    // buries the uphill side — the solver pushes it back out
                    // along the normal and the body creeps downhill a little
                    // more each frame. Scaling by `n.y` lands it on the surface
                    // instead of through it.
                    new_pos.y -= drop * n.y.max(0.0);
                    new_vel.y = 0.0;
                    loco.grounded = true;
                    timers.since_grounded = 0.0;
                }
            }
        }

        // ── Wedge recovery ──────────────────────────────────────────────────
        //
        // Airborne, gravity applied, and yet the body did not move: it is
        // pinned between colliders. Left alone this never resolves — the fall
        // animation plays forever and no input helps.
        let moved_this_frame = (new_pos - tf.translation).length();
        if !grounded && moved_this_frame < stuck_below && !can_jump {
            timers.stuck_time += dt;
        } else {
            timers.stuck_time = 0.0;
            timers.stuck_reported = false;
        }

        if timers.stuck_time > STUCK_GRACE {
            if let Some(free) =
                unwedge_where(&spatial, collider, new_pos, tf.rotation, &filter, m.capsule_radius, &solid)
            {
                warn!(
                    "avatar: wedged for {:.2}s — freeing to {:?}",
                    timers.stuck_time, free
                );
                new_pos = free;
                new_vel = Vec3::ZERO;
            } else if !timers.stuck_reported {
                // No room anywhere near: say what holds the body, once per
                // spell, so a character hanging in the air has an answer.
                timers.stuck_reported = true;
                let mut inside = Vec::new();
                spatial.shape_intersections_callback(collider, new_pos, tf.rotation, &filter, |e| {
                    inside.push(e);
                    inside.len() < 8
                });
                let inside: Vec<String> = inside
                    .iter()
                    .map(|&e| {
                        let name = names.get(e).map_or("unnamed", |n| n.as_str());
                        format!("{name} ({e}, {})", if solid(e) { "solid" } else { "sensor" })
                    })
                    .collect();
                let probe = hit.map(|(_, dist, ground)| {
                    let name = names.get(ground).map_or("unnamed", |n| n.as_str());
                    format!("{name} ({ground}), gap to the feet {:.3} m", dist - gap_to_feet)
                });
                tracing::warn!(
                    "avatar: stuck in the air at {new_pos:?}, vertical speed {:.2} m/s, no free space nearby; \
                     overlapping [{}]; ground probe: {}",
                    new_vel.y,
                    inside.join(", "),
                    probe.as_deref().unwrap_or("nothing within reach")
                );
            }
            timers.stuck_time = 0.0;
        }

        // Last line of defence before the state is kept: nothing non-finite is
        // written, whatever above produced it. Said once per avatar, naming
        // it, so the source can be found.
        if !new_pos.is_finite() || !new_vel.is_finite() {
            if !timers.non_finite_reported {
                timers.non_finite_reported = true;
                warn!(
                    "avatar: non-finite move on {entity:?} ({:?}): pos {new_pos:?} vel {new_vel:?}, kept in place",
                    body.control
                );
            }
            if !new_pos.is_finite() {
                new_pos = tf.translation;
            }
            new_vel = Vec3::ZERO;
        }

        tf.translation = new_pos;
        vel.0 = new_vel;

        // ── The gait signal ─────────────────────────────────────────────────
        let planar = Vec3::new(new_vel.x, 0.0, new_vel.z);
        loco.planar_speed = planar.length();
        // Against the pace the body actually runs at, so a capped sprint
        // reads as a run and not as a jog toward an unreachable pace.
        let run = motion.capped_run_and_sprint().0;
        loco.speed_norm = if run > 1e-3 { (loco.planar_speed / run).min(2.0) } else { 0.0 };
        loco.vertical_velocity = new_vel.y;
    }
}

/// Sphere-cast downward against the colliders `solid` admits; return the
/// surface normal, the distance, and the collider hit.
fn move_and_slide_ground_probe(
    spatial: &SpatialQuery,
    probe: &Collider,
    origin: Vec3,
    max_dist: f32,
    filter: &SpatialQueryFilter,
    solid: &dyn Fn(Entity) -> bool,
) -> Option<(Vec3, f32, Entity)> {
    let hit = spatial.cast_shape_predicate(
        probe,
        origin,
        Quat::IDENTITY,
        Dir3::NEG_Y,
        &ShapeCastConfig::from_max_distance(max_dist),
        filter,
        solid,
    )?;
    let normal = Vec3::from(hit.normal1);
    // A cast that starts overlapping can report a zero normal. That is no
    // surface to stand on or walk along, and stored as the ground normal it
    // turns every angle measured against it into NaN, so it counts as no hit.
    (normal.is_finite() && normal.length_squared() > 0.25 && hit.distance.is_finite())
        .then_some((normal, hit.distance, hit.entity))
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
    // Solid colliders only, as in `drive_locomotion`.
    let solid = |e: Entity| move_and_slide.colliders.contains(e);

    // 1. Is there headroom to lift into?
    let lifted = from + Vec3::Y * STEP_HEIGHT;
    // `ignore_origin_penetration` MUST be set.
    //
    // It defaults to false, so a shape cast that begins in contact reports a
    // hit at distance zero. A grounded capsule is always in slight contact
    // with the floor it is standing on, so this headroom test reported
    // "blocked" every single time and `try_step_up` returned `None` before it
    // ever tried anything. Step-up has never worked.
    let mut headroom = ShapeCastConfig::from_max_distance(STEP_HEIGHT);
    headroom.ignore_origin_penetration = true;
    if spatial.cast_shape_predicate(collider, from, rot, Dir3::Y, &headroom, filter, &solid).is_some() {
        return None;
    }

    // 2. Move horizontally at the raised height, far enough to CLEAR the edge.
    //
    // This used to advance by one frame's velocity — about 24 mm at walking
    // speed. A capsule nudged 24 mm past a step edge still overhangs it by
    // most of its radius, so the drop cast in step 3 hit the floor the
    // character was already standing on, `landed` came back at the original
    // height, and the "step" was rejected as a fall. Step-up could never
    // succeed at any speed.
    //
    // The advance has to exceed the capsule radius, so it is expressed as a
    // distance and converted back into a velocity for `move_and_slide`.
    let secs = dt.as_secs_f32().max(1e-4);
    let advance = planar_vel.normalize_or_zero() * (STEP_FORWARD_CLEARANCE / secs);
    let moved = move_and_slide.move_and_slide(
        collider,
        lifted,
        rot,
        if advance == Vec3::ZERO { planar_vel } else { advance },
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
    // Same origin-penetration trap as the headroom cast: after moving
    // horizontally at the raised height the capsule may be grazing the step's
    // side, and a zero-distance hit would "land" the character in mid-air.
    let mut down = ShapeCastConfig::from_max_distance(STEP_HEIGHT + 0.05);
    down.ignore_origin_penetration = true;
    let Some(drop) = spatial.cast_shape_predicate(collider, moved.position, rot, Dir3::NEG_Y, &down, filter, &solid)
    else {
        return None;
    };

    let landed = moved.position - Vec3::Y * drop.distance;
    // Reject a "step" that is really a fall.
    if landed.y < from.y - 0.01 {
        return None;
    }
    let _ = half_extent;
    Some(landed)
}

#[cfg(test)]
mod slope_tests {
    use super::*;

    fn normal_for(deg: f32) -> Vec3 {
        // Ramp tilted about X: normal leans in -Z as the slope rises toward +Z.
        Quat::from_rotation_x(deg.to_radians()) * Vec3::Y
    }

    #[test]
    fn flat_ground_leaves_the_direction_untouched() {
        let d = Vec3::new(0.0, 0.0, -1.0);
        assert!((slope_project(d, Vec3::Y, true) - d).length() < 1e-6);
    }

    #[test]
    fn airborne_movement_is_never_projected() {
        let d = Vec3::new(0.0, 0.0, -1.0);
        assert!((slope_project(d, normal_for(45.0), false) - d).length() < 1e-6);
    }

    /// The bug: a horizontal target walks the capsule off a descending slope
    /// into the air. The projected direction must carry a DOWNWARD component
    /// so the body tracks the surface.
    #[test]
    fn descending_a_slope_produces_downward_motion() {
        for deg in [15.0_f32, 30.0, 45.0] {
            let n = normal_for(deg);
            // Walking in the direction the slope falls away.
            let out = slope_project(Vec3::new(0.0, 0.0, 1.0), n, true);
            assert!(
                out.y < -0.05,
                "{deg}°: projected {out:?} has no descent — the capsule leaves the surface"
            );
            assert!((out.length() - 1.0).abs() < 1e-4, "{deg}°: not unit length");
        }
    }

    #[test]
    fn ascending_a_slope_produces_upward_motion() {
        let out = slope_project(Vec3::new(0.0, 0.0, -1.0), normal_for(30.0), true);
        assert!(out.y > 0.05, "projected {out:?} does not climb");
    }

    /// Speed along the surface must be preserved, not reduced by cos θ — that
    /// reduction is exactly what made slopes feel sluggish.
    #[test]
    fn projection_preserves_speed_along_the_surface() {
        for deg in [10.0_f32, 25.0, 40.0, 49.0] {
            let out = slope_project(Vec3::new(0.0, 0.0, 1.0), normal_for(deg), true);
            assert!(
                (out.length() - 1.0).abs() < 1e-4,
                "{deg}°: speed along the slope is {} not 1.0",
                out.length()
            );
            // And it must lie IN the surface, not through it.
            assert!(
                out.dot(normal_for(deg)).abs() < 1e-4,
                "{deg}°: motion is not parallel to the ground plane"
            );
        }
    }

    /// The bug: a world-down seating bias has a component ALONG any sloped
    /// face, and collide-and-slide turns that into motion. Standing still on a
    /// ramp, the character slid downhill at a constant rate.
    #[test]
    fn the_seating_press_has_no_component_along_the_slope() {
        for deg in [0.0_f32, 10.0, 25.0, 40.0, 49.0] {
            let n = normal_for(deg);
            let seat = ground_seat(n);
            // Tangential part = what is left after removing the normal part.
            let tangential = seat - n * seat.dot(n);
            assert!(
                tangential.length() < 1e-5,
                "{deg}°: seating press has {} m/s along the face — that IS the slide",
                tangential.length()
            );
        }
    }

    /// And the old form demonstrably did not have that property, so the fix is
    /// not a no-op restatement.
    #[test]
    fn a_world_down_bias_would_slide_on_a_slope() {
        let n = normal_for(30.0);
        let world_down = Vec3::NEG_Y * 2.0;
        let tangential = world_down - n * world_down.dot(n);
        assert!(
            tangential.length() > 0.5,
            "world-down bias tangential component {} — expected a real slide",
            tangential.length()
        );
        // And it points DOWNHILL, which is the direction the character drifted.
        assert!(tangential.y < 0.0, "slide direction is not downhill: {tangential:?}");
    }

    #[test]
    fn the_seating_press_still_pushes_into_flat_ground() {
        let seat = ground_seat(Vec3::Y);
        assert!(seat.y < -1.0, "flat ground must still be pressed into: {seat:?}");
        assert!(seat.x.abs() < 1e-6 && seat.z.abs() < 1e-6);
    }

    #[test]
    fn a_degenerate_projection_falls_back_instead_of_producing_zero() {
        // Facing straight into a vertical face: the projection collapses.
        let n = Vec3::Z;
        let d = Vec3::Z;
        let out = slope_project(d, n, true);
        assert!(out.length() > 0.5, "degenerate projection produced {out:?}");
    }
}
