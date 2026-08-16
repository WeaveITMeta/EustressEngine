//! # Traversal — hang, shimmy, transfer, mantle, lower
//!
//! Assassin's-Creed / Uncharted-style movement on walls. This module owns the
//! **mechanic**: which grip the body is on, what the player can do from there,
//! and how the body moves between grips. [`super::grip`] owns *finding* grips;
//! [`super::ik`] owns *placing the limbs* once a grip is chosen.
//!
//! ## Why the hang is holdable
//!
//! The first version auto-advanced from hang to mantle after 0.25 s with no
//! player involvement, which made the whole climb a 1.3-second cutscene: of the
//! four transitions, only one was reachable by input. Nothing else can exist in
//! that design — shimmy, ledge jumps and cliff ascent all need a state the
//! player can *stay in*, because they are all things you do **while hanging**.
//!
//! So `Hanging` now terminates only on player action. That single change is
//! what the rest of this module is built on.
//!
//! ## Why the body is posed procedurally
//!
//! The shipped clip library is eight Mixamo files — idle, walk, run, jump, per
//! sex. There is no hang, no shimmy, no mantle, no roll. A traversal system
//! built on clips we do not have would be a stub; one built on transform
//! motion plus two-bone IK is a real, playable mechanic that looks plain until
//! clips exist. Every phase carries a [`ClimbPhase::clip_hint`] naming the clip
//! that should displace its procedural pose.
//!
//! **Honest limitation:** because the motion graph has no climb node and
//! `locomotion` reports `grounded = false` while climbing, the animation
//! blend currently drives the *jump* clip through every phase here. The pose
//! you see is the IK, fighting a clip that thinks it is falling. Fixing that
//! is a motion-graph change, not a change to this module.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::grip::{self, Grip, ProbeConfig};
use super::locomotion::{capsule_fits, unwedge};
use super::spawn::{AvatarBody, AvatarIntent, AvatarLocomotion};
use super::{AvatarSystems, SpawnedByAvatarRuntime};

/// Ledges below this (relative to the feet) are handled by the step-up in the
/// locomotion controller instead.
///
/// Sits just above the controller's 0.30 m `STEP_HEIGHT` so the two cover the
/// range between them with no gap. At the old 0.45 there was a dead band from
/// 0.30 to 0.45 — too tall to step, too short to grab — where the character
/// simply stopped against the obstacle with no way over it.
pub const MIN_LEDGE_HEIGHT: f32 = 0.32;

/// Grips at or below this height above the feet are VAULTED — mantled directly
/// with no hang.
///
/// Hanging from a low ledge is geometric nonsense: the drop is ~1.02 m on a
/// 1.75 m body, so hanging off a 0.50 m lip would place the body centre at
/// y = −0.52 and bury the character in the floor it is standing on. Waist-high
/// obstacles are things you pull yourself over, not things you dangle from.
const VAULT_MAX_FRAC: f32 = 0.62;
/// Highest reach, as a multiple of body height. ~1.3x puts a 1.75 m character's
/// limit around 2.3 m, which is roughly a human's max mantle.
pub const MAX_LEDGE_REACH: f32 = 1.30;
/// Furthest a grip may be from the body, as a multiple of body height.
///
/// A hard ceiling on EVERY grip, wherever it came from. Without it a probe that
/// happens to hit distant geometry hands back a hold metres away and the climb
/// snaps the body to it — reaching across open ground to a ledge the character
/// could not possibly touch. Roughly an arm plus a lean; anything further is
/// not a reach, it is a teleport.
const MAX_GRAB_DISTANCE_FRAC: f32 = 0.75;

/// Is this grip close enough to actually take hold of?
pub(crate) fn grip_within_reach(grip: &Grip, body_pos: Vec3, body: &AvatarBody) -> bool {
    let max = body.metrics.height_m * MAX_GRAB_DISTANCE_FRAC;
    grip.point.is_finite() && grip.point.distance(body_pos) <= max
}

/// How far ahead to look for a wall, as a multiple of capsule radius.
const WALL_PROBE_REACH: f32 = 2.2;
/// Seconds a deliberate drop suppresses re-grabbing for. Long enough to clear
/// the ledge under gravity, short enough not to feel like a lockout.
const REGRAB_LOCKOUT: f32 = 0.45;
/// Seconds the pull-up takes.
///
/// 0.65 s to clear a ~2 m ledge is roughly 3 m/s of vertical body movement —
/// faster than a person can pull their own mass, so it read as a teleport.
const MANTLE_DURATION: f32 = 1.05;
/// Lateral hand-over-hand speed, m/s.
const SHIMMY_SPEED: f32 = 1.15;
/// How far the extent scan looks along a lip, and its step.
const EXTENT_SCAN: f32 = 6.0;
const EXTENT_STEP: f32 = 0.35;
/// Seconds a grip-to-grip transfer takes.
const TRANSFER_DURATION: f32 = 0.42;

/// How long to wait before retrying a transfer that could not complete.
///
/// Long enough that a blocked corner reads as the character holding position,
/// rather than as a stutter.
const TRANSFER_RETRY_COOLDOWN: f32 = 0.35;

/// How long the hop onto a low block takes, in seconds.
///
/// Much faster than a pull-up. A vault is a stride taken at walking pace; drag
/// it out and the character appears to rise under its own power rather than
/// carry its momentum over the obstacle.
const VAULT_DURATION: f32 = 0.46;

/// How far above the landing the hop arcs, in metres. Small — this is a step
/// up, not a jump.
const VAULT_HOP_RISE: f32 = 0.13;

/// Fraction of run speed the character carries off the far side of a vault.
///
/// A vault used to end with the velocity zeroed, so clearing an ankle-high
/// block brought the character to a dead stop on top of it. The whole point of
/// vaulting rather than climbing is that the movement continues.
const VAULT_EXIT_SPEED_FRAC: f32 = 0.75;

/// Extra seconds per metre of reach, on top of [`TRANSFER_DURATION`].
///
/// A hand moving 20 cm to the next hold and a body crossing a metre and a half
/// of open air are not the same motion, and playing both over a fixed 0.42 s
/// makes the short one look laboured and the long one look like a teleport.
/// Cost per distance is what makes a leap read as committed.
const TRANSFER_SECONDS_PER_M: f32 = 0.30;
/// Seconds the step-off-and-hang takes.
const LOWER_DURATION: f32 = 0.55;
/// Fraction of full arm extension the body hangs at. Below ~0.7 the IK folds
/// the elbows out sideways and the pose reads as a flex rather than a hang;
/// above ~0.95 the chain hits the solver's straight-line clamp and locks.
const HANG_ARM_EXTENSION: f32 = 0.70;
/// Arm span (shoulder to wrist) as a fraction of standing height.
pub(crate) const ARM_SPAN_FRAC: f32 = 0.30;
/// Shoulder height above the body centre, as a fraction of standing height.
pub(crate) const SHOULDER_ABOVE_CENTRE_FRAC: f32 = 0.33;

/// What the avatar is doing with a wall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClimbPhase {
    #[default]
    None,
    /// Hands on a grip, body hanging. Held indefinitely until the player acts.
    Hanging,
    /// Moving hand-over-hand along the lip.
    Shimmy,
    /// Moving between two grips — up a face, down, or a sideways jump.
    Transfer,
    /// Pulling up and over onto a standable top — a HANG becoming a stand.
    Mantling,
    /// Hopping up onto a low block without ever hanging from it.
    ///
    /// Separate from [`ClimbPhase::Mantling`] because it is a different motion,
    /// not a shorter one. A pull-up starts from hanging with the arms bearing
    /// the whole body; stepping onto a knee-high block is a stride — one leg
    /// drives up and forward, the other trails, the arms swing for momentum and
    /// the walk continues off the far side. Running the pull-up pose over it is
    /// what made low blocks look like the character was floating upward with
    /// its knees doing something strange.
    Vaulting,
    /// Stepping backwards off a ledge from standing into a hang.
    Lowering,
}

impl ClimbPhase {
    /// The clip that should drive this phase once one exists. Nothing consumes
    /// this yet — it is the seam for authored animation, kept next to the
    /// state it belongs to so the two cannot drift apart.
    pub const fn clip_hint(self) -> Option<&'static str> {
        match self {
            ClimbPhase::None => None,
            ClimbPhase::Hanging => Some("hang_idle"),
            ClimbPhase::Shimmy => Some("hang_shimmy"),
            ClimbPhase::Transfer => Some("hang_reach"),
            ClimbPhase::Mantling => Some("mantle_up"),
            ClimbPhase::Vaulting => Some("vault_up"),
            ClimbPhase::Lowering => Some("ledge_lower"),
        }
    }

    /// True while the body is attached to a wall and the locomotion controller
    /// must stay out of the way.
    pub const fn is_attached(self) -> bool {
        !matches!(self, ClimbPhase::None)
    }
}

/// Per-avatar climb state.
#[derive(Component, Debug, Clone, Default)]
pub struct AvatarClimb {
    pub phase: ClimbPhase,
    /// The grip currently held.
    pub grip: Option<Grip>,
    /// Where a transfer is heading.
    pub target: Option<Grip>,
    /// How far the lip continues (left, right) along `grip.tangent`.
    pub extent: (f32, f32),
    /// Where each hand is holding, in world space — `[left, right]`.
    ///
    /// Two INDEPENDENT holds, not one grip with a shoulder-width offset. In
    /// Uncharted 4's debug footage the climber is almost never symmetric: one
    /// hand anchors while the other reaches to a different hold entirely, and
    /// the torso angles toward the reach. A single shared grip point cannot
    /// express that, and a body posed from it always reads as hanging rather
    /// than climbing.
    pub holds: [Vec3; 2],
    /// Which hand moves next, 0 = left. Reaches alternate, because a climber
    /// never releases both hands at once.
    pub reaching: usize,
    /// The surface normal each hand is holding against — `[left, right]`.
    ///
    /// Per-hand, because at a corner the two hands sit on faces pointing
    /// DIFFERENT ways, and the footage holds that pose for several frames with
    /// the torso yawing between them. Deriving body facing from a single grip
    /// normal cannot express it: the character would snap to one face while a
    /// hand is demonstrably on the other.
    pub hold_normals: [Vec3; 2],
    /// True while the free hand is searching — open and flat on the face
    /// rather than closed on a hold.
    pub searching: bool,
    /// Signed lateral travel, smoothed: −1 hard left, +1 hard right, 0 still.
    ///
    /// Drives the shimmy pose. A climber moving sideways is not a hanging
    /// climber who happens to be translating — the lead arm extends along the
    /// lip, the trail arm bends and pulls, and the legs cross. That is a
    /// different pose, and it has to know which way.
    pub travel: f32,
    /// Position in the hand-over-hand cycle, counted in strides.
    ///
    /// The body slides continuously along a lip but hands cannot: each is either
    /// planted or moving. Without this both hands were re-seated symmetrically
    /// every frame, which is why moving sideways read as the whole body gliding
    /// with the arms welded on.
    pub cycle: f32,
    /// Progress through the current phase: seconds for `Hanging`, 0..1 for the
    /// timed phases.
    pub t: f32,
    /// Seconds left before a ledge can be grabbed again.
    pub regrab_lockout: f32,
    /// Seconds left before another transfer may be attempted.
    ///
    /// A transfer that cannot complete used to drop straight back to hanging
    /// and be retried on the very next frame, because the corner probe still
    /// found the same hold. The body lurched a little way around the corner,
    /// snapped back to the hang position, and started again — several times a
    /// second. That is the juddering, frame-skipping pause at corners.
    pub transfer_cooldown: f32,
    /// Position the current timed phase started from.
    start: Vec3,
}

