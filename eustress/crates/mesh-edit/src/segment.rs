//! Splitting one imported triangle mesh into parts that edit separately.
//!
//! An imported mesh is one buffer of triangles. Each function here sorts
//! those triangles into groups, and every group becomes one part.
//!
//! - [`split_by_connectivity`]: triangles that touch through a vertex form
//!   one part. Loose shells come apart; a closed body stays whole.
//! - [`split_by_crease`]: triangles join across an edge only while the faces
//!   on either side bend by at most a given angle. One body splits at its
//!   hard edges (a handle from a block, a lid from a box).
//! - [`split_by_group`]: triangles that already carry a group id (a glTF
//!   primitive or a material slot) stay together.
//! - [`split_by_predicate`]: triangles whose centroid is inside a region (a
//!   box, a sphere, a half-space) on one side and the rest on the other.
//!   [`connected_parts`] then splits either side into its loose pieces.
//!
//! [`extract`] turns a group into a self-contained [`Piece`] that remembers
//! which source vertex each of its vertices came from, so [`gather`] can carry
//! normals, UVs or colours across unchanged.
//!
//! Positions are welded before connectivity is read. glTF writes a vertex
//! again wherever its normal, UV or material changes, so without welding a
//! closed mesh reads as loose triangles.
//!
//! Triangles that are incomplete or index past the vertex buffer are dropped
//! from every group, not repaired.

use glam::Vec3;
use std::collections::HashMap;

/// Positions closer than this are one vertex. Units are the mesh's own, which
/// are metres for Eustress imports, so this is 10 micrometres: far below any
/// authored detail, and well above float noise at building scale.
pub const WELD_EPSILON: f32 = 1e-5;

/// For each vertex, the lowest index of the vertex it welds to: itself when it
/// welds to nothing earlier.
pub fn weld(positions: &[Vec3], epsilon: f32) -> Vec<u32> {
    let epsilon = epsilon.max(1e-9);
    let limit = epsilon * epsilon;
    let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
    let mut canonical = Vec::with_capacity(positions.len());
    for (i, &p) in positions.iter().enumerate() {
        let cell = cell_of(p, epsilon);
        // A match can sit in a neighbouring cell, so search all 27 around it.
        let found = neighbouring_cells(cell).find_map(|key| {
            grid.get(&key)?
                .iter()
                .copied()
                .find(|&j| (positions[j as usize] - p).length_squared() <= limit)
        });
        match found {
            Some(j) => canonical.push(j),
            None => {
                canonical.push(i as u32);
                grid.entry(cell).or_default().push(i as u32);
            }
        }
    }
    canonical
}

fn cell_of(p: Vec3, epsilon: f32) -> (i64, i64, i64) {
    let c = (p / epsilon).floor();
    // Float-to-int casts saturate, and NaN becomes 0, so this never panics.
    (c.x as i64, c.y as i64, c.z as i64)
}

fn neighbouring_cells(c: (i64, i64, i64)) -> impl Iterator<Item = (i64, i64, i64)> {
    (-1i64..=1).flat_map(move |dx| {
        (-1i64..=1).flat_map(move |dy| {
            (-1i64..=1).map(move |dz| {
                (c.0.saturating_add(dx), c.1.saturating_add(dy), c.2.saturating_add(dz))
            })
        })
    })
}

/// Triangle `t` as three source vertex ids, or `None` when it is incomplete or
/// one of its ids is past the vertex buffer.
fn triangle(indices: &[u32], vertex_count: usize, t: usize) -> Option<[u32; 3]> {
    let base = t * 3;
    let v = [*indices.get(base)?, *indices.get(base + 1)?, *indices.get(base + 2)?];
    v.iter().all(|&i| (i as usize) < vertex_count).then_some(v)
}

fn triangle_count(indices: &[u32]) -> usize {
    indices.len() / 3
}

/// Union-find. Merges toward the smaller root, so the sets it builds do not
/// depend on the order the joins arrive in.
struct Sets {
    parent: Vec<u32>,
}

