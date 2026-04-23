//! Mesh-edit operations — extrude, bevel, inset, loop cut.
//!
//! Each op takes `&mut HalfEdgeMesh` + the target selection and
//! mutates in place. Ops that can't operate on the selection (wrong
//! kind, non-manifold) return `MeshEditError`.

use glam::Vec3;
use crate::{HalfEdgeMesh, FaceId, EdgeId, VertexId, MeshEditError, MeshEditResult};

/// Extrude a face along its normal by `distance`. Produces:
/// - a duplicate face at `original + normal * distance`
/// - quad side faces connecting the old perimeter to the new
///
/// v0 scope: single face, triangulated or polygon. Multi-face
/// connected extrusion (shared edge = single bridge quad) lands in
/// a follow-up.
pub fn extrude_face(
    mesh: &mut HalfEdgeMesh,
    face: FaceId,
    distance: f32,
) -> MeshEditResult<FaceId> {
    if face.0 as usize >= mesh.faces.len() {
        return Err(MeshEditError::InvalidFace(face.0));
    }
    let normal = mesh.face_normal(face);
    let offset = normal * distance;

    // 1. Duplicate each perimeter vertex at the offset position.
    let perimeter: Vec<VertexId> = mesh.face_vertex_ids(face);
    if perimeter.len() < 3 {
        return Err(MeshEditError::NonManifold {
            op: "extrude_face".into(),
            reason: "face has fewer than 3 vertices".into(),
        });
    }
    let new_verts: Vec<VertexId> = perimeter.iter().map(|v| {
        let p = mesh.vertices[v.0 as usize].position + offset;
        mesh.push_vertex(p)
    }).collect();

    // 2. Build the new top face (same winding, new verts).
    //    Also build the bridge quads (one per original edge).
    //    Rebuild the whole mesh's indexed form + create a new
    //    HalfEdgeMesh from scratch — simpler than patching topology
    //    in-place, and adequate for v0 (extrude rarely on huge meshes).
    let (old_positions, old_indices) = mesh.to_indexed_positions();
    let mut new_positions = old_positions.clone();
    let mut new_indices = old_indices.clone();

    // Side quads — for each original edge `(v_i, v_{i+1})`, build
    // two triangles: `v_i → v_{i+1} → new_{i+1}` and `v_i → new_{i+1} → new_i`.
    let n = perimeter.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let v_old_i = perimeter[i].0;
        let v_old_j = perimeter[j].0;
        let v_new_i = new_verts[i].0;
        let v_new_j = new_verts[j].0;
        new_indices.extend_from_slice(&[v_old_i, v_old_j, v_new_j]);
        new_indices.extend_from_slice(&[v_old_i, v_new_j, v_new_i]);
    }

    // New top face — triangulate polygon as a fan from new_verts[0].
    for i in 1..(n - 1) {
        new_indices.extend_from_slice(&[
            new_verts[0].0,
            new_verts[i].0,
            new_verts[i + 1].0,
        ]);
    }

    // Include the new positions (already in mesh.vertices post-push).
    new_positions = mesh.vertices.iter().map(|v| v.position).collect();

    // Rebuild. The *index* of the new top face is one-past the old
    // face count + the n side quads = mesh.faces.len() + n.
    let new_top_face_ix = mesh.faces.len() + n;

    // v0: the old face is dropped from the rebuilt mesh (extrude
    // semantics: the original is consumed by the side quads).
    // Filter out the old face's triangles from the rebuilt indices.
    let old_face_tris = {
        let verts = mesh.face_vertex_ids(face);
        let mut tris = Vec::new();
        for i in 1..(verts.len() - 1) {
            tris.push((verts[0].0, verts[i].0, verts[i + 1].0));
        }
        tris
    };
    let filtered_indices = filter_triangles(&new_indices, &old_face_tris);

    let rebuilt = HalfEdgeMesh::new(&new_positions, &filtered_indices, None)?;
    *mesh = rebuilt;
    Ok(FaceId(new_top_face_ix as u32))
}

