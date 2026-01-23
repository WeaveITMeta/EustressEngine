//! # Vertex Operations
//!
//! Low-level vertex manipulation for deformation.

use bevy::prelude::*;
use bevy::mesh::{Mesh, VertexAttributeValues, Indices};

// ============================================================================
// Vertex Data
// ============================================================================

/// Extracted vertex data for deformation calculations
#[derive(Clone, Debug)]
pub struct VertexData {
    /// Original positions
    pub positions: Vec<Vec3>,
    /// Original normals
    pub normals: Vec<Vec3>,
    /// Tangents (if available)
    pub tangents: Vec<Vec4>,
    /// UV coordinates
    pub uvs: Vec<Vec2>,
    /// Vertex indices (triangles)
    pub indices: Vec<u32>,
}

impl Default for VertexData {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            tangents: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
        }
    }
}

impl VertexData {
    /// Extract vertex data from mesh
    pub fn from_mesh(mesh: &Mesh) -> Self {
        let mut data = Self::default();
        
        // Positions
        if let Some(VertexAttributeValues::Float32x3(positions)) = 
            mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            data.positions = positions.iter()
                .map(|p| Vec3::new(p[0], p[1], p[2]))
                .collect();
        }
        
        // Normals
        if let Some(VertexAttributeValues::Float32x3(normals)) = 
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            data.normals = normals.iter()
                .map(|n| Vec3::new(n[0], n[1], n[2]))
                .collect();
        }
        
        // Tangents
        if let Some(VertexAttributeValues::Float32x4(tangents)) = 
            mesh.attribute(Mesh::ATTRIBUTE_TANGENT) {
            data.tangents = tangents.iter()
                .map(|t| Vec4::new(t[0], t[1], t[2], t[3]))
                .collect();
        }
        
        // UVs
        if let Some(VertexAttributeValues::Float32x2(uvs)) = 
            mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            data.uvs = uvs.iter()
                .map(|uv| Vec2::new(uv[0], uv[1]))
                .collect();
        }
        
        // Indices
        if let Some(indices) = mesh.indices() {
            data.indices = match indices {
                Indices::U16(idx) => idx.iter().map(|i| *i as u32).collect(),
                Indices::U32(idx) => idx.clone(),
            };
        }
        
        data
    }
    
    /// Get vertex count
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }
    
    /// Get triangle count
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
    
    /// Get bounding box
    pub fn bounds(&self) -> (Vec3, Vec3) {
        if self.positions.is_empty() {
            return (Vec3::ZERO, Vec3::ZERO);
        }
        
        let mut min = self.positions[0];
        let mut max = self.positions[0];
        
        for pos in &self.positions {
            min = min.min(*pos);
            max = max.max(*pos);
        }
        
        (min, max)
    }
    
    /// Get center of mass (assuming uniform density)
    pub fn center(&self) -> Vec3 {
        if self.positions.is_empty() {
            return Vec3::ZERO;
        }
        
        let sum: Vec3 = self.positions.iter().sum();
        sum / self.positions.len() as f32
    }
}

// ============================================================================
// Vertex Displacement
// ============================================================================

/// Apply displacement to vertex positions
pub fn apply_displacement(
    original: &[Vec3],
    displacement: &[Vec3],
    output: &mut Vec<[f32; 3]>,
) {
    output.clear();
    output.reserve(original.len());
    
    for (i, pos) in original.iter().enumerate() {
        let disp = displacement.get(i).copied().unwrap_or(Vec3::ZERO);
        let new_pos = *pos + disp;
        output.push([new_pos.x, new_pos.y, new_pos.z]);
    }
}

/// Recalculate normals after deformation
pub fn recalculate_normals(
    positions: &[[f32; 3]],
    indices: &[u32],
    normals: &mut Vec<[f32; 3]>,
) {
    normals.clear();
    normals.resize(positions.len(), [0.0, 0.0, 0.0]);
    
    // Accumulate face normals
    for tri in indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;
        
        if i0 >= positions.len() || i1 >= positions.len() || i2 >= positions.len() {
            continue;
        }
        
        let p0 = Vec3::from_array(positions[i0]);
        let p1 = Vec3::from_array(positions[i1]);
        let p2 = Vec3::from_array(positions[i2]);
        
        let edge1 = p1 - p0;
        let edge2 = p2 - p0;
        let face_normal = edge1.cross(edge2);
        
        // Add to each vertex
        for &idx in &[i0, i1, i2] {
            normals[idx][0] += face_normal.x;
            normals[idx][1] += face_normal.y;
            normals[idx][2] += face_normal.z;
        }
    }
    
    // Normalize
    for normal in normals.iter_mut() {
        let n = Vec3::from_array(*normal);
        let normalized = n.normalize_or_zero();
        *normal = [normalized.x, normalized.y, normalized.z];
    }
}

// ============================================================================
// Influence Functions
// ============================================================================

/// Calculate influence weight based on distance (linear falloff)
pub fn linear_falloff(distance: f32, radius: f32) -> f32 {
    if distance >= radius {
        0.0
    } else {
        1.0 - distance / radius
    }
}

/// Calculate influence weight (quadratic falloff)
pub fn quadratic_falloff(distance: f32, radius: f32) -> f32 {
    if distance >= radius {
        0.0
    } else {
        let t = 1.0 - distance / radius;
        t * t
    }
}

