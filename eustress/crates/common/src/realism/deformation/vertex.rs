//! # Vertex Operations
//!
//! Low-level vertex manipulation for deformation.

use bevy::prelude::*;
use bevy::mesh::{Indices, Mesh, VertexAttributeValues};

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
// Adaptive local refinement
// ============================================================================

/// What one [`RefineForest::refine`] call added.
///
/// New vertices come AFTER every vertex that already existed and nothing is
/// renumbered. That is the property that makes mid-simulation refinement safe:
/// displacement arrays, the reference pose and the index buffer all stay valid
/// and only need extending.
#[derive(Clone, Debug)]
pub struct Refinement {
    /// The two already-existing vertices each appended vertex is the midpoint
    /// of, in append order. Entry `k` describes vertex
    /// `vertex_count_before_call + k`, so the caller can interpolate whatever
    /// per-vertex state it keeps alongside the mesh.
    pub added_parents: Vec<(u32, u32)>,
    /// Complete replacement index buffer.
    pub indices: Vec<u32>,
    /// Triangles in [`indices`](Self::indices).
    pub triangle_count: usize,
}

/// One triangle in the refinement forest. Only leaves are rendered.
#[derive(Clone, Debug)]
struct RefineTri {
    v: [u32; 3],
    split: bool,
}

#[inline]
fn edge_key(a: u32, b: u32) -> (u32, u32) {
    if a < b { (a, b) } else { (b, a) }
}

/// The persistent refinement structure for one deformable mesh.
///
/// WHY NOT UNIFORM SUBDIVISION. Vertex deformation can only MOVE vertices that
/// exist, and the authored primitives are 24-vertex cubes whose vertices all sit
/// at the eight corners — a dent in the middle of a face has literally nothing
/// within its radius to displace. Subdividing the whole mesh at load fixes that
/// but charges every deformable part 12,288 triangles on a 3 m plate whether or
/// not anything ever touches it, and still resolves a 30 cm crater with about
/// 2% of them. This adds resolution only where a crater actually lands.
///
/// WHY THE FOREST IS KEPT. Refinement is repeated: every fresh impact adds more
/// detail to a mesh that earlier impacts already refined. The tree is what makes
/// that stable. Rendering needs a "green closure" — transition triangles that
/// stitch a refined region to its coarser neighbours — and those are the WRONG
/// thing to refine further: they are thin by construction, so splitting them
/// compounds into slivers and, once the sliver normals invert, a visibly
/// shredded surface. Keeping the tree means greens are re-derived from the
/// leaves on every call and never enter it, so the structure that gets split is
/// always the well-shaped red one no matter how many impacts land.
///
/// (This was originally written stateless, re-deriving the tree from the
/// previous closed index buffer each call. One impact looked correct; the second
/// impact on the same part went from 732 triangles to 2815 with 691 inverted
/// normals.)
///
/// # Conformity
///
/// Splitting some triangles and not their neighbours leaves T-junction cracks:
/// the fine side gains a vertex in the middle of a shared edge that the coarse
/// side has no matching vertex for, and displacing it tears the surface open.
/// This is the standard red-green scheme:
///
/// * RED — a triangle that needs refining splits into four, inserting midpoints
///   on all three of its edges. Midpoints are shared through a cache keyed on
///   the undirected edge, so the neighbour sees the same vertex.
/// * BALANCE — a leaf whose edge has been split more than once (its neighbour is
///   two or more levels finer) is split too, repeatedly, until no edge of any
///   leaf carries more than one hanging node. Detected purely from the midpoint
///   cache: edge `a-b` is split twice exactly when `a-m` or `m-b` also has a
///   midpoint, so no adjacency structure is needed.
/// * GREEN — the leaves are then triangulated to include whatever hanging nodes
///   their edges picked up, which is what makes the result watertight.
#[derive(Clone, Debug, Default)]
pub struct RefineForest {
    tris: Vec<RefineTri>,
    edge_mid: std::collections::HashMap<(u32, u32), u32>,
    seeded: bool,
}

impl RefineForest {
    /// Triangles currently rendered, i.e. after green closure. Zero until the
    /// first successful [`refine`](Self::refine).
    pub fn leaf_count(&self) -> usize {
        self.tris.iter().filter(|t| !t.split).count()
    }