/// Inset a face — shrink toward centroid, produce a quad ring +
/// a smaller inner face. `factor` in (0, 1) — 0.2 = 20% shrink.
pub fn inset_face(
    mesh: &mut HalfEdgeMesh,
    face: FaceId,
    factor: f32,
) -> MeshEditResult<FaceId> {
    if face.0 as usize >= mesh.faces.len() {
        return Err(MeshEditError::InvalidFace(face.0));
    }
    let factor = factor.clamp(0.001, 0.999);
    let centroid = mesh.face_centroid(face);
    let perimeter: Vec<VertexId> = mesh.face_vertex_ids(face);
    if perimeter.len() < 3 {
        return Err(MeshEditError::NonManifold {
            op: "inset_face".into(),
            reason: "face has fewer than 3 vertices".into(),
        });
    }

    let inner_verts: Vec<VertexId> = perimeter.iter().map(|v| {
        let p = mesh.vertices[v.0 as usize].position;
        let shrunk = p.lerp(centroid, factor);
        mesh.push_vertex(shrunk)
    }).collect();

    let (positions, mut indices) = mesh.to_indexed_positions();
    let mut new_positions = mesh.vertices.iter().map(|v| v.position).collect::<Vec<_>>();
    let _ = positions;

    // Replace old face triangles with: ring quads + inner polygon.
    let old_face_tris = {
        let verts = mesh.face_vertex_ids(face);
        let mut tris = Vec::new();
        for i in 1..(verts.len() - 1) {
            tris.push((verts[0].0, verts[i].0, verts[i + 1].0));
        }
        tris
    };
    indices = filter_triangles(&indices, &old_face_tris);

    // Ring quads.
    let n = perimeter.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let v_old_i = perimeter[i].0;
        let v_old_j = perimeter[j].0;
        let v_in_i = inner_verts[i].0;
        let v_in_j = inner_verts[j].0;
        indices.extend_from_slice(&[v_old_i, v_old_j, v_in_j]);
        indices.extend_from_slice(&[v_old_i, v_in_j, v_in_i]);
    }
    // Inner polygon — fan.
    for i in 1..(n - 1) {
        indices.extend_from_slice(&[
            inner_verts[0].0,
            inner_verts[i].0,
            inner_verts[i + 1].0,
        ]);
    }

    new_positions = mesh.vertices.iter().map(|v| v.position).collect();
    let rebuilt = HalfEdgeMesh::new(&new_positions, &indices, None)?;
    *mesh = rebuilt;
    // Inner face index is the last face added (fan of n-2 triangles
    // appended in order — first triangle's face is the v0).
    let inner_face_ix = mesh.faces.len().saturating_sub(1);
    Ok(FaceId(inner_face_ix as u32))
}

/// Bevel an edge — split it into a small face. v0 returns
/// NotImplemented; topology-walker for dual edge loops is pending.
pub fn bevel_edge(
    _mesh: &mut HalfEdgeMesh,
    _edge: EdgeId,
    _width: f32,
) -> MeshEditResult<()> {
    Err(MeshEditError::NotImplemented(
        "bevel_edge — edge-loop walker + split pending"
    ))
}

/// Loop cut across an edge loop. v0 returns NotImplemented —
/// loop-walker traverses quads via `twin.next` alternation; lands
/// alongside bevel since they share the same walker.
pub fn loop_cut(
    _mesh: &mut HalfEdgeMesh,
    _seed_edge: EdgeId,
) -> MeshEditResult<Vec<EdgeId>> {
    Err(MeshEditError::NotImplemented(
        "loop_cut — edge-loop walker pending"
    ))
}

// ============================================================================
// Helpers
// ============================================================================

/// Remove triangles matching any of the given `(a, b, c)` tuples from
/// a flat index buffer. Matching is rotation-aware (same winding
/// counts).
fn filter_triangles(indices: &[u32], drop: &[(u32, u32, u32)]) -> Vec<u32> {
    let mut out = Vec::with_capacity(indices.len());
    for chunk in indices.chunks_exact(3) {
        let (a, b, c) = (chunk[0], chunk[1], chunk[2]);
        let matches_any = drop.iter().any(|(da, db, dc)| {
            (a == *da && b == *db && c == *dc)
                || (a == *db && b == *dc && c == *da)
                || (a == *dc && b == *da && c == *db)
        });
        if !matches_any {
            out.extend_from_slice(&[a, b, c]);
        }
    }
    out
}
