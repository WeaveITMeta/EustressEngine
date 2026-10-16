//! # What a drawn part carries
//!
//! The mesh file a part was drawn from and the part it stands for, shared
//! by Studio's spawners and Play's apply step.

use bevy::prelude::*;

use crate::classes::PartType;

/// Tracks the source .glb file for a part's mesh (file-system-first architecture).
/// When present, the mesh was loaded from this path rather than generated inline.
/// The Scale Tool uses Transform.scale instead of regenerating the mesh.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct MeshSource {
    /// Relative path to the .glb file (from engine assets root)
    pub path: String,
}

impl MeshSource {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

/// Map PartType to the corresponding .glb file path in assets/parts/
pub fn part_type_to_glb_path(part_type: &PartType) -> &'static str {
    match part_type {
        PartType::Block => "parts/block.glb",
        PartType::Ball => "parts/ball.glb",
        PartType::Cylinder => "parts/cylinder.glb",
        PartType::Wedge => "parts/wedge.glb",
        PartType::CornerWedge => "parts/corner_wedge.glb",
        PartType::Cone => "parts/cone.glb",
    }
}

/// Component to track which part this entity represents
#[derive(Component)]
pub struct PartEntity {
    pub part_id: String,
}

/// Local-space collider half-extents that, after Avian re-applies the
/// entity's (global) Transform scale to the collider, yield a *world*
/// collider matching the part's visible `size`.
///
/// ## Why (verified 2026-05-25 against live scene + Avian 0.6 source)
///
/// Parts render a UNIT GLB mesh scaled by `Transform.scale`, and Avian
/// scales the collider by the SAME accumulated scale
/// (`propagate_collider_transforms` assigns `ColliderTransform.scale` =
/// global scale; `update_collider_scale`'s child-collider path applies it
/// ungated). Building at `size/2` therefore double-counts the scale: a part
/// of size `s` (whose local `Transform.scale == s`, parent Model at scale 1)
/// got a world collider of `(s/2)·s = s²/2` half-extent — verified by the
/// non-linear signature the user saw (size 3 → 9 wide, size 5.2 → 27 wide,
/// size 0.9 → 0.81). Dividing the local half-extents by the local Transform
/// scale cancels Avian's multiply so the world collider resolves to `size/2`
/// (a UNIT collider when `scale == size`). The cancellation is robust to
/// ancestor (folder/Model) scale: Avian applies `A·S`, and
/// `(size/2 / S)·(A·S) = size/2 · A`, which equals the visible extent for
/// both the unit-mesh convention (`S=size`) and the baked-mesh one (`S=1`).
pub fn collider_local_half(size: Vec3, transform_scale: Vec3) -> Vec3 {
    let inv = |s: f32| if s.is_finite() && s.abs() > 1e-4 { 1.0 / s.abs() } else { 1.0 };
    Vec3::new(
        size.x * 0.5 * inv(transform_scale.x),
        size.y * 0.5 * inv(transform_scale.y),
        size.z * 0.5 * inv(transform_scale.z),
    )
}

/// The Avian collider for a part of `shape`: the one place that decides what a
/// part collides as.
///
/// `local_full` is the collider's FULL length along each axis in the entity's
/// local space, which is the part's size divided by its transform scale
/// ([`collider_local_half`] times two) and so normally exactly ONE.
/// `transform_scale` is the scale Avian multiplies the collider by.
///
/// Block, ball, cylinder and cone are Avian's own primitives. A wedge and a
/// corner wedge are convex hulls of the corner tables in
/// [`crate::part_meshes`], the points their drawn meshes are generated from,
/// so a ramp collides as the slope it shows and weighs what it shows.
#[cfg(feature = "physics")]
pub fn shape_collider(shape: PartType, local_full: Vec3, transform_scale: Vec3) -> avian3d::prelude::Collider {
    use avian3d::prelude::Collider;
    match shape {
        PartType::Block => Collider::cuboid(local_full.x, local_full.y, local_full.z),
        PartType::Ball => Collider::sphere(local_full.x * 0.5),
        PartType::Cylinder | PartType::Cone => Collider::cylinder(local_full.x * 0.5, local_full.y),
        PartType::Wedge | PartType::CornerWedge => wedge_collider(shape, local_full, transform_scale),
    }
}

