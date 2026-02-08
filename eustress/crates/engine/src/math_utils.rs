// ============================================================================
// Eustress Engine - Math Utilities
// Shared math utilities (ray intersection, AABB, etc.)
// ============================================================================

use bevy::prelude::*;

/// Ray-AABB intersection test
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

/// Calculate AABB from a set of points
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

/// Clamp a value to a range
pub fn clamp<T: PartialOrd>(value: T, min: T, max: T) -> T {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Ray-plane intersection
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

/// Ray to line segment distance
pub fn ray_to_line_segment_distance(
    ray_origin: Vec3,
    ray_direction: Vec3,
    _line_start: Vec3,
    _line_end: Vec3,
) -> f32 {
    // Simplified stub - returns distance to ray origin
    ray_origin.length()
}

/// Ray to point distance
pub fn ray_to_point_distance(
    ray_origin: Vec3,
    ray_direction: Vec3,
    point: Vec3,
) -> f32 {
    let v = point - ray_origin;
    let t = v.dot(ray_direction);
    if t <= 0.0 {
        v.length()
    } else {
        (v - ray_direction * t).length()
    }
}

/// Check if ray intersects a part (simplified)
pub fn ray_intersects_part(
    _ray_origin: Vec3,
    _ray_direction: Vec3,
    _part_transform: &Transform,
    _part_size: Vec3,
) -> Option<f32> {
    // Stub - always returns None
    None
}

/// Calculate rotated AABB
pub fn calculate_rotated_aabb(
    center: Vec3,
    half_extents: Vec3,
    _rotation: Quat,
) -> (Vec3, Vec3) {
    // Simplified - ignores rotation
    (center - half_extents, center + half_extents)
}

/// Ray-OBB intersection
pub fn ray_obb_intersection(
    ray_origin: Vec3,
    ray_direction: Vec3,
    obb_center: Vec3,
    obb_half_extents: Vec3,
    _obb_rotation: Quat,
) -> Option<f32> {
    // Simplified - treats as AABB
    ray_aabb_intersection(
        ray_origin,
        ray_direction,
        obb_center - obb_half_extents,
        obb_center + obb_half_extents,
    )
}

/// Align to surface normal
pub fn align_to_surface(
    _position: Vec3,
    surface_normal: Vec3,
) -> Quat {
    Quat::from_rotation_arc(Vec3::Y, surface_normal)
}