    /// Whether this forest has taken its seed topology yet.
    pub fn is_seeded(&self) -> bool {
        self.seeded
    }

    /// Add resolution wherever a crater lands, and return the new topology.
    ///
    /// `positions` is the mesh's reference pose and is GROWN IN PLACE — new
    /// midpoints are appended, never inserted, so existing indices keep meaning
    /// the same vertex. `seed_indices` is used only on the first call, to take
    /// the authored topology; afterwards the forest is the source of truth and
    /// the argument is ignored.
    ///
    /// `scale` converts local positions to world metres. `center` is the impact
    /// point in the SAME local space as `positions`; `radius` and `target_edge`
    /// are world metres.
    ///
    /// Returns `None` when nothing needed refining, the input is unusable, or
    /// the budget cannot hold a conforming result. A refusal leaves both the
    /// forest and `positions` untouched, so a caller can always fall back to the
    /// topology it already had.
    pub fn refine(
        &mut self,
        positions: &mut Vec<Vec3>,
        seed_indices: &[u32],
        scale: Vec3,
        center: Vec3,
        radius: f32,
        target_edge: f32,
        max_levels: u32,
        max_triangles: usize,
    ) -> Option<Refinement> {
        if positions.is_empty() {
            return None;
        }
        if !(radius > 0.0) || !(target_edge > 0.0) || !center.is_finite() {
            return None;
        }

        if !self.seeded {
            if seed_indices.len() < 3 {
                return None;
            }
            self.tris = seed_indices
                .chunks_exact(3)
                .map(|t| RefineTri { v: [t[0], t[1], t[2]], split: false })
                .collect();
            if self.tris.is_empty() {
                return None;
            }
            self.seeded = true;
        }

        // Work on copies so a mid-way refusal cannot leave a half-refined
        // forest or orphaned vertices behind.
        let mut tris = self.tris.clone();
        let mut edge_mid = self.edge_mid.clone();
        let mut pos = positions.clone();
        let base_count = pos.len();
        let mut parents: Vec<(u32, u32)> = Vec::new();

        let center_w = center * scale;

        // Budget is counted in LEAVES, which the green closure turns into one to
        // four rendered triangles each (four only along the thin transition
        // ring). The floor is the CURRENT leaf count, never a multiple of it: a
        // cap that scaled with the mesh it is capping would ratchet upward
        // impact after impact and bound nothing.
        let mut leaves = tris.iter().filter(|t| !t.split).count();
        let leaf_cap = max_triangles.max(leaves);

        // ---- RED: refine leaves the crater covers, level by level ----------
        for _level in 0..max_levels {
            let mut todo: Vec<usize> = Vec::new();
            for i in 0..tris.len() {
                if tris[i].split {
                    continue;
                }
                let v = tris[i].v;
                let a = pos[v[0] as usize] * scale;
                let b = pos[v[1] as usize] * scale;
                let c = pos[v[2] as usize] * scale;
                let longest = (b - a)
                    .length()
                    .max((c - b).length())
                    .max((a - c).length());
                if !(longest > target_edge) {
                    continue;
                }
                // Reach test: distance from the impact point to the triangle's
                // bounding box. A lower bound on the true distance, so it never
                // rejects a triangle the crater touches, and far tighter than
                // testing the centroid — on a plate it rejects the five faces
                // that are not being hit outright, at the first level.
                let lo = a.min(b).min(c);
                let hi = a.max(b).max(c);
                if center_w.clamp(lo, hi).distance(center_w) > radius {
                    continue;
                }
                todo.push(i);
            }
            if todo.is_empty() {
                break;
            }
            if leaves + todo.len() * 3 > leaf_cap {
                break;
            }
            leaves += todo.len() * 3;
            for i in todo {
                red_split(i, &mut tris, &mut pos, &mut parents, &mut edge_mid);
            }
        }

        if parents.is_empty() {
            // Nothing was refined — the crater already resolves at this
            // resolution, so the existing topology stands.
            return None;
        }

        // ---- BALANCE: no leaf may carry two hanging nodes on one edge ------
        // Each pass can only split leaves, and a split leaf stops being one, so
        // this terminates; the iteration cap is a backstop.
        let mut balanced = false;
        for _ in 0..64 {
            let mut to_split: Vec<usize> = Vec::new();
            for i in 0..tris.len() {
                if tris[i].split {
                    continue;
                }
                let v = tris[i].v;
                for e in 0..3 {
                    let (a, b) = (v[e], v[(e + 1) % 3]);
                    let Some(&m) = edge_mid.get(&edge_key(a, b)) else {
                        continue;
                    };
                    if edge_mid.contains_key(&edge_key(a, m))
                        || edge_mid.contains_key(&edge_key(m, b))
                    {
                        to_split.push(i);
                        break;
                    }
                }
            }
            if to_split.is_empty() {
                balanced = true;
                break;
            }
            if leaves + to_split.len() * 3 > leaf_cap {
                break;
            }
            leaves += to_split.len() * 3;
            for i in to_split {
                red_split(i, &mut tris, &mut pos, &mut parents, &mut edge_mid);
            }
        }
        if !balanced {
            // Could not finish enforcing conformity. Refuse outright rather
            // than hand back a mesh that renders with cracks in it — and
            // because nothing above touched `self` or `positions`, refusing
            // costs only the work, not the existing topology.
            return None;
        }

        let indices = close_leaves(&tris, &edge_mid);
        if indices.len() < 3 {
            return None;
        }

        // Commit.
        debug_assert_eq!(pos.len() - base_count, parents.len());
        self.tris = tris;
        self.edge_mid = edge_mid;
        positions.extend_from_slice(&pos[base_count..]);

        Some(Refinement {
            added_parents: parents,
            triangle_count: indices.len() / 3,
            indices,
        })
    }
}

