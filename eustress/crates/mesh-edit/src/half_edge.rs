//! Half-edge mesh data structure.

use glam::Vec3;
use std::collections::{HashMap, HashSet};
use crate::{MeshEditError, MeshEditResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VertexId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EdgeId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FaceId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HalfEdgeId(pub u32);

#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: Vec3,
    /// One of the half-edges originating at this vertex. None for
    /// isolated vertices (none in a well-formed mesh).
    pub outgoing: Option<HalfEdgeId>,
}

#[derive(Debug, Clone, Copy)]
pub struct HalfEdge {
    /// Vertex this half-edge originates from.
    pub origin: VertexId,
    /// The opposite half-edge. Must be set for every real edge.
    pub twin: HalfEdgeId,
    /// Next half-edge around the bounded face.
    pub next: HalfEdgeId,
    /// Face this half-edge bounds. `None` = boundary half-edge.
    pub face: Option<FaceId>,
    /// Edge id this half-edge belongs to (pair of half-edges per edge).
    pub edge: EdgeId,
}

#[derive(Debug, Clone, Copy)]
pub struct Edge {
    /// One of the two half-edges. The other is `he.twin`.
    pub he: HalfEdgeId,
}

#[derive(Debug, Clone, Copy)]
pub struct Face {
    /// One of the half-edges bounding this face.
    pub he: HalfEdgeId,
}

#[derive(Debug, Clone, Default)]
pub struct HalfEdgeMesh {
    pub vertices: Vec<Vertex>,
    pub half_edges: Vec<HalfEdge>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
}

impl HalfEdgeMesh {
    /// Build from flat triangles (positions + triples of vertex indices).
    ///
    /// Non-triangulated polygons are accepted too — supply `face_sizes`
    /// telling the builder how many indices per face. Pass `None` to
    /// assume every face is a triangle.
    pub fn new(
        positions: &[Vec3],
        indices: &[u32],
        face_sizes: Option<&[u32]>,
    ) -> MeshEditResult<Self> {
        if indices.is_empty() { return Err(MeshEditError::Empty); }
        let mut mesh = HalfEdgeMesh::default();
        mesh.vertices = positions.iter().map(|p| Vertex {
            position: *p,
            outgoing: None,
        }).collect();

        // Walk face chunks.
        let mut cursor = 0usize;
        let default_sizes: Vec<u32>;
        let face_sizes: &[u32] = match face_sizes {
            Some(fs) => fs,
            None => {
                // assume triangles
                default_sizes = vec![3u32; indices.len() / 3];
                &default_sizes
            }
        };

        // `(origin, target) → half_edge_id` map used to pair up twins.
        let mut pair_map: HashMap<(u32, u32), HalfEdgeId> = HashMap::new();

        for &n in face_sizes {
            let n = n as usize;
            if n < 3 { continue; }
            let face_verts: Vec<u32> = indices[cursor..cursor + n].to_vec();
            cursor += n;

            let face_id = FaceId(mesh.faces.len() as u32);
            let first_he = HalfEdgeId(mesh.half_edges.len() as u32);

            // Reserve half-edge ids + placeholder entries; fill once
            // next + edge + twin are known.
            for _ in 0..n {
                mesh.half_edges.push(HalfEdge {
                    origin: VertexId(u32::MAX),
                    twin: HalfEdgeId(u32::MAX),
                    next: HalfEdgeId(u32::MAX),
                    face: Some(face_id),
                    edge: EdgeId(u32::MAX),
                });
            }

            for i in 0..n {
                let he_id = HalfEdgeId(first_he.0 + i as u32);
                let next_id = HalfEdgeId(first_he.0 + ((i + 1) % n) as u32);
                let v_from = face_verts[i];
                let v_to   = face_verts[(i + 1) % n];

                mesh.half_edges[he_id.0 as usize].origin = VertexId(v_from);
                mesh.half_edges[he_id.0 as usize].next   = next_id;

                // Pair with twin if the reverse direction was seen.
                let key = (v_from, v_to);
                let reverse_key = (v_to, v_from);
                if let Some(twin_id) = pair_map.remove(&reverse_key) {
                    mesh.half_edges[he_id.0 as usize].twin = twin_id;
                    mesh.half_edges[twin_id.0 as usize].twin = he_id;
                    // Reuse the twin's edge id.
                    let edge = mesh.half_edges[twin_id.0 as usize].edge;
                    mesh.half_edges[he_id.0 as usize].edge = edge;
                } else {
                    // Create a fresh edge.
                    let edge_id = EdgeId(mesh.edges.len() as u32);
                    mesh.edges.push(Edge { he: he_id });
                    mesh.half_edges[he_id.0 as usize].edge = edge_id;
                    pair_map.insert(key, he_id);
                }

                // First-outgoing for this vertex.
                if mesh.vertices[v_from as usize].outgoing.is_none() {
                    mesh.vertices[v_from as usize].outgoing = Some(he_id);
                }
            }
            mesh.faces.push(Face { he: first_he });
        }

        // Unpaired half-edges are boundary; create synthetic twins so
        // every he has a `twin` pointer (they carry `face: None`).
        let unpaired: Vec<(HalfEdgeId, u32, u32)> = pair_map.iter()
            .map(|(&(vf, vt), &he)| (he, vf, vt))
            .collect();
        for (he_id, _vf, vt) in unpaired {
            let boundary_id = HalfEdgeId(mesh.half_edges.len() as u32);
            let edge = mesh.half_edges[he_id.0 as usize].edge;
            mesh.half_edges.push(HalfEdge {
                origin: VertexId(vt),
                twin: he_id,
                next: HalfEdgeId(u32::MAX), // left unchained; boundary walkers set this lazily
                face: None,
                edge,
            });
            mesh.half_edges[he_id.0 as usize].twin = boundary_id;
        }

        Ok(mesh)
    }