/// A wedge's or corner wedge's convex hull, falling back to a plain hull and
/// then to a box when the exact one cannot be built (a degenerate size).
#[cfg(feature = "physics")]
fn wedge_collider(shape: PartType, local_full: Vec3, transform_scale: Vec3) -> avian3d::prelude::Collider {
    use crate::part_meshes::{CORNER_WEDGE_CORNERS, WEDGE_CORNERS};
    use avian3d::prelude::Collider;

    let corners: &[[f32; 3]] = match shape {
        PartType::CornerWedge => &CORNER_WEDGE_CORNERS,
        _ => &WEDGE_CORNERS,
    };
    let local: Vec<Vec3> = corners.iter().map(|c| Vec3::from_array(*c) * local_full).collect();
    if let Some(hull) = compensated_hull(&local, transform_scale) {
        return hull;
    }
    Collider::convex_hull(local).unwrap_or_else(|| Collider::cuboid(local_full.x, local_full.y, local_full.z))
}

/// A convex-hull collider of `local` (a part's corners in collider-local
/// space) that comes out as the exact hull of `local * transform_scale` once
/// Avian has applied the scale, with true face normals.
///
/// Avian applies `|scale|` to a hull through parry's
/// `ConvexPolyhedron::scaled`. That multiplies the points by the scale, which
/// is right, and multiplies each face normal by it as well, where a normal has
/// to be divided. On a non-uniform scale (every stretched ramp) the stored
/// normals come out skewed, and contact generation uses them to pick which
/// face of the ramp a body touches. Scaling by `f` and then by `g` is the same
/// as scaling by `f * g`, so a hull stored already scaled by one over the
/// scale Avian will apply comes out exact, normals included. A negative scale
/// cannot reach the hull (Avian takes its absolute value), so a mirrored part's
/// mirror is baked into the points.
///
/// Normals depend on direction only, so the hull is built from points brought
/// to a unit reach first: a micron-sized part would otherwise sit inside the
/// hull builder's coplanarity tolerance.
///
/// `None` when the hull cannot be built; the caller falls back to a plain one.
#[cfg(feature = "physics")]
fn compensated_hull(local: &[Vec3], transform_scale: Vec3) -> Option<avian3d::prelude::Collider> {
    use avian3d::parry::shape::{ConvexPolyhedron, SharedShape};

    let axis = |s: f32| if s.is_finite() && s.abs() > 1.0e-12 { s } else { 1.0 };
    let signed = Vec3::new(axis(transform_scale.x), axis(transform_scale.y), axis(transform_scale.z));
    let applied = signed.abs();
    let world: Vec<Vec3> = local.iter().map(|p| *p * signed).collect();
    let reach = world.iter().fold(0.0_f32, |m, p| m.max(p.abs().max_element()));
    if !(reach.is_finite() && reach > 0.0) {
        return None;
    }
    let conditioned: Vec<Vec3> = world.iter().map(|p| *p / reach).collect();
    let stored = ConvexPolyhedron::from_convex_hull(&conditioned)?.scaled(Vec3::splat(reach) / applied)?;
    Some(avian3d::prelude::Collider::from(SharedShape::new(stored)))
}

#[cfg(all(test, feature = "physics"))]
mod shape_collider_tests {
    use super::*;
    use crate::part_meshes::{CORNER_WEDGE_CORNERS, WEDGE_CORNERS};
    use avian3d::parry::shape::ConvexPolyhedron;
    use avian3d::prelude::Collider;

    /// Sizes that stretch a ramp hard along different axes, and one that does not.
    const SIZES: [Vec3; 4] = [
        Vec3::new(4.0, 1.0, 2.0),
        Vec3::new(1.0, 8.0, 1.0),
        Vec3::new(0.3, 5.0, 40.0),
        Vec3::new(2.0, 2.0, 2.0),
    ];

    /// The collider once Avian has applied the part's scale, as it does when
    /// the entity's transform scale reaches it.
    fn avian_scales(mut collider: Collider, scale: Vec3) -> Collider {
        collider.set_scale(scale, 10);
        collider
    }