/// Split a leaf into four children, inserting (or reusing) the midpoint of each
/// of its edges.
fn red_split(
    i: usize,
    tris: &mut Vec<RefineTri>,
    pos: &mut Vec<Vec3>,
    parents: &mut Vec<(u32, u32)>,
    edge_mid: &mut std::collections::HashMap<(u32, u32), u32>,
) {
    let v = tris[i].v;
    let ab = midpoint(v[0], v[1], pos, parents, edge_mid);
    let bc = midpoint(v[1], v[2], pos, parents, edge_mid);
    let ca = midpoint(v[2], v[0], pos, parents, edge_mid);

    tris[i].split = true;
    tris.push(RefineTri { v: [v[0], ab, ca], split: false });
    tris.push(RefineTri { v: [v[1], bc, ab], split: false });
    tris.push(RefineTri { v: [v[2], ca, bc], split: false });
    tris.push(RefineTri { v: [ab, bc, ca], split: false });
}

/// Split an edge once and share the result with whichever triangle meets it
/// from the other side. Sharing through the cache is what keeps the two sides
/// referencing the SAME vertex rather than two coincident copies that drift
/// apart the moment either is displaced.
fn midpoint(
    a: u32,
    b: u32,
    pos: &mut Vec<Vec3>,
    parents: &mut Vec<(u32, u32)>,
    edge_mid: &mut std::collections::HashMap<(u32, u32), u32>,
) -> u32 {
    let k = edge_key(a, b);
    if let Some(&existing) = edge_mid.get(&k) {
        return existing;
    }
    let p = (pos[a as usize] + pos[b as usize]) * 0.5;
    pos.push(p);
    parents.push(k);
    let idx = (pos.len() - 1) as u32;
    edge_mid.insert(k, idx);
    idx
}

