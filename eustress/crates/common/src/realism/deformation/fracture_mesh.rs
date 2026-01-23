//! # Fracture Mesh Operations
//!
//! Procedural mesh splitting for fracture simulation.

use bevy::prelude::*;
use bevy::mesh::{Mesh, VertexAttributeValues, Indices, PrimitiveTopology};

use super::vertex::VertexData;

// ============================================================================
// Mesh Splitting
// ============================================================================

/// Result of mesh split operation
#[derive(Debug)]
pub struct MeshSplitResult {
    /// Mesh on positive side of plane
    pub positive: Option<Mesh>,
    /// Mesh on negative side of plane
    pub negative: Option<Mesh>,
    /// New vertices created at cut
    pub cut_vertices: Vec<Vec3>,
    /// Success flag
    pub success: bool,
}

impl Default for MeshSplitResult {
    fn default() -> Self {
        Self {
            positive: None,
            negative: None,
            cut_vertices: Vec::new(),
            success: false,
        }
    }
}

/// Split mesh by plane
/// 
/// # Arguments
/// * `mesh` - Source mesh to split
/// * `plane_origin` - Point on the cutting plane
/// * `plane_normal` - Normal of the cutting plane
pub fn split_mesh_by_plane(
    mesh: &Mesh,
    plane_origin: Vec3,
    plane_normal: Vec3,
) -> MeshSplitResult {
    let vertex_data = VertexData::from_mesh(mesh);
    
    if vertex_data.positions.is_empty() || vertex_data.indices.is_empty() {
        return MeshSplitResult::default();
    }
    
    let normal = plane_normal.normalize_or_zero();
    if normal.length_squared() < 0.001 {
        return MeshSplitResult::default();
    }
    
    // Classify vertices
    let mut vertex_sides: Vec<i8> = Vec::with_capacity(vertex_data.positions.len());
    for pos in &vertex_data.positions {
        let d = (*pos - plane_origin).dot(normal);
        if d > 0.001 {
            vertex_sides.push(1);  // Positive side
        } else if d < -0.001 {
            vertex_sides.push(-1); // Negative side
        } else {
            vertex_sides.push(0);  // On plane
        }
    }
    
    // Check if plane actually splits the mesh
    let has_positive = vertex_sides.iter().any(|&s| s > 0);
    let has_negative = vertex_sides.iter().any(|&s| s < 0);
    
    if !has_positive || !has_negative {
        // Plane doesn't split mesh
        return MeshSplitResult::default();
    }
    
    // Build new meshes
    let mut pos_positions: Vec<[f32; 3]> = Vec::new();
    let mut pos_normals: Vec<[f32; 3]> = Vec::new();
    let mut pos_uvs: Vec<[f32; 2]> = Vec::new();
    let mut pos_indices: Vec<u32> = Vec::new();
    
    let mut neg_positions: Vec<[f32; 3]> = Vec::new();
    let mut neg_normals: Vec<[f32; 3]> = Vec::new();
    let mut neg_uvs: Vec<[f32; 2]> = Vec::new();
    let mut neg_indices: Vec<u32> = Vec::new();
    
    let mut cut_vertices: Vec<Vec3> = Vec::new();
    
    // Vertex index mapping (original -> new for each side)
    let mut pos_vertex_map: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
    let mut neg_vertex_map: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
    
    // Process each triangle
    for tri in vertex_data.indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;
        
        let s0 = vertex_sides[i0];
        let s1 = vertex_sides[i1];
        let s2 = vertex_sides[i2];
        
        // All on same side - add to that mesh
        if s0 >= 0 && s1 >= 0 && s2 >= 0 && (s0 > 0 || s1 > 0 || s2 > 0) {
            // Positive side
            add_triangle_to_mesh(
                &vertex_data, i0, i1, i2,
                &mut pos_positions, &mut pos_normals, &mut pos_uvs, &mut pos_indices,
                &mut pos_vertex_map,
            );
        } else if s0 <= 0 && s1 <= 0 && s2 <= 0 && (s0 < 0 || s1 < 0 || s2 < 0) {
            // Negative side
            add_triangle_to_mesh(
                &vertex_data, i0, i1, i2,
                &mut neg_positions, &mut neg_normals, &mut neg_uvs, &mut neg_indices,
                &mut neg_vertex_map,
            );
        } else {
            // Triangle crosses plane - need to split
            split_triangle(
                &vertex_data,
                i0, i1, i2,
                s0, s1, s2,
                plane_origin, normal,
                &mut pos_positions, &mut pos_normals, &mut pos_uvs, &mut pos_indices,
                &mut neg_positions, &mut neg_normals, &mut neg_uvs, &mut neg_indices,
                &mut cut_vertices,
            );
        }
    }
    
    // Build meshes
    let positive = if !pos_positions.is_empty() {
        Some(build_mesh(pos_positions, pos_normals, pos_uvs, pos_indices))
    } else {
        None
    };
    
    let negative = if !neg_positions.is_empty() {
        Some(build_mesh(neg_positions, neg_normals, neg_uvs, neg_indices))
    } else {
        None
    };
    
    let success = positive.is_some() && negative.is_some();
    MeshSplitResult {
        positive,
        negative,
        cut_vertices,
        success,
    }
}

