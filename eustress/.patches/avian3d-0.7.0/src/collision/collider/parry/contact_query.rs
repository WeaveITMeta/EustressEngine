//! Geometric queries for computing information about contacts between two [`Collider`]s.
//!
//! This module contains the following contact queries:
//!
//! | Contact query         | Description                                                               |
//! | --------------------- | ------------------------------------------------------------------------- |
//! | [`contact`]           | Computes one pair of contact points between two [`Collider`]s.            |
//! | [`contact_manifolds`] | Computes all [`ContactManifold`]s between two [`Collider`]s.              |
//! | [`closest_points`]    | Computes the closest points between two [`Collider`]s.                    |
//! | [`distance`]          | Computes the minimum distance separating two [`Collider`]s.               |
//! | [`intersection_test`] | Tests whether two [`Collider`]s are intersecting each other.              |
//! | [`time_of_impact`]    | Computes when two moving [`Collider`]s hit each other for the first time. |
//!
//! For geometric queries that query the entire world for intersections, like raycasting, shapecasting
//! and point projection, see [spatial queries](spatial_query).

use crate::{collision::contact_types::SingleContact, prelude::*};
use bevy::prelude::*;
use parry::query::{PersistentQueryDispatcher, ShapeCastOptions, Unsupported};

/// An error indicating that a [contact query](self) is not supported for one of the [`Collider`] shapes.
pub type UnsupportedShape = Unsupported;

/// Computes one pair of contact points between two [`Collider`]s.
///
/// Returns `None` if the colliders are separated by a distance greater than `prediction_distance`
/// or if the given shapes are invalid.
///
/// # Example
///
/// ```
/// # #[cfg(feature = "2d")]
/// # use avian2d::{collision::collider::contact_query::contact, prelude::*};
/// # #[cfg(feature = "3d")]
/// use avian3d::{collision::collider::contact_query::contact, prelude::*};
/// use bevy::prelude::*;
///
/// # #[cfg(all(feature = "3d", feature = "f32"))]
/// # {
/// let collider1 = Collider::sphere(0.5);
/// let collider2 = Collider::cuboid(1.0, 1.0, 1.0);
///
/// // Compute a contact that should have a penetration depth of 0.5
/// let contact = contact(
///     // First collider
///     &collider1,
///     Vec3::default(),
///     Quat::default(),
///     // Second collider
///     &collider2,
///     Vec3::X * 0.5,
///     Quat::default(),
///     // Prediction distance
///     0.0,
/// )
/// .expect("Unsupported collider shape");
///
/// assert_eq!(
///     contact.is_some_and(|contact| contact.penetration == 0.5),
///     true
/// );
/// # }
/// ```
pub fn contact(
    collider1: &Collider,
    position1: impl Into<Position>,
    rotation1: impl Into<Rotation>,
    collider2: &Collider,
    position2: impl Into<Position>,
    rotation2: impl Into<Rotation>,
    prediction_distance: Scalar,
) -> Result<Option<SingleContact>, UnsupportedShape> {
    let rotation1: Rotation = rotation1.into();
    let rotation2: Rotation = rotation2.into();
    let isometry1 = make_pose(position1, rotation1);
    let isometry2 = make_pose(position2, rotation2);

    parry::query::contact(
        &isometry1,
        collider1.shape_scaled().0.as_ref(),
        &isometry2,
        collider2.shape_scaled().0.as_ref(),
        prediction_distance,
    )
    .map(|contact| {
        if let Some(contact) = contact {
            // Transform contact data into local space
            let point1: Vector = rotation1.inverse() * contact.point1;
            let point2: Vector = rotation2.inverse() * contact.point2;
            let normal1: Vector = (rotation1.inverse() * contact.normal1).normalize();
            let normal2: Vector = (rotation2.inverse() * contact.normal2).normalize();

            // Make sure the normals are valid
            if !normal1.is_normalized() || !normal2.is_normalized() {
                return None;
            }

            Some(SingleContact::new(
                point1,
                point2,
                normal1,
                normal2,
                -contact.dist,
            ))
        } else {
            None
        }
    })
}