    fn corners_of(shape: PartType) -> &'static [[f32; 3]] {
        match shape {
            PartType::CornerWedge => &CORNER_WEDGE_CORNERS,
            _ => &WEDGE_CORNERS,
        }
    }

    /// The hull built straight from the part's corners at its final size: the
    /// truth a scaled collider has to match.
    fn true_hull(shape: PartType, size: Vec3) -> ConvexPolyhedron {
        let points: Vec<Vec3> = corners_of(shape).iter().map(|c| Vec3::from_array(*c) * size).collect();
        ConvexPolyhedron::from_convex_hull(&points).expect("the corners make a hull")
    }

    fn has_corner(hull: &ConvexPolyhedron, p: Vec3) -> bool {
        hull.points().iter().any(|q| q.distance(p) < 1.0e-4 * (1.0 + p.length()))
    }

    /// Every face of `b` has a face of `a` facing the same way.
    fn covers(a: &ConvexPolyhedron, b: &ConvexPolyhedron) -> bool {
        b.faces().iter().all(|f| a.faces().iter().any(|g| g.normal.dot(f.normal) > 1.0 - 1.0e-4))
    }

    fn same_hull(got: &ConvexPolyhedron, want: &ConvexPolyhedron) -> bool {
        want.points().iter().all(|p| has_corner(got, *p))
            && got.points().iter().all(|p| has_corner(want, *p))
            && covers(got, want)
            && covers(want, got)
    }

    fn scaled_hull(collider: &Collider) -> &ConvexPolyhedron {
        collider.shape_scaled().as_convex_polyhedron().expect("a wedge collides as a convex hull")
    }

    #[test]
    fn a_stretched_ramp_collides_with_the_faces_it_draws() {
        for shape in [PartType::Wedge, PartType::CornerWedge] {
            for size in SIZES {
                let collider = avian_scales(shape_collider(shape, Vec3::ONE, size), size);
                assert!(
                    same_hull(scaled_hull(&collider), &true_hull(shape, size)),
                    "{shape:?} at {size}: corners or face normals differ from the drawn shape"
                );
            }
        }
    }

    /// The compensation exists because parry skews a stretched hull's normals.
    /// The day this fails parry has fixed `ConvexPolyhedron::scaled`, and the
    /// compensation in `compensated_hull` would then skew them the other way:
    /// remove it and build the plain hull.
    #[test]
    fn parry_scaling_alone_skews_a_slope() {
        let size = Vec3::new(4.0, 1.0, 2.0);
        let naive = true_hull(PartType::Wedge, Vec3::ONE).scaled(size).expect("scales");
        assert!(!covers(&naive, &true_hull(PartType::Wedge, size)));
    }

    #[test]
    fn a_collider_sized_in_local_space_is_scaled_to_the_part() {
        // The local size is a part's size over its scale, here a half-size part.
        let collider = avian_scales(shape_collider(PartType::Wedge, Vec3::splat(0.5), Vec3::splat(2.0)), Vec3::splat(2.0));
        assert!(same_hull(scaled_hull(&collider), &true_hull(PartType::Wedge, Vec3::ONE)));
    }

    #[test]
    fn a_mirrored_part_collides_mirrored() {
        // Avian applies the absolute value of a scale, so the mirror is in the points.
        let scale = Vec3::new(2.0, 1.0, -3.0);
        let collider = avian_scales(shape_collider(PartType::Wedge, Vec3::ONE, scale), scale);
        let want = {
            let points: Vec<Vec3> = WEDGE_CORNERS.iter().map(|c| Vec3::from_array(*c) * scale).collect();
            ConvexPolyhedron::from_convex_hull(&points).expect("hull")
        };
        assert!(same_hull(scaled_hull(&collider), &want));
        assert!(!same_hull(scaled_hull(&collider), &true_hull(PartType::Wedge, scale.abs())), "front and back differ");
    }

    #[test]
    fn a_ramp_weighs_its_volume() {
        let size = Vec3::new(4.0, 1.0, 2.0);
        let box_volume = size.x * size.y * size.z;
        for (shape, share) in [(PartType::Wedge, 0.5), (PartType::CornerWedge, 1.0 / 3.0)] {
            let collider = avian_scales(shape_collider(shape, Vec3::ONE, size), size);
            let mass = collider.shape_scaled().mass_properties(1.0).mass();
            assert!((mass - box_volume * share).abs() < 1.0e-3 * box_volume, "{shape:?} weighs {mass}");
        }
    }

    #[test]
    fn the_primitives_keep_their_shapes() {
        let full = Vec3::new(2.0, 3.0, 4.0);
        let block = shape_collider(PartType::Block, full, Vec3::ONE);
        assert_eq!(block.shape().as_cuboid().expect("a box").half_extents, full * 0.5);
        let ball = shape_collider(PartType::Ball, full, Vec3::ONE);
        assert_eq!(ball.shape().as_ball().expect("a ball").radius, 1.0);
        for shape in [PartType::Cylinder, PartType::Cone] {
            let round = shape_collider(shape, full, Vec3::ONE);
            let cylinder = round.shape().as_cylinder().expect("a cylinder");
            assert_eq!((cylinder.radius, cylinder.half_height), (1.0, 1.5));
        }
    }

    #[test]
    fn a_broken_scale_still_makes_a_hull() {
        let collider = shape_collider(PartType::Wedge, Vec3::ONE, Vec3::new(0.0, f32::NAN, 1.0e-20));
        assert!(collider.shape().as_convex_polyhedron().is_some());
    }
}