impl AvatarClimb {
    pub fn is_climbing(&self) -> bool {
        self.phase.is_attached()
    }
    /// World point the hands are holding, if any.
    pub fn grab_point(&self) -> Vec3 {
        self.grip.map(|g| g.point).unwrap_or(Vec3::ZERO)
    }
    /// Outward normal of the wall being held (points back at the climber).
    pub fn wall_normal(&self) -> Vec3 {
        self.grip.map(|g| g.normal).unwrap_or(Vec3::Z)
    }
    pub fn tangent(&self) -> Vec3 {
        self.grip.map(|g| g.tangent).unwrap_or(Vec3::X)
    }

    /// Seat both hands on a grip, shoulder-width apart along its lip. The
    /// starting configuration; reaches move them independently from here.
    pub fn seat_hands(&mut self, g: &Grip, half_width: f32) {
        self.holds = [
            g.point - g.tangent * half_width,
            g.point + g.tangent * half_width,
        ];
        self.hold_normals = [g.normal, g.normal];
    }

    /// Mean of the two hands' surface normals — the direction the body should
    /// actually face. On one flat wall this is just that wall's normal; across
    /// a corner it splits the difference, which is what lets the torso yaw
    /// between two faces instead of snapping to one.
    pub fn facing_normal(&self) -> Vec3 {
        let sum = self.hold_normals[0] + self.hold_normals[1];
        sum.with_y(0.0)
            .try_normalize()
            .unwrap_or_else(|| self.hold_normals[0].with_y(0.0).normalize_or(Vec3::Z))
    }

    /// Midpoint of the two holds — what the body hangs beneath.
    pub fn hold_centre(&self) -> Vec3 {
        (self.holds[0] + self.holds[1]) * 0.5
    }

    /// The frame the hands should solve to this frame, as
    /// `(point, normal, tangent)`.
    ///
    /// Across a transfer the hands **lead** the body: they interpolate to the
    /// new hold faster than the torso arcs to it, because a reach that moves
    /// hands and hips together reads as sliding rather than reaching.
    pub fn hand_frame(&self) -> Option<(Vec3, Vec3, Vec3)> {
        let g = self.grip?;
        match (self.phase, self.target) {
            (ClimbPhase::Transfer, Some(to)) => {
                let lead = (self.t.clamp(0.0, 1.0) * 1.35).min(1.0);
                Some((
                    g.point.lerp(to.point, lead),
                    g.normal.lerp(to.normal, lead).normalize_or(g.normal),
                    g.tangent.lerp(to.tangent, lead).normalize_or(g.tangent),
                ))
            }
            // Mantling: the palms press on the TOP surface, not the lip edge.
            //
            // During a pull-up the hands are flat on the ledge pushing down.
            // Targeting the edge left the arms hanging off the front while the
            // body rose past them, so the legs appeared to do all the work and
            // the arms did nothing — which is exactly what a vault should not
            // look like.
            (ClimbPhase::Mantling, _) => {
                // The palms hold the LIP through the pull, then move onto the
                // top once the body has risen to meet it.
                //
                // Targeting the top from t=0 asks for a point still out of
                // range and the chain locks straight; migrating too early
                // leaves the hands behind the rising torso. Holding until the
                // body is at lip height and transitioning over the back half
                // is the order a real pull-up happens in.
                let k = self.t.clamp(0.0, 1.0);
                let onto_top = ((k - 0.5) / 0.3).clamp(0.0, 1.0);
                let p = g.point.lerp(g.top - g.normal * 0.12, onto_top);
                Some((p, g.normal, g.tangent))
            }
            _ => Some((g.point, g.normal, g.tangent)),
        }
    }
}

/// How far the body's centre sits below the lip while hanging.
///
/// Derived from the arm, not picked by feel. The hands are pinned to the lip,
/// so the drop *is* what decides how extended the arms end up.
///
/// On a ledge barely taller than the character this legitimately leaves the
/// feet near the ground — that is what hanging off a chest-high wall looks
/// like, not a bug to clamp away.
pub(crate) fn hang_drop(body: &AvatarBody) -> f32 {
    let m = &body.metrics;
    m.height_m * ARM_SPAN_FRAC * HANG_ARM_EXTENSION + m.height_m * SHOULDER_ABOVE_CENTRE_FRAC
}

/// How far the body centre stands off the wall face while hanging, as a
/// multiple of capsule radius.
///
/// **Must exceed 1.0.** At 0.9 the capsule overlapped the wall by a tenth of a
/// radius, so the solver pushed the body out every frame while the hang lerp
/// pulled it back in — the two fought at ~14 Hz and the result was a visible
/// wiggle for the whole hang. The extra 0.08 is collision-skin clearance.
const HANG_STANDOFF: f32 = 1.08;

/// Exposed so the grip tests can assert the capsule clears the wall.
pub(crate) const fn hang_standoff() -> f32 {
    HANG_STANDOFF
}

/// Where the body centre sits for a given grip.
fn hang_pose(grip: &Grip, body: &AvatarBody) -> Vec3 {
    grip.point + grip.normal * (body.metrics.capsule_radius * HANG_STANDOFF)
        - Vec3::Y * hang_drop(body)
}

/// How far the body leans toward the ANCHORED hand while the other reaches, as
/// a fraction of the distance between the two holds.
///
/// Naughty Dog describe the root as part of the IK — "all four limbs and your
/// root and how they move together" — and the reason is weight. A climber
/// reaching with one arm shifts their mass over the arm still holding on; a
/// body that stays centred while one hand stretches away looks weightless,
/// because nothing appears to be carrying the load.
const ROOT_WEIGHT_SHIFT: f32 = 0.34;

/// Where the body hangs, given the two hands' actual holds.
///
/// This is the ROOT half of a full-body solve. [`hang_pose`] positions the body
/// from the grip — one point shared by both hands — which cannot express a
/// reach at all: the hands can move anywhere and the body never responds.
/// Hanging from the hold midpoint, biased toward whichever hand bears weight,
/// is what makes a reach look like it costs something.
pub(crate) fn hang_pose_from_holds(
    holds: &[Vec3; 2],
    anchored: usize,
    normal: Vec3,
    body: &AvatarBody,
) -> Vec3 {
    let centre = (holds[0] + holds[1]) * 0.5;
    let toward_anchor = (holds[anchored.min(1)] - centre) * ROOT_WEIGHT_SHIFT;
    centre
        + toward_anchor
        + normal * (body.metrics.capsule_radius * HANG_STANDOFF)
        - Vec3::Y * hang_drop(body)
}

/// How far the torso rolls to follow the hand line, in radians, clamped.
///
/// Frame analysis of Uncharted 4 puts the torso axis anywhere from ~8° to ~35°
/// off wall-vertical, swinging up to 35° between two half-second samples, with
/// the shoulder line tilting high on the anchored side. A body held perfectly
/// upright while the hands sit at different heights is the single most
/// mannequin-like thing a climb can do.
const MAX_TORSO_ROLL: f32 = 0.52;

/// Body rotation for a climb: face the wall, then ROLL so the shoulders follow
/// the line between the two hands.
pub(crate) fn climb_body_rotation(grip: &Grip, holds: &[Vec3; 2]) -> Quat {
    climb_body_rotation_with(grip.normal, grip.tangent, holds)
}

/// As above, but taking the facing normal explicitly so a corner pose can pass
/// the mean of the two hands' normals rather than one grip's.
pub(crate) fn climb_body_rotation_with(normal: Vec3, tangent: Vec3, holds: &[Vec3; 2]) -> Quat {
    let n = normal.with_y(0.0).normalize_or(Vec3::Z);
    let into = -n;
    let facing = Quat::from_rotation_y((-into.x).atan2(-into.z));
    let grip = Grip {
        point: Vec3::ZERO,
        normal: n,
        tangent,
        top: Vec3::ZERO,
        standable: false,
        kind: crate::avatar::grip::GripKind::Ledge,
    };
    let _ = &grip;
    let span = holds[1] - holds[0];
    let lateral = span.dot(tangent);
    let rise = span.y;
    if lateral.abs() < 1e-3 && rise.abs() < 1e-3 {
        return facing;
    }
    // Positive rise on the +tangent side means the right hand is higher, so the
    // torso rolls that way. Roll is about the wall normal — the axis the body
    // actually pivots on when hanging off a face.
    let roll = rise.atan2(lateral.abs().max(0.12)).clamp(-MAX_TORSO_ROLL, MAX_TORSO_ROLL);
    Quat::from_axis_angle(n, -roll) * facing
}

/// Yaw that faces into the wall.
fn face_wall(grip: &Grip) -> Quat {
    let into = -grip.normal.with_y(0.0).normalize_or(Vec3::NEG_Z);
    Quat::from_rotation_y((-into.x).atan2(-into.z))
}

pub(crate) fn probe_config_for(body: &AvatarBody, feet_y: f32) -> ProbeConfig {
    probe_config(body, feet_y)
}

fn probe_config(body: &AvatarBody, feet_y: f32) -> ProbeConfig {
    let m = &body.metrics;
    ProbeConfig {
        feet_y,
        body_height: m.height_m,
        capsule_radius: m.capsule_radius,
        capsule_cylinder_len: m.capsule_cylinder_len,
        reach_min: MIN_LEDGE_HEIGHT,
        reach_max: m.height_m * MAX_LEDGE_REACH,
        forward_reach: m.capsule_radius * WALL_PROBE_REACH,
    }
}

pub(crate) struct AvatarClimbPlugin;

impl Plugin for AvatarClimbPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, attach_climb_state.in_set(AvatarSystems::Lifecycle))
            // Before locomotion: while attached, the controller must not also
            // be driving the body, or the two fight over position.
            .add_systems(
                Update,
                drive_climb
                    .in_set(AvatarSystems::Locomotion)
                    .before(super::locomotion::drive_locomotion),
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

/// Half-angle of the acquisition fan, radians. A single ray demands the player
/// be aimed almost exactly at the wall; ±22° is the difference between
/// "climbing is unreliable" and "climbing works".
const GRAB_FAN: f32 = 0.38;

/// Look for a ledge in front of the character.
///
/// Fans three probes rather than casting one ray. Walls are rarely square to
/// the direction you are holding, and a lip found only when perfectly aligned
/// reads as the mechanic randomly failing.
pub fn detect_ledge(
    spatial: &SpatialQuery,
    origin: Vec3,
    forward: Vec3,
    body: &AvatarBody,
    exclude: Entity,
) -> Option<Grip> {
    let filter = SpatialQueryFilter::default().with_excluded_entities([exclude]);
    let feet = origin.y - body.metrics.capsule_half_extent();
    let cfg = probe_config(body, feet);
    let fwd = forward.with_y(0.0).normalize_or_zero();
    if fwd.length_squared() < 1e-6 {
        return None;
    }

    let mut best: Option<Grip> = None;
    for angle in [0.0, -GRAB_FAN, GRAB_FAN] {
        let d = Quat::from_rotation_y(angle) * fwd;
        if let Some(g) = grip::probe(spatial, &filter, origin, d, &cfg) {
            // Prefer the lip most square to the direction being held, so a
            // wall you are facing wins over one you are merely beside.
            let score = (-g.normal).dot(fwd);
            if best.map_or(true, |b| score > (-b.normal).dot(fwd)) {
                best = Some(g);
            }
        }
    }
    best
}

/// Lateral component of the player's intent, in the grip's frame.
///
/// Positive is along `+tangent`. Uses the *wall's* frame rather than the
/// camera's so that shimmy direction stays stable as the body rotates to face
/// the wall — camera-relative input flips sign mid-shimmy on a curved wall.
fn lateral_intent(intent: &AvatarIntent, grip: &Grip) -> f32 {
    intent.direction.with_y(0.0).dot(grip.tangent)
}

/// Below this a lateral push is treated as noise rather than a shimmy.
const LATERAL_DEADZONE: f32 = 0.35;

/// What the player is asking for while hanging.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum HangAction {
    /// Stay on the grip. This is what NEUTRAL INPUT means, and the reason the
    /// hang is holdable: elapsed time is not an input to this decision, so
    /// doing nothing hangs forever.
    Hold,
    /// Let go deliberately.
    Drop,
    /// Up — mantle if the grip is standable, else reach for a higher one.
    Up,
    /// Travel along the lip; the sign is the direction along `tangent`.
    Shimmy(f32),
    /// Asked to travel but out of lip that way; try to turn the corner.
    Corner(f32),
    /// Push off the wall — jump while steering away from the face.
    WallJump,
}