// TODO: Add a persistent version of this that tries to reuse previous contact manifolds
// by exploiting spatial and temporal coherence. This is supported by Parry's contact_manifolds,
// but requires using Parry's `ContactManifold` type.
/// Computes all [`ContactManifold`]s between two [`Collider`]s.
///
/// The contact points are expressed in world space, relative to the origin
/// of the first and second shape respectively.
///
/// Returns an empty vector if the colliders are separated by a distance greater than `prediction_distance`
/// or if the given shapes are invalid.
///
/// # Example
///
/// ```
/// # #[cfg(feature = "2d")]
/// # use avian2d::{collision::collider::contact_query::contact_manifolds, prelude::*};
/// # #[cfg(feature = "3d")]
/// use avian3d::{collision::collider::contact_query::contact_manifolds, prelude::*};
/// use bevy::prelude::*;
///
/// # #[cfg(all(feature = "3d", feature = "f32"))]
/// # {
/// let collider1 = Collider::sphere(0.5);
/// let collider2 = Collider::cuboid(1.0, 1.0, 1.0);
///
/// // Compute contact manifolds a collision that should be penetrating
/// let mut manifolds = Vec::new();
/// contact_manifolds(
///     // First collider
///     &collider1,
///     Vec3::default(),
///     Quat::default(),
///     // Second collider
///     &collider2,
///     Vec3::X * 0.25,
///     Quat::default(),
///     // Prediction distance
///     0.0,
///     // Output manifolds
///     &mut manifolds,
/// );
///
/// assert_eq!(manifolds.is_empty(), false);
/// # }
/// ```
pub fn contact_manifolds(
    collider1: &Collider,
    position1: impl Into<Position>,
    rotation1: impl Into<Rotation>,
    collider2: &Collider,
    position2: impl Into<Position>,
    rotation2: impl Into<Rotation>,
    prediction_distance: Scalar,
    manifolds: &mut Vec<ContactManifold>,
) {
    let position1: Position = position1.into();
    let position2: Position = position2.into();
    let rotation1: Rotation = rotation1.into();
    let rotation2: Rotation = rotation2.into();
    let isometry1 = make_pose(position1, rotation1);
    let isometry2 = make_pose(position2, rotation2);
    let isometry12 = isometry1.inv_mul(&isometry2);

    // EUSTRESS PATCH (see PATCH.md): a cuboid much larger than the shape it touches
    // goes to the query as the part of it near that shape.
    let shape1 = collider1.shape_scaled().0.as_ref();
    let shape2 = collider2.shape_scaled().0.as_ref();
    let near1 = shape1
        .as_cuboid()
        .and_then(|cuboid| near_part_of_cuboid(cuboid, shape2, &isometry12, prediction_distance));
    let near2 = match near1 {
        Some(_) => None,
        None => shape2.as_cuboid().and_then(|cuboid| {
            near_part_of_cuboid(cuboid, shape1, &isometry12.inverse(), prediction_distance)
        }),
    };
    let (query_shape1, query_shape2, query_pose12, near_pose1) = match (&near1, &near2) {
        (Some((part, at)), _) => (
            part.shape(),
            shape2,
            parry::math::Pose::from_translation(-*at) * isometry12,
            Some(parry::math::Pose::from_translation(*at)),
        ),
        (None, Some((part, at))) => (
            shape1,
            part.shape(),
            isometry12 * parry::math::Pose::from_translation(*at),
            None,
        ),
        (None, None) => (shape1, shape2, isometry12, None),
    };

    // TODO: Reuse manifolds from previous frame to improve performance
    let mut new_manifolds =
        Vec::<parry::query::ContactManifold<(), ()>>::with_capacity(manifolds.len());
    let result = parry::query::DefaultQueryDispatcher.contact_manifolds(
        &query_pose12,
        query_shape1,
        query_shape2,
        prediction_distance,
        &mut new_manifolds,
        &mut None,
    );

    // Clear the old manifolds.
    manifolds.clear();

    // Fall back to support map contacts for unsupported (custom) shapes.
    if result.is_err()
        && let (Some(shape1), Some(shape2)) = (
            collider1.shape_scaled().as_support_map(),
            collider2.shape_scaled().as_support_map(),
        )
        && let Some(contact) = parry::query::contact::contact_support_map_support_map(
            &isometry12,
            shape1,
            shape2,
            prediction_distance,
        )
    {
        let normal = rotation1 * contact.normal1;

        // Make sure the normal is valid
        if !normal.is_normalized() {
            return;
        }

        let local_point1: Vector = contact.point1;

        // The contact point is the midpoint of the two points in world space.
        // The anchors are relative to the positions of the colliders.
        let point1 = rotation1 * local_point1;
        let anchor1 = point1 + normal * contact.dist * 0.5;
        let anchor2 = anchor1 + (position1.0 - position2.0);
        let world_point = position1.0 + anchor1;
        let points = [ContactPoint::new(
            anchor1,
            anchor2,
            world_point,
            -contact.dist,
        )];

        manifolds.push(ContactManifold::new(points, normal));
    }

    manifolds.extend(new_manifolds.iter().filter_map(|manifold| {
        // Skip empty manifolds.
        if manifold.contacts().is_empty() {
            return None;
        }

        // EUSTRESS PATCH (see PATCH.md): points on a first cuboid's near part are in the part's frame.
        let subpos1 = manifold.subshape_pos1.or(near_pose1).unwrap_or_default();
        let local_normal: Vector = (subpos1.rotation * manifold.local_n1).normalize();
        let normal = rotation1 * local_normal;

        // Make sure the normal is valid
        if !normal.is_normalized() {
            return None;
        }

        let points = manifold.contacts().iter().map(|contact| {
            // The contact point is the midpoint of the two points in world space.
            // The anchors are relative to the positions of the colliders.
            let point1 = rotation1 * subpos1.transform_point(contact.local_p1);
            let anchor1 = point1 + normal * contact.dist * 0.5;
            let anchor2 = anchor1 + (position1.0 - position2.0);
            let world_point = position1.0 + anchor1;
            ContactPoint::new(anchor1, anchor2, world_point, -contact.dist)
                .with_feature_ids(contact.fid1.into(), contact.fid2.into())
        });

        let manifold = ContactManifold::new(points, normal);

        Some(manifold)
    }));
}