impl Sets {
    fn new(n: usize) -> Self {
        Self { parent: (0..n as u32).collect() }
    }

    fn find(&mut self, mut x: u32) -> u32 {
        while self.parent[x as usize] != x {
            let grand = self.parent[self.parent[x as usize] as usize];
            self.parent[x as usize] = grand;
            x = grand;
        }
        x
    }

    fn join(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra.max(rb) as usize] = ra.min(rb);
        }
    }
}

/// Groups triangles by key. Items are `(triangle, key)`; an item with no key is
/// left out. Each group is sorted by triangle, and the groups are ordered
/// largest first, ties by their first triangle, so the same input always
/// yields the same parts in the same order.
fn collect(items: impl Iterator<Item = (u32, Option<u32>)>) -> Vec<Vec<u32>> {
    let mut slot: HashMap<u32, usize> = HashMap::new();
    let mut groups: Vec<Vec<u32>> = Vec::new();
    for (t, key) in items {
        let Some(key) = key else { continue };
        let g = *slot.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[g].push(t);
    }
    for group in &mut groups {
        group.sort_unstable();
    }
    groups.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
    groups
}

/// Every connected part of the mesh. Two triangles are connected when they
/// share a welded vertex.
pub fn split_by_connectivity(positions: &[Vec3], indices: &[u32]) -> Vec<Vec<u32>> {
    let all: Vec<u32> = (0..triangle_count(indices) as u32).collect();
    connected_parts(positions, indices, &all)
}

/// The connected parts among `triangles` only. Two of them are connected when
/// they share a welded vertex. Use this to split a region from
/// [`split_by_predicate`] into its loose pieces.
pub fn connected_parts(positions: &[Vec3], indices: &[u32], triangles: &[u32]) -> Vec<Vec<u32>> {
    let canonical = weld(positions, WELD_EPSILON);
    let corners = |t: u32| {
        triangle(indices, positions.len(), t as usize).map(|v| v.map(|i| canonical[i as usize]))
    };
    let mut sets = Sets::new(positions.len());
    for &t in triangles {
        if let Some([a, b, c]) = corners(t) {
            sets.join(a, b);
            sets.join(b, c);
        }
    }
    let items: Vec<(u32, Option<u32>)> = triangles
        .iter()
        .map(|&t| {
            let key = match corners(t) {
                Some([a, _, _]) => Some(sets.find(a)),
                None => None,
            };
            (t, key)
        })
        .collect();
    collect(items.into_iter())
}

/// Splits a body at its hard edges. Two triangles that share an edge join the
/// same part while the faces on either side bend by no more than
/// `max_angle_deg`. A flat surface is one part at any angle; an angle of 90 or
/// more keeps every body whole, since no two faces bend further than that.
///
/// The angle is taken between the two faces regardless of which way each is
/// wound, so one triangle wound the wrong way does not cut a smooth surface
/// apart.
pub fn split_by_crease(positions: &[Vec3], indices: &[u32], max_angle_deg: f32) -> Vec<Vec<u32>> {
    let canonical = weld(positions, WELD_EPSILON);
    let n = triangle_count(indices);
    let corners: Vec<Option<[u32; 3]>> = (0..n)
        .map(|t| triangle(indices, positions.len(), t).map(|v| v.map(|i| canonical[i as usize])))
        .collect();
    let normals: Vec<Vec3> = (0..n)
        .map(|t| match triangle(indices, positions.len(), t) {
            Some([a, b, c]) => {
                let (p, q, r) = (positions[a as usize], positions[b as usize], positions[c as usize]);
                (q - p).cross(r - p).normalize_or_zero()
            }
            None => Vec3::ZERO,
        })
        .collect();

    // Every undirected edge, with the triangles that use it.
    let mut edges: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    for (t, corner) in corners.iter().enumerate() {
        let Some([a, b, c]) = *corner else { continue };
        for (x, y) in [(a, b), (b, c), (c, a)] {
            if x != y {
                edges.entry((x.min(y), x.max(y))).or_default().push(t as u32);
            }
        }
    }

    let cos_limit = max_angle_deg.clamp(0.0, 180.0).to_radians().cos();
    let mut sets = Sets::new(n);
    for faces in edges.values() {
        for (i, &a) in faces.iter().enumerate() {
            for &b in &faces[i + 1..] {
                let (na, nb) = (normals[a as usize], normals[b as usize]);
                // A degenerate triangle has no direction to bend away from.
                let smooth = na == Vec3::ZERO || nb == Vec3::ZERO || na.dot(nb).abs() >= cos_limit;
                if smooth {
                    sets.join(a, b);
                }
            }
        }
    }

    let items: Vec<(u32, Option<u32>)> = (0..n)
        .map(|t| {
            let key = if corners[t].is_some() { Some(sets.find(t as u32)) } else { None };
            (t as u32, key)
        })
        .collect();
    collect(items.into_iter())
}