    /// Flatten back to positions + indices (triangle fan per face).
    /// Usable for `bevy::prelude::Mesh` rebuild after editing.
    pub fn to_indexed_positions(&self) -> (Vec<Vec3>, Vec<u32>) {
        let positions: Vec<Vec3> = self.vertices.iter().map(|v| v.position).collect();
        let mut indices = Vec::new();
        for face in &self.faces {
            let verts = self.face_vertex_ids(FaceId(face.he.0)); // will re-walk
            // face.he is a half-edge id; refactor to take FaceId directly.
            let _ = verts; let _ = face;
        }
        // Correct walker below — the above placeholder was wrong; iterate
        // by face index properly.
        indices.clear();
        for (f_ix, _face) in self.faces.iter().enumerate() {
            let verts = self.face_vertices_by_index(f_ix);
            if verts.len() < 3 { continue; }
            for i in 1..verts.len() - 1 {
                indices.push(verts[0].0);
                indices.push(verts[i].0);
                indices.push(verts[i + 1].0);
            }
        }
        (positions, indices)
    }

    /// Walk a face's half-edges and return its vertex ids in order.
    pub fn face_vertices_by_index(&self, face_ix: usize) -> Vec<VertexId> {
        let Some(face) = self.faces.get(face_ix) else { return Vec::new(); };
        let mut out = Vec::new();
        let start = face.he;
        let mut he = start;
        // Cap the walk at `half_edges.len()` to avoid any accidental
        // infinite loop from malformed topology.
        let cap = self.half_edges.len();
        for _ in 0..cap {
            out.push(self.half_edges[he.0 as usize].origin);
            he = self.half_edges[he.0 as usize].next;
            if he == start { break; }
        }
        out
    }

    /// Same but with FaceId — convenience.
    pub fn face_vertex_ids(&self, face: FaceId) -> Vec<VertexId> {
        self.face_vertices_by_index(face.0 as usize)
    }

    /// Face normal from first three vertices (CCW convention).
    pub fn face_normal(&self, face: FaceId) -> Vec3 {
        let verts = self.face_vertex_ids(face);
        if verts.len() < 3 { return Vec3::Y; }
        let a = self.vertices[verts[0].0 as usize].position;
        let b = self.vertices[verts[1].0 as usize].position;
        let c = self.vertices[verts[2].0 as usize].position;
        (b - a).cross(c - a).normalize_or_zero()
    }

    pub fn face_centroid(&self, face: FaceId) -> Vec3 {
        let verts = self.face_vertex_ids(face);
        if verts.is_empty() { return Vec3::ZERO; }
        let sum: Vec3 = verts.iter()
            .map(|v| self.vertices[v.0 as usize].position)
            .sum();
        sum / verts.len() as f32
    }

    /// Push a fresh vertex at `pos`, return its id. No topology hookup —
    /// callers wire half-edges in afterwards.
    pub fn push_vertex(&mut self, pos: Vec3) -> VertexId {
        let id = VertexId(self.vertices.len() as u32);
        self.vertices.push(Vertex { position: pos, outgoing: None });
        id
    }
}

// ============================================================================
// Selection
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind { Vertex, Edge, Face }

#[derive(Debug, Clone, Default)]
pub struct MeshSelection {
    pub kind: Option<SelectionKind>,
    pub vertices: HashSet<VertexId>,
    pub edges:    HashSet<EdgeId>,
    pub faces:    HashSet<FaceId>,
}

impl MeshSelection {
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.edges.clear();
        self.faces.clear();
        self.kind = None;
    }

    pub fn select_vertex(&mut self, v: VertexId) {
        self.kind = Some(SelectionKind::Vertex);
        self.vertices.insert(v);
    }
    pub fn select_edge(&mut self, e: EdgeId) {
        self.kind = Some(SelectionKind::Edge);
        self.edges.insert(e);
    }
    pub fn select_face(&mut self, f: FaceId) {
        self.kind = Some(SelectionKind::Face);
        self.faces.insert(f);
    }
}