/// EUSTRESS PATCH (see PATCH.md): the part of a large cuboid that a contact query
/// against a much smaller shape is made with.
enum NearPart {
    /// The half-space bounded by the face the shape is over.
    Face(parry::shape::HalfSpace),
    /// The cuboid cut down to the shape's neighbourhood, near an edge or a corner.
    Cut(parry::shape::Cuboid),
}

impl NearPart {
    fn shape(&self) -> &dyn parry::shape::Shape {
        match self {
            Self::Face(half_space) => half_space,
            Self::Cut(cuboid) => cuboid,
        }
    }
}

/// EUSTRESS PATCH (see PATCH.md): the part of `cuboid` near `other`, and where it
/// sits in the cuboid's frame, or `None` to query the whole cuboid.
///
/// Parry finds the contacts between a cuboid and a cylinder, cone, capsule or
/// convex hull with GJK and EPA, whose support points are the cuboid's corners. On
/// a ground 4 km across those sit 2 km from the contact, and in `f32` the normal
/// of a sub-millimetre gap measured from them is noise: a wheel resting on such a
/// ground read a normal 32 degrees off vertical and was thrown 6 m into the air.
/// Cutting the cuboid down to a few metres only shrinks the noise, because a shape
/// resting on a face is exactly the case where GJK divides by a vanishing gap.
///
/// So over the middle of a face, the query is made with that face's half-space,
/// which parry solves exactly: the other shape's support feature against a plane.
/// It gives the cuboid's own contacts when the shape lies over the face, keeps
/// clear of the side faces by more than it is deep, and is no deeper than the
/// cuboid's mid-plane, since then no edge or side face is within reach and the
/// face is the shortest way out. Near an edge or a corner the cuboid is cut down
/// to the shape's neighbourhood instead: the cut's own faces are farther from the
/// shape than the prediction distance and than any way out through a face kept
/// whole, so the contacts are the same, measured from corners a few metres away.
///
/// Cuboid against ball, cuboid or triangle has exact paths of its own in parry
/// and is left alone.
fn near_part_of_cuboid(
    cuboid: &parry::shape::Cuboid,
    other: &dyn parry::shape::Shape,
    other_in_cuboid: &parry::math::Pose,
    prediction_distance: Scalar,
) -> Option<(NearPart, Vector)> {
    use parry::shape::ShapeType;

    if !other.is_convex()
        || matches!(
            other.shape_type(),
            ShapeType::Ball | ShapeType::Cuboid | ShapeType::Triangle | ShapeType::HalfSpace
        )
    {
        return None;
    }
    let aabb = other.compute_aabb(other_in_cuboid);
    let extent = (aabb.maxs - aabb.mins).max_element();
    let half = cuboid.half_extents;
    let near = extent + prediction_distance;
    // Only a cuboid reaching far past the other shape is worth replacing.
    let far_axes = half.cmpgt(Vector::splat(4.0 * near));
    if !far_axes.any() || !near.is_finite() {
        return None;
    }

    // The face the shape is over: of the faces it could be resolved through, the
    // one it is least deep behind.
    let mut face: Option<(usize, Scalar, Scalar)> = None;
    for axis in 0..half.to_array().len() {
        for side in [-1.0, 1.0] {
            let deepest = if side > 0.0 {
                aabb.mins[axis]
            } else {
                -aabb.maxs[axis]
            };
            let depth = half[axis] - deepest;
            if depth > half[axis] {
                continue;
            }
            let clearance = depth.max(0.0) + prediction_distance;
            let over = (0..half.to_array().len()).all(|other_axis| {
                other_axis == axis
                    || (aabb.mins[other_axis] - clearance >= -half[other_axis]
                        && aabb.maxs[other_axis] + clearance <= half[other_axis])
            });
            if over && face.is_none_or(|(_, _, least)| depth < least) {
                face = Some((axis, side, depth));
            }
        }
    }
    if let Some((axis, side, _)) = face {
        let mut normal = Vector::ZERO;
        normal[axis] = side;
        return Some((
            NearPart::Face(parry::shape::HalfSpace::new(normal)),
            normal * half[axis],
        ));
    }

    // Near an edge or a corner: cut the far axes down to the shape's neighbourhood.
    let kept_thickness = Vector::select(far_axes, Vector::ZERO, 2.0 * half).max_element();
    let reach = near + extent + kept_thickness;
    let lo = Vector::select(
        far_axes,
        (aabb.mins - Vector::splat(reach)).max(-half),
        -half,
    );
    let hi = Vector::select(far_axes, (aabb.maxs + Vector::splat(reach)).min(half), half);
    if hi.cmple(lo).any() {
        // Out of reach of each other: the whole cuboid gives the same empty answer.
        return None;
    }
    Some((
        NearPart::Cut(parry::shape::Cuboid::new((hi - lo) * 0.5)),
        (lo + hi) * 0.5,
    ))
}

