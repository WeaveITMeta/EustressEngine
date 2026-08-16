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
    
    // Close both halves across the cut. Skipping this leaves open shells,
    // which look hollow AND give Parry meaningless volume integrals — a
    // dynamic body built from one can end up with zero/negative mass and a
    // NaN inertia tensor.
    append_cut_cap(
        &cut_vertices,
        normal,
        &mut pos_positions,
        &mut pos_normals,
        &mut pos_uvs,
        &mut pos_indices,
        &mut neg_positions,
        &mut neg_normals,
        &mut neg_uvs,
        &mut neg_indices,
    );

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
    
    // ── actually clip the triangle at the plane ──────────────────────────
    //
    // The previous implementation computed `intersections` above and then
    // THREW THEM AWAY, assigning the whole triangle to whichever side held
    // ≥2 vertices. That made every cut triangle-granular (visibly jagged,
    // never planar) and left geometry poking through the cut plane. Here the
    // triangle is genuinely subdivided at the crossing points.
    //
    // A triangle straddling a plane always has exactly one vertex alone on
    // one side (the "apex") and two on the other, so the split is always
    // 1 triangle (apex side) + 2 triangles (the quad on the other side).
    // Vertices exactly ON the plane are folded into whichever side keeps the
    // winding intact — they need no new point.

    /// One interpolated corner of the clipped triangle.
    #[derive(Clone, Copy)]
    struct Corner {
        pos: Vec3,
        normal: Vec3,
        uv: Vec2,
    }

    let corners = [
        Corner { pos: p0, normal: n0, uv: uv0 },
        Corner { pos: p1, normal: n1, uv: uv1 },
        Corner { pos: p2, normal: n2, uv: uv2 },
    ];
    let sides = [s0, s1, s2];

    // Interpolate a new corner where edge a→b crosses the plane.
    //
    // The parameter is ALWAYS computed from the lower vertex index toward the
    // higher one, so the two triangles sharing this edge produce a
    // bit-identical point. Interpolating in edge-local order instead would
    // give two very slightly different positions and tear the seam open.
    let lerp_corner = |ia: usize, ib: usize| -> Corner {
        let (lo, hi) = if ia < ib { (ia, ib) } else { (ib, ia) };
        let a = corners[lo];
        let b = corners[hi];
        let d_a = (a.pos - plane_origin).dot(plane_normal);
        let d_b = (b.pos - plane_origin).dot(plane_normal);
        let denom = d_a - d_b;
        let t = if denom.abs() > 1.0e-12 { d_a / denom } else { 0.5 };
        let t = t.clamp(0.0, 1.0);
        Corner {
            pos: a.pos + (b.pos - a.pos) * t,
            normal: (a.normal + (b.normal - a.normal) * t).normalize_or_zero(),
            uv: a.uv + (b.uv - a.uv) * t,
        }
    };

    // Identify the lone vertex. With a genuine crossing exactly one of the
    // three is strictly opposite the other two.
    let apex = (0..3).find(|&i| {
        let a = sides[i];
        let b = sides[(i + 1) % 3];
        let c = sides[(i + 2) % 3];
        a != 0 && (b == -a || b == 0) && (c == -a || c == 0) && (b != 0 || c != 0)
    });

    let Some(apex) = apex else {
        // Degenerate (e.g. an entire edge lying on the plane). Nothing sane
        // to clip — drop the triangle rather than emit self-intersecting
        // geometry. Whole-side triangles are handled by the caller.
        return;
    };

    let i_a = apex;
    let i_b = (apex + 1) % 3;
    let i_c = (apex + 2) % 3;

    let ca = corners[i_a];
    let cb = corners[i_b];
    let cc = corners[i_c];

    // Crossing points on the two edges leaving the apex.
    let ab = lerp_corner(i_a, i_b);
    let ac = lerp_corner(i_a, i_c);

    cut_vertices.push(ab.pos);
    cut_vertices.push(ac.pos);

    // Emit a triangle into one side's buffers, preserving winding.
    fn emit(
        positions: &mut Vec<[f32; 3]>,
        normals: &mut Vec<[f32; 3]>,
        uvs: &mut Vec<[f32; 2]>,
        indices: &mut Vec<u32>,
        tri: [&Corner; 3],
    ) {
        let base = positions.len() as u32;
        for c in tri {
            positions.push([c.pos.x, c.pos.y, c.pos.z]);
            normals.push([c.normal.x, c.normal.y, c.normal.z]);
            uvs.push([c.uv.x, c.uv.y]);
        }
        indices.push(base);
        indices.push(base + 1);
        indices.push(base + 2);
    }

    // The apex side keeps the single triangle (apex, ab, ac); the far side
    // keeps the quad (ab, b, c, ac) as two triangles. Original winding
    // a→b→c is preserved in both.
    if sides[i_a] > 0 {
        emit(pos_positions, pos_normals, pos_uvs, pos_indices, [&ca, &ab, &ac]);
        emit(neg_positions, neg_normals, neg_uvs, neg_indices, [&ab, &cb, &cc]);
        emit(neg_positions, neg_normals, neg_uvs, neg_indices, [&ab, &cc, &ac]);
    } else {
        emit(neg_positions, neg_normals, neg_uvs, neg_indices, [&ca, &ab, &ac]);
        emit(pos_positions, pos_normals, pos_uvs, pos_indices, [&ab, &cb, &cc]);
        emit(pos_positions, pos_normals, pos_uvs, pos_indices, [&ab, &cc, &ac]);
    }
}

