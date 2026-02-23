//! # Vector Feature Rendering
//!
//! Converts projected geospatial geometries into Bevy 3D meshes.
//! - LineString → extruded tube/ribbon mesh
//! - Point → instanced marker (sphere, cylinder, cube)
//! - Polygon → flat extruded prism or draped surface
//!
//! ## Table of Contents
//! 1. Tube mesh generation (LineString → 3D tube)
//! 2. Marker mesh generation (Point → 3D primitive)
//! 3. Polygon mesh generation (Polygon → flat prism)

use bevy::prelude::*;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::asset::RenderAssetUsages;

// ============================================================================
// 1. Tube mesh generation (LineString → 3D tube)
// ============================================================================

/// Generate a tube mesh along a polyline path.
///
/// Creates a circular cross-section extruded along the path vertices.
/// Used for rendering pipeline routes, rivers, roads.
///
/// - `path` — Ordered vertices in Bevy world space
/// - `radius` — Tube radius in world units
/// - `segments` — Number of segments around the tube circumference (8–16 typical)
pub fn generate_tube_mesh(path: &[Vec3], radius: f32, segments: u32) -> Mesh {
    if path.len() < 2 {
        tracing::warn!("Tube mesh requires at least 2 path vertices, got {}", path.len());
        return Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    }

    let seg = segments.max(3) as usize;
    let num_path = path.len();
    let num_verts = num_path * (seg + 1);
    let num_indices = (num_path - 1) * seg * 6;

    let mut positions = Vec::with_capacity(num_verts);
    let mut normals = Vec::with_capacity(num_verts);
    let mut uvs = Vec::with_capacity(num_verts);
    let mut indices = Vec::with_capacity(num_indices);

    // Compute tangent, normal, binormal at each path vertex (Frenet frame)
    let tangents: Vec<Vec3> = (0..num_path)
        .map(|i| {
            if i == 0 {
                (path[1] - path[0]).normalize_or_zero()
            } else if i == num_path - 1 {
                (path[i] - path[i - 1]).normalize_or_zero()
            } else {
                ((path[i + 1] - path[i]).normalize_or_zero()
                    + (path[i] - path[i - 1]).normalize_or_zero())
                .normalize_or_zero()
            }
        })
        .collect();

    // Accumulated path length for UV mapping
    let mut accumulated_length = vec![0.0f32; num_path];
    for i in 1..num_path {
        accumulated_length[i] = accumulated_length[i - 1] + path[i].distance(path[i - 1]);
    }
    let total_length = accumulated_length.last().copied().unwrap_or(1.0).max(0.001);

    // Generate ring of vertices at each path point
    for (path_idx, &center) in path.iter().enumerate() {
        let tangent = tangents[path_idx];

        // Choose an initial normal perpendicular to tangent
        let up_candidate = if tangent.dot(Vec3::Y).abs() > 0.99 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let normal = tangent.cross(up_candidate).normalize_or_zero();
        let binormal = tangent.cross(normal).normalize_or_zero();

        let v = accumulated_length[path_idx] / total_length;

        for s in 0..=seg {
            let angle = (s as f32 / seg as f32) * std::f32::consts::TAU;
            let (sin_a, cos_a) = angle.sin_cos();

            let offset = normal * cos_a * radius + binormal * sin_a * radius;
            let pos = center + offset;
            let norm = offset.normalize_or_zero();
            let u = s as f32 / seg as f32;

            positions.push(pos.to_array());
            normals.push(norm.to_array());
            uvs.push([u, v]);
        }
    }

    // Generate triangle indices connecting adjacent rings
    let ring_size = (seg + 1) as u32;
    for i in 0..(num_path - 1) as u32 {
        let base_current = i * ring_size;
        let base_next = (i + 1) * ring_size;

        for s in 0..seg as u32 {
            let a = base_current + s;
            let b = base_current + s + 1;
            let c = base_next + s;
            let d = base_next + s + 1;

            // Two triangles per quad
            indices.push(a);
            indices.push(c);
            indices.push(b);

            indices.push(b);
            indices.push(c);
            indices.push(d);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Generate a flat ribbon mesh along a polyline path (cheaper than tube).
///
/// Creates a flat strip of quads along the path, always facing up (Y-axis).
/// Used for roads, boundaries, and other flat linear features.
///
/// - `path` — Ordered vertices in Bevy world space
/// - `width` — Ribbon width in world units
pub fn generate_ribbon_mesh(path: &[Vec3], width: f32) -> Mesh {
    if path.len() < 2 {
        tracing::warn!("Ribbon mesh requires at least 2 path vertices, got {}", path.len());
        return Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    }

    let num_path = path.len();
    let half_w = width * 0.5;

    let mut positions = Vec::with_capacity(num_path * 2);
    let mut normals = Vec::with_capacity(num_path * 2);
    let mut uvs = Vec::with_capacity(num_path * 2);
    let mut indices = Vec::with_capacity((num_path - 1) * 6);

    // Accumulated length for UV
    let mut accumulated_length = vec![0.0f32; num_path];
    for i in 1..num_path {
        accumulated_length[i] = accumulated_length[i - 1] + path[i].distance(path[i - 1]);
    }
    let total_length = accumulated_length.last().copied().unwrap_or(1.0).max(0.001);

    for (i, &center) in path.iter().enumerate() {
        // Tangent direction (forward along path)
        let tangent = if i == 0 {
            (path[1] - path[0]).normalize_or_zero()
        } else if i == num_path - 1 {
            (path[i] - path[i - 1]).normalize_or_zero()
        } else {
            ((path[i + 1] - path[i]).normalize_or_zero()
                + (path[i] - path[i - 1]).normalize_or_zero())
            .normalize_or_zero()
        };

        // Perpendicular in XZ plane (ribbon lies flat)
        let right = Vec3::new(-tangent.z, 0.0, tangent.x).normalize_or_zero() * half_w;

        let v = accumulated_length[i] / total_length;

        // Left vertex
        positions.push((center - right).to_array());
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([0.0, v]);

        // Right vertex
        positions.push((center + right).to_array());
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([1.0, v]);
    }

    // Indices: two triangles per segment
    for i in 0..(num_path - 1) as u32 {
        let bl = i * 2;
        let br = i * 2 + 1;
        let tl = (i + 1) * 2;
        let tr = (i + 1) * 2 + 1;

        indices.push(bl);
        indices.push(tl);
        indices.push(br);

        indices.push(br);
        indices.push(tl);
        indices.push(tr);
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ============================================================================
// 2. Marker mesh generation (Point → 3D primitive)
// ============================================================================

/// Marker shape for point features
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerShape {
    Sphere,
    Cylinder,
    Cube,
}

impl MarkerShape {
    /// Parse from string (from geo.toml style config)
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "sphere" | "ball" => MarkerShape::Sphere,
            "cylinder" | "tube" => MarkerShape::Cylinder,
            "cube" | "block" | "box" => MarkerShape::Cube,
            _ => MarkerShape::Sphere,
        }
    }
}

/// Generate a marker mesh for a point feature
pub fn generate_marker_mesh(shape: MarkerShape, radius: f32) -> Mesh {
    match shape {
        MarkerShape::Sphere => {
            Mesh::from(bevy::math::primitives::Sphere::new(radius))
        }
        MarkerShape::Cylinder => {
            Mesh::from(bevy::math::primitives::Cylinder::new(radius, radius * 4.0))
        }
        MarkerShape::Cube => {
            Mesh::from(bevy::math::primitives::Cuboid::new(
                radius * 2.0,
                radius * 2.0,
                radius * 2.0,
            ))
        }
    }
}

// ============================================================================
// 3. Polygon mesh generation (Polygon → flat prism)
// ============================================================================

/// Generate a flat polygon mesh from an outer ring (no holes, no extrusion).
///
/// Uses a simple ear-clipping triangulation projected onto the XZ plane.
/// Suitable for boundary overlays and flat area fills.
///
/// - `outer_ring` — Vertices in Bevy world space (should be roughly coplanar in XZ)
pub fn generate_flat_polygon_mesh(outer_ring: &[Vec3]) -> Mesh {
    if outer_ring.len() < 3 {
        tracing::warn!("Polygon mesh requires at least 3 vertices, got {}", outer_ring.len());
        return Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    }

    // Simple fan triangulation from first vertex (works for convex polygons)
    // For concave polygons, a proper ear-clipping algorithm would be needed
    let positions: Vec<[f32; 3]> = outer_ring.iter().map(|v| v.to_array()).collect();
    let normals: Vec<[f32; 3]> = vec![[0.0, 1.0, 0.0]; outer_ring.len()];

    // UV: project XZ to 0..1 range
    let (min_x, max_x, min_z, max_z) = outer_ring.iter().fold(
        (f32::MAX, f32::MIN, f32::MAX, f32::MIN),
        |(min_x, max_x, min_z, max_z), v| {
            (min_x.min(v.x), max_x.max(v.x), min_z.min(v.z), max_z.max(v.z))
        },
    );
    let range_x = (max_x - min_x).max(0.001);
    let range_z = (max_z - min_z).max(0.001);
    let uvs: Vec<[f32; 2]> = outer_ring
        .iter()
        .map(|v| [(v.x - min_x) / range_x, (v.z - min_z) / range_z])
        .collect();

    // Fan triangulation
    let mut indices = Vec::with_capacity((outer_ring.len() - 2) * 3);
    for i in 1..(outer_ring.len() - 1) {
        indices.push(0u32);
        indices.push(i as u32);
        indices.push((i + 1) as u32);
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