/// Information about the closest points between two [`Collider`]s.
///
/// The closest points can be computed using [`closest_points`].
#[derive(Reflect, Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serialize", reflect(Serialize, Deserialize))]
#[reflect(Debug, PartialEq)]
pub enum ClosestPoints {
    /// The two shapes are intersecting each other.
    Intersecting,
    /// The two shapes are not intersecting each other but the distance between the closest points
    /// is below the user-defined maximum distance.
    ///
    /// The points are expressed in world space.
    WithinMargin(Vector, Vector),
    /// The two shapes are not intersecting each other and the distance between the closest points
    /// exceeds the user-defined maximum distance.
    OutsideMargin,
}

/// Computes the [`ClosestPoints`] between two [`Collider`]s.
///
/// Returns `Err(UnsupportedShape)` if either of the collider shapes is not supported.
///
/// # Example
///
/// ```
/// # #[cfg(feature = "2d")]
/// # use avian2d::{collision::collider::contact_query::*, prelude::*};
/// # #[cfg(feature = "3d")]
/// use avian3d::{collision::collider::contact_query::*, prelude::*};
/// use bevy::prelude::*;
///
/// # #[cfg(all(feature = "3d", feature = "f32"))]
/// # {
/// let collider1 = Collider::sphere(0.5);
/// let collider2 = Collider::cuboid(1.0, 1.0, 1.0);
///
/// // The shapes are intersecting
/// assert_eq!(
///     closest_points(
///         &collider1,
///         Vec3::default(),
///         Quat::default(),
///         &collider2,
///         Vec3::default(),
///         Quat::default(),
///         2.0,
///     )
///     .expect("Unsupported collider shape"),
///     ClosestPoints::Intersecting,
/// );
///
/// // The shapes are not intersecting but the distance between the closest points is below 2.0
/// assert_eq!(
///     closest_points(
///         &collider1,
///         Vec3::default(),
///         Quat::default(),
///         &collider2,
///         Vec3::X * 1.5,
///         Quat::default(),
///         2.0,
///     )
///     .expect("Unsupported collider shape"),
///     ClosestPoints::WithinMargin(Vec3::X * 0.5, Vec3::X * 1.0),
/// );
///
/// // The shapes are not intersecting and the distance between the closest points exceeds 2.0
/// assert_eq!(
///     closest_points(
///         &collider1,
///         Vec3::default(),
///         Quat::default(),
///         &collider2,
///         Vec3::X * 5.0,
///         Quat::default(),
///         2.0,
///     )
///     .expect("Unsupported collider shape"),
///     ClosestPoints::OutsideMargin,
/// );
/// # }
/// ```
pub fn closest_points(
    collider1: &Collider,
    position1: impl Into<Position>,
    rotation1: impl Into<Rotation>,
    collider2: &Collider,
    position2: impl Into<Position>,
    rotation2: impl Into<Rotation>,
    max_distance: Scalar,
) -> Result<ClosestPoints, UnsupportedShape> {
    let rotation1: Rotation = rotation1.into();
    let rotation2: Rotation = rotation2.into();
    let isometry1 = make_pose(position1, rotation1);
    let isometry2 = make_pose(position2, rotation2);

    parry::query::closest_points(
        &isometry1,
        collider1.shape_scaled().0.as_ref(),
        &isometry2,
        collider2.shape_scaled().0.as_ref(),
        max_distance,
    )
    .map(|closest_points| match closest_points {
        parry::query::ClosestPoints::Intersecting => ClosestPoints::Intersecting,
        parry::query::ClosestPoints::WithinMargin(point1, point2) => {
            ClosestPoints::WithinMargin(point1, point2)
        }
        parry::query::ClosestPoints::Disjoint => ClosestPoints::OutsideMargin,
    })
}