/// Add triangle to mesh being built
fn add_triangle_to_mesh(
    vertex_data: &VertexData,
    i0: usize, i1: usize, i2: usize,
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    vertex_map: &mut std::collections::HashMap<usize, u32>,
) {
    for &idx in &[i0, i1, i2] {
        let new_idx = if let Some(&mapped) = vertex_map.get(&idx) {
            mapped
        } else {
            let new_idx = positions.len() as u32;
            
            let pos = vertex_data.positions[idx];
            positions.push([pos.x, pos.y, pos.z]);
            
            if idx < vertex_data.normals.len() {
                let n = vertex_data.normals[idx];
                normals.push([n.x, n.y, n.z]);
            } else {
                normals.push([0.0, 1.0, 0.0]);
            }
            
            if idx < vertex_data.uvs.len() {
                let uv = vertex_data.uvs[idx];
                uvs.push([uv.x, uv.y]);
            } else {
                uvs.push([0.0, 0.0]);
            }
            
            vertex_map.insert(idx, new_idx);
            new_idx
        };
        
        indices.push(new_idx);
    }
}

/// Split triangle that crosses plane
#[allow(clippy::too_many_arguments)]
fn split_triangle(
    vertex_data: &VertexData,
    i0: usize, i1: usize, i2: usize,
    s0: i8, s1: i8, s2: i8,
    plane_origin: Vec3, plane_normal: Vec3,
    pos_positions: &mut Vec<[f32; 3]>,
    pos_normals: &mut Vec<[f32; 3]>,
    pos_uvs: &mut Vec<[f32; 2]>,
    pos_indices: &mut Vec<u32>,
    neg_positions: &mut Vec<[f32; 3]>,
    neg_normals: &mut Vec<[f32; 3]>,
    neg_uvs: &mut Vec<[f32; 2]>,
    neg_indices: &mut Vec<u32>,
    cut_vertices: &mut Vec<Vec3>,
) {
    let p0 = vertex_data.positions[i0];
    let p1 = vertex_data.positions[i1];
    let p2 = vertex_data.positions[i2];
    
    let n0 = vertex_data.normals.get(i0).copied().unwrap_or(Vec3::Y);
    let n1 = vertex_data.normals.get(i1).copied().unwrap_or(Vec3::Y);
    let n2 = vertex_data.normals.get(i2).copied().unwrap_or(Vec3::Y);
    
    let uv0 = vertex_data.uvs.get(i0).copied().unwrap_or(Vec2::ZERO);
    let uv1 = vertex_data.uvs.get(i1).copied().unwrap_or(Vec2::ZERO);
    let uv2 = vertex_data.uvs.get(i2).copied().unwrap_or(Vec2::ZERO);
    
    // Find intersection points on edges that cross the plane
    let mut intersections: Vec<(Vec3, Vec3, Vec2, usize, usize)> = Vec::new();
    
    // Check each edge
    for &(ia, ib, sa, sb) in &[(i0, i1, s0, s1), (i1, i2, s1, s2), (i2, i0, s2, s0)] {
        if (sa > 0 && sb < 0) || (sa < 0 && sb > 0) {
            let pa = vertex_data.positions[ia];
            let pb = vertex_data.positions[ib];
            
            // Find intersection point
            let d_a = (pa - plane_origin).dot(plane_normal);
            let d_b = (pb - plane_origin).dot(plane_normal);
            let t = d_a / (d_a - d_b);
            
            let intersection = pa + (pb - pa) * t;
            
            // Interpolate normal and UV
            let na = vertex_data.normals.get(ia).copied().unwrap_or(Vec3::Y);
            let nb = vertex_data.normals.get(ib).copied().unwrap_or(Vec3::Y);
            let interp_normal = (na + (nb - na) * t).normalize_or_zero();
            
            let uva = vertex_data.uvs.get(ia).copied().unwrap_or(Vec2::ZERO);
            let uvb = vertex_data.uvs.get(ib).copied().unwrap_or(Vec2::ZERO);
            let interp_uv = uva + (uvb - uva) * t;
            
            intersections.push((intersection, interp_normal, interp_uv, ia, ib));
            cut_vertices.push(intersection);
        }
    }
    
    // Simplified: just add the original triangle to both sides
    // Full implementation would properly triangulate the split pieces
    
    // For now, add to positive side if majority positive
    let pos_count = [s0, s1, s2].iter().filter(|&&s| s > 0).count();
    
    if pos_count >= 2 {
        let idx_base = pos_positions.len() as u32;
        pos_positions.push([p0.x, p0.y, p0.z]);
        pos_positions.push([p1.x, p1.y, p1.z]);
        pos_positions.push([p2.x, p2.y, p2.z]);
        pos_normals.push([n0.x, n0.y, n0.z]);
        pos_normals.push([n1.x, n1.y, n1.z]);
        pos_normals.push([n2.x, n2.y, n2.z]);
        pos_uvs.push([uv0.x, uv0.y]);
        pos_uvs.push([uv1.x, uv1.y]);
        pos_uvs.push([uv2.x, uv2.y]);
        pos_indices.push(idx_base);
        pos_indices.push(idx_base + 1);
        pos_indices.push(idx_base + 2);
    } else {
        let idx_base = neg_positions.len() as u32;
        neg_positions.push([p0.x, p0.y, p0.z]);
        neg_positions.push([p1.x, p1.y, p1.z]);
        neg_positions.push([p2.x, p2.y, p2.z]);
        neg_normals.push([n0.x, n0.y, n0.z]);
        neg_normals.push([n1.x, n1.y, n1.z]);
        neg_normals.push([n2.x, n2.y, n2.z]);
        neg_uvs.push([uv0.x, uv0.y]);
        neg_uvs.push([uv1.x, uv1.y]);
        neg_uvs.push([uv2.x, uv2.y]);
        neg_indices.push(idx_base);
        neg_indices.push(idx_base + 1);
        neg_indices.push(idx_base + 2);
    }
}

