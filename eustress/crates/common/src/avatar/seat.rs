//! # Riding a seat
//!
//! Seating is decided elsewhere: the host's seat system, and on a Player the
//! replication glue, insert and remove [`AvatarSeated`] on the avatar entity
//! (`docs/networking/SEATS.md`). A plain insert or remove is all they do;
//! everything else follows from the component:
//!
//! * The avatar rides. Once every mover of the seat has run this frame, its
//!   root sits at the seat's pose times `offset` ([`AvatarSystems::Ride`], in
//!   `PostUpdate`, before animation, the camera follow and transform
//!   propagation).
//! * Locomotion stands down: no gravity, collide and slide, stepping or jump.
//!   The locomotion sample reports `seated`, so the Humanoid is `Seated` and
//!   its Animate plays the sit animation.
//! * A climb in progress ends, and the foot IK and beam balance let go, so the
//!   sit animation poses the legs. The facing stays the seat's.
//! * The avatar's collider becomes a sensor, so it never pushes the seat or its
//!   vehicle, while ray casts and touches still find it.
//!
//! Removing the component leaves the avatar where it sits, with a jump it
//! pressed still pending, and gives its collider back its contacts.
//!
//! The seat's pose is composed from its `Transform` chain
//! ([`TransformHelper`]). `GlobalTransform` holds last frame's pose until
//! propagation runs, and Avian's `Position` sees a script's `CFrame` write
//! only at the next physics step; either would leave the rider a frame behind
//! a moving seat.

use avian3d::prelude::{LinearVelocity, Sensor};
use bevy::prelude::*;
use bevy::transform::helper::TransformHelper;

use super::climb::AvatarClimb;
use super::{AvatarSystems, BodyMetrics, SpawnedByAvatarRuntime};

/// The avatar rides a seat. Inserted and removed only by whatever decides
/// seating.
#[derive(Component, Debug, Clone, Copy)]
pub struct AvatarSeated {
    /// The seat part's entity.
    pub seat: Entity,
    /// The root's pose in the seat's frame, from [`seat_offset`].
    pub offset: Transform,
}

/// The hip joints' height above the surface a body sits on, as a share of leg
/// length: the thickness of the buttocks and thighs under them.
const HIP_CLEARANCE_FRAC: f32 = 0.11;

/// The root's height above the surface a seated avatar sits on. The root is
/// the capsule's centre, `capsule_half_extent` above the feet, and the hip
/// joints stand `hip_height` above the feet.
pub fn seated_root_height(m: &BodyMetrics) -> f32 {
    m.capsule_half_extent() - m.hip_height + HIP_CLEARANCE_FRAC * m.leg_length
}

/// Where a seated avatar's root sits in its seat's frame: centred on the
/// seat's top face, facing the seat's front. One formula for the host's
/// `SeatWeld` (its `C0`) and for riding on every machine, so the two agree.
pub fn seat_offset(seat_size_y: f32, metrics: &BodyMetrics) -> Transform {
    Transform::from_xyz(0.0, seat_size_y * 0.5 + seated_root_height(metrics), 0.0)
}

/// A sensor the seat put on the avatar's collider, removed when it gets up.
#[derive(Component, Debug)]
struct SeatSensor;

pub(crate) struct AvatarSeatPlugin;

impl Plugin for AvatarSeatPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            PostUpdate,
            AvatarSystems::Ride
                .before(bevy::app::AnimationSystems)
                .before(TransformSystems::Propagate),
        )
        .add_systems(Update, follow_seating.in_set(AvatarSystems::Lifecycle))
        .add_systems(PostUpdate, ride_seats.in_set(AvatarSystems::Ride));
    }
}