/// One part per group id. `group[t]` is triangle `t`'s id. A triangle with no
/// entry in `group` (the slice is shorter than the triangle count) is left out.
pub fn split_by_group(vertex_count: usize, indices: &[u32], group: &[u32]) -> Vec<Vec<u32>> {
    let items = (0..triangle_count(indices)).map(|t| {
        let key = if triangle(indices, vertex_count, t).is_some() { group.get(t).copied() } else { None };
        (t as u32, key)
    });
    collect(items)
}

/// The triangles whose centroid is inside, and the rest. Each side is in
/// triangle order and may be empty. Incomplete or out-of-range triangles
/// appear on neither side.
pub fn split_by_predicate(
    positions: &[Vec3],
    indices: &[u32],
    inside: impl Fn(Vec3) -> bool,
) -> (Vec<u32>, Vec<u32>) {
    let (mut yes, mut no) = (Vec::new(), Vec::new());
    for t in 0..triangle_count(indices) {
        let Some([a, b, c]) = triangle(indices, positions.len(), t) else { continue };
        let centroid = (positions[a as usize] + positions[b as usize] + positions[c as usize]) / 3.0;
        if inside(centroid) {
            yes.push(t as u32);
        } else {
            no.push(t as u32);
        }
    }
    (yes, no)
}

/// One part of a source mesh, as its own triangle buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    /// The source vertex behind each vertex of the piece. Carry any
    /// per-vertex attribute across with [`gather`].
    pub source_vertices: Vec<u32>,
    /// Three indices into `source_vertices` per triangle.
    pub indices: Vec<u32>,
    /// The source triangle behind each triangle of the piece.
    pub source_triangles: Vec<u32>,
}

/// The piece made of `triangles`. Source vertices are kept as they are in the
/// source, not welded: a UV or normal seam stays split, so each copy keeps the
/// attribute it was authored with.
pub fn extract(vertex_count: usize, indices: &[u32], triangles: &[u32]) -> Piece {
    let mut local: HashMap<u32, u32> = HashMap::new();
    let mut piece = Piece {
        source_vertices: Vec::new(),
        indices: Vec::new(),
        source_triangles: Vec::new(),
    };
    for &t in triangles {
        let Some(corners) = triangle(indices, vertex_count, t as usize) else { continue };
        piece.source_triangles.push(t);
        for v in corners {
            let id = *local.entry(v).or_insert_with(|| {
                piece.source_vertices.push(v);
                (piece.source_vertices.len() - 1) as u32
            });
            piece.indices.push(id);
        }
    }
    piece
}

