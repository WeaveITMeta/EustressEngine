// ============================================================================
// Eustress Engine - Math Utilities
// ============================================================================
// ## Table of Contents
// 1. Ray-AABB intersection
// 2. Ray-OBB intersection (proper rotation support)
// 3. Rotated AABB calculation
// 4. Ray-to-segment / ray-to-point distance
// 5. Ray-plane intersection
// 6. Surface alignment
// 7. Part intersection helpers
// ============================================================================

use bevy::prelude::*;

// ============================================================================
// 1. Ray-AABB Intersection
// ============================================================================

/// Ray-AABB intersection test. Returns t (distance along ray) or None.
pub fn ray_aabb_intersection(
    ray_origin: Vec3,
    ray_direction: Vec3,
    aabb_min: Vec3,
    aabb_max: Vec3,
) -> Option<f32> {
    let inv_dir = Vec3::new(
        1.0 / ray_direction.x,
        1.0 / ray_direction.y,
        1.0 / ray_direction.z,
    );

    let t1 = (aabb_min.x - ray_origin.x) * inv_dir.x;
    let t2 = (aabb_max.x - ray_origin.x) * inv_dir.x;
    let t3 = (aabb_min.y - ray_origin.y) * inv_dir.y;
    let t4 = (aabb_max.y - ray_origin.y) * inv_dir.y;
    let t5 = (aabb_min.z - ray_origin.z) * inv_dir.z;
    let t6 = (aabb_max.z - ray_origin.z) * inv_dir.z;

    let tmin = t1.min(t2).max(t3.min(t4)).max(t5.min(t6));
    let tmax = t1.max(t2).min(t3.max(t4)).min(t5.max(t6));

    if tmax < 0.0 || tmin > tmax {
        None
    } else {
        Some(tmin.max(0.0))
    }
}

// ============================================================================
// 2. Ray-OBB Intersection (proper rotation)
// ============================================================================

/// Ray-OBB intersection using the separating axis theorem.
/// Transforms the ray into OBB local space and does an AABB test there.
/// Returns t (distance along ray) or None.
pub fn ray_obb_intersection(
    ray_origin: Vec3,
    ray_direction: Vec3,
    obb_center: Vec3,
    obb_half_extents: Vec3,
    obb_rotation: Quat,
) -> Option<f32> {
    // Transform ray into OBB local space
    let inv_rot = obb_rotation.inverse();
    let local_origin = inv_rot * (ray_origin - obb_center);
    let local_dir = inv_rot * ray_direction;

    ray_aabb_intersection(
        local_origin,
        local_dir,
        -obb_half_extents,
        obb_half_extents,
    )
}

// ============================================================================
// 3. Rotated AABB Calculation
// ============================================================================

/// Calculate world-space AABB of a rotated box.
/// Returns (min, max) of the axis-aligned bounding box that contains the OBB.
pub fn calculate_rotated_aabb(
    center: Vec3,
    half_extents: Vec3,
    rotation: Quat,
) -> (Vec3, Vec3) {
    // Project the three rotated half-extent vectors onto world axes
    let rx = rotation * Vec3::new(half_extents.x, 0.0, 0.0);
    let ry = rotation * Vec3::new(0.0, half_extents.y, 0.0);
    let rz = rotation * Vec3::new(0.0, 0.0, half_extents.z);

    // The AABB half-extents are the sum of absolute projections
    let aabb_half = Vec3::new(
        rx.x.abs() + ry.x.abs() + rz.x.abs(),
        rx.y.abs() + ry.y.abs() + rz.y.abs(),
        rx.z.abs() + ry.z.abs() + rz.z.abs(),
    );

    (center - aabb_half, center + aabb_half)
}