/// Getting up restores the collider; sitting down ends a climb, stops the
/// body and makes its collider a sensor. Getting up runs first, so a seat
/// swapped for another in one frame keeps its sensor.
#[allow(clippy::type_complexity)]
fn follow_seating(
    mut commands: Commands,
    mut left: RemovedComponents<AvatarSeated>,
    still: Query<(), With<AvatarSeated>>,
    ours: Query<(), With<SeatSensor>>,
    mut sat: Query<
        (Entity, Option<&mut AvatarClimb>, Option<&mut LinearVelocity>, Has<Sensor>),
        (Added<AvatarSeated>, With<SpawnedByAvatarRuntime>),
    >,
) {
    for entity in left.read() {
        if still.contains(entity) || !ours.contains(entity) {
            continue;
        }
        // `try_remove`: the avatar may be despawning this same frame.
        commands.entity(entity).try_remove::<(Sensor, SeatSensor)>();
    }
    for (entity, climb, velocity, sensor) in sat.iter_mut() {
        if let Some(mut climb) = climb {
            if climb.is_climbing() {
                *climb = AvatarClimb::default();
            }
        }
        if let Some(mut velocity) = velocity {
            velocity.0 = Vec3::ZERO;
        }
        if !sensor {
            commands.entity(entity).try_insert((Sensor, SeatSensor));
        }
    }
}

/// Each seated avatar at its seat's pose this frame, times its offset. The
/// seat's scale is its size, so only its position and rotation carry over.
fn ride_seats(
    mut set: ParamSet<(
        TransformHelper,
        Query<(Entity, &AvatarSeated, &mut Transform), With<SpawnedByAvatarRuntime>>,
    )>,
) {
    let riders: Vec<(Entity, Entity, Transform)> =
        set.p1().iter().map(|(entity, seated, _)| (entity, seated.seat, seated.offset)).collect();
    if riders.is_empty() {
        return;
    }
    let mut poses = Vec::with_capacity(riders.len());
    {
        let helper = set.p0();
        for (entity, seat, offset) in riders {
            let Ok(seat_pose) = helper.compute_global_transform(seat) else { continue };
            poses.push((entity, riding_pose(&seat_pose, &offset)));
        }
    }
    let mut avatars = set.p1();
    for (entity, pose) in poses {
        let Some((translation, rotation)) = pose else { continue };
        if let Ok((_, _, mut tf)) = avatars.get_mut(entity) {
            tf.translation = translation;
            tf.rotation = rotation;
        }
    }
}

/// The root's world position and rotation on a seat, or `None` when the seat's
/// pose is not finite (a body that is not posed is left where it is).
fn riding_pose(seat: &GlobalTransform, offset: &Transform) -> Option<(Vec3, Quat)> {
    let (_, seat_rotation, seat_translation) = seat.to_scale_rotation_translation();
    let rotation = (seat_rotation * offset.rotation).normalize();
    let translation = seat_translation + seat_rotation * offset.translation;
    (translation.is_finite() && rotation.is_finite()).then_some((translation, rotation))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> BodyMetrics {
        super::super::BodyMorphs::default().metrics(1.75)
    }

    #[test]
    fn the_root_sits_just_above_the_hips_on_the_seat() {
        let m = body();
        let h = seated_root_height(&m);
        // Hip joints a hand's width above the surface; the root sits at hip
        // height give or take the capsule's split, never a body length up.
        assert!(h > 0.0 && h < 0.3, "seated root height {h}");
        let offset = seat_offset(1.0, &m);
        assert!((offset.translation.y - (0.5 + h)).abs() < 1e-6, "on the seat's top face");
        assert_eq!(offset.rotation, Quat::IDENTITY, "facing the seat's front");
    }

    #[test]
    fn a_seat_carries_its_rider_without_its_size() {
        // A 2 x 1 x 2 seat, turned a quarter about Y, at (10, 3, 0): the
        // part's scale is its size, which must not stretch the offset.
        let seat = GlobalTransform::from(
            Transform::from_xyz(10.0, 3.0, 0.0)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::new(2.0, 1.0, 2.0)),
        );
        let offset = Transform::from_xyz(0.0, 0.6, 0.0);
        let (at, facing) = riding_pose(&seat, &offset).unwrap();
        assert!((at - Vec3::new(10.0, 3.6, 0.0)).length() < 1e-5, "offset in metres, not scaled: {at}");
        assert!(facing.angle_between(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)) < 1e-5);
    }

    #[test]
    fn an_unposed_seat_leaves_the_rider_alone() {
        let seat = GlobalTransform::from(Transform::from_xyz(f32::NAN, 0.0, 0.0));
        assert!(riding_pose(&seat, &Transform::IDENTITY).is_none());
    }
}