/// How much of the kick goes OUT from the wall versus UP.
///
/// Weighted toward out: a wall kick that mostly goes up is just a jump with
/// extra steps. Pushing away is what carries momentum across a gap, which is
/// the whole point of using the wall as a surface rather than a destination.
const WALL_KICK_OUT: f32 = 1.15;
const WALL_KICK_UP: f32 = 0.85;

/// Velocity for a kick off a wall whose outward normal is `normal`.
///
/// The player's steer is blended with the wall normal rather than replacing
/// it: pushing off a wall can only ever send you away from it, but *which*
/// away is the player's to choose, which is what makes it a traversal move
/// instead of a bounce.
pub fn wall_jump_velocity(normal: Vec3, steer: Vec3, jump_speed: f32) -> Vec3 {
    let out = normal.with_y(0.0).normalize_or(Vec3::Z);
    let steer = steer.with_y(0.0).normalize_or_zero();
    // Never let steer flip the result back into the wall.
    let blended = (out + steer * 0.6).normalize_or(out);
    let away = if blended.dot(out) > 0.1 { blended } else { out };
    away * (jump_speed * WALL_KICK_OUT) + Vec3::Y * (jump_speed * WALL_KICK_UP)
}

/// The hang decision, extracted from the system so it is testable without an
/// ECS world, physics, or a running App — none of which the previous
/// source-grep "test" actually exercised.
pub(crate) fn hang_action(intent: &AvatarIntent, grip: &Grip, extent: (f32, f32)) -> HangAction {
    if intent.crouch {
        return HangAction::Drop;
    }
    if intent.jump_pressed {
        // Steering away from the face turns the jump into a push-off.
        let away = intent.direction.with_y(0.0).normalize_or_zero().dot(grip.normal);
        return if away > 0.45 { HangAction::WallJump } else { HangAction::Up };
    }
    let lat = lateral_intent(intent, grip);
    if lat.abs() > LATERAL_DEADZONE {
        let room = if lat > 0.0 { extent.1 } else { extent.0 };
        return if room > EXTENT_STEP {
            HangAction::Shimmy(lat.signum())
        } else {
            HangAction::Corner(lat.signum())
        };
    }
    HangAction::Hold
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
        let filter = SpatialQueryFilter::default().with_excluded_entities([entity]);
        let half = body.metrics.capsule_half_extent();
        // Slightly under body size. A hang deliberately parks the capsule close
        // to the wall it is holding, so a full-width probe would report contact
        // every frame and refuse to move at all; this clears grazing contact
        // while still catching anything the body would actually be inside of.
        let clear = clearance_shape(body);

        match climb.phase {
            ClimbPhase::None => {
                climb.transfer_cooldown = (climb.transfer_cooldown - dt).max(0.0);
                if climb.regrab_lockout > 0.0 {
                    climb.regrab_lockout -= dt;
                    continue;
                }

                // Deliberate step-off: grounded, crouching, walking at an edge.
                if loco.grounded && intent.crouch && intent.direction.length_squared() > 1e-4 {
                    if let Some(g) = probe_ledge_below(
                        &spatial,
                        &filter,
                        tf.translation - Vec3::Y * half,
                        intent.direction.normalize_or_zero(),
                        body,
                    ) {
                        begin_lower(&mut climb, &mut tf, &mut vel, g);
                        continue;
                    }
                }

                // Grab from the GROUND as well as the air.
                //
                // Requiring `!grounded` meant every climb began with a
                // correctly-timed jump into a wall. That is a timing puzzle,
                // not a traversal verb: the probe samples the feet position,
                // which is sweeping upward during a jump, so whether the lip
                // landed inside the reach band depended on which frame the
                // detector happened to run. Walking into a chest-high ledge
                // and taking hold of it is the ordinary case and now works.
                //
                // A grounded grab still requires a lip ABOVE step-up height,
                // so ordinary walking over kerbs is untouched.
                if intent.direction.length_squared() < 1e-4 {
                    continue;
                }
                let forward = intent.direction.normalize_or_zero();
                let Some(g) = detect_ledge(&spatial, tf.translation, forward, body, entity) else {
                    continue;
                };
                // Never take a hold the body could not physically reach.
                if !grip_within_reach(&g, tf.translation, body) {
                    continue;
                }
                // Low and standable: vault straight over it. Only tall grips
                // are worth hanging from, and only a standable top can be
                // vaulted onto — a low lip with nothing to land on is still a
                // hang, however short.
                let feet = tf.translation.y - half;
                if g.height_above(feet) <= body.metrics.height_m * VAULT_MAX_FRAC
                    && can_mantle(&spatial, &filter, &g, body, face_wall(&g))
                {
                    climb.grip = Some(g);
                    climb.seat_hands(&g, body.metrics.shoulder_half_width.max(0.10));
                    // FACE THE WALL before posing against it.
                    //
                    // The authored climb pose is expressed in the body's own
                    // frame, so it only means "arms toward the ledge" if the
                    // body is pointed at the ledge. A vault entered straight
                    // from walking kept whatever heading the character had,
                    // and the arms came out aligned to that instead — which
                    // reads as them facing a fixed axis, or square to the wall.
                    // Heading is NOT forced square to the block — see the
                    // Vaulting arm. A stride keeps the direction you are
                    // travelling in.
                    //
                    // Momentum is NOT cleared either. A vault carries the walk
                    // over the block; the phase converts it, not stops it.
                    begin_vault(&mut climb, &tf);
                    info!("🧗 vault over {:?} ({:.2} m)", g.point, g.height_above(feet));
                    continue;
                }

                // No room to hang: vault it if we can, otherwise refuse. A
                // grab that buries the character is worse than no grab.
                if !has_hang_clearance(&spatial, &filter, &g, body) {
                    if can_mantle(&spatial, &filter, &g, body, face_wall(&g)) {
                        climb.grip = Some(g);
                        tf.rotation = face_wall(&g);
                        // Low enough to stride over, or high enough that it has
                        // to be pulled? Same decision as above, and it has to
                        // be made here too — this branch is reached by anything
                        // with no room to hang, at any height.
                        if g.height_above(tf.translation.y - half)
                            <= body.metrics.height_m * VAULT_MAX_FRAC
                        {
                            begin_vault(&mut climb, &tf);
                        } else {
                            begin_mantle(&mut climb, &tf);
                            vel.0 = Vec3::ZERO;
                        }
                    }
                    continue;
                }

                begin_hang(&mut climb, &spatial, &filter, &mut vel, g, body);
                info!("🧗 grip at {:?} (standable: {})", g.point, g.standable);
            }

            ClimbPhase::Hanging => {
                let Some(g) = climb.grip else {
                    climb.phase = ClimbPhase::None;
                    continue;
                };
                vel.0 = Vec3::ZERO;
                // Relax out of the sideways lean rather than snapping upright.
                climb.travel *= 1.0 - (6.0 * dt).min(1.0);
                let anchored = 1 - climb.reaching;
                let want = hang_pose_from_holds(&climb.holds, anchored, g.normal, body);
                settle_to(
                    &mut tf,
                    want,
                    climb_body_rotation_with(climb.facing_normal(), g.tangent, &climb.holds),
                    dt,
                    &spatial,
                    &clear,
                    &filter,
                    body.metrics.capsule_radius,
                );
                climb.t += dt;

                // ── The free hand SEARCHES ──────────────────────────────────
                //
                // In the reference footage the two hands are almost never in
                // the same state: one is closed on a hold bearing load, the
                // other is open and flat against the face, moving where the
                // player is steering. Ours had both hands parked symmetrically
                // on the lip, which is why a hang looked static no matter what
                // the player did.
                //
                // The reaching hand tracks input continuously — below the
                // shimmy threshold this is just a reach, and it reads as the
                // character looking for the next hold.
                let steer = intent.direction.with_y(0.0);
                climb.searching = steer.length_squared() > 0.0025;
                {
                    let free = climb.reaching;
                    let hw = body.metrics.shoulder_half_width.max(0.10);
                    let side = if free == 0 { -1.0 } else { 1.0 };
                    let settled = g.point + g.tangent * (side * hw);

                    let want = if climb.searching {
                        let reach = body.metrics.height_m * ARM_SPAN_FRAC * 0.85;
                        let along = steer.dot(g.tangent).clamp(-1.0, 1.0);
                        // Pushing INTO the wall reads as reaching upward — you
                        // cannot go through it, so that is where the hand goes.
                        let up = steer.dot(-g.normal).clamp(0.0, 1.0);
                        settled + g.tangent * (along * reach) + Vec3::Y * (up * reach * 0.75)
                    } else {
                        settled
                    };
                    climb.holds[free] = climb.holds[free].lerp(want, (9.0 * dt).min(1.0));
                    climb.hold_normals[free] = g.normal;
                }

                match hang_action(intent, &g, climb.extent) {
                    HangAction::Hold => {}

                    // The lockout is what makes this a drop rather than a
                    // re-grab: without it the `None` arm re-acquires the same
                    // lip next frame while a direction key is held.
                    HangAction::Drop => {
                        // Catch the next hold down if there is one. Releasing
                        // is the fallback, not the default: a ledge above
                        // another ledge should be climbed down, not fallen off.
                        if let Some(below) = probe_below(&spatial, &filter, &g, body) {
                            info!("🧗 lowering onto {:?}", below.point);
                            begin_lower(&mut climb, &mut tf, &mut vel, below);
                        } else {
                            climb.phase = ClimbPhase::None;
                            climb.grip = None;
                            climb.regrab_lockout = REGRAB_LOCKOUT;
                        }
                    }

                    // Mantle if there is somewhere to stand, otherwise reach
                    // for a higher grip. This is the cliff-ascent verb.
                    HangAction::Up => {
                        if can_mantle(&spatial, &filter, &g, body, tf.rotation) {
                            begin_mantle(&mut climb, &tf);
                        } else if let Some(up) =
                            probe_next_grip(&spatial, &filter, &g, body, Vec3::Y)
                        {
                            begin_transfer(&mut climb, &tf, up);
                        } else if g.standable {
                            // There IS a top — something is sitting on it. A
                            // ceiling, a pipe, an overhang. Keep hanging.
                            //
                            // Distinct from the dead end below: that is a lip
                            // with nowhere to go, where leaving is the only
                            // move. This one has an exit that is merely
                            // blocked, and kicking off it every time the player
                            // asks to go up turns a blocked top-out into an
                            // endless bounce off the wall.
                            debug!("🧗 top-out obstructed, holding the hang");
                        } else {
                            // Nowhere to stand and nothing higher to reach.
                            //
                            // This used to do NOTHING, silently — the player
                            // pressed jump and the character just hung there,
                            // which is indistinguishable from the input being
                            // broken. It is reachable on any lip whose top is
                            // too steep to stand on, which the course is full
                            // of. Kicking off is the honest answer: the wall
                            // has been established as a dead end, so leaving it
                            // is the only move left.
                            let launch = wall_jump_velocity(
                                g.normal,
                                intent.direction,
                                body.motion.jump_velocity(),
                            );
                            climb.phase = ClimbPhase::None;
                            climb.grip = None;
                            climb.regrab_lockout = REGRAB_LOCKOUT;
                            vel.0 = launch;
                            info!("🧗 dead-end grip — kicking off instead of hanging");
                        }
                    }

                    // Kick off the wall. Holding a direction that points AWAY
                    // from the face turns jump from "climb it" into "leave
                    // it" — the parkour verb, where the wall is a surface to
                    // push against rather than an obstacle to surmount.
                    HangAction::WallJump => {
                        let launch = wall_jump_velocity(
                            g.normal,
                            intent.direction,
                            body.motion.jump_velocity(),
                        );
                        climb.phase = ClimbPhase::None;
                        climb.grip = None;
                        climb.regrab_lockout = REGRAB_LOCKOUT;
                        vel.0 = launch;
                        info!("🧗 wall kick at {:.1} m/s", launch.length());
                    }

                    HangAction::Shimmy(_) => climb.phase = ClimbPhase::Shimmy,

                    // Out of lip — try to turn the corner rather than stop dead.
                    HangAction::Corner(sign) => {
                        climb.transfer_cooldown = (climb.transfer_cooldown - dt).max(0.0);
                        if climb.transfer_cooldown > 0.0 {
                            continue;
                        }
                        // Try the corner placement first — it is the one that
                        // can actually see a perpendicular face — then fall
                        // back to the general lateral probe for a wall that
                        // simply continues past a gap.
                        let side = g.tangent * sign;
                        let next = probe_corner(&spatial, &filter, &g, body, sign)
                            .or_else(|| probe_next_grip(&spatial, &filter, &g, body, side))
                            // Last: LEAP the gap. Reaching around a corner and
                            // reaching along a continuing wall are both static
                            // moves; when neither finds anything the lip has
                            // genuinely ended, and jumping is the verb.
                            .or_else(|| probe_leap(&spatial, &filter, &g, body, side));
                        if let Some(next) = next {
                            begin_transfer(&mut climb, &tf, next);
                        }
                    }
                }
            }

            ClimbPhase::Shimmy => {
                let Some(mut g) = climb.grip else {
                    climb.phase = ClimbPhase::None;
                    continue;
                };
                vel.0 = Vec3::ZERO;

                let lat = lateral_intent(intent, &g);
                if lat.abs() <= 0.35 || intent.crouch {
                    climb.phase = ClimbPhase::Hanging;
                    continue;
                }

                let dir = lat.signum();
                let room = if dir > 0.0 { climb.extent.1 } else { climb.extent.0 };
                let step = SHIMMY_SPEED * dt;
                if room <= step {
                    climb.phase = ClimbPhase::Hanging;
                    continue;
                }

                // Slide the grip along the lip, then re-probe from the new spot
                // so the hands follow real geometry rather than an extrapolated
                // straight line — walls are not required to be flat.
                let moved = g.point + g.tangent * (dir * step);
                let cfg = probe_config(body, moved.y - hang_drop(body) - half);
                let stand_off = moved + g.normal * (body.metrics.capsule_radius * 1.5);
                if let Some(re) = grip::probe(&spatial, &filter, stand_off, -g.normal, &cfg) {
                    g = re;
                } else {
                    g.point = moved;
                }
                climb.grip = Some(g);

                // HAND OVER HAND, not a symmetric slide.
                //
                // The grip point slides continuously, but the hands take turns:
                // the one on the leading side reaches out along the lip while
                // the other holds, then they swap. `seat_hands` put both at a
                // fixed shoulder offset every frame, which is exactly why
                // shimmying had no motion in it beyond the body translating.
                climb.cycle += step / SHIMMY_STRIDE;
                climb.travel = climb.travel + (dir - climb.travel) * (8.0 * dt).min(1.0);
                let hw = body.metrics.shoulder_half_width.max(0.10);
                let lead = usize::from(dir > 0.0);
                for hand in 0..2 {
                    let lag = shimmy_hand_lead(climb.cycle, hand == lead);
                    let side = if hand == 0 { -1.0 } else { 1.0 };
                    climb.holds[hand] =
                        g.point + g.tangent * (side * hw + dir * lag * SHIMMY_STRIDE);
                    climb.hold_normals[hand] = g.normal;
                }
                // The hand currently swinging is the one that is open.
                climb.searching = true;
                climb.reaching = lead;

                climb.extent = grip::edge_extent(
                    &spatial, &filter, &g, &probe_config(body, g.point.y - hang_drop(body) - half),
                    EXTENT_SCAN, EXTENT_STEP,
                );
                let anchored = 1 - climb.reaching;
                let want = hang_pose_from_holds(&climb.holds, anchored, g.normal, body);
                if !settle_to(
                    &mut tf,
                    want,
                    climb_body_rotation_with(climb.facing_normal(), g.tangent, &climb.holds),
                    dt,
                    &spatial,
                    &clear,
                    &filter,
                    body.metrics.capsule_radius,
                ) {
                    // Shimmied into something. Stop at the obstruction and hang
                    // there instead of grinding the body along it.
                    climb.phase = ClimbPhase::Hanging;
                }
            }

            ClimbPhase::Transfer => {
                let (Some(from), Some(to)) = (climb.grip, climb.target) else {
                    climb.phase = ClimbPhase::Hanging;
                    continue;
                };
                vel.0 = Vec3::ZERO;
                let span = hang_pose(&from, body).distance(hang_pose(&to, body));
                climb.t += dt / (TRANSFER_DURATION + span * TRANSFER_SECONDS_PER_M);
                let k = climb.t.clamp(0.0, 1.0);

                // Arc through the midpoint lifted by a fraction of the rise, so
                // the body swings to the new hold instead of sliding along the
                // wall through whatever is between the two.
                let a = hang_pose(&from, body);
                let b = hang_pose(&to, body);
                // Arc height scales with the crossing too: a short reach stays
                // flat along the wall, a leap throws the body up and over.
                let lift = ((b.y - a.y).abs().max(0.25) + span * 0.30) * 0.35;

                // SWING AROUND THE CORNER, not through it.
                //
                // At an outer corner the two holds sit on faces pointing
                // different ways, and the straight line between them passes
                // through the solid corner itself. Before collisions existed
                // the body clipped the edge; now the move is blocked instead,
                // which is worse — the transition simply fails.
                //
                // Pushing the midpoint out along the average of the two face
                // normals traces the OUTSIDE of the corner. Scaled by how much
                // the faces disagree, so a flat wall is untouched (both normals
                // are the same, the term goes to zero) and a right-angle corner
                // gets better than a body radius of clearance.
                let out = (from.normal + to.normal).normalize_or(from.normal);
                let disagree = (1.0 - from.normal.dot(to.normal)).clamp(0.0, 1.0);
                let mid = (a + b) * 0.5
                    + Vec3::Y * lift
                    + out * (body.metrics.capsule_radius * 2.6 * disagree);

                let arc = quadratic(a, mid, b, ease_in_out(k));
                // A blocked frame is NOT a failed transfer. `place_body` stops
                // the body against whatever it met and the swing keeps running;
                // only arriving nowhere counts as failure, checked at the end.
                let _ = place_body(&mut tf, arc, &spatial, &clear, &filter, body.metrics.capsule_radius);
                tf.rotation = tf.rotation.slerp(face_wall(&to), (12.0 * dt).min(1.0));

                // Move only the REACHING hand toward the new hold. The other
                // stays where it is, which is what makes a transfer read as a
                // reach rather than the whole body sliding sideways.
                let reach_k = (k * 1.35).min(1.0);
                let hw = body.metrics.shoulder_half_width.max(0.10);
                let reaching = climb.reaching;
                let side = if reaching == 0 { -1.0 } else { 1.0 };
                let dest = to.point + to.tangent * (side * hw);
                let from_hold = from.point + from.tangent * (side * hw);
                climb.holds[reaching] = from_hold.lerp(dest, ease_in_out(reach_k));

                if k >= 1.0 {
                    // Did the body actually get there? If an obstruction held
                    // it up the whole way, the hold in hand is still the old
                    // one, and pretending otherwise strands the character on a
                    // grip its hands are nowhere near.
                    if tf.translation.distance(b) > body.metrics.capsule_radius * 2.0 {
                        debug!("🧗 transfer blocked the whole way, staying put");
                        climb.target = None;
                        climb.phase = ClimbPhase::Hanging;
                        climb.t = 0.0;
                        climb.transfer_cooldown = TRANSFER_RETRY_COOLDOWN;
                        climb.seat_hands(&from, body.metrics.shoulder_half_width.max(0.10));
                        continue;
                    }
                    climb.grip = Some(to);
                    climb.target = None;
                    // The anchored hand catches up to the new grip, and the
                    // next reach uses the other arm.
                    let other = 1 - reaching;
                    let os = if other == 0 { -1.0 } else { 1.0 };
                    climb.holds[other] = to.point + to.tangent * (os * hw);
                    climb.reaching = other;
                    climb.extent = grip::edge_extent(
                        &spatial, &filter, &to,
                        &probe_config(body, to.point.y - hang_drop(body) - half),
                        EXTENT_SCAN, EXTENT_STEP,
                    );
                    climb.phase = ClimbPhase::Hanging;
                    climb.t = 0.0;
                }
            }

            ClimbPhase::Lowering => {
                let Some(g) = climb.grip else {
                    climb.phase = ClimbPhase::None;
                    continue;
                };
                vel.0 = Vec3::ZERO;
                climb.t += dt / LOWER_DURATION;
                let k = climb.t.clamp(0.0, 1.0);
                let down = climb.start.lerp(hang_pose(&g, body), ease_in_out(k));
                if !place_body(&mut tf, down, &spatial, &clear, &filter, body.metrics.capsule_radius) {
                    // The hang position is occupied — most often the ledge is
                    // low enough that the drop would end inside the floor.
                    debug!("🧗 lower blocked, releasing");
                    climb.phase = ClimbPhase::None;
                    climb.regrab_lockout = REGRAB_LOCKOUT;
                    continue;
                }
                tf.rotation = tf.rotation.slerp(face_wall(&g), (10.0 * dt).min(1.0));
                if k >= 1.0 {
                    climb.phase = ClimbPhase::Hanging;
                    climb.t = 0.0;
                }
            }

            ClimbPhase::Vaulting => {
                let Some(g) = climb.grip else {
                    climb.phase = ClimbPhase::None;
                    continue;
                };
                climb.t += dt / VAULT_DURATION;
                let k = climb.t.clamp(0.0, 1.0);

                // Face the way we are GOING — not square to the block.
                //
                // A vault is a stride, and a stride is aimed along your travel.
                // Snapping square to the wall makes the authored swing follow
                // the BLOCK's facing instead of the player's heading, so the
                // arms appear to swing along one fixed world direction however
                // you approach. Hanging is different: there the wall really is
                // what the body is oriented to.
                //
                // With no input, hold the heading rather than turning to the
                // wall, so releasing the key mid-stride does not snap the body.
                let travel_dir = intent.direction.with_y(0.0).normalize_or_zero();
                if travel_dir.length_squared() > 1e-4 {
                    let want =
                        Quat::from_rotation_y((-travel_dir.x).atan2(-travel_dir.z));
                    tf.rotation = tf.rotation.slerp(want, (14.0 * dt).min(1.0));
                }

                let top = g.top + Vec3::Y * (half + 0.02);
                // A HOP, not a lift.
                //
                // The mantle raises the body vertically and only then moves it
                // forward, which is right for hauling yourself out of a hang
                // and completely wrong here — it is what reads as floating up
                // the face of a knee-high block. A stride goes up and forward
                // together and arcs slightly OVER the landing, so the foot
                // comes down onto the top rather than rising to meet it.
                let along = climb.start.lerp(top, ease_in_out(k));
                // Parabola peaking mid-stride and exactly zero at both ends, so
                // the landing is on the surface and not above it.
                let arc = VAULT_HOP_RISE * 4.0 * k * (1.0 - k);
                let hop = Vec3::new(along.x, along.y + arc, along.z);
                let _ = place_body(
                    &mut tf, hop, &spatial, &clear, &filter, body.metrics.capsule_radius,
                );

                // Keep the body moving through the vault instead of parking it.
                // Velocity is what the locomotion controller reads the instant
                // this phase ends, and a zero there is a dead stop on the lip.
                let forward = -g.normal;
                vel.0 = forward * (body.motion.run_speed * VAULT_EXIT_SPEED_FRAC);

                if k >= 1.0 {
                    let _ = place_body(
                        &mut tf, top, &spatial, &clear, &filter, body.metrics.capsule_radius,
                    );
                    climb.phase = ClimbPhase::None;
                    climb.grip = None;
                    climb.t = 0.0;
                    // Hand back to locomotion still walking. Without this the
                    // character lands and stands there until the player lets go
                    // of the key and presses it again.
                    vel.0 = forward * (body.motion.run_speed * VAULT_EXIT_SPEED_FRAC);
                }
            }

            ClimbPhase::Mantling => {
                vel.0 = Vec3::ZERO;
                let Some(g) = climb.grip else {
                    climb.phase = ClimbPhase::None;
                    continue;
                };
                // Hold the wall facing for the whole pull-up. Only `Hanging`
                // maintained it, so a mantle could drift off-axis part-way
                // through and take the authored arm directions with it.
                tf.rotation = tf.rotation.slerp(face_wall(&g), (10.0 * dt).min(1.0));
                climb.t += dt / MANTLE_DURATION;

                // Up first, then forward. A straight lerp to the top clips the
                // character through the ledge corner; going vertical before
                // horizontal traces the shape of the obstacle.
                let k = climb.t.clamp(0.0, 1.0);
                let up_phase = (k / 0.6).clamp(0.0, 1.0);
                let fwd_phase = ((k - 0.4) / 0.6).clamp(0.0, 1.0);

                let top = g.top + Vec3::Y * (half + 0.02);
                // `ease_in_out`, not `ease_out`.
                //
                // `ease_out` is heavily front-loaded: 30% through the move the
                // body was already 75% of the way up, while the hands were
                // still holding the lip below it. The arms stretched down to a
                // ledge the torso had already passed, which reads as a dive
                // rather than a pull-up. A symmetric curve keeps the body and
                // the hands in the same part of the motion.
                let lifted = Vec3::new(
                    climb.start.x,
                    climb.start.y + (top.y - climb.start.y) * ease_in_out(up_phase),
                    climb.start.z,
                );
                let up = lifted.lerp(
                    Vec3::new(top.x, lifted.y.max(top.y), top.z),
                    ease_in_out(fwd_phase),
                );
                if !place_body(&mut tf, up, &spatial, &clear, &filter, body.metrics.capsule_radius) {
                    // A ceiling or an overhang above the lip. Give the ledge
                    // back rather than pulling the body up through it.
                    debug!("🧗 mantle blocked, back to hang");
                    climb.phase = ClimbPhase::Hanging;
                    climb.t = 0.0;
                    continue;
                }

                if k >= 1.0 {
                    let _ = place_body(&mut tf, top, &spatial, &clear, &filter, body.metrics.capsule_radius);
                    climb.phase = ClimbPhase::None;
                    climb.grip = None;
                    climb.t = 0.0;
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Transitions
// ─────────────────────────────────────────────────────────────────────────────

fn begin_hang(
    climb: &mut AvatarClimb,
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    vel: &mut LinearVelocity,
    g: Grip,
    body: &AvatarBody,
) {
    let half = body.metrics.capsule_half_extent();
    climb.phase = ClimbPhase::Hanging;
    climb.grip = Some(g);
    climb.seat_hands(&g, body.metrics.shoulder_half_width.max(0.10));
    climb.t = 0.0;
    climb.extent = grip::edge_extent(
        spatial,
        filter,
        &g,
        &probe_config(body, g.point.y - hang_drop(body) - half),
        EXTENT_SCAN,
        EXTENT_STEP,
    );
    vel.0 = Vec3::ZERO;
}

fn begin_vault(climb: &mut AvatarClimb, tf: &Transform) {
    climb.phase = ClimbPhase::Vaulting;
    climb.t = 0.0;
    climb.start = tf.translation;
}

fn begin_mantle(climb: &mut AvatarClimb, tf: &Transform) {
    climb.phase = ClimbPhase::Mantling;
    climb.t = 0.0;
    climb.start = tf.translation;
}

fn begin_transfer(climb: &mut AvatarClimb, tf: &Transform, to: Grip) {
    climb.phase = ClimbPhase::Transfer;
    climb.target = Some(to);
    climb.t = 0.0;
    climb.start = tf.translation;
}

fn begin_lower(climb: &mut AvatarClimb, tf: &mut Transform, vel: &mut LinearVelocity, g: Grip) {
    climb.phase = ClimbPhase::Lowering;
    climb.grip = Some(g);
    climb.t = 0.0;
    climb.start = tf.translation;
    vel.0 = Vec3::ZERO;
}

/// Is there room to actually hang from this grip without clipping into
/// whatever is underneath it?
///
/// A hang drops the body ~1.02 m below the lip on a 1.75 m character, plus a
/// capsule half-extent of clearance beneath that. Taking hold of a lip with
/// less space than that below buries the character in the floor — which is
/// exactly what stepping off a knee-high block did: the lower path found the
/// lip it was standing on, dropped the body a metre, and put it underground.
///
/// Checked against real geometry rather than assumed from the grip height,
/// because "how tall is this part" and "what is under it" are different
/// questions — a ledge 3 m up with a floor 0.2 m below it is just as bad.
fn has_hang_clearance(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    g: &Grip,
    body: &AvatarBody,
) -> bool {
    let m = &body.metrics;
    let pose = hang_pose(g, body);
    if !pose.is_finite() {
        return false;
    }

    // Test the CAPSULE, at the position it will actually occupy.
    //
    // The first version cast a ray downward from under the lip, which answers
    // "is anything below" — a different question, and one that says "clear"
    // for the emptiest possible case. Past the edge of the world the ray hit
    // nothing, so grabbing the rim of the ground plate reported clearance and
    // dropped the body straight through the floor.
    //
    // A shape test at the destination cannot be fooled that way: it asks
    // whether the body fits where it is going, which is the actual question.
    let blocked = spatial
        .cast_shape(
            &clearance_shape(body),
            pose,
            Quat::IDENTITY,
            Dir3::Y,
            &ShapeCastConfig::from_max_distance(0.01),
            filter,
        )
        .is_some();
    if blocked {
        return false;
    }

    // And the hang must not put the body below the surface the grip sits on.
    // A lip you can hang under has solid material beneath it going down; the
    // rim of a thin plate does not, and hanging there is falling.
    let under = g.point - Vec3::Y * 0.02 - g.normal * (m.capsule_radius * 0.3);
    under.is_finite()
        && spatial
            .cast_ray(under, Dir3::NEG_Y, hang_drop(body) * 0.5, true, filter)
            .is_some()
}

/// Look for another grip offset from the current one in `dir`.
///
/// Used for both the upward reach (cliff ascent) and the corner turn: the only
/// difference is which way the offset points.
fn probe_next_grip(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    from: &Grip,
    body: &AvatarBody,
    dir: Vec3,
) -> Option<Grip> {
    let m = &body.metrics;
    let reach = m.height_m * ARM_SPAN_FRAC * 1.6;
    let d = dir.normalize_or_zero();

    // Stand the probe off the face so the forward cast has room to run, and
    // offset it along the search direction.
    let origin = from.point + d * reach + from.normal * (m.capsule_radius * 1.5);
    // The probe measures reach from the feet, so put a virtual foot level under
    // the offset origin at the same drop the body would hang at.
    let feet = origin.y - hang_drop(body) - m.capsule_half_extent();
    let cfg = probe_config(body, feet);

    // Fan the cast direction, not just the origin.
    //
    // A corner is precisely the place where the next lip faces a DIFFERENT way,
    // so casting only along the current wall's normal can never find one — the
    // ray runs parallel to the face it is looking for. Sweeping ±45° and ±90°
    // picks up both outer corners (the face turns away) and inner ones (it
    // turns toward you), which is why A/D at a corner did nothing before.
    for sweep in [0.0_f32, -0.78, 0.78, -1.57, 1.57] {
        let cast = Quat::from_rotation_y(sweep) * -from.normal;
        let Some(found) = grip::probe(spatial, filter, origin, cast, &cfg) else {
            continue;
        };
        // Reject a "new" grip that is really the one already held, or one so
        // far away that moving to it would be a teleport rather than a reach.
        let moved_on = found.point.distance(from.point);
        if moved_on < m.capsule_radius
            || moved_on > body.metrics.height_m * MAX_GRAB_DISTANCE_FRAC
        {
            continue;
        }
        return Some(found);
    }
    None
}

/// Look for a grip around an OUTER corner — the far side of the wall you are
/// on, past the end of its lip.
///
/// The general `probe_next_grip` stands off the CURRENT face and casts along
/// its normal, which can never see a corner: the new face is perpendicular to
/// the old one and sits inside the old wall's footprint, so a probe that stays
/// in front of the old face is looking parallel to the surface it wants.
///
/// This positions past the lip end and BEHIND the old face plane, then casts
/// back along the lip — the one placement from which an end face is visible.
fn probe_corner(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    from: &Grip,
    body: &AvatarBody,
    sign: f32,
) -> Option<Grip> {
    let m = &body.metrics;
    let t = from.tangent * sign;
    let origin = from.point + t * (m.capsule_radius * 2.2) - from.normal * (m.capsule_radius * 3.0);
    let feet = origin.y - hang_drop(body) - m.capsule_half_extent();
    let cfg = probe_config(body, feet);
    let found = grip::probe(spatial, filter, origin, -t, &cfg)?;
    let moved_on = found.point.distance(from.point);
    // Far enough to be a different hold, near enough to be a reach.
    (moved_on > m.capsule_radius && moved_on <= body.metrics.height_m * MAX_GRAB_DISTANCE_FRAC)
        .then_some(found)
}

/// Find a lip *below and in front of* the feet — the step-off-and-hang case.
///
/// This is the query the old detector could not express at all: it only ever
/// looked forward and up, so a character standing on a ledge could never see
/// the edge it was standing on.
fn probe_ledge_below(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    feet: Vec3,
    dir: Vec3,
    body: &AvatarBody,
) -> Option<Grip> {
    let m = &body.metrics;
    let ahead = feet + dir * (m.capsule_radius * 1.8);
    if !ahead.is_finite() || !feet.is_finite() {
        return None;
    }

    // Is there actually a drop? If the floor continues, this is not an edge.
    let floor = spatial.cast_ray(
        ahead + Vec3::Y * 0.1,
        Dir3::NEG_Y,
        MIN_LEDGE_HEIGHT + 0.2,
        true,
        filter,
    );
    if floor.is_some() {
        return None;
    }

    // Probe back toward the edge from below and beyond it.
    let origin = feet + dir * (m.capsule_radius * 2.6);
    let cfg = probe_config(body, feet.y - hang_drop(body));
    let g = grip::probe(spatial, filter, origin, -dir, &cfg)?;

    // Stepping off is only a lower if there is somewhere to hang. Off a
    // knee-high block there is not, and the old code lowered the body a metre
    // anyway — straight through the floor it had been standing on.
    has_hang_clearance(spatial, filter, &g, body).then_some(g)
}

/// How far one hand travels per step of the shimmy, in metres.
///
/// Bounded by ARM REACH, not by what looks like a confident stride. The body
/// hangs from the anchored hand, so the swinging hand ends up `2 * shoulder
/// half-width + STRIDE` away from it. At 0.42 m that put the reaching hand
/// about 0.52 m from its own shoulder against a ~0.50 m limit, so the solver
/// clamped it short and the hand visibly missed the ledge.
pub(crate) const SHIMMY_STRIDE: f32 = 0.26;

/// Where one hand sits in the shimmy cycle, as a signed lead/lag in strides.
///
/// Positive is ahead of the sliding body, negative behind. Each hand moves
/// during its own half of the cycle and is planted for the other half, so the
/// two alternate: one reaches out along the lip while the other bears the load,
/// then they swap. Over a full cycle each hand advances exactly one stride, so
/// neither drifts away from the shoulders.
pub(crate) fn shimmy_hand_lead(cycle: f32, leads: bool) -> f32 {
    let phase = cycle.rem_euclid(1.0);
    let local = if leads { phase * 2.0 } else { phase * 2.0 - 1.0 };
    smooth_step(local.clamp(0.0, 1.0)) - phase
}

/// Where one foot sits in the shimmy cycle: `(offset in strides, lift 0..1)`.
///
/// The same alternation as [`shimmy_hand_lead`], plus a lift so the foot clears
/// the wall while it is moving instead of scraping along it. A limb that slides
/// to its next position without ever leaving the surface does not read as a
/// step, which is why the legs looked frozen while the arms worked.
pub(crate) fn shimmy_foot_step(cycle: f32, leads: bool) -> (f32, f32) {
    let phase = cycle.rem_euclid(1.0);
    let local = if leads { phase * 2.0 } else { phase * 2.0 - 1.0 };
    let clamped = local.clamp(0.0, 1.0);
    let along = smooth_step(clamped) - phase;
    // Lifted only during this foot's own half of the cycle; planted otherwise.
    let lift = if (0.0..=1.0).contains(&local) {
        (std::f32::consts::PI * clamped).sin()
    } else {
        0.0
    };
    (along, lift)
}

fn smooth_step(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// How far a DYNAMIC leap may cross, as a multiple of body height.
///
/// Deliberately larger than [`MAX_GRAB_DISTANCE_FRAC`]. That bound governs
/// grabbing — what the character can quietly take hold of from where it is
/// hanging — and keeping it tight is what stops a probe from snapping the body
/// across open ground. A leap is the opposite: an explicit, committed move the
/// player asked for by pushing into a gap that the lip does not cross. It is
/// still bounded, and still only along the line the hands are already on.
const LEAP_REACH_FRAC: f32 = 1.05;

/// How far apart two samples are while scanning across a gap, in metres.
const LEAP_STEP: f32 = 0.22;

/// Look for a hold across a gap, along the lip, in the direction of travel.
///
/// The lip running out is not the end of the route — in the reference footage
/// it is a cue to jump. Without this every gap was a hard stop: the character
/// shimmied to the end of a ledge and simply refused to go further, even with
/// an obvious hold half a metre away.
pub(crate) fn probe_leap(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    from: &Grip,
    body: &AvatarBody,
    dir: Vec3,
) -> Option<Grip> {
    let m = &body.metrics;
    let d = dir.normalize_or_zero();
    if d.length_squared() < 0.5 {
        return None;
    }
    let max = m.height_m * LEAP_REACH_FRAC;

    // Walk outward from the current hold. The NEAREST hold across the gap wins:
    // a leap should land on the first thing that can be caught, not the
    // furthest thing in range.
    let mut travelled = m.capsule_radius * 2.0;
    while travelled <= max {
        // Sample slightly above and level, because the far side of a gap is
        // rarely at exactly the same height as the near side.
        for rise in [0.0_f32, 0.35, -0.30] {
            let at = from.point + d * travelled + Vec3::Y * rise;
            let origin = at + from.normal * (m.capsule_radius * 1.5);
            let feet = origin.y - hang_drop(body) - m.capsule_half_extent();
            let cfg = probe_config(body, feet);
            for sweep in [0.0_f32, -0.78, 0.78] {
                let cast = Quat::from_rotation_y(sweep) * -from.normal;
                let Some(found) = grip::probe(spatial, filter, origin, cast, &cfg) else {
                    continue;
                };
                let gap = found.point.distance(from.point);
                // Must be a genuinely different hold, and inside leap range.
                if gap > m.capsule_radius * 2.0 && gap <= max {
                    return Some(found);
                }
            }
        }
        travelled += LEAP_STEP;
    }
    None
}

/// Look for a hold BELOW the current one, to drop onto rather than fall from.
///
/// Releasing a ledge above another ledge dropped the character past it into
/// whatever was underneath. Catching the next hold down is the controlled
/// descent from the footage — the hand lowers you onto it instead of letting go.
pub(crate) fn probe_below(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    from: &Grip,
    body: &AvatarBody,
) -> Option<Grip> {
    let m = &body.metrics;
    // Reachable by lowering: about a body's worth below the current hold. Any
    // further and it is a fall, which is what releasing already does.
    let max_drop = m.height_m * 1.15;
    let mut down = m.height_m * 0.55;
    while down <= max_drop {
        // Sample OUT from the face as well as down it.
        //
        // A flush wall has no intermediate lip by definition — anything below
        // that can be caught is protruding: a shelf, a ledge, a balcony. Only
        // casting straight down the current face finds nothing but the face.
        for out in [0.0_f32, 0.45, 0.9] {
            let at = from.point - Vec3::Y * down + from.normal * out;
            let origin = at + from.normal * (m.capsule_radius * 1.5);
            let feet = origin.y - hang_drop(body) - m.capsule_half_extent();
            let cfg = probe_config(body, feet);
            let Some(found) = grip::probe(spatial, filter, origin, -from.normal, &cfg) else {
                continue;
            };
            // Strictly lower, or this is the hold already in hand.
            if found.point.y < from.point.y - m.capsule_radius
                && has_hang_clearance(spatial, filter, &found, body)
            {
                return Some(found);
            }
        }
        down += LEAP_STEP;
    }
    None
}

/// Is there somewhere to actually STAND at the top of this ledge?
///
/// `Grip::standable` only says the top surface is flat and deep enough. It says
/// nothing about what is above that surface, so a ledge under a low ceiling, a
/// pipe, or an overhang read as mantle-able and the character committed to a
/// pull-up it could never finish — rising into open air in front of the ledge,
/// stalling against the obstruction, and dropping back to a hang, over and over
/// for as long as the button was held.
///
/// Refusing up front is both correct and better looking: the character simply
/// stays hanging, which is what a climber does when there is no top out.
fn can_mantle(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    g: &Grip,
    body: &AvatarBody,
    rot: Quat,
) -> bool {
    if !g.standable {
        return false;
    }
    let stand = g.top + Vec3::Y * (body.metrics.capsule_half_extent() + 0.02);
    capsule_fits(spatial, &clearance_shape(body), stand, rot, filter)
}

/// The capsule used for every climb clearance test.
pub(crate) fn clearance_shape(body: &AvatarBody) -> Collider {
    let m = &body.metrics;
    Collider::capsule(m.capsule_radius * 0.85, m.capsule_cylinder_len * 0.85)
}

#[must_use]
/// Put the climbing body at `target`, stopping short of anything solid.
///
/// Every climb phase drives the body by hand — an arc across a transfer, a lerp
/// down a lower, a two-stage lift through a mantle, a settle while hanging — and
/// none of it goes through the character controller that resolves contacts for
/// ordinary movement. Whatever lay on the path was simply passed through: into a
/// wall on a transfer, under the floor on a lower, through a ceiling on a mantle.
///
/// This is the only place a climb writes the body position. If the capsule fits
/// at `target` the body goes there. If not it stops at the last clear point on
/// the way, and the caller is told the move was blocked so it can end the phase
/// instead of grinding into geometry.
pub(crate) fn place_body(
    tf: &mut Transform,
    target: Vec3,
    spatial: &SpatialQuery,
    collider: &Collider,
    filter: &SpatialQueryFilter,
    radius: f32,
) -> bool {
    let rot = tf.rotation;
    if capsule_fits(spatial, collider, target, rot, filter) {
        tf.translation = target;
        return true;
    }
    let from = tf.translation;

    // ALREADY INSIDE SOMETHING.
    //
    // The bisection below measures back toward `from`, so if `from` itself is
    // solid every candidate fails and the only answer it can give is "stay
    // put" — which welds the body inside the geometry permanently. Anything
    // that clipped the character once, at a corner or through a limb, left
    // them stuck there for good.
    //
    // Being invalid already means moving cannot make it worse, so take the
    // requested move and then try to climb back out into free space.
    if !capsule_fits(spatial, collider, from, rot, filter) {
        tf.translation = target;
        if let Some(free) = unwedge(spatial, collider, target, rot, filter, radius) {
            tf.translation = free;
        }
        return false;
    }

    // Blocked. Bisect back toward where the body already is for the furthest
    // point that still clears, so the motion stops against the obstacle rather
    // than either teleporting through it or freezing a whole step early.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..6 {
        let mid = 0.5 * (lo + hi);
        if capsule_fits(spatial, collider, from.lerp(target, mid), rot, filter) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    if lo > 0.0 {
        tf.translation = from.lerp(target, lo);
    }
    false
}

fn settle_to(
    tf: &mut Transform,
    pos: Vec3,
    rot: Quat,
    dt: f32,
    spatial: &SpatialQuery,
    collider: &Collider,
    filter: &SpatialQueryFilter,
    radius: f32,
) -> bool {
    let moved = place_body(
        tf,
        tf.translation.lerp(pos, (14.0 * dt).min(1.0)),
        spatial,
        collider,
        filter,
        radius,
    );
    tf.rotation = tf.rotation.slerp(rot, (10.0 * dt).min(1.0));
    moved
}

fn quadratic(a: Vec3, b: Vec3, c: Vec3, t: f32) -> Vec3 {
    let u = 1.0 - t;
    a * (u * u) + b * (2.0 * u * t) + c * (t * t)
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t)
}

pub(crate) fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::avatar::grip::GripKind;

    fn g(point: Vec3, normal: Vec3, standable: bool) -> Grip {
        Grip {
            point,
            normal,
            tangent: Vec3::Y.cross(normal).normalize_or(Vec3::X),
            top: point,
            standable,
            kind: GripKind::Ledge,
        }
    }

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
        for p in [
            ClimbPhase::Hanging,
            ClimbPhase::Shimmy,
            ClimbPhase::Transfer,
            ClimbPhase::Mantling,
            ClimbPhase::Lowering,
        ] {
            assert!(p.clip_hint().is_some(), "{p:?} has no authored-clip seam");
            assert!(p.is_attached(), "{p:?} must suppress the locomotion controller");
        }
        assert!(!ClimbPhase::None.is_attached());
    }

    /// "I should not be able to ... reach halfway across the baseplate to
    /// ledges."
    #[test]
    fn a_grip_out_of_arms_reach_is_refused() {
        let body = reach_test_body();
        let at = Vec3::new(0.0, 1.0, 0.0);
        let max = body.metrics.height_m * MAX_GRAB_DISTANCE_FRAC;

        let near = grip_at(at + Vec3::NEG_Z * (max * 0.5));
        assert!(grip_within_reach(&near, at, &body), "an arm's length away was refused");

        let far = grip_at(at + Vec3::NEG_Z * (max + 0.5));
        assert!(!grip_within_reach(&far, at, &body), "a hold {max:.2} m+ away was accepted");

        // Across the baseplate — the case from the report.
        let across = grip_at(at + Vec3::new(60.0, 0.0, 0.0));
        assert!(!grip_within_reach(&across, at, &body), "grabbed a ledge 60 m away");

        // A garbage probe result must never be reachable.
        assert!(!grip_within_reach(&grip_at(Vec3::splat(f32::NAN)), at, &body));
    }

    /// The gait is what makes a traverse read as hand-over-hand rather than a
    /// body on rails, so its shape is worth pinning precisely.
    #[test]
    fn the_shimmy_gait_alternates_planted_hands() {
        // Absolute position of a hand along the lip, in strides.
        let abs = |c: f32, leads: bool| c + shimmy_hand_lead(c, leads);

        let steps = 240;
        let mut moving_together = 0;
        let mut planted = [0, 0];
        for i in 0..steps {
            let c = i as f32 / steps as f32;
            let d = 1.0 / steps as f32;
            for (h, leads) in [(0usize, true), (1usize, false)] {
                let v = (abs(c + d, leads) - abs(c, leads)) / d;
                assert!(v >= -1e-3, "hand {h} moved BACKWARDS along the lip: {v:.3}");
                if v < 0.05 {
                    planted[h] += 1;
                }
            }
            let lv = (abs(c + d, true) - abs(c, true)) / d;
            let tv = (abs(c + d, false) - abs(c, false)) / d;
            if lv > 0.05 && tv > 0.05 {
                moving_together += 1;
            }
        }
        assert_eq!(moving_together, 0, "both hands let go of the lip at once");
        for (h, n) in planted.iter().enumerate() {
            let frac = *n as f32 / steps as f32;
            assert!(
                (0.35..=0.65).contains(&frac),
                "hand {h} is planted {:.0}% of the cycle, expected about half",
                frac * 100.0
            );
        }

        // Periodic, so a long traverse never accumulates drift.
        for c in [0.0, 0.17, 0.5, 0.83] {
            for leads in [true, false] {
                assert!(
                    (shimmy_hand_lead(c, leads) - shimmy_hand_lead(c + 3.0, leads)).abs() < 1e-5,
                    "gait drifts after three strides"
                );
                assert!(shimmy_hand_lead(c, leads).abs() <= 0.51, "hand ran a full stride off");
            }
        }
    }

    fn reach_test_body() -> AvatarBody {
        use eustress_avatar_schema::{resolve, AvatarDescriptor, NOMINAL_BIND_HEIGHT_M};
        let (metrics, motion) = resolve(&AvatarDescriptor::default(), NOMINAL_BIND_HEIGHT_M);
        AvatarBody {
            metrics,
            motion,
            control: crate::avatar::AvatarControl::LocalPlayer,
            metrics_finalised: false,
        }
    }

    fn grip_at(point: Vec3) -> Grip {
        Grip {
            point,
            normal: Vec3::Z,
            tangent: Vec3::X,
            top: point,
            standable: true,
            kind: grip::GripKind::Ledge,
        }
    }


    #[test]
    fn reach_band_excludes_what_step_up_already_handles() {
        assert!(MIN_LEDGE_HEIGHT > 0.30, "overlaps the controller's step height");
        assert!(MAX_LEDGE_REACH > 1.0 && MAX_LEDGE_REACH < 1.6, "implausible human reach");
    }

    /// There must be NO height an avatar can neither step over nor grab.
    /// `STEP_HEIGHT` is 0.30; anything above it has to be reachable.
    #[test]
    fn no_obstacle_height_is_both_too_tall_to_step_and_too_short_to_grab() {
        const STEP_HEIGHT: f32 = 0.30;
        assert!(
            MIN_LEDGE_HEIGHT <= STEP_HEIGHT + 0.05,
            "dead band from {STEP_HEIGHT} to {MIN_LEDGE_HEIGHT} m: too tall to step, \
             too short to grab, so the character just stops"
        );
    }

    /// Hanging from a waist-high lip would put the body centre underground.
    /// The vault threshold has to keep the hang above the floor.
    #[test]
    fn anything_low_enough_to_bury_the_body_is_vaulted_instead() {
        for height_m in [1.5_f32, 1.75, 2.0] {
            let drop = height_m * ARM_SPAN_FRAC * HANG_ARM_EXTENSION
                + height_m * SHOULDER_ABOVE_CENTRE_FRAC;
            let vault_max = height_m * VAULT_MAX_FRAC;
            // A grip at exactly the vault threshold must, if hung from, still
            // leave the body above ground — i.e. the threshold is high enough
            // to catch every case that would clip.
            assert!(
                vault_max >= drop - 1e-3,
                "body {height_m} m: grips up to {vault_max} m vault, but hanging \
                 needs {drop} m of clearance — the band between them buries the \
                 character in the floor"
            );
        }
    }

    /// The regression that made the whole climb a cutscene: the hang expired
    /// on a timer, so every phase but the mantle was unreachable by input.
    ///
    /// This asserts the property directly — elapsed time is not an argument to
    /// the decision, so no amount of hanging can change the answer.
    #[test]
    fn neutral_input_hangs_indefinitely() {
        let grip = g(Vec3::ZERO, Vec3::Z, true);
        let intent = AvatarIntent::default();
        assert_eq!(
            hang_action(&intent, &grip, (5.0, 5.0)),
            HangAction::Hold,
            "doing nothing must keep hanging — shimmy, transfer and a \
             deliberate drop are all things you do WHILE hanging"
        );
    }

    #[test]
    fn every_hang_verb_is_reachable_from_input() {
        let grip = g(Vec3::ZERO, Vec3::Z, false);
        let roomy = (5.0_f32, 5.0_f32);

        let mut drop = AvatarIntent::default();
        drop.crouch = true;
        assert_eq!(hang_action(&drop, &grip, roomy), HangAction::Drop);

        let mut up = AvatarIntent::default();
        up.jump_pressed = true;
        assert_eq!(hang_action(&up, &grip, roomy), HangAction::Up);

        let mut right = AvatarIntent::default();
        right.direction = grip.tangent;
        assert_eq!(hang_action(&right, &grip, roomy), HangAction::Shimmy(1.0));

        let mut left = AvatarIntent::default();
        left.direction = -grip.tangent;
        assert_eq!(hang_action(&left, &grip, roomy), HangAction::Shimmy(-1.0));
    }

    /// Running out of lip must offer a corner turn, not a dead stop — that is
    /// the difference between a ledge that ends and a ledge you cannot leave.
    #[test]
    fn running_out_of_lip_asks_for_a_corner() {
        let grip = g(Vec3::ZERO, Vec3::Z, false);
        let mut right = AvatarIntent::default();
        right.direction = grip.tangent;
        // Plenty of lip to the left, none to the right.
        assert_eq!(hang_action(&right, &grip, (5.0, 0.0)), HangAction::Corner(1.0));
    }

    /// Pushing into the wall must not be read as sideways travel.
    #[test]
    fn pushing_into_the_wall_is_not_a_shimmy() {
        let grip = g(Vec3::ZERO, Vec3::Z, false);
        let mut into = AvatarIntent::default();
        into.direction = -grip.normal;
        assert_eq!(hang_action(&into, &grip, (5.0, 5.0)), HangAction::Hold);
    }

    /// Crouch beats everything: a player asking to let go must always let go,
    /// even mid-shimmy, or a drop can be swallowed by a held direction key.
    #[test]
    fn drop_wins_over_every_other_input() {
        let grip = g(Vec3::ZERO, Vec3::Z, true);
        let mut all = AvatarIntent::default();
        all.crouch = true;
        all.jump_pressed = true;
        all.direction = grip.tangent;
        assert_eq!(hang_action(&all, &grip, (5.0, 5.0)), HangAction::Drop);
    }

    #[test]
    fn lateral_intent_is_measured_in_the_wall_frame_not_the_camera() {
        // Wall facing +Z (normal +Z means the climber is on the +Z side).
        let grip = g(Vec3::ZERO, Vec3::Z, false);
        let mut intent = AvatarIntent::default();

        // Pushing along +tangent reads positive regardless of body yaw.
        intent.direction = grip.tangent;
        assert!(lateral_intent(&intent, &grip) > 0.9);

        intent.direction = -grip.tangent;
        assert!(lateral_intent(&intent, &grip) < -0.9);

        // Pushing straight into the wall is NOT lateral — it must not shimmy.
        intent.direction = -grip.normal;
        assert!(lateral_intent(&intent, &grip).abs() < 1e-5);
    }

    #[test]
    fn facing_a_wall_points_the_body_into_it() {
        for yaw in [0.0_f32, 1.1, 2.6, 4.0, 5.9] {
            let normal = Vec3::new(yaw.cos(), 0.0, yaw.sin());
            let grip = g(Vec3::ZERO, normal, false);
            // Bevy forward is -Z; rotating it by the facing quat must point
            // INTO the wall, i.e. opposite the outward normal.
            let fwd = face_wall(&grip) * Vec3::NEG_Z;
            assert!(
                fwd.dot(normal) < -0.99,
                "yaw {yaw}: body faces {fwd:?} against normal {normal:?}"
            );
        }
    }

    #[test]
    fn the_transfer_arc_clears_both_endpoints() {
        // A straight lerp between two holds slides along the wall through
        // whatever sits between them; the arc must bow away.
        let a = Vec3::new(0.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.5, 0.0);
        let mid = (a + b) * 0.5 + Vec3::Y * 0.525;
        let quarter = quadratic(a, mid, b, 0.5);
        assert!(
            quarter.y > (a.y + b.y) * 0.5,
            "midpoint {quarter:?} did not bow above the chord"
        );
    }

    #[test]
    fn hang_drop_leaves_the_arms_extended_but_not_locked() {
        for height in [1.5_f32, 1.75, 2.0] {
            let arm = height * ARM_SPAN_FRAC;
            let shoulder = height * SHOULDER_ABOVE_CENTRE_FRAC;
            let drop = arm * HANG_ARM_EXTENSION + shoulder;
            let frac = (drop - shoulder) / arm;
            assert!(
                (0.70..0.95).contains(&frac),
                "height {height}: arm extension {frac} — folded below 0.7, locked above 0.95"
            );
        }
    }
}

#[cfg(test)]
mod wall_kick_tests {
    use super::*;
    use crate::avatar::grip::GripKind;

    fn g(point: Vec3, normal: Vec3, standable: bool) -> Grip {
        Grip {
            point,
            normal,
            tangent: Vec3::Y.cross(normal).normalize_or(Vec3::X),
            top: point,
            standable,
            kind: GripKind::Ledge,
        }
    }

    #[test]
    fn a_kick_always_leaves_the_wall() {
        let n = Vec3::Z;
        // Even steering hard INTO the face, the result must go away from it —
        // a push-off that drives you into the wall is not a push-off.
        for steer in [Vec3::Z, -Vec3::Z, Vec3::X, -Vec3::X, Vec3::ZERO] {
            let v = wall_jump_velocity(n, steer, 5.0);
            assert!(
                v.with_y(0.0).dot(n) > 0.0,
                "steer {steer:?} produced {v:?}, which moves into the wall"
            );
            assert!(v.y > 0.0, "steer {steer:?} produced no lift");
        }
    }

    #[test]
    fn steering_redirects_the_kick_without_reversing_it() {
        let n = Vec3::Z;
        let straight = wall_jump_velocity(n, Vec3::ZERO, 5.0);
        let sideways = wall_jump_velocity(n, Vec3::X, 5.0);
        assert!(
            sideways.x > straight.x + 0.5,
            "steering sideways did not redirect: {sideways:?} vs {straight:?}"
        );
        assert!(sideways.z > 0.0, "steering lost the outward push");
    }

    #[test]
    fn the_kick_carries_more_outward_than_upward() {
        let v = wall_jump_velocity(Vec3::Z, Vec3::ZERO, 5.0);
        assert!(
            v.with_y(0.0).length() > v.y,
            "kick is mostly vertical ({v:?}) — that is just a jump"
        );
    }

    #[test]
    fn jumping_while_steering_into_the_wall_still_climbs() {
        let grip = g(Vec3::ZERO, Vec3::Z, false);
        let mut into = AvatarIntent::default();
        into.jump_pressed = true;
        into.direction = -grip.normal;
        assert_eq!(hang_action(&into, &grip, (5.0, 5.0)), HangAction::Up);
    }

    #[test]
    fn jumping_while_steering_away_kicks_off() {
        let grip = g(Vec3::ZERO, Vec3::Z, false);
        let mut away = AvatarIntent::default();
        away.jump_pressed = true;
        away.direction = grip.normal;
        assert_eq!(hang_action(&away, &grip, (5.0, 5.0)), HangAction::WallJump);
    }
}

#[cfg(test)]
mod hold_tests {
    use super::*;
    use crate::avatar::grip::GripKind;

    fn grip_at(p: Vec3, n: Vec3) -> Grip {
        Grip {
            point: p,
            normal: n,
            tangent: Vec3::Y.cross(n).normalize_or(Vec3::X),
            top: p,
            standable: false,
            kind: GripKind::Ledge,
        }
    }

    #[test]
    fn seating_puts_the_hands_shoulder_width_apart_on_the_lip() {
        let g = grip_at(Vec3::new(0.0, 2.0, 0.0), Vec3::Z);
        let mut c = AvatarClimb::default();
        c.seat_hands(&g, 0.2);
        let span = (c.holds[1] - c.holds[0]).length();
        assert!((span - 0.4).abs() < 1e-5, "hands {span} apart, expected 0.4");
        // Both on the lip, and the centre back on the grip.
        assert!((c.hold_centre() - g.point).length() < 1e-5);
    }

    #[test]
    fn the_hold_centre_follows_the_hands_not_the_grip() {
        let mut c = AvatarClimb::default();
        c.holds = [Vec3::new(-1.0, 2.0, 0.0), Vec3::new(1.0, 3.0, 0.0)];
        let mid = c.hold_centre();
        assert!((mid.x).abs() < 1e-5);
        assert!((mid.y - 2.5).abs() < 1e-5, "centre y {} not between the holds", mid.y);
    }

    /// A climber never releases both hands at once, so reaches alternate.
    #[test]
    fn reaches_alternate_hands() {
        let mut c = AvatarClimb::default();
        assert_eq!(c.reaching, 0);
        c.reaching = 1 - c.reaching;
        assert_eq!(c.reaching, 1);
        c.reaching = 1 - c.reaching;
        assert_eq!(c.reaching, 0);
    }
}

#[cfg(test)]
mod root_weight_tests {
    use super::*;
    use eustress_avatar_schema::{resolve, AvatarDescriptor, NOMINAL_BIND_HEIGHT_M};

    fn body() -> AvatarBody {
        let (metrics, motion) = resolve(&AvatarDescriptor::default(), NOMINAL_BIND_HEIGHT_M);
        AvatarBody {
            metrics,
            motion,
            control: crate::avatar::AvatarControl::LocalPlayer,
            metrics_finalised: false,
        }
    }

    /// With both hands level and even, the body hangs centred between them.
    #[test]
    fn even_holds_hang_the_body_in_the_middle() {
        let b = body();
        let holds = [Vec3::new(-0.2, 2.0, 0.0), Vec3::new(0.2, 2.0, 0.0)];
        // Anchor either hand: symmetric holds give the same answer.
        let l = hang_pose_from_holds(&holds, 0, Vec3::Z, &b);
        let r = hang_pose_from_holds(&holds, 1, Vec3::Z, &b);
        assert!((l.x + r.x).abs() < 1e-4, "not symmetric: {} vs {}", l.x, r.x);
    }

    /// The whole point: when one hand reaches away, the body moves OVER the
    /// hand still holding on. A body that stays put looks weightless.
    #[test]
    fn the_body_shifts_onto_the_hand_that_is_holding_on() {
        let b = body();
        // Right hand has reached far out; left is anchored.
        let holds = [Vec3::new(-0.2, 2.0, 0.0), Vec3::new(1.4, 2.4, 0.0)];
        let centre = (holds[0] + holds[1]) * 0.5;
        let pose = hang_pose_from_holds(&holds, 0, Vec3::Z, &b);
        assert!(
            pose.x < centre.x - 0.05,
            "body at x={:.2} did not shift toward the anchored left hand \
             (midpoint {:.2})",
            pose.x,
            centre.x
        );
        // And anchoring the other hand leans the other way.
        let other = hang_pose_from_holds(&holds, 1, Vec3::Z, &b);
        assert!(other.x > pose.x, "anchor side does not change the lean");
    }

    /// The body hangs below the hold it is WEIGHTED on.
    ///
    /// Not below both: with one hand at chest height and the other stretched
    /// high, the torso legitimately sits above the low hand. Asserting "below
    /// both" was my error, and it fired at a 1.5 m vertical spread — which is
    /// exactly the reach the footage shows.
    #[test]
    fn the_body_hangs_below_the_hold_it_is_weighted_on() {
        let b = body();
        for spread in [0.0_f32, 0.5, 1.5] {
            let holds = [Vec3::new(-0.2, 2.0, 0.0), Vec3::new(spread, 2.0 + spread, 0.0)];
            for anchor in 0..2 {
                let p = hang_pose_from_holds(&holds, anchor, Vec3::Z, &b);
                assert!(
                    p.y < holds[anchor].y,
                    "spread {spread}, anchor {anchor}: body y={:.2} is not below its                      anchor at y={:.2}",
                    p.y,
                    holds[anchor].y
                );
            }
        }
    }

    /// A reach must move the body LESS than it moves the hand, or the shift
    /// reads as the whole character sliding rather than leaning.
    #[test]
    fn the_body_moves_less_than_the_reaching_hand() {
        let b = body();
        let seated = [Vec3::new(-0.2, 2.0, 0.0), Vec3::new(0.2, 2.0, 0.0)];
        let reached = [Vec3::new(-0.2, 2.0, 0.0), Vec3::new(1.6, 2.0, 0.0)];
        let a = hang_pose_from_holds(&seated, 0, Vec3::Z, &b);
        let c = hang_pose_from_holds(&reached, 0, Vec3::Z, &b);
        let body_moved = (c - a).length();
        let hand_moved = (reached[1] - seated[1]).length();
        assert!(
            body_moved < hand_moved * 0.7,
            "body moved {body_moved:.2} for a {hand_moved:.2} reach — that is sliding, not leaning"
        );
        assert!(body_moved > 0.02, "body did not respond to the reach at all");
    }
}

#[cfg(test)]
mod torso_roll_tests {
    use super::*;
    use crate::avatar::grip::GripKind;

    fn g() -> Grip {
        Grip {
            point: Vec3::new(0.0, 2.0, 0.0),
            normal: Vec3::Z,
            tangent: Vec3::X,
            top: Vec3::new(0.0, 2.0, 0.0),
            standable: false,
            kind: GripKind::Ledge,
        }
    }

    /// Level hands, level shoulders. Anything else is the roll leaking.
    #[test]
    fn level_hands_leave_the_torso_upright() {
        let grip = g();
        let holds = [Vec3::new(-0.2, 2.0, 0.0), Vec3::new(0.2, 2.0, 0.0)];
        let rolled = climb_body_rotation(&grip, &holds);
        let upright = face_wall(&grip);
        assert!(
            rolled.angle_between(upright) < 1e-3,
            "torso rolled {:.3} rad with the hands level",
            rolled.angle_between(upright)
        );
    }

    /// The shoulders follow the hands. A body held vertical while one hand is
    /// high is the most mannequin-like thing a climb can do.
    #[test]
    fn the_torso_rolls_toward_the_higher_hand() {
        let grip = g();
        let upright = face_wall(&grip);
        let right_high = [Vec3::new(-0.3, 2.0, 0.0), Vec3::new(0.3, 2.7, 0.0)];
        let left_high = [Vec3::new(-0.3, 2.7, 0.0), Vec3::new(0.3, 2.0, 0.0)];

        let r = climb_body_rotation(&grip, &right_high);
        let l = climb_body_rotation(&grip, &left_high);
        assert!(r.angle_between(upright) > 0.1, "no roll for a raised right hand");
        assert!(l.angle_between(upright) > 0.1, "no roll for a raised left hand");
        // And the two lean OPPOSITE ways.
        let ru = r * Vec3::Y;
        let lu = l * Vec3::Y;
        assert!(
            (ru.x - lu.x).abs() > 0.1,
            "both hand configurations produced the same lean: {ru:?} vs {lu:?}"
        );
    }

    /// Roll is clamped: a big height difference must not put the body sideways.
    #[test]
    fn the_roll_is_clamped_to_something_human() {
        let grip = g();
        let extreme = [Vec3::new(-0.1, 2.0, 0.0), Vec3::new(0.1, 6.0, 0.0)];
        let q = climb_body_rotation(&grip, &extreme);
        let up = q * Vec3::Y;
        assert!(
            up.y > 0.5,
            "torso rolled past horizontal ({up:?}) — a climber does not do that"
        );
    }
}

#[cfg(test)]
mod corner_facing_tests {
    use super::*;

    #[test]
    fn one_wall_faces_that_wall() {
        let mut c = AvatarClimb::default();
        c.hold_normals = [Vec3::Z, Vec3::Z];
        assert!((c.facing_normal() - Vec3::Z).length() < 1e-5);
    }

    /// At a corner the hands sit on faces pointing different ways, and the
    /// body must yaw BETWEEN them rather than snap to one.
    #[test]
    fn a_corner_faces_between_the_two_surfaces() {
        let mut c = AvatarClimb::default();
        c.hold_normals = [Vec3::Z, Vec3::X];
        let f = c.facing_normal();
        assert!((f.length() - 1.0).abs() < 1e-4, "not normalised: {f:?}");
        assert!(f.x > 0.3 && f.z > 0.3, "did not split the corner: {f:?}");
        // Strictly between, not equal to either face.
        assert!((f - Vec3::Z).length() > 0.2 && (f - Vec3::X).length() > 0.2);
    }

    /// Opposed normals cancel; falling back to one hand beats returning zero,
    /// which would leave the body facing an undefined direction.
    #[test]
    fn opposed_normals_fall_back_instead_of_collapsing() {
        let mut c = AvatarClimb::default();
        c.hold_normals = [Vec3::Z, -Vec3::Z];
        let f = c.facing_normal();
        assert!((f.length() - 1.0).abs() < 1e-4, "collapsed to {f:?}");
    }

    #[test]
    fn seating_hands_sets_both_normals_from_the_grip() {
        use crate::avatar::grip::GripKind;
        let g = Grip {
            point: Vec3::new(0.0, 2.0, 0.0),
            normal: Vec3::X,
            tangent: Vec3::Y.cross(Vec3::X).normalize(),
            top: Vec3::new(0.0, 2.0, 0.0),
            standable: false,
            kind: GripKind::Ledge,
        };
        let mut c = AvatarClimb::default();
        c.seat_hands(&g, 0.2);
        assert_eq!(c.hold_normals, [Vec3::X, Vec3::X]);
        assert!((c.facing_normal() - Vec3::X).length() < 1e-5);
    }
}