/// Calculate influence weight (smooth falloff using smoothstep)
pub fn smooth_falloff(distance: f32, radius: f32) -> f32 {
    if distance >= radius {
        0.0
    } else {
        let t = distance / radius;
        let s = 1.0 - t;
        s * s * (3.0 - 2.0 * s)
    }
}

/// Calculate influence weight (Gaussian falloff)
pub fn gaussian_falloff(distance: f32, radius: f32) -> f32 {
    let sigma = radius / 3.0; // 3-sigma rule
    (-0.5 * (distance / sigma).powi(2)).exp()
}

// ============================================================================
// Deformation Modes
// ============================================================================

/// Apply radial displacement (explosion/implosion)
pub fn radial_displacement(
    positions: &[Vec3],
    center: Vec3,
    magnitude: f32,
    radius: f32,
    output: &mut [Vec3],
) {
    for (i, pos) in positions.iter().enumerate() {
        let to_vertex = *pos - center;
        let distance = to_vertex.length();
        
        if distance < 0.0001 || distance > radius {
            output[i] = Vec3::ZERO;
            continue;
        }
        
        let weight = smooth_falloff(distance, radius);
        let direction = to_vertex / distance;
        output[i] = direction * magnitude * weight;
    }
}

/// Apply directional displacement (push/pull)
pub fn directional_displacement(
    positions: &[Vec3],
    origin: Vec3,
    direction: Vec3,
    magnitude: f32,
    radius: f32,
    output: &mut [Vec3],
) {
    let dir = direction.normalize_or_zero();
    
    for (i, pos) in positions.iter().enumerate() {
        let distance = (*pos - origin).length();
        let weight = smooth_falloff(distance, radius);
        output[i] = dir * magnitude * weight;
    }
}

/// Apply twist deformation around axis
pub fn twist_displacement(
    positions: &[Vec3],
    axis_origin: Vec3,
    axis_direction: Vec3,
    angle_per_unit: f32,
    output: &mut [Vec3],
) {
    let axis = axis_direction.normalize_or_zero();
    
    for (i, pos) in positions.iter().enumerate() {
        let to_vertex = *pos - axis_origin;
        
        // Project onto axis to get height
        let height = to_vertex.dot(axis);
        
        // Get perpendicular component
        let perp = to_vertex - axis * height;
        let perp_dist = perp.length();
        
        if perp_dist < 0.0001 {
            output[i] = Vec3::ZERO;
            continue;
        }
        
        // Rotation angle based on height
        let angle = height * angle_per_unit;
        
        // Rotate perpendicular component
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        
        let perp_norm = perp / perp_dist;
        let tangent = axis.cross(perp_norm);
        
        let rotated_perp = perp_norm * cos_a + tangent * sin_a;
        let new_pos = axis_origin + axis * height + rotated_perp * perp_dist;
        
        output[i] = new_pos - *pos;
    }
}

/// Apply bend deformation
pub fn bend_displacement(
    positions: &[Vec3],
    bend_axis: Vec3,      // Axis to bend around
    bend_center: Vec3,    // Center of bend
    bend_direction: Vec3, // Direction of bend
    curvature: f32,       // 1/radius of curvature
    output: &mut [Vec3],
) {
    let axis = bend_axis.normalize_or_zero();
    let dir = bend_direction.normalize_or_zero();
    
    for (i, pos) in positions.iter().enumerate() {
        let to_vertex = *pos - bend_center;
        
        // Distance along bend direction
        let dist_along = to_vertex.dot(dir);
        
        // Bend angle
        let angle = dist_along * curvature;
        
        if angle.abs() < 0.0001 {
            output[i] = Vec3::ZERO;
            continue;
        }
        
        // Calculate bent position
        let radius = 1.0 / curvature.abs();
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        
        // Height along axis
        let height = to_vertex.dot(axis);
        
        // New position
        let new_dist = radius * sin_a;
        let new_height = height; // Preserved
        let offset = radius * (1.0 - cos_a);
        
        let perp = to_vertex - axis * height - dir * dist_along;
        let new_pos = bend_center + dir * new_dist + axis * new_height + perp + dir.cross(axis) * offset;
        
        output[i] = new_pos - *pos;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_falloff_functions() {
        // At center, weight should be 1
        assert!((linear_falloff(0.0, 1.0) - 1.0).abs() < 0.001);
        assert!((quadratic_falloff(0.0, 1.0) - 1.0).abs() < 0.001);
        assert!((smooth_falloff(0.0, 1.0) - 1.0).abs() < 0.001);
        
        // At edge, weight should be 0
        assert!((linear_falloff(1.0, 1.0) - 0.0).abs() < 0.001);
        assert!((quadratic_falloff(1.0, 1.0) - 0.0).abs() < 0.001);
        assert!((smooth_falloff(1.0, 1.0) - 0.0).abs() < 0.001);
        
        // Outside radius, weight should be 0
        assert_eq!(linear_falloff(2.0, 1.0), 0.0);
    }
    
    #[test]
    fn test_recalculate_normals() {
        // Simple triangle
        let positions = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let indices = vec![0, 1, 2];
        let mut normals = Vec::new();
        
        recalculate_normals(&positions, &indices, &mut normals);
        
        assert_eq!(normals.len(), 3);
        // All normals should point in +Z
        for n in &normals {
            assert!((n[2] - 1.0).abs() < 0.001 || (n[2] - (-1.0)).abs() < 0.001);
        }
    }
}
