//! # Landing response — soft, hard, and the roll
//!
//! ## Why a roll needs its own state
//!
//! Every other landing response is a *pose* — the procedural knee flex in
//! [`super::procedural`] composes onto whatever the character is already
//! doing. A roll is not a pose: it takes control away, moves the body several
//! metres along the ground, and gives control back. That is a transient state,
//! and the motion graph cannot represent one — every node there is played once
//! with `.repeat()` and never stopped.
//!
//! So the roll is driven the same way the mantle is: a transform-driven state
//! with a hand-authored displacement curve. It will look plain until a clip
//! exists, and [`LandingPhase::clip_hint`] names the clip that should replace
//! it. What it will *not* do is wait for the animation system to grow a
//! feature before the mechanic is playable.
//!
//! ## Why the impact signal had to be rebuilt first
//!
//! `land_impact` was normalised over 8 m/s, which saturates after a 3.3 m
//! fall. Every landing from waist height upward produced the same number, so
//! there was no signal to threshold against — a roll trigger written on it
//! would have fired identically for a kerb and a cliff.
//! [`AvatarLocomotion::land_speed_mps`] carries the raw speed for exactly this
//! reason: thresholds belong in real units.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::climb::AvatarClimb;
use super::spawn::{AvatarBody, AvatarIntent, AvatarLocomotion};
use super::{AvatarSystems, SpawnedByAvatarRuntime};

/// Touchdown speed above which a landing costs the player something, m/s.
/// ~4.1 m of fall. Below this the procedural flex alone reads fine.
pub const HARD_LANDING_MPS: f32 = 9.0;
/// Touchdown speed above which an un-rolled landing becomes a stumble.
/// ~10.2 m of fall.
pub const STUMBLE_MPS: f32 = 14.0;
/// How long a roll takes.
const ROLL_DURATION: f32 = 0.72;
/// How far a roll carries the body, metres.
const ROLL_DISTANCE: f32 = 3.1;
/// How long a stumble suppresses full control.
const STUMBLE_DURATION: f32 = 0.55;

/// What the body is doing about a landing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LandingPhase {
    #[default]
    None,
    /// Converting downward speed into forward travel.
    Roll,
    /// Took the impact badly — briefly slowed and off-balance.
    Stumble,
}

impl LandingPhase {
    pub const fn clip_hint(self) -> Option<&'static str> {
        match self {
            LandingPhase::None => None,
            LandingPhase::Roll => Some("land_roll"),
            LandingPhase::Stumble => Some("land_hard"),
        }
    }

    /// True while the landing owns the body and the controller must not also
    /// be steering it.
    pub const fn owns_body(self) -> bool {
        matches!(self, LandingPhase::Roll)
    }
}

#[derive(Component, Debug, Clone, Default)]
pub struct AvatarLanding {
    pub phase: LandingPhase,
    /// 0..1 through the current phase.
    pub t: f32,
    /// Horizontal direction the roll travels.
    pub dir: Vec3,
    /// Touchdown speed that started it, m/s — kept so a consumer can scale
    /// dust, sound or damage without re-deriving it.
    pub entry_mps: f32,
}

impl AvatarLanding {
    pub fn is_rolling(&self) -> bool {
        self.phase == LandingPhase::Roll
    }
    /// How much the controller should scale movement by right now.
    pub fn control_scale(&self) -> f32 {
        match self.phase {
            LandingPhase::None => 1.0,
            LandingPhase::Roll => 0.0,
            // Not zero: a stumble should feel like a loss of authority, not a
            // freeze. A freeze reads as a bug; a slow recovery reads as weight.
            LandingPhase::Stumble => 0.35,
        }
    }
}

/// Decide what a touchdown costs. Pure, so the thresholds are testable without
/// a physics world.
///
/// A roll requires somewhere to go: `wants_forward` is the player still
/// holding a direction. Landing hard with no input is a stumble, because a
/// roll that fires without intent takes the camera somewhere the player did
/// not ask to be.
pub fn landing_outcome(speed_mps: f32, wants_forward: bool) -> LandingPhase {
    if speed_mps < HARD_LANDING_MPS {
        return LandingPhase::None;
    }
    if wants_forward {
        LandingPhase::Roll
    } else if speed_mps >= STUMBLE_MPS {
        LandingPhase::Stumble
    } else {
        LandingPhase::None
    }
}

/// Fraction of the roll distance covered by time `t` (0..1).
///
/// Front-loaded: a roll carries most of its momentum through the first half
/// and coasts out. A linear curve reads like the character is being dragged.
fn roll_travel(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub(crate) struct AvatarLandingPlugin;

impl Plugin for AvatarLandingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, attach_landing.in_set(AvatarSystems::Lifecycle))
            .add_systems(
                Update,
                drive_landing
                    .in_set(AvatarSystems::Locomotion)
                    .before(super::locomotion::drive_locomotion),
            );
    }
}

fn attach_landing(
    mut commands: Commands,
    q: Query<Entity, (With<SpawnedByAvatarRuntime>, Without<AvatarLanding>)>,
) {
    for e in q.iter() {
        commands.entity(e).try_insert(AvatarLanding::default());
    }
}