/// GREEN CLOSURE — triangulate every leaf so it includes whatever hanging nodes
/// its edges picked up from finer neighbours.
///
/// Derived fresh from the leaves on every call and never stored, which is what
/// stops these thin transition triangles from ever being refined further.
fn close_leaves(
    tris: &[RefineTri],
    edge_mid: &std::collections::HashMap<(u32, u32), u32>,
) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for t in tris.iter() {
        if t.split {
            continue;
        }
        let v = t.v;
        let ab = edge_mid.get(&edge_key(v[0], v[1])).copied();
        let bc = edge_mid.get(&edge_key(v[1], v[2])).copied();
        let ca = edge_mid.get(&edge_key(v[2], v[0])).copied();

        // Every arm preserves the input winding, which the normal pass
        // downstream depends on to tell "outward" from "inward".
        match (ab, bc, ca) {
            (None, None, None) => out.extend_from_slice(&[v[0], v[1], v[2]]),

            // One hanging node — bisect toward the opposite corner.
            (Some(m), None, None) => {
                out.extend_from_slice(&[v[0], m, v[2], m, v[1], v[2]]);
            }
            (None, Some(m), None) => {
                out.extend_from_slice(&[v[0], v[1], m, v[0], m, v[2]]);
            }
            (None, None, Some(m)) => {
                out.extend_from_slice(&[v[0], v[1], m, m, v[1], v[2]]);
            }

            // Two hanging nodes — clip the corner between them, then split the
            // remaining quad.
            (Some(m0), Some(m1), None) => {
                out.extend_from_slice(&[m0, v[1], m1, v[0], m0, m1, v[0], m1, v[2]]);
            }
            (None, Some(m1), Some(m2)) => {
                out.extend_from_slice(&[m1, v[2], m2, v[0], v[1], m1, v[0], m1, m2]);
            }
            (Some(m0), None, Some(m2)) => {
                out.extend_from_slice(&[v[0], m0, m2, m0, v[1], v[2], m0, v[2], m2]);
            }

            // Three hanging nodes — the full four-way split, emitted WITHOUT
            // recording children so this stays a leaf and the pattern is
            // re-derived next call rather than becoming permanent structure.
            (Some(m0), Some(m1), Some(m2)) => {
                out.extend_from_slice(&[
                    v[0], m0, m2, v[1], m1, m0, v[2], m2, m1, m0, m1, m2,
                ]);
            }
        }
    }
    out
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

    /// The authored `parts/block.glb` topology: 24 vertices, six faces that do
    /// NOT share corners (each carries its own copies so it can have its own
    /// normals), two consistently wound triangles per face.
    ///
    /// Testing against a single flat quad — as this suite originally did — is
    /// what let a real defect through: it exercises neither the disconnected
    /// patches nor the repeated-impact path.
    fn authored_cube() -> (Vec<Vec3>, Vec<u32>) {
        let mut positions = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);
        // (normal axis, sign) for each of the six faces.
        let faces: [(usize, f32); 6] = [
            (1, 1.0),
            (1, -1.0),
            (0, 1.0),
            (0, -1.0),
            (2, 1.0),
            (2, -1.0),
        ];
        for (axis, sign) in faces {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let base = positions.len() as u32;
            for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let mut p = Vec3::ZERO;
                p[axis] = 0.5 * sign;
                p[u] = 0.5 * su;
                p[v] = 0.5 * sv;
                positions.push(p);
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        (positions, indices)
    }

    /// Every edge must be used by exactly two triangles, except along a face
    /// perimeter where one is correct because the faces are separate patches.
    ///
    /// A T-junction — a vertex sitting mid-way along a neighbour's edge with no
    /// triangle on that side referencing it — shows up here as an interior edge
    /// used exactly ONCE, and on screen as a tear the moment the surface moves.
    /// Nothing about a triangle COUNT would catch it.
    fn assert_watertight(positions: &[Vec3], indices: &[u32], label: &str) {
        let mut use_count: std::collections::HashMap<(u32, u32), u32> =
            std::collections::HashMap::new();
        for tri in indices.chunks_exact(3) {
            assert!(
                tri.iter().all(|i| (*i as usize) < positions.len()),
                "{label}: index out of range"
            );
            assert!(
                tri[0] != tri[1] && tri[1] != tri[2] && tri[2] != tri[0],
                "{label}: degenerate triangle {tri:?}"
            );
            for e in 0..3 {
                *use_count.entry(edge_key(tri[e], tri[(e + 1) % 3])).or_insert(0) += 1;
            }
        }

        for (&(a, b), &n) in use_count.iter() {
            if n == 2 {
                continue;
            }
            assert_eq!(n, 1, "{label}: edge {a}-{b} shared by {n} triangles");
            // A once-used edge is legal only on a cube EDGE line, where two
            // coordinates are simultaneously pinned to the same extreme.
            let (pa, pb) = (positions[a as usize], positions[b as usize]);
            let pinned = (0..3)
                .filter(|&k| {
                    (pa[k] - pb[k]).abs() < 1e-5 && (pa[k].abs() - 0.5).abs() < 1e-5
                })
                .count();
            assert!(pinned >= 2, "{label}: interior T-junction {pa:?}-{pb:?}");
        }
    }

    /// The regression this whole structure exists for.
    ///
    /// Refinement is REPEATED — a bouncing impactor refines the same part again
    /// and again. When the forest was not kept and each call re-derived itself
    /// from the previous green-closed index buffer, the second impact took a
    /// plate from 732 triangles to 2815 with 691 inverted normals, which
    /// rendered as a shredded surface with most of the plate missing. Green
    /// triangles are thin by construction and must never be refined further.
    #[test]
    fn refine_forest_survives_repeated_impacts() {
        let (mut positions, seed) = authored_cube();
        let mut forest = RefineForest::default();
        // The demo plate: 3.0 x 0.5 x 2.5 metres, struck on top.
        let scale = Vec3::new(3.0, 0.5, 2.5);

        let mut last_tris = 0usize;
        // Successive craters, including two that deliberately overlap so a later
        // one lands on an earlier one's transition ring.
        let hits = [
            Vec3::new(0.0, 0.5, 0.0),
            Vec3::new(0.04, 0.5, -0.02),
            Vec3::new(-0.18, 0.5, 0.12),
            Vec3::new(0.0, 0.5, 0.0),
        ];
        for (n, &center) in hits.iter().enumerate() {
            let before = positions.len();
            let Some(r) = forest.refine(
                &mut positions,
                &seed,
                scale,
                center,
                0.319,
                0.08,
                8,
                8000,
            ) else {
                // A refusal is allowed (nothing left to refine), but it must
                // leave the mesh exactly as it was.
                assert_eq!(positions.len(), before, "impact {n}: refusal grew the mesh");
                continue;
            };

            assert_eq!(
                r.added_parents.len(),
                positions.len() - before,
                "impact {n}: parent list does not match appended vertices"
            );
            for (k, &(a, b)) in r.added_parents.iter().enumerate() {
                let created_at = before + k;
                assert!(
                    (a as usize) < created_at && (b as usize) < created_at,
                    "impact {n}: midpoint parents must pre-date the midpoint"
                );
            }
            for p in positions.iter() {
                assert!(p.is_finite(), "impact {n}: non-finite vertex");
            }

            assert_watertight(&positions, &r.indices, &format!("impact {n}"));

            assert!(
                r.triangle_count >= last_tris,
                "impact {n}: triangle count went backwards {last_tris} -> {}",
                r.triangle_count
            );
            last_tris = r.triangle_count;
            assert!(
                r.triangle_count < 8000 * 4,
                "impact {n}: {} triangles blew the budget",
                r.triangle_count
            );
        }

        assert!(last_tris > 200, "never actually refined ({last_tris} tris)");
    }

    /// Refinement must stay LOCAL — that is the entire point of not subdividing
    /// at load. Only the struck face may gain resolution; uniform subdivision to
    /// the same edge length would be over 100,000 triangles.
    #[test]
    fn refine_forest_only_touches_the_struck_face() {
        let (mut positions, seed) = authored_cube();
        let mut forest = RefineForest::default();
        let scale = Vec3::new(3.0, 0.5, 2.5);

        let r = forest
            .refine(&mut positions, &seed, scale, Vec3::new(0.0, 0.5, 0.0), 0.319, 0.08, 8, 8000)
            .expect("a crater on the top face must refine");

        assert!(
            r.triangle_count < 2000,
            "{} triangles is not local refinement",
            r.triangle_count
        );
        // Every vertex added must sit ON the struck face (local y = +0.5);
        // anything else means the reach test let a different face in.
        for p in positions.iter().skip(24) {
            assert!(
                (p.y - 0.5).abs() < 1e-6,
                "refined a face that was not struck: {p:?}"
            );
        }
        assert_watertight(&positions, &r.indices, "single impact");
    }

    #[test]
    fn refine_forest_declines_when_nothing_is_close() {
        let (mut positions, seed) = authored_cube();
        let mut forest = RefineForest::default();
        assert!(
            forest
                .refine(
                    &mut positions,
                    &seed,
                    Vec3::ONE,
                    Vec3::new(40.0, 0.0, 40.0),
                    0.2,
                    0.05,
                    8,
                    8000,
                )
                .is_none(),
            "a crater far off the part must not refine anything"
        );
        assert_eq!(positions.len(), 24, "a refusal must not grow the mesh");
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