// ============================================================================
// Cut Cap
// ============================================================================

/// Triangulate the cut cross-section and append it to both halves.
///
/// Without a cap each half is an OPEN SHELL: you can see straight into a
/// hollow interior, and — more importantly — Parry's volume integrals over an
/// open mesh are meaningless, so a fragment used as a dynamic body can end up
/// with zero or negative mass and a NaN inertia tensor.
///
/// Eustress parts are scaled unit primitives, i.e. convex, and a plane through
/// a convex solid produces exactly ONE convex cross-section. That makes the
/// robust approach cheap: project the crossing points onto a 2D basis in the
/// plane, take their convex hull, and fan-triangulate it. No loop walking, no
/// vertex welding, no constrained triangulation — and no dependence on the
/// intersection points arriving in any particular order.
///
/// (For a concave or multi-loop cut this degenerates to capping the convex
/// outline, which would bridge across a concavity. Handling that needs real
/// loop extraction; it is out of scope while parts are convex primitives.)
fn append_cut_cap(
    cut_vertices: &[Vec3],
    plane_normal: Vec3,
    pos_positions: &mut Vec<[f32; 3]>,
    pos_normals: &mut Vec<[f32; 3]>,
    pos_uvs: &mut Vec<[f32; 2]>,
    pos_indices: &mut Vec<u32>,
    neg_positions: &mut Vec<[f32; 3]>,
    neg_normals: &mut Vec<[f32; 3]>,
    neg_uvs: &mut Vec<[f32; 2]>,
    neg_indices: &mut Vec<u32>,
) {
    if cut_vertices.len() < 3 {
        return;
    }

    // Orthonormal basis in the cut plane.
    let n = plane_normal.normalize_or_zero();
    if n == Vec3::ZERO {
        return;
    }
    let helper = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let u = n.cross(helper).normalize_or_zero();
    if u == Vec3::ZERO {
        return;
    }
    let v = n.cross(u);

    let origin = cut_vertices[0];
    let mut planar: Vec<(f32, f32, Vec3)> = cut_vertices
        .iter()
        .map(|p| {
            let d = *p - origin;
            (d.dot(u), d.dot(v), *p)
        })
        .collect();

    // Monotone-chain convex hull in the plane.
    planar.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    planar.dedup_by(|a, b| (a.0 - b.0).abs() < 1.0e-6 && (a.1 - b.1).abs() < 1.0e-6);
    if planar.len() < 3 {
        return;
    }

    let cross = |o: &(f32, f32, Vec3), a: &(f32, f32, Vec3), b: &(f32, f32, Vec3)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };

    let mut hull: Vec<(f32, f32, Vec3)> = Vec::with_capacity(planar.len() * 2);
    for p in planar.iter() {
        while hull.len() >= 2 && cross(&hull[hull.len() - 2], &hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(*p);
    }
    let lower = hull.len() + 1;
    for p in planar.iter().rev() {
        while hull.len() >= lower && cross(&hull[hull.len() - 2], &hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(*p);
    }
    hull.pop(); // last point repeats the first
    if hull.len() < 3 {
        return;
    }

    // Fan from the hull centroid. Exact for a convex outline.
    let centroid = hull.iter().fold(Vec3::ZERO, |acc, h| acc + h.2) / hull.len() as f32;

    // Planar UVs, normalised over the cap's own extent so the cut face gets a
    // sane 0..1 mapping rather than inheriting meaningless surface UVs.
    let extent = hull
        .iter()
        .map(|h| ((h.0).abs()).max((h.1).abs()))
        .fold(0.0_f32, f32::max)
        .max(1.0e-6);
    let uv_of = |h: &(f32, f32, Vec3)| Vec2::new(h.0 / (2.0 * extent) + 0.5, h.1 / (2.0 * extent) + 0.5);
    let centroid_uv = Vec2::splat(0.5);

    // The cap faces OPPOSITE ways on the two halves: the positive-side piece
    // is closed by a face pointing back along −normal, the negative-side piece
    // by one pointing along +normal. Winding is reversed to match, so both
    // read as solid from outside.
    for i in 0..hull.len() {
        let a = &hull[i];
        let b = &hull[(i + 1) % hull.len()];

        // Positive half: normal −n.
        let base = pos_positions.len() as u32;
        for (p, uv) in [(centroid, centroid_uv), (b.2, uv_of(b)), (a.2, uv_of(a))] {
            pos_positions.push([p.x, p.y, p.z]);
            pos_normals.push([-n.x, -n.y, -n.z]);
            pos_uvs.push([uv.x, uv.y]);
        }
        pos_indices.extend_from_slice(&[base, base + 1, base + 2]);

        // Negative half: normal +n, opposite winding.
        let base = neg_positions.len() as u32;
        for (p, uv) in [(centroid, centroid_uv), (a.2, uv_of(a)), (b.2, uv_of(b))] {
            neg_positions.push([p.x, p.y, p.z]);
            neg_normals.push([n.x, n.y, n.z]);
            neg_uvs.push([uv.x, uv.y]);
        }
        neg_indices.extend_from_slice(&[base, base + 1, base + 2]);
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