/// Calculate AABB from a set of points.
pub fn calculate_aabb(points: &[Vec3]) -> (Vec3, Vec3) {
    if points.is_empty() {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    let mut min = points[0];
    let mut max = points[0];
    for point in points.iter().skip(1) {
        min = min.min(*point);
        max = max.max(*point);
    }
    (min, max)
}

// ============================================================================
// 4. Ray-to-Segment / Ray-to-Point Distance
// ============================================================================

/// Minimum distance from a ray to a 3D line segment.
/// Used for axis handle hit detection.
pub fn ray_to_line_segment_distance(
    ray_origin: Vec3,
    ray_direction: Vec3,
    seg_start: Vec3,
    seg_end: Vec3,
) -> f32 {
    let seg_dir = seg_end - seg_start;
    let seg_len = seg_dir.length();
    if seg_len < 1e-6 {
        return ray_to_point_distance(ray_origin, ray_direction, seg_start);
    }
    let seg_unit = seg_dir / seg_len;

    // Closest point between two infinite lines, then clamp to segment
    let w0 = ray_origin - seg_start;
    let a = ray_direction.dot(ray_direction); // always 1 if normalized
    let b = ray_direction.dot(seg_unit);
    let c = seg_unit.dot(seg_unit); // always 1
    let d = ray_direction.dot(w0);
    let e = seg_unit.dot(w0);

    let denom = a * c - b * b;

    let (sc, tc) = if denom.abs() < 1e-6 {
        // Lines are parallel
        (0.0_f32, e / c)
    } else {
        let sc = (b * e - c * d) / denom;
        let tc = (a * e - b * d) / denom;
        (sc.max(0.0), tc)
    };

    // Clamp tc to [0, seg_len]
    let tc_clamped = tc.clamp(0.0, seg_len);

    let closest_on_ray = ray_origin + ray_direction * sc.max(0.0);
    let closest_on_seg = seg_start + seg_unit * tc_clamped;

    (closest_on_ray - closest_on_seg).length()
}

/// Minimum distance from a ray to a point.
pub fn ray_to_point_distance(
    ray_origin: Vec3,
    ray_direction: Vec3,
    point: Vec3,
) -> f32 {
    let v = point - ray_origin;
    let t = v.dot(ray_direction).max(0.0);
    (v - ray_direction * t).length()
}

// ============================================================================
// 5. Ray-Plane Intersection
// ============================================================================

/// Ray-plane intersection. Returns t (distance along ray) or None.
pub fn ray_plane_intersection(
    ray_origin: Vec3,
    ray_direction: Vec3,
    plane_point: Vec3,
    plane_normal: Vec3,
) -> Option<f32> {
    let denom = plane_normal.dot(ray_direction);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = (plane_point - ray_origin).dot(plane_normal) / denom;
    if t >= 0.0 { Some(t) } else { None }
}

// ============================================================================
// 6. Surface Alignment
// ============================================================================

/// Clamp a value to a range.
pub fn clamp<T: PartialOrd>(value: T, min: T, max: T) -> T {
    if value < min { min } else if value > max { max } else { value }
}

/// Align rotation so that local +Y points along surface_normal.
pub fn align_to_surface(_position: Vec3, surface_normal: Vec3) -> Quat {
    Quat::from_rotation_arc(Vec3::Y, surface_normal)
}

// ============================================================================
// 7. Part Intersection Helpers
// ============================================================================

/// Check if a ray intersects a part using proper OBB test.
/// Returns t (distance along ray) or None.
pub fn ray_intersects_part(
    ray_origin: Vec3,
    ray_direction: Vec3,
    part_transform: &Transform,
    part_size: Vec3,
) -> Option<f32> {
    ray_obb_intersection(
        ray_origin,
        ray_direction,
        part_transform.translation,
        part_size * 0.5,
        part_transform.rotation,
    )
}

/// Check if a ray intersects a part using world-space position/rotation/size.
/// Returns true if the ray hits the OBB.
pub fn ray_intersects_part_rotated(
    ray: &Ray3d,
    center: Vec3,
    rotation: Quat,
    size: Vec3,
) -> bool {
    ray_obb_intersection(
        ray.origin,
        *ray.direction,
        center,
        size * 0.5,
        rotation,
    ).is_some()
}

/// Calculate the offset needed to place a part's bottom face on a surface.
/// Returns the distance from the part center to the surface contact point.
pub fn calculate_surface_offset(
    part_size: &Vec3,
    part_rotation: &Quat,
    surface_normal: &Vec3,
) -> f32 {
    // Project the rotated half-extents onto the surface normal
    // to find the maximum extent in the normal direction
    let half = *part_size * 0.5;
    let rx = (*part_rotation * Vec3::X * half.x).dot(*surface_normal).abs();
    let ry = (*part_rotation * Vec3::Y * half.y).dot(*surface_normal).abs();
    let rz = (*part_rotation * Vec3::Z * half.z).dot(*surface_normal).abs();
    rx + ry + rz
}

/// Find the surface under the cursor using mesh-based raycasting (fallback when no physics).
pub fn find_surface_under_cursor_with_normal<T: bevy::ecs::query::QueryFilter>(
    ray: &Ray3d,
    all_parts_query: &Query<(Entity, &GlobalTransform, &Mesh3d, Option<&crate::rendering::PartEntity>, Option<&crate::classes::Instance>, Option<&crate::classes::BasePart>), T>,
    excluded_entities: &[Entity],
) -> Option<(Vec3, Vec3)> {
    let mut closest_t = f32::MAX;
    let mut closest_hit: Option<(Vec3, Vec3)> = None;

    for (entity, global_transform, _mesh, _part_entity, _instance, base_part) in all_parts_query.iter() {
        if excluded_entities.contains(&entity) {
            continue;
        }

        let t_world = global_transform.compute_transform();
        let size = base_part.map(|bp| bp.size).unwrap_or(t_world.scale);

        if let Some(t) = ray_obb_intersection(
            ray.origin,
            *ray.direction,
            t_world.translation,
            size * 0.5,
            t_world.rotation,
        ) {
            if t < closest_t {
                closest_t = t;
                let hit_point = ray.origin + *ray.direction * t;

                // Estimate surface normal: find which face of the OBB was hit
                let normal = estimate_obb_hit_normal(
                    ray.origin,
                    *ray.direction,
                    t_world.translation,
                    size * 0.5,
                    t_world.rotation,
                );
                closest_hit = Some((hit_point, normal));
            }
        }
    }

    closest_hit
}

/// Estimate which face of an OBB was hit and return its world-space normal.
fn estimate_obb_hit_normal(
    ray_origin: Vec3,
    ray_direction: Vec3,
    obb_center: Vec3,
    obb_half_extents: Vec3,
    obb_rotation: Quat,
) -> Vec3 {
    let inv_rot = obb_rotation.inverse();
    let local_origin = inv_rot * (ray_origin - obb_center);
    let local_dir = inv_rot * ray_direction;

    // Find t in local space
    let t = ray_aabb_intersection(local_origin, local_dir, -obb_half_extents, obb_half_extents)
        .unwrap_or(0.0);

    let local_hit = local_origin + local_dir * t;

    // Find which face is closest (largest normalized component)
    let normalized = local_hit / obb_half_extents.max(Vec3::splat(1e-6));
    let abs_n = normalized.abs();

    let local_normal = if abs_n.x >= abs_n.y && abs_n.x >= abs_n.z {
        Vec3::new(normalized.x.signum(), 0.0, 0.0)
    } else if abs_n.y >= abs_n.x && abs_n.y >= abs_n.z {
        Vec3::new(0.0, normalized.y.signum(), 0.0)
    } else {
        Vec3::new(0.0, 0.0, normalized.z.signum())
    };

    (obb_rotation * local_normal).normalize()
}

/// Find a surface using Avian3D physics spatial query.
pub fn find_surface_with_physics(
    spatial_query: &avian3d::prelude::SpatialQuery,
    ray: &Ray3d,
    excluded_entities: &[Entity],
) -> Option<(Vec3, Vec3, Entity)> {
    use avian3d::prelude::SpatialQueryFilter;

    let filter = SpatialQueryFilter::default()
        .with_excluded_entities(excluded_entities.to_vec());

    let Ok(dir) = Dir3::new(*ray.direction) else { return None };

    let hits = spatial_query.ray_hits(ray.origin, dir, 1000.0, 10, true, &filter);

    hits.into_iter()
        .min_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(std::cmp::Ordering::Equal))
        .map(|hit| {
            let point = ray.origin + *ray.direction * hit.distance;
            let normal = hit.normal.normalize();
            (point, normal, hit.entity)
        })
}

/// Snap a position to a grid.
pub fn snap_to_grid(pos: Vec3, snap_size: f32) -> Vec3 {
    if snap_size <= 0.0 {
        return pos;
    }
    Vec3::new(
        (pos.x / snap_size).round() * snap_size,
        (pos.y / snap_size).round() * snap_size,
        (pos.z / snap_size).round() * snap_size,
    )
}