/// Computes the minimum distance separating two [`Collider`]s.
///
/// Returns `0.0` if the colliders are touching or penetrating, and `Err(UnsupportedShape)`
/// if either of the collider shapes is not supported.
///
/// # Example
///
/// ```
/// # #[cfg(feature = "2d")]
/// # use avian2d::{collision::collider::contact_query::distance, prelude::*};
/// # #[cfg(feature = "3d")]
/// use avian3d::{collision::collider::contact_query::distance, prelude::*};
/// use bevy::prelude::*;
///
/// # #[cfg(all(feature = "3d", feature = "f32"))]
/// # {
/// let collider1 = Collider::sphere(0.5);
/// let collider2 = Collider::cuboid(1.0, 1.0, 1.0);
///
/// // The distance is 1.0
/// assert_eq!(
///     distance(
///         &collider1,
///         Vec3::default(),
///         Quat::default(),
///         &collider2,
///         Vec3::X * 2.0,
///         Quat::default(),
///     )
///     .expect("Unsupported collider shape"),
///     1.0,
/// );
///
/// // The colliders are penetrating, so the distance is 0.0
/// assert_eq!(
///     distance(
///         &collider1,
///         Vec3::default(),
///         Quat::default(),
///         &collider2,
///         Vec3::default(),
///         Quat::default(),
///     )
///     .expect("Unsupported collider shape"),
///     0.0,
/// );
/// # }
/// ```
pub fn distance(
    collider1: &Collider,
    position1: impl Into<Position>,
    rotation1: impl Into<Rotation>,
    collider2: &Collider,
    position2: impl Into<Position>,
    rotation2: impl Into<Rotation>,
) -> Result<Scalar, UnsupportedShape> {
    let rotation1: Rotation = rotation1.into();
    let rotation2: Rotation = rotation2.into();
    let isometry1 = make_pose(position1, rotation1);
    let isometry2 = make_pose(position2, rotation2);

    parry::query::distance(
        &isometry1,
        collider1.shape_scaled().0.as_ref(),
        &isometry2,
        collider2.shape_scaled().0.as_ref(),
    )
}

/// Tests whether two [`Collider`]s are intersecting each other.
///
/// Returns `Err(UnsupportedShape)` if either of the collider shapes is not supported.
///
/// # Example
///
/// ```
/// # #[cfg(feature = "2d")]
/// # use avian2d::{collision::collider::contact_query::intersection_test, prelude::*};
/// # #[cfg(feature = "3d")]
/// use avian3d::{collision::collider::contact_query::intersection_test, prelude::*};
/// use bevy::prelude::*;
///
/// # #[cfg(all(feature = "3d", feature = "f32"))]
/// # {
/// let collider1 = Collider::sphere(0.5);
/// let collider2 = Collider::cuboid(1.0, 1.0, 1.0);
///
/// // These colliders should be intersecting
/// assert_eq!(
///     intersection_test(
///         &collider1,
///         Vec3::default(),
///         Quat::default(),
///         &collider2,
///         Vec3::default(),
///         Quat::default(),
///     )
///     .expect("Unsupported collider shape"),
///     true,
/// );
///
/// // These colliders shouldn't be intersecting
/// assert_eq!(
///     intersection_test(
///         &collider1,
///         Vec3::default(),
///         Quat::default(),
///         &collider2,
///         Vec3::X * 5.0,
///         Quat::default(),
///     )
///     .expect("Unsupported collider shape"),
///     false,
/// );
/// # }
/// ```
pub fn intersection_test(
    collider1: &Collider,
    position1: impl Into<Position>,
    rotation1: impl Into<Rotation>,
    collider2: &Collider,
    position2: impl Into<Position>,
    rotation2: impl Into<Rotation>,
) -> Result<bool, UnsupportedShape> {
    let rotation1: Rotation = rotation1.into();
    let rotation2: Rotation = rotation2.into();
    let isometry1 = make_pose(position1, rotation1);
    let isometry2 = make_pose(position2, rotation2);

    parry::query::intersection_test(
        &isometry1,
        collider1.shape_scaled().0.as_ref(),
        &isometry2,
        collider2.shape_scaled().0.as_ref(),
    )
}