/// Build mesh from vertex data
fn build_mesh(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    
    mesh
}

// ============================================================================
// Voronoi Fracture
// ============================================================================

/// Generate Voronoi fracture pattern
pub fn generate_voronoi_points(
    bounds_min: Vec3,
    bounds_max: Vec3,
    num_points: usize,
    seed: u64,
) -> Vec<Vec3> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut points = Vec::with_capacity(num_points);
    let size = bounds_max - bounds_min;
    
    for i in 0..num_points {
        // Simple pseudo-random based on seed and index
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        i.hash(&mut hasher);
        let h1 = hasher.finish();
        
        (i + 1).hash(&mut hasher);
        let h2 = hasher.finish();
        
        (i + 2).hash(&mut hasher);
        let h3 = hasher.finish();
        
        let x = (h1 as f32 / u64::MAX as f32) * size.x + bounds_min.x;
        let y = (h2 as f32 / u64::MAX as f32) * size.y + bounds_min.y;
        let z = (h3 as f32 / u64::MAX as f32) * size.z + bounds_min.z;
        
        points.push(Vec3::new(x, y, z));
    }
    
    points
}

/// Fracture mesh using Voronoi pattern
pub fn voronoi_fracture(
    mesh: &Mesh,
    impact_point: Vec3,
    num_fragments: usize,
    seed: u64,
) -> Vec<Mesh> {
    let vertex_data = VertexData::from_mesh(mesh);
    let (bounds_min, bounds_max) = vertex_data.bounds();
    
    // Generate Voronoi seed points around impact
    let voronoi_points = generate_voronoi_points(bounds_min, bounds_max, num_fragments, seed);
    
    let mut fragments = Vec::new();
    
    // For each Voronoi cell, create a fragment
    // This is a simplified implementation - full version would use proper Voronoi tessellation
    for (i, &center) in voronoi_points.iter().enumerate() {
        // Create cutting planes between this cell and neighbors
        for (j, &other) in voronoi_points.iter().enumerate() {
            if i >= j {
                continue;
            }
            
            let midpoint = (center + other) * 0.5;
            let normal = (other - center).normalize_or_zero();
            
            // Split mesh by this plane
            let result = split_mesh_by_plane(mesh, midpoint, normal);
            
            if let Some(fragment) = result.positive {
                fragments.push(fragment);
            }
        }
    }
    
    // If no fragments created, return original mesh
    if fragments.is_empty() {
        fragments.push(mesh.clone());
    }
    
    fragments
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_voronoi_points() {
        let points = generate_voronoi_points(
            Vec3::ZERO,
            Vec3::ONE,
            10,
            12345,
        );
        
        assert_eq!(points.len(), 10);
        
        for p in &points {
            assert!(p.x >= 0.0 && p.x <= 1.0);
            assert!(p.y >= 0.0 && p.y <= 1.0);
            assert!(p.z >= 0.0 && p.z <= 1.0);
        }
    }
}
