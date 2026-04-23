//! # eustress-mesh-edit — half-edge mesh-editing kernel
//!
//! Targets **arbitrary non-parametric meshes** (MeshParts, imported
//! GLBs, CSG output). Distinct from [`eustress-cad`], which drives
//! parametric BRep bodies via `truck`.
//!
//! ## Data structure
//!
//! Standard half-edge: every edge is two directional half-edges
//! pointing to each other via `twin`. Each half-edge carries `origin`
//! (vertex id), `face` (face id or None for boundary), `next`
//! (next half-edge around the face), and `twin`.
//!
//! Vertices store an outgoing half-edge; faces store one of their
//! half-edges. All ids are arena indices so operations stay O(1)
//! per edge touched + O(N) per face for face-level ops.
//!
//! ## Shipped operations (v0)
//!
//! | Op          | Status   | Notes                                          |
//! |-------------|----------|------------------------------------------------|
//! | `new_mesh`  | ✅       | Construct from positions + face-index triples  |
//! | `select_vertex/edge/face` | ✅ | Build `MeshSelection` sets             |
//! | `extrude_face` | ✅    | Duplicate face along its normal, bridge with quads |
//! | `inset_face`   | ✅    | Shrink face toward centroid, new quad ring    |
//! | `bevel_edge`   | 🚧    | Replace edge with a small face — topology WIP |
//! | `loop_cut`     | 🚧    | Edge-loop walker + split — topology WIP       |
//! | `to_indexed_positions` | ✅ | Convert back to positions + indices      |
//!
//! Loop cut + bevel ship as topology-walker functions in a follow-up
//! PR — the data structure + canonical operations are the scaffold.

pub mod half_edge;
pub mod ops;
pub mod error;

pub use half_edge::{
    HalfEdgeMesh, VertexId, EdgeId, FaceId, HalfEdgeId,
    MeshSelection, SelectionKind,
};
pub use ops::{extrude_face, inset_face, bevel_edge, loop_cut};
pub use error::{MeshEditError, MeshEditResult};