/// The way the [time of impact](time_of_impact) computation was terminated.
pub type TimeOfImpactStatus = parry::query::details::ShapeCastStatus;

/// The result of a [time of impact](time_of_impact) computation between two moving [`Collider`]s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeOfImpact {
    /// The time at which the colliders come into contact.
    pub time_of_impact: Scalar,
    /// The closest point on the first collider, at the time of impact,
    /// expressed in local space.
    pub point1: Vector,
    /// The closest point on the second collider, at the time of impact,
    /// expressed in local space.
    pub point2: Vector,
    /// The outward normal on the first collider, at the time of impact,
    /// expressed in local space.
    pub normal1: Vector,
    /// The outward normal on the second collider, at the time of impact,
    /// expressed in local space.
    pub normal2: Vector,
    /// The way the time of impact computation was terminated.
    pub status: TimeOfImpactStatus,
}

/// Computes when two moving [`Collider`]s hit each other for the first time.
///
/// Returns `Ok(None)` if the time of impact is greater than `max_time_of_impact`
/// and `Err(UnsupportedShape)` if either of the collider shapes is not supported.
///
/// # Example
///
/// ```
/// # #[cfg(feature = "2d")]
/// # use avian2d::{collision::collider::contact_query::time_of_impact, prelude::*};
/// # #[cfg(feature = "3d")]
/// use avian3d::{collision::collider::contact_query::time_of_impact, prelude::*};
/// use bevy::prelude::*;
///
/// # #[cfg(all(feature = "3d", feature = "f32"))]
/// # {
/// let collider1 = Collider::sphere(0.5);
/// let collider2 = Collider::cuboid(1.0, 1.0, 1.0);
///
/// let result = time_of_impact(
///     &collider1,        // Collider 1
///     Vec3::NEG_X * 5.0, // Position 1
///     Quat::default(),   // Rotation 1
///     Vec3::X,           // Linear velocity 1
///     &collider2,        // Collider 2
///     Vec3::X * 5.0,     // Position 2
///     Quat::default(),   // Rotation 2
///     Vec3::NEG_X,       // Linear velocity 2
///     100.0,             // Maximum time of impact
/// )
/// .expect("Unsupported collider shape");
///
/// assert_eq!(result.unwrap().time_of_impact, 4.5);
/// # }
/// ```
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn time_of_impact(
    collider1: &Collider,
    position1: impl Into<Position>,
    rotation1: impl Into<Rotation>,
    velocity1: impl Into<LinearVelocity>,
    collider2: &Collider,
    position2: impl Into<Position>,
    rotation2: impl Into<Rotation>,
    velocity2: impl Into<LinearVelocity>,
    max_time_of_impact: Scalar,
) -> Result<Option<TimeOfImpact>, UnsupportedShape> {
    let rotation1: Rotation = rotation1.into();
    let rotation2: Rotation = rotation2.into();

    let velocity1: LinearVelocity = velocity1.into();
    let velocity2: LinearVelocity = velocity2.into();

    let isometry1 = make_pose(position1, rotation1);
    let isometry2 = make_pose(position2, rotation2);

    parry::query::cast_shapes(
        &isometry1,
        velocity1.0,
        collider1.shape_scaled().0.as_ref(),
        &isometry2,
        velocity2.0,
        collider2.shape_scaled().0.as_ref(),
        ShapeCastOptions {
            max_time_of_impact,
            stop_at_penetration: true,
            ..default()
        },
    )
    .map(|toi| {
        toi.map(|toi| TimeOfImpact {
            time_of_impact: toi.time_of_impact,
            point1: toi.witness1,
            point2: toi.witness2,
            normal1: toi.normal1,
            normal2: toi.normal2,
            status: toi.status,
        })
    })
}