/// `attribute` (one entry per source vertex) reordered to the piece's vertices.
pub fn gather<T: Copy>(attribute: &[T], piece: &Piece) -> Vec<T> {
    piece.source_vertices.iter().map(|&v| attribute[v as usize]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cube of half-width `half` centred on `c`, written the way a glTF
    /// exporter writes one: each face has its own four vertices, so a corner is
    /// three vertices at one position. Face `f` is triangles `2f` and `2f + 1`.
    fn cube(c: Vec3, half: f32) -> (Vec<Vec3>, Vec<u32>) {
        let mut pos = Vec::new();
        let mut idx = Vec::new();
        let faces = [
            (Vec3::X, Vec3::Y, Vec3::Z),
            (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            (Vec3::Y, Vec3::Z, Vec3::X),
            (Vec3::NEG_Y, Vec3::X, Vec3::Z),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (Vec3::NEG_Z, Vec3::Y, Vec3::X),
        ];
        for (n, u, v) in faces {
            let base = pos.len() as u32;
            let mid = c + n * half;
            for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                pos.push(mid + u * (a * half) + v * (b * half));
            }
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        (pos, idx)
    }

    /// Two unit cubes, the second five metres along +X. Vertices 0..24 belong to
    /// the first cube and 24..48 to the second; triangles 0..12 and 12..24 likewise.
    fn two_cubes() -> (Vec<Vec3>, Vec<u32>) {
        let (mut pos, mut idx) = cube(Vec3::ZERO, 1.0);
        let (p2, i2) = cube(Vec3::new(5.0, 0.0, 0.0), 1.0);
        let base = pos.len() as u32;
        pos.extend(p2);
        idx.extend(i2.into_iter().map(|i| i + base));
        (pos, idx)
    }

    #[test]
    fn a_closed_body_is_one_part() {
        let (pos, idx) = cube(Vec3::ZERO, 1.0);
        let parts = split_by_connectivity(&pos, &idx);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].len(), 12);
    }

    #[test]
    fn loose_shells_come_apart() {
        let (pos, idx) = two_cubes();
        let parts = split_by_connectivity(&pos, &idx);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| p.len() == 12));
        for part in &parts {
            let side: Vec<bool> = part.iter().map(|&t| idx[3 * t as usize] >= 24).collect();
            assert!(side.iter().all(|&s| s == side[0]), "a part mixes both cubes");
        }
    }

    #[test]
    fn every_triangle_lands_in_exactly_one_part() {
        let (pos, idx) = two_cubes();
        let mut all: Vec<u32> = split_by_connectivity(&pos, &idx).concat();
        all.sort_unstable();
        assert_eq!(all, (0..24).collect::<Vec<u32>>());
    }

    #[test]
    fn a_tiny_gap_welds_and_a_real_one_does_not() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let canon = weld(&[a, a + Vec3::new(1e-7, 0.0, 0.0), a + Vec3::new(1e-3, 0.0, 0.0)], WELD_EPSILON);
        assert_eq!(canon, vec![0, 0, 2]);
    }

    #[test]
    fn a_weld_across_a_grid_cell_boundary_still_joins() {
        // 1.0 - 4e-6 and 1.0 + 4e-6 sit in different cells of size WELD_EPSILON,
        // yet are within WELD_EPSILON of each other: only the neighbour search joins them.
        let canon = weld(&[Vec3::new(1.0 - 4e-6, 0.0, 0.0), Vec3::new(1.0 + 4e-6, 0.0, 0.0)], WELD_EPSILON);
        assert_eq!(canon, vec![0, 0]);
    }

    #[test]
    fn a_hard_edge_splits_a_body_at_a_small_angle() {
        let (pos, idx) = cube(Vec3::ZERO, 1.0);
        let faces = split_by_crease(&pos, &idx, 30.0);
        assert_eq!(faces.len(), 6, "each cube face is its own part");
        assert!(faces.iter().all(|p| p.len() == 2));
    }

    #[test]
    fn a_wide_angle_keeps_the_body_whole() {
        let (pos, idx) = cube(Vec3::ZERO, 1.0);
        assert_eq!(split_by_crease(&pos, &idx, 100.0).len(), 1);
    }

    #[test]
    fn a_flipped_triangle_does_not_cut_a_smooth_surface() {
        // Two coplanar triangles sharing the edge 1-2; the second is wound backwards.
        let pos = vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::new(1.0, 1.0, 0.0)];
        let idx = [0, 1, 2, 1, 2, 3];
        assert_eq!(split_by_crease(&pos, &idx, 30.0).len(), 1);
    }

    #[test]
    fn a_group_id_keeps_its_triangles_together() {
        let (pos, idx) = cube(Vec3::ZERO, 1.0);
        let per_face: Vec<u32> = (0..12).map(|t| t / 2).collect();
        assert_eq!(split_by_group(pos.len(), &idx, &per_face).len(), 6);
        let two_ids = [vec![0u32; 6], vec![1u32; 6]].concat();
        assert_eq!(split_by_group(pos.len(), &idx, &two_ids).len(), 2);
    }

    #[test]
    fn a_region_splits_triangles_by_their_centroid() {
        let (pos, idx) = cube(Vec3::ZERO, 1.0);
        let (inside, outside) = split_by_predicate(&pos, &idx, |c| c.x > 0.0);
        assert_eq!(inside.len() + outside.len(), 12);
        // Face 0 is the +X face: both its triangles have centroid x = 1.
        assert!(inside.contains(&0) && inside.contains(&1));
        assert!(outside.iter().all(|t| !inside.contains(t)));
    }

    #[test]
    fn a_region_can_be_split_into_its_connected_parts() {
        let (pos, idx) = two_cubes();
        let (inside, _) = split_by_predicate(&pos, &idx, |c| c.x > 2.5);
        let parts = connected_parts(&pos, &idx, &inside);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].len(), 12);
    }

    #[test]
    fn a_piece_keeps_each_source_vertex_so_attributes_carry_over() {
        let (pos, idx) = two_cubes();
        let parts = split_by_connectivity(&pos, &idx);
        let second = parts.iter().find(|p| p.contains(&12)).expect("the second cube");
        let piece = extract(pos.len(), &idx, second);
        assert_eq!(piece.source_triangles.len(), 12);
        assert_eq!(piece.source_vertices.len(), 24);
        assert!(piece.source_vertices.iter().all(|&v| v >= 24));
        assert_eq!(piece.indices.len(), 36);
        assert!(piece.indices.iter().all(|&i| (i as usize) < piece.source_vertices.len()));

        // Every piece triangle points at the same positions as its source triangle.
        let local = gather(&pos, &piece);
        for (k, &t) in piece.source_triangles.iter().enumerate() {
            for c in 0..3 {
                assert_eq!(local[piece.indices[3 * k + c] as usize], pos[idx[3 * t as usize + c] as usize]);
            }
        }

        // An attribute tagged by source vertex comes out in the piece's order.
        let tag: Vec<f32> = (0..pos.len()).map(|v| v as f32).collect();
        let expected: Vec<f32> = piece.source_vertices.iter().map(|&v| v as f32).collect();
        assert_eq!(gather(&tag, &piece), expected);
    }

    #[test]
    fn a_triangle_past_the_vertex_buffer_is_dropped_not_repaired() {
        let (pos, mut idx) = cube(Vec3::ZERO, 1.0);
        idx[0] = 999;
        let parts = split_by_connectivity(&pos, &idx);
        let kept: usize = parts.iter().map(Vec::len).sum();
        assert_eq!(kept, 11);
        assert!(parts.iter().all(|p| !p.contains(&0)));
        let piece = extract(pos.len(), &idx, &[0, 1]);
        assert_eq!(piece.source_triangles, vec![1]);
    }

    #[test]
    fn empty_or_partial_input_gives_no_parts() {
        assert!(split_by_connectivity(&[], &[]).is_empty());
        assert!(split_by_crease(&[], &[], 30.0).is_empty());
        assert!(split_by_group(0, &[], &[]).is_empty());
        let (pos, idx) = cube(Vec3::ZERO, 1.0);
        let mut partial = idx.clone();
        partial.push(0);
        assert_eq!(split_by_connectivity(&pos, &partial).len(), 1, "a trailing partial triangle is ignored");
    }

    #[test]
    fn the_same_input_names_the_parts_the_same_way() {
        let (pos, idx) = two_cubes();
        assert_eq!(split_by_connectivity(&pos, &idx), split_by_connectivity(&pos, &idx));
        assert_eq!(split_by_crease(&pos, &idx, 30.0), split_by_crease(&pos, &idx, 30.0));
    }
}