#[allow(clippy::too_many_arguments)]
fn drive_landing(
    time: Res<Time>,
    mut q: Query<
        (
            &mut Transform,
            &mut LinearVelocity,
            &mut AvatarLanding,
            &AvatarIntent,
            &AvatarLocomotion,
            &AvatarBody,
            Option<&AvatarClimb>,
        ),
        With<SpawnedByAvatarRuntime>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for (mut tf, mut vel, mut landing, intent, loco, body, climb) in q.iter_mut() {
        // A climb outranks a landing: dropping onto a ledge grab must not also
        // trigger a roll on the frame the grab lands.
        if climb.map(|c| c.is_climbing()).unwrap_or(false) {
            landing.phase = LandingPhase::None;
            landing.t = 0.0;
            continue;
        }

        match landing.phase {
            LandingPhase::None => {
                if !loco.just_landed {
                    continue;
                }
                let wants = intent.direction.with_y(0.0).length_squared() > 1e-4;
                let outcome = landing_outcome(loco.land_speed_mps, wants);
                if outcome == LandingPhase::None {
                    continue;
                }
                landing.phase = outcome;
                landing.t = 0.0;
                landing.entry_mps = loco.land_speed_mps;
                landing.dir = if wants {
                    intent.direction.with_y(0.0).normalize_or(tf.forward().as_vec3())
                } else {
                    tf.forward().as_vec3().with_y(0.0).normalize_or(Vec3::NEG_Z)
                };
                info!(
                    "landing: {:?} at {:.1} m/s",
                    landing.phase, landing.entry_mps
                );
            }

            LandingPhase::Roll => {
                let prev = roll_travel(landing.t);
                landing.t += dt / ROLL_DURATION;
                let now = roll_travel(landing.t);

                // Displacement is applied as VELOCITY, not a teleport, so the
                // controller's own collide-and-slide still resolves walls. A
                // roll that writes translation directly rolls through them.
                let step = (now - prev) * ROLL_DISTANCE;
                let v = landing.dir * (step / dt.max(1e-4));
                vel.0.x = v.x;
                vel.0.z = v.z;

                // Face the way we are going, so the roll does not travel
                // sideways relative to the body.
                let want = Quat::from_rotation_y((-landing.dir.x).atan2(-landing.dir.z));
                tf.rotation = tf.rotation.slerp(want, (14.0 * dt).min(1.0));

                if landing.t >= 1.0 {
                    landing.phase = LandingPhase::None;
                    landing.t = 0.0;
                }
                let _ = body;
            }

            LandingPhase::Stumble => {
                landing.t += dt / STUMBLE_DURATION;
                if landing.t >= 1.0 {
                    landing.phase = LandingPhase::None;
                    landing.t = 0.0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gentle_landing_costs_nothing() {
        // Stepping off a kerb must not roll, with or without input.
        assert_eq!(landing_outcome(3.0, true), LandingPhase::None);
        assert_eq!(landing_outcome(3.0, false), LandingPhase::None);
    }

    #[test]
    fn a_hard_landing_with_intent_rolls_and_without_it_does_not() {
        assert_eq!(landing_outcome(HARD_LANDING_MPS + 0.1, true), LandingPhase::Roll);
        // No input: a roll would move the camera somewhere unasked-for.
        assert_ne!(landing_outcome(HARD_LANDING_MPS + 0.1, false), LandingPhase::Roll);
    }

    #[test]
    fn a_very_hard_landing_without_intent_stumbles() {
        assert_eq!(landing_outcome(STUMBLE_MPS + 1.0, false), LandingPhase::Stumble);
    }

    /// The whole reason the impact signal was rebuilt: these three must not be
    /// the same outcome, and under the old 8 m/s normalisation they were.
    #[test]
    fn different_fall_heights_produce_different_outcomes() {
        let speed = |h: f32| (2.0 * 9.81 * h).sqrt();
        let outcomes: Vec<_> = [1.0_f32, 6.0, 20.0]
            .iter()
            .map(|h| landing_outcome(speed(*h), false))
            .collect();
        assert_eq!(outcomes[0], LandingPhase::None, "1 m fall should be free");
        assert_eq!(outcomes[2], LandingPhase::Stumble, "20 m fall must cost something");
        assert!(
            outcomes[0] != outcomes[2],
            "a 1 m and a 20 m fall produced the same outcome"
        );
    }

    #[test]
    fn roll_travel_is_monotonic_front_loaded_and_complete() {
        let mut prev = -1.0;
        let mut t = 0.0;
        while t <= 1.0 {
            let d = roll_travel(t);
            assert!((0.0..=1.0).contains(&d), "travel {d} out of range at {t}");
            assert!(d >= prev - 1e-6, "travel not monotonic at {t}");
            prev = d;
            t += 0.02;
        }
        assert!((roll_travel(1.0) - 1.0).abs() < 1e-6, "roll must complete its distance");
        assert!(roll_travel(0.0).abs() < 1e-6);
        // Front-loaded: more than half the distance in the first half.
        assert!(roll_travel(0.5) > 0.5, "roll reads as being dragged, not thrown");
    }

    #[test]
    fn only_the_roll_takes_the_body_from_the_controller() {
        assert!(LandingPhase::Roll.owns_body());
        assert!(!LandingPhase::Stumble.owns_body(), "a stumble steers, it does not seize");
        assert!(!LandingPhase::None.owns_body());
    }

    #[test]
    fn a_stumble_slows_without_freezing() {
        let s = AvatarLanding { phase: LandingPhase::Stumble, ..Default::default() };
        let scale = s.control_scale();
        assert!(scale > 0.0, "a frozen character reads as a bug, not as weight");
        assert!(scale < 1.0, "a stumble that costs nothing is not a stumble");
    }

    #[test]
    fn every_costly_phase_names_the_clip_that_should_replace_it() {
        assert!(LandingPhase::None.clip_hint().is_none());
        for p in [LandingPhase::Roll, LandingPhase::Stumble] {
            assert!(p.clip_hint().is_some(), "{p:?} has no authored-clip seam");
        }
    }
}
