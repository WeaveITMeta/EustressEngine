//! Marching cubes for volumetric terrain chunks.
//!
//! A chunk whose LOD-0 vertex columns hold [`TerrainVolume`] bricks (see
//! [`chunk_has_volume`]) is drawn by marching cubes over the terrain field
//! instead of as one height per column, so its caves, overhangs and tunnels
//! show. Every other chunk keeps the heightfield mesh, and so does a
//! volumetric chunk at LOD 1 and coarser: caves are not drawn far away,
//! where their openings cover a few pixels and marching a tall lattice for
//! them would cost more than the rest of the distant ring together.
//! [`generate_chunk_render_mesh`] makes that choice for every system that
//! meshes a chunk; `collider.rs` makes the same one for colliders, which use
//! this surface as a trimesh whatever LOD the chunk renders at.
//!
//! ## Lattice
//!
//! The field is evaluated on the chunk's LOD-0 vertex lattice
//! (`chunk_resolution` cells per side, the same spacing on Y) through
//! [`lattice_field_sample`], which is exactly
//! [`sample_field_lattice`](super::volume::sample_field_lattice). Each
//! lattice cell marches its own band of layers: the lowest to highest ground
//! of its four corner columns and the Y extent of the bricks over them, plus
//! [`MARCH_MARGIN_CELLS`] on both ends. Below the band every cube is solid
//! throughout and above it air, so the work follows the surface rather than
//! the chunk's whole relief, and the surface closes everywhere except along
//! the chunk's four sides. Two cells sharing a face, in one chunk or in two,
//! both march every lattice edge the surface crosses on it: such an edge
//! lies within the ground and brick range of its two end columns, which are
//! corners of both cells. A vertex sits where the field's linear
//! interpolant crosses zero on a lattice edge, at a position local to the
//! chunk entity computed exactly like the heightfield mesh's. Its normal is
//! the field's central-difference gradient at the edge's two lattice points,
//! interpolated to the vertex: a function of global lattice samples only, so
//! a border vertex gets the same normal from both chunks.
//!
//! ## Seams with heightfield chunks
//!
//! No skirt is needed between a volumetric chunk and a heightfield chunk at
//! the same LOD, because where no edit reaches their shared border the two
//! draw the same border polyline. Every border point is then unedited (a
//! brick owning one would make the neighbour volumetric too), so the field
//! there is `y - H`, with `H` the lattice height the heightfield mesh uses.
//! On a vertical lattice edge that is linear in `y` and crosses zero exactly
//! at `H`, the heightfield vertex. On a horizontal edge at layer `y` between
//! border columns of heights `H0` and `H1` it crosses at fraction
//! `t = (H0 - y) / (H0 - H1)`, where the straight heightfield edge is at
//! height `H0 + t (H1 - H0) = y`: the vertex lies on that edge. So the
//! marching-cubes border is the heightfield border with extra vertices on
//! its segments, which the test
//! `a_volumetric_border_without_nearby_edits_is_the_heightfield_border`
//! checks.
//!
//! A volumetric chunk hangs a skirt from every border segment whose lattice
//! edges, and the column below them to skirt depth, hold no edit. Those
//! segments lie on the heightfield border line just described, so the skirt
//! covers the gap to a coarser neighbour, as between two heightfield chunks,
//! and to any neighbour drawn as a heightfield: a volumetric chunk at LOD 1
//! and coarser, or one whose march fell back. No skirt hangs where an edit
//! touches the border, where a cave can cross into a marching-cubes
//! neighbour and a skirt would hang across it.
//!
//! ## Tables
//!
//! Corner, edge and case numbering follow Paul Bourke's "Polygonising a
//! scalar field" tables (public domain, after Lorensen and Cline): case bit
//! `i` is set when corner `i` is solid, and an ambiguous face keeps its solid
//! corners apart. His corners 0 to 3 run around the lower Y layer and 4 to 7
//! sit above them ([`CORNERS`]); in this frame the triangles wind
//! counter-clockwise seen from air, Bevy's front face. [`EDGE_TABLE`] is
//! derived from its definition and [`TRI_TABLE`] is the published table. The
//! tests check every case for closure and orientation and every pair of
//! cases for agreement across a shared face, which is what keeps the surface
//! watertight from one cube to the next.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::height_query::{height_at_world, world_to_uv};
use super::mesh::{generate_chunk_mesh, material_vertex_color, skirt_depth, HeightfieldShading};
use super::volume::{
    brick_lattice_origin, chunk_has_volume, lattice_cell_size, lattice_field_sample,
    lattice_surface_height, lattice_to_brick, material_at, sample_field_parts, FieldTerm,
    TerrainVolume, VolumeBrick, VolumeCell, BRICK_EDGE,
};
use super::{chunk_world_position, TerrainConfig, TerrainData, TerrainMaterial};

/// Lattice cells a volumetric chunk marches below its lowest ground or brick
/// point and above its highest, so its box closes: solid across the bottom
/// layer, air across the top.
pub const MARCH_MARGIN_CELLS: i32 = 2;

/// Most lattice layers one cell of a volumetric chunk marches. A chunk with a
/// cell whose ground and bricks span more than this keeps its heightfield
/// mesh and collider rather than stall the frame.
const MAX_MARCH_LAYERS: i64 = 256;

/// The least building a volumetric chunk's mesh or collider costs against a
/// heightfield chunk's in the per-frame budgets of the systems that build
/// them; a chunk whose bands hold more cubes per cell than this costs that
/// many (see [`chunk_mesh_cost`]). Marching samples a band of lattice layers
/// and bakes a colour from the field at every vertex, and a trimesh collider
/// builds a BVH and its edge topology.
pub const VOLUMETRIC_CHUNK_COST: usize = 8;

// ============================================================================
// Tables
// ============================================================================

/// Cube corner offsets in lattice steps, in Bourke's numbering: 0 to 3 run
/// +X then +Z around the lower Y layer, 4 to 7 sit one layer above them.
pub const CORNERS: [[u32; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [1, 0, 1],
    [0, 0, 1],
    [0, 1, 0],
    [1, 1, 0],
    [1, 1, 1],
    [0, 1, 1],
];

/// The two corners each of the 12 cube edges joins, in Bourke's numbering.
pub const EDGES: [[usize; 2]; 12] = [
    [0, 1],
    [1, 2],
    [2, 3],
    [3, 0],
    [4, 5],
    [5, 6],
    [6, 7],
    [7, 4],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];

/// Each cube edge as a lattice edge: the offset of its lower corner and the
/// axis it runs along (0 = X, 1 = Y, 2 = Z). Vertices are keyed by lattice
/// edge, so the four cubes around an edge share its vertex.
const EDGE_LATTICE: [([u32; 3], usize); 12] = [
    ([0, 0, 0], 0),
    ([1, 0, 0], 2),
    ([0, 0, 1], 0),
    ([0, 0, 0], 2),
    ([0, 1, 0], 0),
    ([1, 1, 0], 2),
    ([0, 1, 1], 0),
    ([0, 1, 0], 2),
    ([0, 0, 0], 1),
    ([1, 0, 0], 1),
    ([1, 0, 1], 1),
    ([0, 0, 1], 1),
];

/// For each case, bit `e` set when edge `e` joins a solid corner to an air
/// corner, so the surface crosses it.
pub const EDGE_TABLE: [u16; 256] = build_edge_table();

const fn build_edge_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut case = 0usize;
    while case < 256 {
        let mut bits = 0u16;
        let mut edge = 0usize;
        while edge < 12 {
            let a = (case >> EDGES[edge][0]) & 1;
            let b = (case >> EDGES[edge][1]) & 1;
            if a != b {
                bits |= 1u16 << edge;
            }
            edge += 1;
        }
        table[case] = bits;
        case += 1;
    }
    table
}

/// For each case, up to five triangles as triples of crossed edges, then -1.
#[rustfmt::skip]
pub const TRI_TABLE: [[i8; 16]; 256] = [
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 9, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 8, 3, 9, 8, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 1, 2, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 2, 10, 0, 2, 9, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 8, 3, 2, 10, 8, 10, 9, 8, -1, -1, -1, -1, -1, -1, -1],
    [3, 11, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 11, 2, 8, 11, 0, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 9, 0, 2, 3, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 11, 2, 1, 9, 11, 9, 8, 11, -1, -1, -1, -1, -1, -1, -1],
    [3, 10, 1, 11, 10, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 10, 1, 0, 8, 10, 8, 11, 10, -1, -1, -1, -1, -1, -1, -1],
    [3, 9, 0, 3, 11, 9, 11, 10, 9, -1, -1, -1, -1, -1, -1, -1],
    [9, 8, 10, 10, 8, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 7, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 3, 0, 7, 3, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 9, 8, 4, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 1, 9, 4, 7, 1, 7, 3, 1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 8, 4, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 4, 7, 3, 0, 4, 1, 2, 10, -1, -1, -1, -1, -1, -1, -1],
    [9, 2, 10, 9, 0, 2, 8, 4, 7, -1, -1, -1, -1, -1, -1, -1],
    [2, 10, 9, 2, 9, 7, 2, 7, 3, 7, 9, 4, -1, -1, -1, -1],
    [8, 4, 7, 3, 11, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [11, 4, 7, 11, 2, 4, 2, 0, 4, -1, -1, -1, -1, -1, -1, -1],
    [9, 0, 1, 8, 4, 7, 2, 3, 11, -1, -1, -1, -1, -1, -1, -1],
    [4, 7, 11, 9, 4, 11, 9, 11, 2, 9, 2, 1, -1, -1, -1, -1],
    [3, 10, 1, 3, 11, 10, 7, 8, 4, -1, -1, -1, -1, -1, -1, -1],
    [1, 11, 10, 1, 4, 11, 1, 0, 4, 7, 11, 4, -1, -1, -1, -1],
    [4, 7, 8, 9, 0, 11, 9, 11, 10, 11, 0, 3, -1, -1, -1, -1],
    [4, 7, 11, 4, 11, 9, 9, 11, 10, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 4, 0, 8, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 5, 4, 1, 5, 0, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [8, 5, 4, 8, 3, 5, 3, 1, 5, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 9, 5, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 8, 1, 2, 10, 4, 9, 5, -1, -1, -1, -1, -1, -1, -1],
    [5, 2, 10, 5, 4, 2, 4, 0, 2, -1, -1, -1, -1, -1, -1, -1],
    [2, 10, 5, 3, 2, 5, 3, 5, 4, 3, 4, 8, -1, -1, -1, -1],
    [9, 5, 4, 2, 3, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 11, 2, 0, 8, 11, 4, 9, 5, -1, -1, -1, -1, -1, -1, -1],
    [0, 5, 4, 0, 1, 5, 2, 3, 11, -1, -1, -1, -1, -1, -1, -1],
    [2, 1, 5, 2, 5, 8, 2, 8, 11, 4, 8, 5, -1, -1, -1, -1],
    [10, 3, 11, 10, 1, 3, 9, 5, 4, -1, -1, -1, -1, -1, -1, -1],
    [4, 9, 5, 0, 8, 1, 8, 10, 1, 8, 11, 10, -1, -1, -1, -1],
    [5, 4, 0, 5, 0, 11, 5, 11, 10, 11, 0, 3, -1, -1, -1, -1],
    [5, 4, 8, 5, 8, 10, 10, 8, 11, -1, -1, -1, -1, -1, -1, -1],
    [9, 7, 8, 5, 7, 9, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 3, 0, 9, 5, 3, 5, 7, 3, -1, -1, -1, -1, -1, -1, -1],
    [0, 7, 8, 0, 1, 7, 1, 5, 7, -1, -1, -1, -1, -1, -1, -1],
    [1, 5, 3, 3, 5, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 7, 8, 9, 5, 7, 10, 1, 2, -1, -1, -1, -1, -1, -1, -1],
    [10, 1, 2, 9, 5, 0, 5, 3, 0, 5, 7, 3, -1, -1, -1, -1],
    [8, 0, 2, 8, 2, 5, 8, 5, 7, 10, 5, 2, -1, -1, -1, -1],
    [2, 10, 5, 2, 5, 3, 3, 5, 7, -1, -1, -1, -1, -1, -1, -1],
    [7, 9, 5, 7, 8, 9, 3, 11, 2, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 7, 9, 7, 2, 9, 2, 0, 2, 7, 11, -1, -1, -1, -1],
    [2, 3, 11, 0, 1, 8, 1, 7, 8, 1, 5, 7, -1, -1, -1, -1],
    [11, 2, 1, 11, 1, 7, 7, 1, 5, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 8, 8, 5, 7, 10, 1, 3, 10, 3, 11, -1, -1, -1, -1],
    [5, 7, 0, 5, 0, 9, 7, 11, 0, 1, 0, 10, 11, 10, 0, -1],
    [11, 10, 0, 11, 0, 3, 10, 5, 0, 8, 0, 7, 5, 7, 0, -1],
    [11, 10, 5, 7, 11, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [10, 6, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 5, 10, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 0, 1, 5, 10, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 8, 3, 1, 9, 8, 5, 10, 6, -1, -1, -1, -1, -1, -1, -1],
    [1, 6, 5, 2, 6, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 6, 5, 1, 2, 6, 3, 0, 8, -1, -1, -1, -1, -1, -1, -1],
    [9, 6, 5, 9, 0, 6, 0, 2, 6, -1, -1, -1, -1, -1, -1, -1],
    [5, 9, 8, 5, 8, 2, 5, 2, 6, 3, 2, 8, -1, -1, -1, -1],
    [2, 3, 11, 10, 6, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [11, 0, 8, 11, 2, 0, 10, 6, 5, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 9, 2, 3, 11, 5, 10, 6, -1, -1, -1, -1, -1, -1, -1],
    [5, 10, 6, 1, 9, 2, 9, 11, 2, 9, 8, 11, -1, -1, -1, -1],
    [6, 3, 11, 6, 5, 3, 5, 1, 3, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 11, 0, 11, 5, 0, 5, 1, 5, 11, 6, -1, -1, -1, -1],
    [3, 11, 6, 0, 3, 6, 0, 6, 5, 0, 5, 9, -1, -1, -1, -1],
    [6, 5, 9, 6, 9, 11, 11, 9, 8, -1, -1, -1, -1, -1, -1, -1],
    [5, 10, 6, 4, 7, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 3, 0, 4, 7, 3, 6, 5, 10, -1, -1, -1, -1, -1, -1, -1],
    [1, 9, 0, 5, 10, 6, 8, 4, 7, -1, -1, -1, -1, -1, -1, -1],
    [10, 6, 5, 1, 9, 7, 1, 7, 3, 7, 9, 4, -1, -1, -1, -1],
    [6, 1, 2, 6, 5, 1, 4, 7, 8, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 5, 5, 2, 6, 3, 0, 4, 3, 4, 7, -1, -1, -1, -1],
    [8, 4, 7, 9, 0, 5, 0, 6, 5, 0, 2, 6, -1, -1, -1, -1],
    [7, 3, 9, 7, 9, 4, 3, 2, 9, 5, 9, 6, 2, 6, 9, -1],
    [3, 11, 2, 7, 8, 4, 10, 6, 5, -1, -1, -1, -1, -1, -1, -1],
    [5, 10, 6, 4, 7, 2, 4, 2, 0, 2, 7, 11, -1, -1, -1, -1],
    [0, 1, 9, 4, 7, 8, 2, 3, 11, 5, 10, 6, -1, -1, -1, -1],
    [9, 2, 1, 9, 11, 2, 9, 4, 11, 7, 11, 4, 5, 10, 6, -1],
    [8, 4, 7, 3, 11, 5, 3, 5, 1, 5, 11, 6, -1, -1, -1, -1],
    [5, 1, 11, 5, 11, 6, 1, 0, 11, 7, 11, 4, 0, 4, 11, -1],
    [0, 5, 9, 0, 6, 5, 0, 3, 6, 11, 6, 3, 8, 4, 7, -1],
    [6, 5, 9, 6, 9, 11, 4, 7, 9, 7, 11, 9, -1, -1, -1, -1],
    [10, 4, 9, 6, 4, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 10, 6, 4, 9, 10, 0, 8, 3, -1, -1, -1, -1, -1, -1, -1],
    [10, 0, 1, 10, 6, 0, 6, 4, 0, -1, -1, -1, -1, -1, -1, -1],
    [8, 3, 1, 8, 1, 6, 8, 6, 4, 6, 1, 10, -1, -1, -1, -1],
    [1, 4, 9, 1, 2, 4, 2, 6, 4, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 8, 1, 2, 9, 2, 4, 9, 2, 6, 4, -1, -1, -1, -1],
    [0, 2, 4, 4, 2, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [8, 3, 2, 8, 2, 4, 4, 2, 6, -1, -1, -1, -1, -1, -1, -1],
    [10, 4, 9, 10, 6, 4, 11, 2, 3, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 2, 2, 8, 11, 4, 9, 10, 4, 10, 6, -1, -1, -1, -1],
    [3, 11, 2, 0, 1, 6, 0, 6, 4, 6, 1, 10, -1, -1, -1, -1],
    [6, 4, 1, 6, 1, 10, 4, 8, 1, 2, 1, 11, 8, 11, 1, -1],
    [9, 6, 4, 9, 3, 6, 9, 1, 3, 11, 6, 3, -1, -1, -1, -1],
    [8, 11, 1, 8, 1, 0, 11, 6, 1, 9, 1, 4, 6, 4, 1, -1],
    [3, 11, 6, 3, 6, 0, 0, 6, 4, -1, -1, -1, -1, -1, -1, -1],
    [6, 4, 8, 11, 6, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [7, 10, 6, 7, 8, 10, 8, 9, 10, -1, -1, -1, -1, -1, -1, -1],
    [0, 7, 3, 0, 10, 7, 0, 9, 10, 6, 7, 10, -1, -1, -1, -1],
    [10, 6, 7, 1, 10, 7, 1, 7, 8, 1, 8, 0, -1, -1, -1, -1],
    [10, 6, 7, 10, 7, 1, 1, 7, 3, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 6, 1, 6, 8, 1, 8, 9, 8, 6, 7, -1, -1, -1, -1],
    [2, 6, 9, 2, 9, 1, 6, 7, 9, 0, 9, 3, 7, 3, 9, -1],
    [7, 8, 0, 7, 0, 6, 6, 0, 2, -1, -1, -1, -1, -1, -1, -1],
    [7, 3, 2, 6, 7, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 3, 11, 10, 6, 8, 10, 8, 9, 8, 6, 7, -1, -1, -1, -1],
    [2, 0, 7, 2, 7, 11, 0, 9, 7, 6, 7, 10, 9, 10, 7, -1],
    [1, 8, 0, 1, 7, 8, 1, 10, 7, 6, 7, 10, 2, 3, 11, -1],
    [11, 2, 1, 11, 1, 7, 10, 6, 1, 6, 7, 1, -1, -1, -1, -1],
    [8, 9, 6, 8, 6, 7, 9, 1, 6, 11, 6, 3, 1, 3, 6, -1],
    [0, 9, 1, 11, 6, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [7, 8, 0, 7, 0, 6, 3, 11, 0, 11, 6, 0, -1, -1, -1, -1],
    [7, 11, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [7, 6, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 8, 11, 7, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 9, 11, 7, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [8, 1, 9, 8, 3, 1, 11, 7, 6, -1, -1, -1, -1, -1, -1, -1],
    [10, 1, 2, 6, 11, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 3, 0, 8, 6, 11, 7, -1, -1, -1, -1, -1, -1, -1],
    [2, 9, 0, 2, 10, 9, 6, 11, 7, -1, -1, -1, -1, -1, -1, -1],
    [6, 11, 7, 2, 10, 3, 10, 8, 3, 10, 9, 8, -1, -1, -1, -1],
    [7, 2, 3, 6, 2, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [7, 0, 8, 7, 6, 0, 6, 2, 0, -1, -1, -1, -1, -1, -1, -1],
    [2, 7, 6, 2, 3, 7, 0, 1, 9, -1, -1, -1, -1, -1, -1, -1],
    [1, 6, 2, 1, 8, 6, 1, 9, 8, 8, 7, 6, -1, -1, -1, -1],
    [10, 7, 6, 10, 1, 7, 1, 3, 7, -1, -1, -1, -1, -1, -1, -1],
    [10, 7, 6, 1, 7, 10, 1, 8, 7, 1, 0, 8, -1, -1, -1, -1],
    [0, 3, 7, 0, 7, 10, 0, 10, 9, 6, 10, 7, -1, -1, -1, -1],
    [7, 6, 10, 7, 10, 8, 8, 10, 9, -1, -1, -1, -1, -1, -1, -1],
    [6, 8, 4, 11, 8, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 6, 11, 3, 0, 6, 0, 4, 6, -1, -1, -1, -1, -1, -1, -1],
    [8, 6, 11, 8, 4, 6, 9, 0, 1, -1, -1, -1, -1, -1, -1, -1],
    [9, 4, 6, 9, 6, 3, 9, 3, 1, 11, 3, 6, -1, -1, -1, -1],
    [6, 8, 4, 6, 11, 8, 2, 10, 1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 3, 0, 11, 0, 6, 11, 0, 4, 6, -1, -1, -1, -1],
    [4, 11, 8, 4, 6, 11, 0, 2, 9, 2, 10, 9, -1, -1, -1, -1],
    [10, 9, 3, 10, 3, 2, 9, 4, 3, 11, 3, 6, 4, 6, 3, -1],
    [8, 2, 3, 8, 4, 2, 4, 6, 2, -1, -1, -1, -1, -1, -1, -1],
    [0, 4, 2, 4, 6, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 9, 0, 2, 3, 4, 2, 4, 6, 4, 3, 8, -1, -1, -1, -1],
    [1, 9, 4, 1, 4, 2, 2, 4, 6, -1, -1, -1, -1, -1, -1, -1],
    [8, 1, 3, 8, 6, 1, 8, 4, 6, 6, 10, 1, -1, -1, -1, -1],
    [10, 1, 0, 10, 0, 6, 6, 0, 4, -1, -1, -1, -1, -1, -1, -1],
    [4, 6, 3, 4, 3, 8, 6, 10, 3, 0, 3, 9, 10, 9, 3, -1],
    [10, 9, 4, 6, 10, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 9, 5, 7, 6, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 4, 9, 5, 11, 7, 6, -1, -1, -1, -1, -1, -1, -1],
    [5, 0, 1, 5, 4, 0, 7, 6, 11, -1, -1, -1, -1, -1, -1, -1],
    [11, 7, 6, 8, 3, 4, 3, 5, 4, 3, 1, 5, -1, -1, -1, -1],
    [9, 5, 4, 10, 1, 2, 7, 6, 11, -1, -1, -1, -1, -1, -1, -1],
    [6, 11, 7, 1, 2, 10, 0, 8, 3, 4, 9, 5, -1, -1, -1, -1],
    [7, 6, 11, 5, 4, 10, 4, 2, 10, 4, 0, 2, -1, -1, -1, -1],
    [3, 4, 8, 3, 5, 4, 3, 2, 5, 10, 5, 2, 11, 7, 6, -1],
    [7, 2, 3, 7, 6, 2, 5, 4, 9, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 4, 0, 8, 6, 0, 6, 2, 6, 8, 7, -1, -1, -1, -1],
    [3, 6, 2, 3, 7, 6, 1, 5, 0, 5, 4, 0, -1, -1, -1, -1],
    [6, 2, 8, 6, 8, 7, 2, 1, 8, 4, 8, 5, 1, 5, 8, -1],
    [9, 5, 4, 10, 1, 6, 1, 7, 6, 1, 3, 7, -1, -1, -1, -1],
    [1, 6, 10, 1, 7, 6, 1, 0, 7, 8, 7, 0, 9, 5, 4, -1],
    [4, 0, 10, 4, 10, 5, 0, 3, 10, 6, 10, 7, 3, 7, 10, -1],
    [7, 6, 10, 7, 10, 8, 5, 4, 10, 4, 8, 10, -1, -1, -1, -1],
    [6, 9, 5, 6, 11, 9, 11, 8, 9, -1, -1, -1, -1, -1, -1, -1],
    [3, 6, 11, 0, 6, 3, 0, 5, 6, 0, 9, 5, -1, -1, -1, -1],
    [0, 11, 8, 0, 5, 11, 0, 1, 5, 5, 6, 11, -1, -1, -1, -1],
    [6, 11, 3, 6, 3, 5, 5, 3, 1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 9, 5, 11, 9, 11, 8, 11, 5, 6, -1, -1, -1, -1],
    [0, 11, 3, 0, 6, 11, 0, 9, 6, 5, 6, 9, 1, 2, 10, -1],
    [11, 8, 5, 11, 5, 6, 8, 0, 5, 10, 5, 2, 0, 2, 5, -1],
    [6, 11, 3, 6, 3, 5, 2, 10, 3, 10, 5, 3, -1, -1, -1, -1],
    [5, 8, 9, 5, 2, 8, 5, 6, 2, 3, 8, 2, -1, -1, -1, -1],
    [9, 5, 6, 9, 6, 0, 0, 6, 2, -1, -1, -1, -1, -1, -1, -1],
    [1, 5, 8, 1, 8, 0, 5, 6, 8, 3, 8, 2, 6, 2, 8, -1],
    [1, 5, 6, 2, 1, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 3, 6, 1, 6, 10, 3, 8, 6, 5, 6, 9, 8, 9, 6, -1],
    [10, 1, 0, 10, 0, 6, 9, 5, 0, 5, 6, 0, -1, -1, -1, -1],
    [0, 3, 8, 5, 6, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [10, 5, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [11, 5, 10, 7, 5, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [11, 5, 10, 11, 7, 5, 8, 3, 0, -1, -1, -1, -1, -1, -1, -1],
    [5, 11, 7, 5, 10, 11, 1, 9, 0, -1, -1, -1, -1, -1, -1, -1],
    [10, 7, 5, 10, 11, 7, 9, 8, 1, 8, 3, 1, -1, -1, -1, -1],
    [11, 1, 2, 11, 7, 1, 7, 5, 1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 1, 2, 7, 1, 7, 5, 7, 2, 11, -1, -1, -1, -1],
    [9, 7, 5, 9, 2, 7, 9, 0, 2, 2, 11, 7, -1, -1, -1, -1],
    [7, 5, 2, 7, 2, 11, 5, 9, 2, 3, 2, 8, 9, 8, 2, -1],
    [2, 5, 10, 2, 3, 5, 3, 7, 5, -1, -1, -1, -1, -1, -1, -1],
    [8, 2, 0, 8, 5, 2, 8, 7, 5, 10, 2, 5, -1, -1, -1, -1],
    [9, 0, 1, 5, 10, 3, 5, 3, 7, 3, 10, 2, -1, -1, -1, -1],
    [9, 8, 2, 9, 2, 1, 8, 7, 2, 10, 2, 5, 7, 5, 2, -1],
    [1, 3, 5, 3, 7, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 7, 0, 7, 1, 1, 7, 5, -1, -1, -1, -1, -1, -1, -1],
    [9, 0, 3, 9, 3, 5, 5, 3, 7, -1, -1, -1, -1, -1, -1, -1],
    [9, 8, 7, 5, 9, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [5, 8, 4, 5, 10, 8, 10, 11, 8, -1, -1, -1, -1, -1, -1, -1],
    [5, 0, 4, 5, 11, 0, 5, 10, 11, 11, 3, 0, -1, -1, -1, -1],
    [0, 1, 9, 8, 4, 10, 8, 10, 11, 10, 4, 5, -1, -1, -1, -1],
    [10, 11, 4, 10, 4, 5, 11, 3, 4, 9, 4, 1, 3, 1, 4, -1],
    [2, 5, 1, 2, 8, 5, 2, 11, 8, 4, 5, 8, -1, -1, -1, -1],
    [0, 4, 11, 0, 11, 3, 4, 5, 11, 2, 11, 1, 5, 1, 11, -1],
    [0, 2, 5, 0, 5, 9, 2, 11, 5, 4, 5, 8, 11, 8, 5, -1],
    [9, 4, 5, 2, 11, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 5, 10, 3, 5, 2, 3, 4, 5, 3, 8, 4, -1, -1, -1, -1],
    [5, 10, 2, 5, 2, 4, 4, 2, 0, -1, -1, -1, -1, -1, -1, -1],
    [3, 10, 2, 3, 5, 10, 3, 8, 5, 4, 5, 8, 0, 1, 9, -1],
    [5, 10, 2, 5, 2, 4, 1, 9, 2, 9, 4, 2, -1, -1, -1, -1],
    [8, 4, 5, 8, 5, 3, 3, 5, 1, -1, -1, -1, -1, -1, -1, -1],
    [0, 4, 5, 1, 0, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [8, 4, 5, 8, 5, 3, 9, 0, 5, 0, 3, 5, -1, -1, -1, -1],
    [9, 4, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 11, 7, 4, 9, 11, 9, 10, 11, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 4, 9, 7, 9, 11, 7, 9, 10, 11, -1, -1, -1, -1],
    [1, 10, 11, 1, 11, 4, 1, 4, 0, 7, 4, 11, -1, -1, -1, -1],
    [3, 1, 4, 3, 4, 8, 1, 10, 4, 7, 4, 11, 10, 11, 4, -1],
    [4, 11, 7, 9, 11, 4, 9, 2, 11, 9, 1, 2, -1, -1, -1, -1],
    [9, 7, 4, 9, 11, 7, 9, 1, 11, 2, 11, 1, 0, 8, 3, -1],
    [11, 7, 4, 11, 4, 2, 2, 4, 0, -1, -1, -1, -1, -1, -1, -1],
    [11, 7, 4, 11, 4, 2, 8, 3, 4, 3, 2, 4, -1, -1, -1, -1],
    [2, 9, 10, 2, 7, 9, 2, 3, 7, 7, 4, 9, -1, -1, -1, -1],
    [9, 10, 7, 9, 7, 4, 10, 2, 7, 8, 7, 0, 2, 0, 7, -1],
    [3, 7, 10, 3, 10, 2, 7, 4, 10, 1, 10, 0, 4, 0, 10, -1],
    [1, 10, 2, 8, 7, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 9, 1, 4, 1, 7, 7, 1, 3, -1, -1, -1, -1, -1, -1, -1],
    [4, 9, 1, 4, 1, 7, 0, 8, 1, 8, 7, 1, -1, -1, -1, -1],
    [4, 0, 3, 7, 4, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 8, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 10, 8, 10, 11, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 9, 3, 9, 11, 11, 9, 10, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 10, 0, 10, 8, 8, 10, 11, -1, -1, -1, -1, -1, -1, -1],
    [3, 1, 10, 11, 3, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 11, 1, 11, 9, 9, 11, 8, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 9, 3, 9, 11, 1, 2, 9, 2, 11, 9, -1, -1, -1, -1],
    [0, 2, 11, 8, 0, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 2, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 3, 8, 2, 8, 10, 10, 8, 9, -1, -1, -1, -1, -1, -1, -1],
    [9, 10, 2, 0, 9, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 3, 8, 2, 8, 10, 0, 1, 8, 1, 10, 8, -1, -1, -1, -1],
    [1, 10, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 3, 8, 9, 1, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 9, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 3, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
];

// ============================================================================
// Marching cubes over a lattice box
// ============================================================================

/// Scalar field samples on a box of lattice points. Negative is solid.
///
/// A box from [`Self::from_fn`] stores every point, `x` fastest, then `y`,
/// then `z`. A terrain chunk's box stores only a band of layers per XZ
/// column, one column after another, and reads the heightfield term
/// everywhere else, where no brick reaches and that term is the field.
#[derive(Clone, Debug)]
pub struct LatticeField {
    dims: UVec3,
    values: Vec<f32>,
    /// `None` when every point is stored.
    columns: Option<Box<ColumnBands>>,
}

/// Which layers each XZ column of a banded [`LatticeField`] stores, and the
/// heightfield term standing in for the rest.
#[derive(Clone, Debug)]
struct ColumnBands {
    /// Per column, `x` fastest: first stored layer, stored layer count, and
    /// the index of its first sample in `values`.
    stored: Vec<(u32, u32, usize)>,
    /// Per column: the ground height the heightfield term subtracts.
    ground: Vec<f32>,
    /// Global lattice Y of box layer 0.
    layer_base: i32,
    /// Lattice cell size.
    cell: f32,
}

impl LatticeField {
    /// Sample `field` at every point of a box of `dims` points.
    pub fn from_fn(dims: UVec3, mut field: impl FnMut(UVec3) -> f32) -> Self {
        let mut values = Vec::with_capacity(dims.x as usize * dims.y as usize * dims.z as usize);
        for z in 0..dims.z {
            for y in 0..dims.y {
                for x in 0..dims.x {
                    values.push(field(UVec3::new(x, y, z)));
                }
            }
        }
        Self { dims, values, columns: None }
    }

    /// Points along each axis.
    pub fn dims(&self) -> UVec3 {
        self.dims
    }

    /// Index of `p` in the full box, stored or not.
    #[inline]
    fn dense_index(&self, p: UVec3) -> usize {
        p.x as usize + self.dims.x as usize * (p.y as usize + self.dims.y as usize * p.z as usize)
    }

    /// Index of `p`'s sample in `values`, `None` for a layer its banded
    /// column does not store.
    #[inline]
    fn storage_index(&self, p: UVec3) -> Option<usize> {
        match &self.columns {
            None => Some(self.dense_index(p)),
            Some(bands) => {
                let (first, count, offset) = bands.stored[p.x as usize + self.dims.x as usize * p.z as usize];
                let layer = p.y.checked_sub(first)?;
                (layer < count).then(|| offset + layer as usize)
            }
        }
    }

    /// The sample at point `p`.
    #[inline]
    pub fn value(&self, p: UVec3) -> f32 {
        if let Some(index) = self.storage_index(p) {
            return self.values[index];
        }
        match &self.columns {
            // The same arithmetic the stored samples' heightfield term uses,
            // so a layer reads the same float stored or not.
            Some(bands) => {
                let ground = bands.ground[p.x as usize + self.dims.x as usize * p.z as usize];
                (bands.layer_base + p.y as i32) as f32 * bands.cell - ground
            }
            None => f32::NAN,
        }
    }

    /// Central-difference gradient at `p`, in field units per lattice step,
    /// one-sided on the faces of the box. Points from solid toward air.
    pub fn gradient(&self, p: UVec3) -> Vec3 {
        let mut gradient = Vec3::ZERO;
        for axis in 0..3 {
            let (mut lo, mut hi) = (p, p);
            if p[axis] > 0 {
                lo[axis] -= 1;
            }
            if p[axis] + 1 < self.dims[axis] {
                hi[axis] += 1;
            }
            let span = hi[axis] - lo[axis];
            if span > 0 {
                gradient[axis] = (self.value(hi) - self.value(lo)) / span as f32;
            }
        }
        gradient
    }

    /// Unit surface normal at `vertex`: the gradient at the two ends of its
    /// edge interpolated to it, straight up where that vanishes.
    pub fn edge_normal(&self, vertex: &EdgeVertex) -> Vec3 {
        let a = self.gradient(vertex.point);
        let b = self.gradient(vertex.end());
        let normal = (a + (b - a) * vertex.t).normalize_or_zero();
        if normal == Vec3::ZERO {
            Vec3::Y
        } else {
            normal
        }
    }
}

/// A surface vertex: where the field's linear interpolant crosses zero on
/// the lattice edge from `point` one step along `axis` (0 = X, 1 = Y, 2 = Z),
/// at fraction `t` of the way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeVertex {
    /// Lower end of the edge, as a point of the field's box.
    pub point: UVec3,
    /// Axis the edge runs along.
    pub axis: u8,
    /// Fraction of the edge from `point` to the crossing.
    pub t: f32,
}

impl EdgeVertex {
    /// Upper end of the edge.
    pub fn end(&self) -> UVec3 {
        let mut end = self.point;
        end[self.axis as usize] += 1;
        end
    }

    /// Position in lattice steps from the box's first point.
    pub fn lattice_position(&self) -> Vec3 {
        let mut position = self.point.as_vec3();
        position[self.axis as usize] += self.t;
        position
    }
}

/// The triangles marching cubes made: one vertex per crossed lattice edge,
/// shared by every triangle that uses the edge, and index triples wound
/// counter-clockwise seen from air.
#[derive(Clone, Debug, Default)]
pub struct MarchedSurface {
    pub vertices: Vec<EdgeVertex>,
    pub indices: Vec<u32>,
}

/// Where the linear interpolant from `from` (the edge's lower end) to `to`
/// crosses zero, as a fraction of the edge. Always measured from the lower
/// end, so every cube and every chunk sharing an edge puts its vertex on
/// the same float.
#[inline]
fn crossing_fraction(from: f32, to: f32) -> f32 {
    let t = from / (from - to);
    if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

/// Classic marching cubes over the cubes whose lower corners run from
/// `cube_min` up to (not including) `cube_max`. A point is solid when its
/// value is negative; zero and NaN count as air.
pub fn march_cubes(field: &LatticeField, cube_min: UVec3, cube_max: UVec3) -> MarchedSurface {
    let dims = field.dims;
    let last = UVec3::new(dims.x.saturating_sub(1), dims.y.saturating_sub(1), dims.z.saturating_sub(1));
    let cube_max = cube_max.min(last);
    let columns = cube_max.x.saturating_sub(cube_min.x) as usize * cube_max.z.saturating_sub(cube_min.z) as usize;
    march_cube_bands(field, cube_min, cube_max, &vec![(cube_min.y, cube_max.y); columns])
}

/// [`march_cubes`] visiting, in each XZ column of cubes, only the layers of
/// its band: `bands[(z - cube_min.z) * (cube_max.x - cube_min.x) + (x -
/// cube_min.x)]` is the range `start..end` of lower-corner layers marched
/// there. The caller vouches that every cube outside the bands is solid or
/// air throughout, so skipping it drops no triangle.
pub fn march_cube_bands(field: &LatticeField, cube_min: UVec3, cube_max: UVec3, bands: &[(u32, u32)]) -> MarchedSurface {
    let dims = field.dims;
    let last = UVec3::new(dims.x.saturating_sub(1), dims.y.saturating_sub(1), dims.z.saturating_sub(1));
    let cube_max = cube_max.min(last);
    let mut surface = MarchedSurface::default();
    if cube_min.x >= cube_max.x || cube_min.y >= cube_max.y || cube_min.z >= cube_max.z {
        return surface;
    }
    let row = (cube_max.x - cube_min.x) as usize;

    // Vertex id per lattice edge, made on first use: `sample index * 3 +
    // axis` for a stored point, the box point and axis for any other.
    let mut stored_vertex = vec![u32::MAX; field.values.len() * 3];
    let mut other_vertex: HashMap<(usize, usize), u32> = HashMap::new();
    for z in cube_min.z..cube_max.z {
        for x in cube_min.x..cube_max.x {
            let Some(&(band_start, band_end)) = bands.get((z - cube_min.z) as usize * row + (x - cube_min.x) as usize)
            else {
                continue;
            };
            for y in band_start.max(cube_min.y)..band_end.min(cube_max.y) {
                let base = UVec3::new(x, y, z);
                let mut case = 0usize;
                for (i, corner) in CORNERS.iter().enumerate() {
                    if field.value(base + UVec3::from_array(*corner)) < 0.0 {
                        case |= 1 << i;
                    }
                }
                if case == 0 || case == 255 {
                    continue;
                }

                let crossed = EDGE_TABLE[case];
                let mut ids = [u32::MAX; 12];
                for (edge, (offset, axis)) in EDGE_LATTICE.iter().enumerate() {
                    if crossed & (1u16 << edge) == 0 {
                        continue;
                    }
                    let axis = *axis;
                    let point = base + UVec3::from_array(*offset);
                    let id = match field.storage_index(point) {
                        Some(index) => &mut stored_vertex[index * 3 + axis],
                        None => other_vertex.entry((field.dense_index(point), axis)).or_insert(u32::MAX),
                    };
                    if *id == u32::MAX {
                        let mut end = point;
                        end[axis] += 1;
                        let t = crossing_fraction(field.value(point), field.value(end));
                        *id = surface.vertices.len() as u32;
                        surface.vertices.push(EdgeVertex { point, axis: axis as u8, t });
                    }
                    ids[edge] = *id;
                }

                for triangle in TRI_TABLE[case].chunks_exact(3) {
                    if triangle[0] < 0 {
                        break;
                    }
                    surface.indices.extend_from_slice(&[
                        ids[triangle[0] as usize],
                        ids[triangle[1] as usize],
                        ids[triangle[2] as usize],
                    ]);
                }
            }
        }
    }
    surface
}

// ============================================================================
// Terrain chunks
// ============================================================================

/// The layers each lattice cell of a chunk marches, found from the ground
/// heights and the bricks' extents alone, before any field is sampled.
struct ChunkBands {
    /// Lattice cells per chunk side.
    cells: i32,
    /// Global lattice column of the chunk's first vertex column, on X and Z.
    x0: i32,
    z0: i32,
    /// Ground height of every column from `x0 - 1` to `x0 + cells + 1` (and
    /// the same on Z), `x` fastest: the chunk's vertex columns plus the ring
    /// around them its gradients read.
    ground: Vec<f32>,
    /// Per cell, `x` fastest: the global lattice layers `(lo, hi)` its corners
    /// span, marched as the cubes `lo..hi`.
    bands: Vec<(i32, i32)>,
}

impl ChunkBands {
    /// Cubes the bands hold altogether.
    fn cubes(&self) -> usize {
        self.bands.iter().map(|&(lo, hi)| (hi - lo) as usize).sum()
    }
}

/// The bands of `chunk_pos`'s lattice cells: the lowest to highest ground of
/// each cell's four corner columns and the lattice Y extent of every brick
/// owning a point of one of them, plus [`MARCH_MARGIN_CELLS`] on both ends.
/// Below a band no brick reaches and the ground is more than the margin above
/// every corner, so each cube there is solid throughout, and above it air.
///
/// `None` without a height raster (a terrain without one draws its chunks
/// from noise the field does not see: the field reads the band floor there,
/// so marching would put a flat plane among noise hills), for a bad config
/// or a non-finite height, and for a cell taller than [`MAX_MARCH_LAYERS`],
/// which `quiet` keeps from being logged.
fn chunk_bands(
    chunk_pos: IVec2,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
    quiet: bool,
) -> Option<ChunkBands> {
    if data.height_cache.is_empty() || !(config.chunk_size.is_finite() && config.chunk_size > 0.0) {
        return None;
    }
    let cells = config.resolution_for_lod(0).max(1) as i32;
    let cell = lattice_cell_size(config);
    let x0 = chunk_pos.x.checked_mul(cells)?;
    let z0 = chunk_pos.y.checked_mul(cells)?;

    // Ground height of every padded column, looked up once per column.
    let side = (cells + 3) as usize;
    let mut ground = Vec::with_capacity(side * side);
    for k in -1..=cells + 1 {
        for i in -1..=cells + 1 {
            let h = lattice_surface_height(config, data, x0 + i, z0 + k);
            if !h.is_finite() {
                return None;
            }
            ground.push(h);
        }
    }

    // Lattice layers the bricks over each vertex column own.
    let corners = (cells + 1) as usize;
    let mut brick_layers: Vec<Option<(i32, i32)>> = vec![None; corners * corners];
    for (coord, _) in volume.bricks_in_chunk_column(config, chunk_pos) {
        let origin = brick_lattice_origin(coord);
        let owned = (origin.y, origin.y + BRICK_EDGE - 1);
        let (i0, i1) = ((origin.x - x0).max(0), (origin.x + BRICK_EDGE - 1 - x0).min(cells));
        let (k0, k1) = ((origin.z - z0).max(0), (origin.z + BRICK_EDGE - 1 - z0).min(cells));
        for k in k0..=k1 {
            for i in i0..=i1 {
                let slot = &mut brick_layers[k as usize * corners + i as usize];
                *slot = Some(slot.map_or(owned, |(lo, hi)| (lo.min(owned.0), hi.max(owned.1))));
            }
        }
    }

    let mut bands = Vec::with_capacity((cells * cells) as usize);
    for k in 0..cells as usize {
        for i in 0..cells as usize {
            let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
            let mut bricks: Option<(i32, i32)> = None;
            for (di, dk) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let h = ground[(k + dk + 1) * side + i + di + 1];
                low = low.min(h);
                high = high.max(h);
                if let Some((lo, hi)) = brick_layers[(k + dk) * corners + i + di] {
                    bricks = Some(bricks.map_or((lo, hi), |(a, b)| (a.min(lo), b.max(hi))));
                }
            }
            let (mut lo, mut hi) = ((low / cell).floor() as i64, (high / cell).ceil() as i64);
            if let Some((brick_lo, brick_hi)) = bricks {
                lo = lo.min(brick_lo as i64);
                hi = hi.max(brick_hi as i64);
            }
            let (lo, hi) = (lo - MARCH_MARGIN_CELLS as i64, hi + MARCH_MARGIN_CELLS as i64);
            let layers = hi - lo + 1;
            if layers > MAX_MARCH_LAYERS || lo <= i32::MIN as i64 / 2 || hi >= i32::MAX as i64 / 2 {
                if !quiet {
                    tracing::warn!(
                        chunk = ?chunk_pos,
                        layers,
                        "terrain chunk has a cell spanning more lattice layers than one march may visit; drawing its heightfield instead"
                    );
                }
                return None;
            }
            bands.push((lo as i32, hi as i32));
        }
    }
    Some(ChunkBands { cells, x0, z0, ground, bands })
}

/// The terrain field over one chunk's LOD-0 lattice, stored in the bands of
/// its cells (see [`chunk_bands`]).
struct ChunkLattice {
    /// Lattice cells per chunk side.
    cells: u32,
    /// Global lattice Y of the lowest marched layer.
    layer0: i32,
    /// The field over the marched box padded by one point on every side, so
    /// every marched point has the neighbours its gradient reads. Box point
    /// `(1, 1, 1)` is the chunk's first column at `layer0`. Each column
    /// stores the layers the cells around it march and read, one more above
    /// and below for the gradient.
    field: LatticeField,
    /// Per sample of `field`: whether its lattice point carries an edit.
    edited: Vec<bool>,
    /// Per cell, `x` fastest: the box layers `start..end` of the cubes it
    /// marches.
    bands: Vec<(u32, u32)>,
}

impl ChunkLattice {
    fn sample(chunk_pos: IVec2, config: &TerrainConfig, data: &TerrainData, volume: &TerrainVolume) -> Option<Self> {
        let ChunkBands { cells, x0, z0, ground, bands } = chunk_bands(chunk_pos, config, data, volume, false)?;
        let cell = lattice_cell_size(config);
        let layer0 = bands.iter().map(|&(lo, _)| lo).min()?;
        let last = bands.iter().map(|&(_, hi)| hi).max()?;
        let layers = last - layer0 + 1;
        let side = (cells + 3) as usize;
        let dims = UVec3::new(side as u32, (layers + 2) as u32, side as u32);
        let cube_bands: Vec<(u32, u32)> =
            bands.iter().map(|&(lo, hi)| ((lo - layer0 + 1) as u32, (hi - layer0 + 1) as u32)).collect();

        // A cell's corners are box columns `i + 1 ..= i + 2`, and their
        // gradients read one column further each way, so the cell reaches
        // columns `i ..= i + 3` at its layers and one beyond them.
        let mut reach: Vec<Option<(u32, u32)>> = vec![None; side * side];
        for k in 0..cells as usize {
            for i in 0..cells as usize {
                let (start, end) = cube_bands[k * cells as usize + i];
                let reads = (start - 1, end + 1);
                for b in k..=k + 3 {
                    for a in i..=i + 3 {
                        let slot = &mut reach[b * side + a];
                        *slot = Some(slot.map_or(reads, |(lo, hi)| (lo.min(reads.0), hi.max(reads.1))));
                    }
                }
            }
        }

        let total: usize = reach.iter().map(|r| r.map_or(0, |(lo, hi)| (hi - lo + 1) as usize)).sum();
        let mut values = Vec::with_capacity(total);
        let mut edited = Vec::with_capacity(total);
        let mut stored = Vec::with_capacity(side * side);
        // Points up a column share a brick for 16 layers at a time.
        let mut cached: Option<(IVec3, Option<&VolumeBrick>)> = None;
        for b in 0..side {
            for a in 0..side {
                let column = b * side + a;
                let Some((lo, hi)) = reach[column] else {
                    stored.push((0, 0, values.len()));
                    continue;
                };
                stored.push((lo, hi - lo + 1, values.len()));
                let (nx, nz) = (x0 + a as i32 - 1, z0 + b as i32 - 1);
                for layer in lo..=hi {
                    let y = layer0 - 1 + layer as i32;
                    let heightfield = y as f32 * cell - ground[column];
                    let (coord, index) = lattice_to_brick(IVec3::new(nx, y, nz));
                    let brick = match cached {
                        Some((cached_coord, brick)) if cached_coord == coord => brick,
                        _ => {
                            let brick = volume.brick(coord);
                            cached = Some((coord, brick));
                            brick
                        }
                    };
                    let stored_cell = brick.map_or(VolumeCell::NONE, |brick| brick.cell(index));
                    values.push(lattice_field_sample(heightfield, stored_cell, cell).value);
                    edited.push(stored_cell.is_edited());
                }
            }
        }
        let columns = ColumnBands { stored, ground, layer_base: layer0 - 1, cell };
        Some(Self {
            cells: cells as u32,
            layer0,
            field: LatticeField { dims, values, columns: Some(Box::new(columns)) },
            edited,
            bands: cube_bands,
        })
    }

    fn march(&self) -> MarchedSurface {
        let dims = self.field.dims();
        march_cube_bands(&self.field, UVec3::ONE, dims - UVec3::splat(2), &self.bands)
    }

    /// Whether no lattice point of box column `(x, z)` from `depth_layers`
    /// below layer `top` up to it carries an edit. A layer the column does
    /// not store has none: every brick owning a point of a vertex column
    /// lies inside the band of each cell that has the column as a corner.
    fn column_unedited(&self, x: u32, z: u32, top: u32, depth_layers: u32) -> bool {
        let top = top.min(self.field.dims().y - 1);
        (top.saturating_sub(depth_layers)..=top).all(|y| {
            self.field.storage_index(UVec3::new(x, y, z)).map_or(true, |index| !self.edited[index])
        })
    }

    /// Position of `vertex` local to the chunk entity. Columns go through
    /// `i / cells * size`, the heightfield mesh's `u * size`, and layers
    /// through `n * cell`, the field's own Y, so a vertex on a lattice line
    /// the heightfield mesh also draws lands on the same floats.
    fn local_position(&self, config: &TerrainConfig, vertex: &EdgeVertex) -> Vec3 {
        let cells = self.cells as f32;
        let size = config.chunk_size;
        let cell = lattice_cell_size(config);
        let at = |p: UVec3| {
            Vec3::new(
                (p.x as f32 - 1.0) / cells * size,
                (self.layer0 + p.y as i32 - 1) as f32 * cell,
                (p.z as f32 - 1.0) / cells * size,
            )
        };
        let start = at(vertex.point);
        start + (at(vertex.end()) - start) * vertex.t
    }

    /// The chunk sides, in the order -X, +X, -Z, +Z, whose plane `vertex`
    /// lies on.
    fn sides(&self, vertex: &EdgeVertex) -> [bool; 4] {
        let last = self.cells + 1;
        let (p, axis) = (vertex.point, vertex.axis);
        [
            axis != 0 && p.x == 1,
            axis != 0 && p.x == last,
            axis != 2 && p.z == 1,
            axis != 2 && p.z == last,
        ]
    }
}

/// Render geometry of a volumetric chunk, positions local to the chunk
/// entity like the heightfield mesh's.
#[derive(Clone, Debug, Default)]
pub struct VolumeChunkGeometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    /// The `ATTRIBUTE_UV_1` the textured terrain material reads the brick
    /// material from: see [`brick_material_uv`].
    pub brick_uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Vertices before this are the surface's; the rest hang skirts.
    pub surface_vertex_count: usize,
    /// Indices before this belong to the surface; the rest are skirts.
    pub surface_index_count: usize,
}

impl VolumeChunkGeometry {
    /// The render mesh: the attributes the heightfield mesh carries, plus the
    /// brick material in `ATTRIBUTE_UV_1`.
    pub fn into_mesh(self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, self.brick_uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }

    /// The surface without its skirts, as the trimesh collider takes it.
    pub fn surface_triangles(&self) -> (Vec<Vec3>, Vec<[u32; 3]>) {
        let positions = self.positions[..self.surface_vertex_count].iter().map(|&p| Vec3::from_array(p)).collect();
        let triangles =
            self.indices[..self.surface_index_count].chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
        (positions, triangles)
    }

    /// Copy of vertex `vertex` moved `depth` along Y, for a skirt.
    fn push_lowered(&mut self, vertex: u32, depth: f32) -> u32 {
        let i = vertex as usize;
        let [x, y, z] = self.positions[i];
        let (normal, uv, color, brick) = (self.normals[i], self.uvs[i], self.colors[i], self.brick_uvs[i]);
        self.positions.push([x, y + depth, z]);
        self.normals.push(normal);
        self.uvs.push(uv);
        self.colors.push(color);
        self.brick_uvs.push(brick);
        (self.positions.len() - 1) as u32
    }

    /// Gives every surface triangle whose brick vertices name more than one
    /// slot its own copies of its three vertices, all naming one slot. The
    /// rasterizer interpolates UV_1, and between two different `[slot + 1, 1]`
    /// values `x / y` sweeps through every slot id in between, which the
    /// shader would draw as stripes of materials the volume does not hold.
    fn split_mixed_brick_triangles(&mut self, indices: &mut [u32]) {
        for triangle in indices.chunks_exact_mut(3) {
            let bricks = [
                self.brick_uvs[triangle[0] as usize],
                self.brick_uvs[triangle[1] as usize],
                self.brick_uvs[triangle[2] as usize],
            ];
            // Built by brick_material_uv, so y is exactly 1 or 0 here.
            let codes = bricks.map(|b| (b[1] > 0.0).then_some(b[0]));
            let mut named = codes.iter().flatten().copied();
            let Some(first) = named.next() else { continue };
            if named.all(|c| c == first) {
                continue;
            }
            // Majority wins (two of three), otherwise the first vertex's.
            let code = codes
                .iter()
                .flatten()
                .copied()
                .find(|&c| codes.iter().flatten().filter(|&&d| d == c).count() >= 2)
                .unwrap_or(first);
            for k in 0..3 {
                let i = triangle[k] as usize;
                let (position, normal, uv, color) = (self.positions[i], self.normals[i], self.uvs[i], self.colors[i]);
                // Brick vertices take the triangle's slot at full weight, so a
                // boundary between two brick materials is a step along triangle
                // edges rather than a fade into the material map; heightfield
                // vertices keep [0, 0], so x = (slot + 1) * y holds and x / y
                // stays constant across the triangle.
                let brick = if bricks[k][1] > 0.0 { [code, 1.0] } else { [0.0, 0.0] };
                self.positions.push(position);
                self.normals.push(normal);
                self.uvs.push(uv);
                self.colors.push(color);
                self.brick_uvs.push(brick);
                triangle[k] = (self.positions.len() - 1) as u32;
            }
        }
    }
}

/// The `ATTRIBUTE_UV_1` of a volumetric-chunk vertex: `[slot + 1, 1]` on a
/// surface an edit made in `material` (its slot is its discriminant), and
/// `[0, 0]` where the heightfield term wins, which tells the textured
/// terrain material to read the material map there instead.
///
/// The second component is what makes the slot recoverable between
/// vertices: the rasterizer interpolates both, and across a triangle joining
/// brick and heightfield vertices of one brick material `x / y` stays
/// exactly `slot + 1` while `y` falls from 1 to 0, so the shader blends that
/// slot out by `y` rather than reading a slot id that interpolation has
/// turned into some other slot's.
///
/// Across a triangle joining brick vertices of two different materials,
/// `x / y` would sweep through the slot ids in between.
/// [`build_volume_chunk_geometry`] therefore gives each such triangle its own
/// vertices, all naming one slot (see
/// `VolumeChunkGeometry::split_mixed_brick_triangles`), so every triangle's
/// UV_1 names at most one slot.
pub fn brick_material_uv(material: Option<TerrainMaterial>) -> [f32; 2] {
    match material {
        Some(material) => [f32::from(material.to_u8()) + 1.0, 1.0],
        None => [0.0, 0.0],
    }
}

/// Surface edges on the chunk's sides that a single surface triangle uses
/// (the chunk's border, directed as that triangle winds them), kept where
/// both ends are clean: no lattice point of either end's column carries an
/// edit from the edge's upper end down `depth_layers` layers, so a skirt
/// hung from the edge passes through unedited ground only. Sorted, so the
/// mesh built from them is reproducible.
fn open_side_edges(lattice: &ChunkLattice, surface: &MarchedSurface, depth_layers: u32) -> Vec<(u32, u32)> {
    let on_side: Vec<[bool; 4]> = surface.vertices.iter().map(|vertex| lattice.sides(vertex)).collect();
    let clean: Vec<bool> = surface
        .vertices
        .iter()
        .map(|vertex| {
            // The upper end's layer, so a vertical edge's whole span counts.
            let top = vertex.end().y;
            [vertex.point, vertex.end()].iter().all(|p| lattice.column_unedited(p.x, p.z, top, depth_layers))
        })
        .collect();
    let mut uses: HashMap<(u32, u32), (u32, u32, u32)> = HashMap::new();
    for triangle in surface.indices.chunks_exact(3) {
        for (from, to) in [(triangle[0], triangle[1]), (triangle[1], triangle[2]), (triangle[2], triangle[0])] {
            let (a, b) = (on_side[from as usize], on_side[to as usize]);
            if !(0..4).any(|side| a[side] && b[side]) {
                continue;
            }
            uses.entry((from.min(to), from.max(to))).or_insert((0, from, to)).0 += 1;
        }
    }
    let mut open: Vec<(u32, u32)> = uses
        .into_values()
        .filter(|&(count, from, to)| count == 1 && clean[from as usize] && clean[to as usize])
        .map(|(_, from, to)| (from, to))
        .collect();
    open.sort_unstable();
    open
}

/// March `chunk_pos` over the terrain field and build its render geometry:
/// the surface, coloured like the heightfield where the heightfield term
/// wins and by the edit's material elsewhere (which [`brick_material_uv`]
/// also records for the textured material), plus skirts hanging from every
/// border segment whose lattice edges, and the column below them to skirt
/// depth, carry no edit (see the module docs). `None` when the lattice
/// cannot be sampled (a bad config, no height raster, a non-finite height, a
/// cell too tall).
pub fn build_volume_chunk_geometry(
    chunk_pos: IVec2,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
) -> Option<VolumeChunkGeometry> {
    let lattice = ChunkLattice::sample(chunk_pos, config, data, volume)?;
    let surface = lattice.march();
    if surface.indices.is_empty() {
        return None;
    }
    let origin = chunk_world_position(chunk_pos, config);
    let size = config.chunk_size;
    let cell = lattice_cell_size(config);
    let shading = HeightfieldShading::new(config, data);

    let count = surface.vertices.len();
    let mut geometry = VolumeChunkGeometry {
        positions: Vec::with_capacity(count),
        normals: Vec::with_capacity(count),
        uvs: Vec::with_capacity(count),
        colors: Vec::with_capacity(count),
        brick_uvs: Vec::with_capacity(count),
        ..default()
    };
    for vertex in &surface.vertices {
        let local = lattice.local_position(config, vertex);
        let normal = lattice.field.edge_normal(vertex);
        let world = origin + local;
        let (color, brick) = if sample_field_parts(config, data, volume, world).term() == FieldTerm::Heightfield {
            // The same inputs the heightfield mesher feeds the shared
            // colouring, one LOD-0 step apart.
            let (world_u, world_v) = world_to_uv(config, world.x, world.z);
            let neighbours = [
                height_at_world(config, data, world.x - cell, world.z),
                height_at_world(config, data, world.x + cell, world.z),
                height_at_world(config, data, world.x, world.z - cell),
                height_at_world(config, data, world.x, world.z + cell),
            ];
            let color = shading.color(world_u, world_v, world.x, world.z, world.y, neighbours, normal);
            (color, brick_material_uv(None))
        } else {
            let material = material_at(config, volume, world).unwrap_or(TerrainMaterial::Rock);
            let color = material_vertex_color(&data.slot_palette, material, world.x, world.z, normal, config.seed);
            (color, brick_material_uv(Some(material)))
        };
        geometry.positions.push(local.to_array());
        geometry.normals.push(normal.to_array());
        geometry.uvs.push([local.x / size, local.z / size]);
        geometry.colors.push(color);
        geometry.brick_uvs.push(brick);
    }

    let depth = skirt_depth(size);
    // Layers from an edge's upper end down past its skirt's foot: the skirt's
    // depth in cells, plus one because the edge's vertex can sit up to a
    // layer below that upper end.
    let depth_layers = (depth.abs() / cell).ceil() as u32 + 1;
    let open_edges = open_side_edges(&lattice, &surface, depth_layers);
    // Split before the surface counts are taken, so the copies count as
    // surface and `surface_triangles` (the spawn path's collider) covers
    // them. Skirts need nothing: open edges join clean vertices only, which
    // carry [0, 0], so no triangle with a skirted edge is mixed.
    let mut indices = surface.indices;
    geometry.split_mixed_brick_triangles(&mut indices);
    geometry.surface_vertex_count = geometry.positions.len();
    geometry.surface_index_count = indices.len();
    geometry.indices = indices;
    let mut lowered: HashMap<u32, u32> = HashMap::new();
    for (from, to) in open_edges {
        let low_from = *lowered.entry(from).or_insert_with(|| geometry.push_lowered(from, depth));
        let low_to = *lowered.entry(to).or_insert_with(|| geometry.push_lowered(to, depth));
        // Crosses the shared edge `to -> from`, against its triangle, so the
        // skirt carries the surface's orientation over the edge and down:
        // it faces out of the chunk, like a heightfield skirt.
        geometry.indices.extend_from_slice(&[from, low_from, to, to, low_from, low_to]);
    }
    Some(geometry)
}

/// The LOD-0 marching-cubes surface of `chunk_pos` as triangles, positions
/// local to the chunk entity: what its trimesh collider is built from. No
/// skirts and no colours. `None` when the lattice cannot be sampled.
pub fn volume_chunk_triangles(
    chunk_pos: IVec2,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
) -> Option<(Vec<Vec3>, Vec<[u32; 3]>)> {
    let lattice = ChunkLattice::sample(chunk_pos, config, data, volume)?;
    let surface = lattice.march();
    if surface.indices.is_empty() {
        return None;
    }
    let positions = surface.vertices.iter().map(|vertex| lattice.local_position(config, vertex)).collect();
    let triangles = surface.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
    Some((positions, triangles))
}

/// Whether `chunk_pos` renders at `lod` by marching cubes: at LOD 0 when
/// bricks reach its columns. Coarser LODs keep the heightfield mesh.
pub fn chunk_uses_marching_cubes(chunk_pos: IVec2, lod: u32, config: &TerrainConfig, volume: &TerrainVolume) -> bool {
    lod == 0 && !volume.is_empty() && chunk_has_volume(chunk_pos, config, volume)
}

/// The render mesh of `chunk_pos` at `lod`: marching cubes over the terrain
/// field when [`chunk_uses_marching_cubes`], else the heightfield mesh.
///
/// Every system that meshes a chunk (the initial fill, streaming, LOD
/// changes, the dirty-chunk remesh) comes through here, or through
/// [`generate_chunk_render_mesh_and_surface`] when it spawns the chunk. A
/// volumetric chunk whose lattice cannot be marched falls back to its
/// heightfield mesh.
pub fn generate_chunk_render_mesh(
    chunk_pos: IVec2,
    lod: u32,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
    meshes: &mut Assets<Mesh>,
) -> Handle<Mesh> {
    if chunk_uses_marching_cubes(chunk_pos, lod, config, volume) {
        if let Some(geometry) = build_volume_chunk_geometry(chunk_pos, config, data, volume) {
            return meshes.add(geometry.into_mesh());
        }
    }
    generate_chunk_mesh(chunk_pos, lod, config, data, meshes)
}

/// [`generate_chunk_render_mesh`] for a chunk being spawned, also returning
/// the marched surface when it has one, so its collider reuses the march
/// instead of sampling the lattice a second time this frame.
pub fn generate_chunk_render_mesh_and_surface(
    chunk_pos: IVec2,
    lod: u32,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
    meshes: &mut Assets<Mesh>,
) -> (Handle<Mesh>, Option<(Vec<Vec3>, Vec<[u32; 3]>)>) {
    if chunk_uses_marching_cubes(chunk_pos, lod, config, volume) {
        if let Some(geometry) = build_volume_chunk_geometry(chunk_pos, config, data, volume) {
            let surface = geometry.surface_triangles();
            return (meshes.add(geometry.into_mesh()), Some(surface));
        }
    }
    (generate_chunk_mesh(chunk_pos, lod, config, data, meshes), None)
}

/// What marching `chunk_pos` costs, in heightfield builds: the cubes its
/// cell bands hold, per cell, and at least [`VOLUMETRIC_CHUNK_COST`]. Found
/// from the ground heights and brick extents without sampling the field. A
/// chunk whose bands cannot be found draws its heightfield, which costs 1.
fn volume_chunk_cost(chunk_pos: IVec2, config: &TerrainConfig, data: &TerrainData, volume: &TerrainVolume) -> usize {
    match chunk_bands(chunk_pos, config, data, volume, true) {
        Some(bands) => {
            let cells = (bands.cells as usize * bands.cells as usize).max(1);
            VOLUMETRIC_CHUNK_COST.max(bands.cubes().div_ceil(cells))
        }
        None => 1,
    }
}

/// What rebuilding `chunk_pos`'s render mesh at `lod` costs, in heightfield
/// remeshes (see [`VOLUMETRIC_CHUNK_COST`]).
pub fn chunk_mesh_cost(
    chunk_pos: IVec2,
    lod: u32,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
) -> usize {
    if chunk_uses_marching_cubes(chunk_pos, lod, config, volume) {
        volume_chunk_cost(chunk_pos, config, data, volume)
    } else {
        1
    }
}

/// What rebuilding `chunk_pos`'s collider costs, in heightfield collider
/// builds. A volumetric chunk collides on its marching-cubes surface at
/// every LOD.
pub fn chunk_collider_cost(chunk_pos: IVec2, config: &TerrainConfig, data: &TerrainData, volume: &TerrainVolume) -> usize {
    if !volume.is_empty() && chunk_has_volume(chunk_pos, config, volume) {
        volume_chunk_cost(chunk_pos, config, data, volume)
    } else {
        1
    }
}

/// What spawning `chunk_pos` at `lod` costs: its mesh and its collider,
/// which share one march at LOD 0.
pub fn chunk_spawn_cost(
    chunk_pos: IVec2,
    lod: u32,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
) -> usize {
    chunk_mesh_cost(chunk_pos, lod, config, data, volume).max(chunk_collider_cost(chunk_pos, config, data, volume))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use crate::terrain::volume::{apply_box, field_gradient, sample_field_lattice, CsgOp};

    // ------------------------------------------------------------------
    // The tables
    // ------------------------------------------------------------------

    fn corner_solid(case: usize, corner: usize) -> bool {
        (case >> corner) & 1 == 1
    }

    fn case_triangles(case: usize) -> Vec<[usize; 3]> {
        TRI_TABLE[case]
            .chunks_exact(3)
            .take_while(|t| t[0] >= 0)
            .map(|t| [t[0] as usize, t[1] as usize, t[2] as usize])
            .collect()
    }

    fn corner_at(p: [u32; 3]) -> usize {
        CORNERS.iter().position(|c| *c == p).expect("a cube corner")
    }

    fn edge_between(a: usize, b: usize) -> usize {
        EDGES
            .iter()
            .position(|e| (e[0] == a && e[1] == b) || (e[0] == b && e[1] == a))
            .expect("a cube edge")
    }

    /// Two cube edges lie on a common face when their corners agree on one
    /// coordinate.
    fn share_face(e1: usize, e2: usize) -> bool {
        let corners = [EDGES[e1][0], EDGES[e1][1], EDGES[e2][0], EDGES[e2][1]];
        (0..3).any(|axis| corners.iter().all(|&c| CORNERS[c][axis] == CORNERS[corners[0]][axis]))
    }

    /// Edges of `case`'s triangles that only one of them uses, directed as
    /// that triangle winds them.
    fn open_edges(case: usize) -> Vec<(usize, usize)> {
        let key = |a: usize, b: usize| (a.min(b), a.max(b));
        let mut count: HashMap<(usize, usize), usize> = HashMap::new();
        let mut directed = Vec::new();
        for [a, b, c] in case_triangles(case) {
            for (from, to) in [(a, b), (b, c), (c, a)] {
                *count.entry(key(from, to)).or_default() += 1;
                directed.push((from, to));
            }
        }
        directed.into_iter().filter(|&(from, to)| count[&key(from, to)] == 1).collect()
    }

    #[test]
    fn edge_lattice_matches_the_corner_numbering() {
        for (edge, [a, b]) in EDGES.iter().enumerate() {
            let (ca, cb) = (UVec3::from_array(CORNERS[*a]), UVec3::from_array(CORNERS[*b]));
            let (offset, axis) = EDGE_LATTICE[edge];
            assert_eq!(UVec3::from_array(offset), ca.min(cb), "edge {edge} lower corner");
            let mut step = UVec3::ZERO;
            step[axis] = 1;
            assert_eq!(ca.max(cb) - ca.min(cb), step, "edge {edge} axis");
        }
    }

    #[test]
    fn the_edge_table_is_the_published_one() {
        for (case, bits) in [
            (0usize, 0x000u16),
            (1, 0x109),
            (2, 0x203),
            (3, 0x30a),
            (16, 0x190),
            (17, 0x099),
            (85, 0x0ff),
            (127, 0x8c0),
            (128, 0x8c0),
            (254, 0x109),
            (255, 0x000),
        ] {
            assert_eq!(EDGE_TABLE[case], bits, "case {case}");
        }
        for case in 0..256 {
            assert_eq!(EDGE_TABLE[case], EDGE_TABLE[255 - case], "case {case} and its complement");
        }
    }

    #[test]
    fn every_case_triangulates_exactly_its_crossed_edges() {
        for case in 0..256 {
            let row = &TRI_TABLE[case];
            let used = row.iter().position(|&e| e < 0).unwrap_or(row.len());
            assert_eq!(used % 3, 0, "case {case} ends mid-triangle");
            assert!(row[used..].iter().all(|&e| e == -1), "case {case} has entries past its end");
            let mut edges = 0u16;
            for &e in &row[..used] {
                assert!((0..12).contains(&e), "case {case} names edge {e}");
                edges |= 1u16 << e;
            }
            assert_eq!(edges, EDGE_TABLE[case], "case {case} uses other edges than it crosses");
            for [a, b, c] in case_triangles(case) {
                assert!(a != b && b != c && c != a, "case {case} has a degenerate triangle");
            }
        }
    }

    #[test]
    fn every_case_is_an_oriented_patch_open_only_across_cube_faces() {
        for case in 0..256 {
            let mut directed: HashMap<(usize, usize), usize> = HashMap::new();
            for [a, b, c] in case_triangles(case) {
                for (from, to) in [(a, b), (b, c), (c, a)] {
                    *directed.entry((from, to)).or_default() += 1;
                }
            }
            for (&(from, to), &uses) in &directed {
                assert_eq!(uses, 1, "case {case} runs {from} -> {to} twice");
                if !directed.contains_key(&(to, from)) {
                    assert!(share_face(from, to), "case {case} leaves {from} - {to} open inside the cube");
                }
            }
        }
    }

    /// Two cubes that meet at a face see the same corner signs on it, so
    /// their surfaces must cross it along the same segments, run in opposite
    /// directions. That is the whole of watertightness from cube to cube.
    #[test]
    fn neighbouring_cubes_agree_on_every_shared_face() {
        let open: Vec<Vec<(usize, usize)>> = (0..256).map(open_edges).collect();
        for axis in 0..3 {
            let high: Vec<usize> = (0..8).filter(|&c| CORNERS[c][axis] == 1).collect();
            let to_low = |c: usize| {
                let mut p = CORNERS[c];
                p[axis] = 0;
                corner_at(p)
            };
            let on_face = |edge: usize, side: u32| EDGES[edge].iter().all(|&c| CORNERS[c][axis] == side);
            let edge_to_low = |edge: usize| edge_between(to_low(EDGES[edge][0]), to_low(EDGES[edge][1]));
            for a in 0..256usize {
                let mut across_a: Vec<(usize, usize)> = open[a]
                    .iter()
                    .filter(|&&(from, to)| on_face(from, 1) && on_face(to, 1))
                    .map(|&(from, to)| (edge_to_low(from), edge_to_low(to)))
                    .collect();
                across_a.sort_unstable();
                for b in 0..256usize {
                    if high.iter().any(|&c| corner_solid(a, c) != corner_solid(b, to_low(c))) {
                        continue;
                    }
                    let mut across_b: Vec<(usize, usize)> = open[b]
                        .iter()
                        .filter(|&&(from, to)| on_face(from, 0) && on_face(to, 0))
                        .map(|&(from, to)| (to, from))
                        .collect();
                    across_b.sort_unstable();
                    assert_eq!(across_a, across_b, "cases {a} and {b} disagree across the face normal to axis {axis}");
                }
            }
        }
    }

    /// With every pair of neighbouring cases agreeing, one case fixes the
    /// orientation of all of them.
    #[test]
    fn a_lone_solid_corner_is_capped_by_a_triangle_facing_away_from_it() {
        let midpoint = |edge: usize| {
            (UVec3::from_array(CORNERS[EDGES[edge][0]]) + UVec3::from_array(CORNERS[EDGES[edge][1]])).as_vec3() * 0.5
        };
        let [a, b, c] = case_triangles(1)[0];
        let normal = (midpoint(b) - midpoint(a)).cross(midpoint(c) - midpoint(a));
        assert!(normal.dot(Vec3::ONE) > 0.0, "the cap over solid corner 0 faces +X+Y+Z, toward air");
    }

    // ------------------------------------------------------------------
    // Closed surfaces
    // ------------------------------------------------------------------

    /// Asserts every undirected edge is used by exactly two triangles, in
    /// opposite directions, and returns `V - E + F`.
    fn euler_characteristic_of_closed(indices: &[u32]) -> i64 {
        let mut directed = HashSet::new();
        for t in indices.chunks_exact(3) {
            assert!(t[0] != t[1] && t[1] != t[2] && t[2] != t[0], "degenerate triangle {t:?}");
            for (from, to) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                assert!(directed.insert((from, to)), "edge {from} -> {to} used twice in one direction");
            }
        }
        for &(from, to) in &directed {
            assert!(directed.contains(&(to, from)), "edge {from} - {to} is open");
        }
        let vertices: HashSet<u32> = indices.iter().copied().collect();
        vertices.len() as i64 - (directed.len() / 2) as i64 + (indices.len() / 3) as i64
    }

    fn sphere() -> (LatticeField, impl Fn(Vec3) -> Vec3) {
        let centre = Vec3::new(12.37, 11.61, 12.13);
        let field = LatticeField::from_fn(UVec3::splat(26), move |p| p.as_vec3().distance(centre) - 7.43);
        (field, move |p: Vec3| (p - centre).normalize())
    }

    fn torus() -> (LatticeField, impl Fn(Vec3) -> Vec3) {
        let centre = Vec3::new(13.21, 12.47, 12.83);
        let (ring, tube) = (7.3f32, 2.71f32);
        let field = LatticeField::from_fn(UVec3::splat(28), move |p| {
            let q = p.as_vec3() - centre;
            Vec2::new(Vec2::new(q.x, q.z).length() - ring, q.y).length() - tube
        });
        let gradient = move |p: Vec3| {
            let q = p - centre;
            let around = Vec2::new(q.x, q.z);
            let on_ring = around.normalize() * ring;
            (q - Vec3::new(on_ring.x, 0.0, on_ring.y)).normalize()
        };
        (field, gradient)
    }

    fn march_all(field: &LatticeField) -> MarchedSurface {
        march_cubes(field, UVec3::ZERO, field.dims())
    }

    #[test]
    fn a_sphere_marches_to_a_closed_surface_of_euler_characteristic_two() {
        let (field, _) = sphere();
        let surface = march_all(&field);
        assert!(surface.indices.len() / 3 > 500, "the sphere is resolved by many triangles");
        assert_eq!(euler_characteristic_of_closed(&surface.indices), 2);
    }

    #[test]
    fn a_torus_marches_to_a_closed_surface_of_euler_characteristic_zero() {
        let (field, _) = torus();
        let surface = march_all(&field);
        assert!(surface.indices.len() / 3 > 500, "the torus is resolved by many triangles");
        assert_eq!(euler_characteristic_of_closed(&surface.indices), 0);
    }

    /// Triangles face along the field gradient (toward air), and so do the
    /// interpolated vertex normals. Slivers are weighted by their area,
    /// since a near-degenerate triangle's plane says little about the
    /// surface.
    fn assert_normals_follow(field: &LatticeField, gradient: impl Fn(Vec3) -> Vec3) {
        let surface = march_all(field);
        let positions: Vec<Vec3> = surface.vertices.iter().map(EdgeVertex::lattice_position).collect();
        let (mut agreement, mut total) = (0.0f32, 0.0f32);
        for t in surface.indices.chunks_exact(3) {
            let [a, b, c] = [positions[t[0] as usize], positions[t[1] as usize], positions[t[2] as usize]];
            let cross = (b - a).cross(c - a);
            let area = cross.length() * 0.5;
            if area < 1e-6 {
                continue;
            }
            let dot = cross.normalize().dot(gradient((a + b + c) / 3.0));
            if area > 0.05 {
                assert!(dot > 0.0, "triangle {t:?} of area {area} faces into the solid ({dot})");
            }
            agreement += dot * area;
            total += area;
        }
        assert!(agreement / total > 0.97, "area-weighted agreement {}", agreement / total);
        for (vertex, position) in surface.vertices.iter().zip(&positions) {
            let dot = field.edge_normal(vertex).dot(gradient(*position));
            assert!(dot > 0.9, "vertex normal at {position} is off the gradient ({dot})");
        }
    }

    #[test]
    fn triangle_and_vertex_normals_follow_the_field_gradient() {
        let (field, gradient) = sphere();
        assert_normals_follow(&field, gradient);
        let (field, gradient) = torus();
        assert_normals_follow(&field, gradient);
    }

    // ------------------------------------------------------------------
    // Terrain chunks
    // ------------------------------------------------------------------

    /// 32 m chunks at 16 cells: a 2 m lattice.
    fn test_config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 16,
            chunks_x: 2,
            chunks_z: 2,
            lod_levels: 2,
            lod_distances: vec![64.0, 128.0],
            view_distance: 512.0,
            height_scale: 64.0,
            height_offset: 0.0,
            seed: 1,
        }
    }

    /// Gently rolling ground between about 11 and 21 m.
    fn rolling_data(config: &TerrainConfig) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        let w = data.cache_width as usize;
        for (i, h) in data.height_cache.iter_mut().enumerate() {
            let (x, z) = ((i % w) as f32, (i / w) as f32);
            *h = 0.25 + 0.05 * (x * 0.21).sin() * (z * 0.17 + 0.4).cos() + 0.03 * (z * 0.09 + 1.0).sin();
        }
        data
    }

    /// A shaft carved down through the ground of chunk (0, 0). Its bricks
    /// own the chunk's -X and -Z border columns, though its edits reach none
    /// of their points, but not its +X and +Z ones.
    fn shaft_volume(config: &TerrainConfig) -> TerrainVolume {
        let mut volume = TerrainVolume::new();
        let edit = apply_box(config, &mut volume, Vec3::new(13.0, 12.3, 13.0), Vec3::new(4.0, 20.1, 4.0), CsgOp::Carve, None);
        assert!(!edit.is_empty());
        volume
    }

    const SHAFT_CHUNK: IVec2 = IVec2::new(0, 0);
    const PLAIN_CHUNK: IVec2 = IVec2::new(1, 0);

    #[test]
    fn chunk_lattices_sample_exactly_the_lattice_field_and_close_top_and_bottom() {
        let config = test_config();
        let data = rolling_data(&config);
        let volume = shaft_volume(&config);
        let lattice = ChunkLattice::sample(SHAFT_CHUNK, &config, &data, &volume).expect("samples");
        let dims = lattice.field.dims();
        let cells = lattice.cells as i32;
        for z in 0..dims.z {
            for y in 0..dims.y {
                for x in 0..dims.x {
                    let n = IVec3::new(
                        SHAFT_CHUNK.x * cells + x as i32 - 1,
                        lattice.layer0 + y as i32 - 1,
                        SHAFT_CHUNK.y * cells + z as i32 - 1,
                    );
                    let expected = sample_field_lattice(&config, &data, &volume, n).value;
                    let got = lattice.field.value(UVec3::new(x, y, z));
                    assert_eq!(got.to_bits(), expected.to_bits(), "lattice point {n}");
                }
            }
        }
        for z in 1..dims.z - 1 {
            for x in 1..dims.x - 1 {
                assert!(lattice.field.value(UVec3::new(x, 1, z)) < 0.0, "the lowest layer is solid");
                assert!(lattice.field.value(UVec3::new(x, dims.y - 2, z)) > 0.0, "the highest layer is air");
            }
        }
    }

    #[test]
    fn an_unedited_chunk_marches_onto_its_heightfield() {
        let config = test_config();
        let data = rolling_data(&config);
        let volume = TerrainVolume::new();
        let chunk = IVec2::new(1, -1);
        let lattice = ChunkLattice::sample(chunk, &config, &data, &volume).expect("samples");
        let surface = lattice.march();
        let cells = lattice.cells as i32;
        let column = |p: UVec3| {
            lattice_surface_height(&config, &data, chunk.x * cells + p.x as i32 - 1, chunk.y * cells + p.z as i32 - 1)
        };
        for vertex in &surface.vertices {
            let local = lattice.local_position(&config, vertex);
            let (h0, h1) = (column(vertex.point), column(vertex.end()));
            let expected = h0 + (h1 - h0) * vertex.t;
            assert!((local.y - expected).abs() < 1e-3, "vertex {vertex:?} at {local} is off the ground {expected}");
        }
        // One sheet: every lattice column crosses the ground exactly once.
        let crossings = surface.vertices.iter().filter(|vertex| vertex.axis == 1).count();
        assert_eq!(crossings, (cells as usize + 1).pow(2));
    }

    /// The mandatory seam proof: where no edit is near the border, the
    /// volumetric chunk's border polyline is the heightfield chunk's.
    #[test]
    fn a_volumetric_border_without_nearby_edits_is_the_heightfield_border() {
        let config = test_config();
        let data = rolling_data(&config);
        let volume = shaft_volume(&config);
        assert!(chunk_has_volume(SHAFT_CHUNK, &config, &volume));
        assert!(!chunk_has_volume(PLAIN_CHUNK, &config, &volume));

        // The heightfield border: the plain chunk's x = 0 vertex column.
        let mut meshes = Assets::<Mesh>::default();
        let handle = generate_chunk_mesh(PLAIN_CHUNK, 0, &config, &data, &mut meshes);
        let mesh = meshes.get(&handle).expect("mesh was just added");
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|values| values.as_float3())
            .expect("terrain mesh positions are Float32x3");
        let r = config.resolution_for_lod(0) as usize;
        let plain_origin = chunk_world_position(PLAIN_CHUNK, &config);
        let border: Vec<Vec3> = (0..=r).map(|z| Vec3::from_array(positions[z * (r + 1)]) + plain_origin).collect();
        let height_on_border = |z: f32| {
            let k = border.partition_point(|p| p.z <= z).clamp(1, r) - 1;
            let (a, b) = (border[k], border[k + 1]);
            a.y + (b.y - a.y) * (z - a.z) / (b.z - a.z)
        };

        // The marching-cubes surface of the shaft chunk, without skirts.
        let geometry = build_volume_chunk_geometry(SHAFT_CHUNK, &config, &data, &volume).expect("marches");
        let surface = &geometry.indices[..geometry.surface_index_count];
        let origin = chunk_world_position(SHAFT_CHUNK, &config);
        let world = |i: u32| Vec3::from_array(geometry.positions[i as usize]) + origin;
        let border_x = plain_origin.x;
        assert_eq!(origin.x + config.chunk_size, border_x);

        let mut uses: HashMap<(u32, u32), usize> = HashMap::new();
        for t in surface.chunks_exact(3) {
            for (from, to) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *uses.entry((from.min(to), from.max(to))).or_default() += 1;
            }
        }
        let mut segments: Vec<(f32, f32)> = Vec::new();
        let mut border_vertices: Vec<Vec3> = Vec::new();
        for (&(a, b), &count) in &uses {
            let (pa, pb) = (world(a), world(b));
            if (pa.x - border_x).abs() > 1e-4 || (pb.x - border_x).abs() > 1e-4 {
                continue;
            }
            assert_eq!(count, 1, "a border edge belongs to one surface triangle");
            for p in [pa, pb] {
                let expected = height_on_border(p.z);
                assert!((p.y - expected).abs() < 1e-3, "border vertex {p} is off the heightfield border ({expected})");
                border_vertices.push(p);
            }
            segments.push((pa.z.min(pb.z), pa.z.max(pb.z)));
        }

        // The segments chain from one end of the border to the other.
        segments.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut reached = border[0].z;
        for (start, end) in &segments {
            assert!((start - reached).abs() < 1e-4, "gap or overlap at z = {reached} (next segment starts at {start})");
            reached = *end;
        }
        assert!((reached - border[r].z).abs() < 1e-4, "the border stops at z = {reached}");

        // And every heightfield border vertex is one of its vertices.
        for p in &border {
            assert!(
                border_vertices.iter().any(|q| q.distance(*p) < 1e-3),
                "heightfield border vertex {p} is missing from the marching-cubes border"
            );
        }
    }

    #[test]
    fn brick_surfaces_carry_their_slot_in_uv1_and_the_heightfield_reads_the_material_map() {
        let config = test_config();
        let data = rolling_data(&config);
        // A brick block floating over the rolling ground (about 11 to 21 m)
        // of chunk (0, 0): its faces are the edit's, the ground around it the
        // heightfield's.
        let mut volume = TerrainVolume::new();
        let edit = apply_box(
            &config,
            &mut volume,
            Vec3::new(13.0, 27.0, 13.0),
            Vec3::new(4.0, 3.0, 4.0),
            CsgOp::Add,
            Some(TerrainMaterial::Brick),
        );
        assert!(!edit.is_empty());
        let geometry = build_volume_chunk_geometry(SHAFT_CHUNK, &config, &data, &volume).expect("marches");
        assert_eq!(geometry.brick_uvs.len(), geometry.positions.len(), "skirts copy the attribute too");

        let brick = brick_material_uv(Some(TerrainMaterial::Brick));
        assert_eq!(brick, [f32::from(TerrainMaterial::Brick.to_u8()) + 1.0, 1.0]);
        assert_eq!(brick_material_uv(None), [0.0, 0.0]);
        for uv in &geometry.brick_uvs {
            let ground = *uv == [0.0, 0.0];
            let edit = uv[1] == 1.0 && uv[0] >= 1.0 && uv[0].fract() == 0.0;
            assert!(ground || edit, "{uv:?} is neither a slot + 1 with weight 1 nor the material-map marker");
        }
        assert!(geometry.brick_uvs.contains(&brick), "the block's faces name Brick");
        assert!(geometry.brick_uvs.contains(&[0.0, 0.0]), "the ground around it reads the material map");

        // The mesh carries it as UV_1, which the shader's VERTEX_UVS_B gate
        // keys on; heightfield meshes have none.
        let mesh = geometry.into_mesh();
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_1).is_some());
        let mut meshes = Assets::<Mesh>::default();
        let plain = generate_chunk_mesh(PLAIN_CHUNK, 0, &config, &data, &mut meshes);
        assert!(meshes.get(&plain).expect("mesh was just added").attribute(Mesh::ATTRIBUTE_UV_1).is_none());
    }

    #[test]
    fn every_surface_triangle_names_at_most_one_brick_slot() {
        let config = test_config();
        let data = rolling_data(&config);
        // Two touching blocks floating over the rolling ground of chunk
        // (0, 0), Rock over x 9..13 and Brick over x 13..17. Their joint top
        // face crosses the seam, so its cubes between the lattice columns
        // x = 12 (Rock) and x = 14 (Brick) march triangles joining both.
        let mut volume = TerrainVolume::new();
        for (center_x, material) in [(11.0, TerrainMaterial::Rock), (15.0, TerrainMaterial::Brick)] {
            let edit = apply_box(
                &config,
                &mut volume,
                Vec3::new(center_x, 27.3, 13.0),
                Vec3::new(2.0, 3.0, 4.0),
                CsgOp::Add,
                Some(material),
            );
            assert!(!edit.is_empty());
        }
        let geometry = build_volume_chunk_geometry(SHAFT_CHUNK, &config, &data, &volume).expect("marches");
        let [rock, brick] = [TerrainMaterial::Rock, TerrainMaterial::Brick].map(|m| brick_material_uv(Some(m)));
        assert!(geometry.brick_uvs.contains(&rock), "the Rock block is drawn");
        assert!(geometry.brick_uvs.contains(&brick), "the Brick block is drawn");

        let surface = &geometry.indices[..geometry.surface_index_count];
        assert!(
            surface.iter().all(|&i| (i as usize) < geometry.surface_vertex_count),
            "a surface triangle uses a vertex past the surface's"
        );
        for t in surface.chunks_exact(3) {
            let mut named = t.iter().map(|&i| geometry.brick_uvs[i as usize]).filter(|uv| uv[1] > 0.0).map(|uv| uv[0]);
            if let Some(first) = named.next() {
                assert!(named.all(|x| x == first), "triangle {t:?} names more than one brick slot");
            }
        }

        // The seam's triangles were given vertices of their own (the march
        // alone has fewer), and the spawn path's collider surface covers them.
        let (marched, _) = volume_chunk_triangles(SHAFT_CHUNK, &config, &data, &volume).expect("marches");
        assert!(geometry.surface_vertex_count > marched.len(), "no triangle across the seam was split");
        let (positions, triangles) = geometry.surface_triangles();
        assert_eq!(positions.len(), geometry.surface_vertex_count);
        assert_eq!(triangles.len() * 3, geometry.surface_index_count);
    }

    #[test]
    fn a_carved_shaft_opens_a_hole_and_the_surface_is_open_only_at_the_chunk_sides() {
        let config = test_config();
        let data = rolling_data(&config);
        let volume = shaft_volume(&config);
        let lattice = ChunkLattice::sample(SHAFT_CHUNK, &config, &data, &volume).expect("samples");
        let surface = lattice.march();
        let lowest = surface
            .vertices
            .iter()
            .map(|vertex| lattice.local_position(&config, vertex).y)
            .fold(f32::INFINITY, f32::min);
        assert!(lowest < -6.0, "the shaft floor near y = -7.8 is drawn, lowest vertex {lowest}");

        let mut uses: HashMap<(u32, u32), usize> = HashMap::new();
        for t in surface.indices.chunks_exact(3) {
            for (from, to) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *uses.entry((from.min(to), from.max(to))).or_default() += 1;
            }
        }
        for (&(a, b), &count) in &uses {
            assert!(count <= 2, "edge {a} - {b} is used {count} times");
            if count == 1 {
                let (sa, sb) = (lattice.sides(&surface.vertices[a as usize]), lattice.sides(&surface.vertices[b as usize]));
                assert!((0..4).any(|side| sa[side] && sb[side]), "edge {a} - {b} is open inside the chunk");
            }
        }
    }

    #[test]
    fn chunk_triangles_face_along_the_terrain_field_gradient() {
        let config = test_config();
        let data = rolling_data(&config);
        let volume = shaft_volume(&config);
        let (positions, triangles) = volume_chunk_triangles(SHAFT_CHUNK, &config, &data, &volume).expect("marches");
        let origin = chunk_world_position(SHAFT_CHUNK, &config);
        let cell = lattice_cell_size(&config);
        let (mut agreement, mut total) = (0.0f32, 0.0f32);
        for t in &triangles {
            let [a, b, c] = (*t).map(|i| positions[i as usize] + origin);
            let cross = (b - a).cross(c - a);
            let area = cross.length() * 0.5;
            if area < 1e-6 {
                continue;
            }
            let gradient = field_gradient(&config, &data, &volume, (a + b + c) / 3.0).normalize_or_zero();
            let dot = cross.normalize().dot(gradient);
            if area > 0.5 * cell * cell {
                assert!(dot > 0.0, "large triangle {t:?} faces into the terrain ({dot})");
            }
            agreement += dot * area;
            total += area;
        }
        assert!(agreement / total > 0.8, "area-weighted agreement {}", agreement / total);
    }

    /// The skirt vertices of `geometry` (built for `chunk`) as world points,
    /// each checked to hang exactly the skirt depth below a surface vertex on
    /// a chunk side, through lattice columns that carry no edit from that
    /// vertex down to the skirt's foot.
    fn checked_skirts(config: &TerrainConfig, volume: &TerrainVolume, chunk: IVec2, geometry: &VolumeChunkGeometry) -> Vec<Vec3> {
        let size = config.chunk_size;
        let depth = skirt_depth(size);
        let cell = lattice_cell_size(config);
        let origin = chunk_world_position(chunk, config);
        let surface = &geometry.positions[..geometry.surface_vertex_count];
        // Lattice lines at or either side of a coordinate, in cells.
        let span = |v: f32| ((v + 1e-3).floor() as i32, (v - 1e-3).ceil() as i32);
        let mut skirts = Vec::new();
        for &[x, y, z] in &geometry.positions[geometry.surface_vertex_count..] {
            assert!(
                surface.iter().any(|p| p[0] == x && p[2] == z && p[1] + depth == y),
                "skirt vertex ({x}, {y}, {z}) does not hang {depth} below a border vertex"
            );
            let on_side = [x.abs() < 1e-4, (x - size).abs() < 1e-4, z.abs() < 1e-4, (z - size).abs() < 1e-4];
            assert!(on_side.contains(&true), "skirt vertex ({x}, {y}, {z}) is off the chunk's sides");
            let foot = origin + Vec3::new(x, y, z);
            let (x0, x1) = span(foot.x / cell);
            let (z0, z1) = span(foot.z / cell);
            let (y0, y1) = ((foot.y / cell).floor() as i32, ((foot.y - depth) / cell).ceil() as i32);
            for nz in z0..=z1 {
                for nx in x0..=x1 {
                    for ny in y0..=y1 {
                        assert!(
                            !volume.cell_at_lattice(IVec3::new(nx, ny, nz)).is_edited(),
                            "the skirt down to {foot} hangs through the edited lattice point ({nx}, {ny}, {nz})"
                        );
                    }
                }
            }
            skirts.push(foot);
        }
        skirts
    }

    #[test]
    fn only_volumetric_chunks_march_at_lod_zero_and_skirt_along_unedited_border() {
        let config = test_config();
        let data = rolling_data(&config);
        let volume = shaft_volume(&config);
        assert!(chunk_uses_marching_cubes(SHAFT_CHUNK, 0, &config, &volume));
        assert!(!chunk_uses_marching_cubes(SHAFT_CHUNK, 1, &config, &volume), "caves are not drawn far away");
        assert!(!chunk_uses_marching_cubes(PLAIN_CHUNK, 0, &config, &volume));
        assert!(!chunk_uses_marching_cubes(SHAFT_CHUNK, 0, &config, &TerrainVolume::new()));

        // A volumetric chunk costs the cubes its cell bands hold, per cell.
        let lattice = ChunkLattice::sample(SHAFT_CHUNK, &config, &data, &volume).expect("samples");
        let cubes: usize = lattice.bands.iter().map(|&(start, end)| (end - start) as usize).sum();
        let cost = VOLUMETRIC_CHUNK_COST.max(cubes.div_ceil((lattice.cells * lattice.cells) as usize));
        assert_eq!(chunk_mesh_cost(SHAFT_CHUNK, 0, &config, &data, &volume), cost);
        assert_eq!(chunk_mesh_cost(SHAFT_CHUNK, 1, &config, &data, &volume), 1);
        assert_eq!(chunk_collider_cost(SHAFT_CHUNK, &config, &data, &volume), cost);
        assert_eq!(chunk_collider_cost(PLAIN_CHUNK, &config, &data, &volume), 1);
        assert_eq!(chunk_spawn_cost(SHAFT_CHUNK, 1, &config, &data, &volume), cost, "the collider marches at every LOD");

        // The shaft's bricks own the -X and -Z border columns, but no edit
        // reaches their points, so every side of the chunk is skirted.
        let geometry = build_volume_chunk_geometry(SHAFT_CHUNK, &config, &data, &volume).expect("marches");
        assert!(geometry.indices.len() > geometry.surface_index_count, "skirts were added");
        assert!(geometry.positions.len() > geometry.surface_vertex_count);
        let skirts = checked_skirts(&config, &volume, SHAFT_CHUNK, &geometry);
        let origin = chunk_world_position(SHAFT_CHUNK, &config);
        let size = config.chunk_size;
        for (name, axis, offset) in [("-X", 0usize, 0.0f32), ("+X", 0, size), ("-Z", 2, 0.0), ("+Z", 2, size)] {
            assert!(
                skirts.iter().any(|p| (p[axis] - origin[axis] - offset).abs() < 1e-4),
                "the unedited {name} side has no skirt"
            );
        }

        let mut meshes = Assets::<Mesh>::default();
        let handle = generate_chunk_render_mesh(SHAFT_CHUNK, 0, &config, &data, &volume, &mut meshes);
        assert_eq!(meshes.get(&handle).expect("added").count_vertices(), geometry.positions.len());
        // The spawn path hands its collider the same surface the collider
        // would march for itself.
        let (handle, surface) = generate_chunk_render_mesh_and_surface(SHAFT_CHUNK, 0, &config, &data, &volume, &mut meshes);
        assert_eq!(meshes.get(&handle).expect("added").count_vertices(), geometry.positions.len());
        assert_eq!(surface, volume_chunk_triangles(SHAFT_CHUNK, &config, &data, &volume));
        assert!(generate_chunk_render_mesh_and_surface(SHAFT_CHUNK, 1, &config, &data, &volume, &mut meshes).1.is_none());
        let (handle, surface) = generate_chunk_render_mesh_and_surface(PLAIN_CHUNK, 0, &config, &data, &volume, &mut meshes);
        assert!(surface.is_none());
        let r = config.resolution_for_lod(0) as usize;
        assert_eq!(
            meshes.get(&handle).expect("added").count_vertices(),
            (r + 1) * (r + 1) + 4 * (r + 1),
            "a chunk without bricks keeps its heightfield mesh and skirts"
        );
    }

    #[test]
    fn no_skirt_hangs_where_an_edit_reaches_the_border() {
        let config = test_config();
        let data = rolling_data(&config);
        let mut volume = TerrainVolume::new();
        // A tunnel through the ground across the border between the two
        // chunks (x = 32) at z 9.5..14.5, so both of them march.
        apply_box(&config, &mut volume, Vec3::new(32.0, 15.0, 12.0), Vec3::new(6.0, 2.5, 2.5), CsgOp::Carve, None);
        assert!(chunk_uses_marching_cubes(SHAFT_CHUNK, 0, &config, &volume));
        assert!(chunk_uses_marching_cubes(PLAIN_CHUNK, 0, &config, &volume));

        let geometry = build_volume_chunk_geometry(SHAFT_CHUNK, &config, &data, &volume).expect("marches");
        let skirts = checked_skirts(&config, &volume, SHAFT_CHUNK, &geometry);
        let border_x = chunk_world_position(PLAIN_CHUNK, &config).x;
        let shared: Vec<Vec3> = skirts.into_iter().filter(|p| (p.x - border_x).abs() < 1e-4).collect();
        // The edit reaches the border's points within four cells of the
        // tunnel, z 2..22: past that the shared side is still skirted, which
        // covers the gap to the neighbour whenever it draws coarser or as a
        // heightfield.
        assert!(shared.iter().any(|p| p.z > 24.0), "the unedited stretch of the shared side has no skirt");
        assert!(shared.iter().all(|p| !(4.0..20.0).contains(&p.z)), "a skirt hangs across the tunnel");
    }

    /// Each triangle as its three vertices' lattice edges and crossings,
    /// rotated to start at the least and sorted: equal for two surfaces that
    /// differ only in the order they were built.
    fn canonical_triangles(surface: &MarchedSurface) -> Vec<[(u32, u32, u32, u8, u32); 3]> {
        let key = |i: u32| {
            let v = &surface.vertices[i as usize];
            (v.point.x, v.point.y, v.point.z, v.axis, v.t.to_bits())
        };
        let mut triangles: Vec<[(u32, u32, u32, u8, u32); 3]> = surface
            .indices
            .chunks_exact(3)
            .map(|t| {
                let mut keys = [key(t[0]), key(t[1]), key(t[2])];
                let least = (0..3).min_by_key(|&i| keys[i]).unwrap_or(0);
                keys.rotate_left(least);
                keys
            })
            .collect();
        triangles.sort_unstable();
        triangles
    }

    #[test]
    fn banded_marching_matches_marching_the_whole_box_and_visits_fewer_cubes() {
        // 64 m chunks at 32 cells, so the shaft's bricks cover a quarter of
        // the chunk and the rest of it marches only a band around the ground.
        let config = TerrainConfig { chunk_size: 64.0, chunk_resolution: 32, ..test_config() };
        let data = rolling_data(&config);
        let volume = shaft_volume(&config);
        let lattice = ChunkLattice::sample(SHAFT_CHUNK, &config, &data, &volume).expect("samples");
        let dims = lattice.field.dims();
        let cells = lattice.cells as i32;
        let whole = LatticeField::from_fn(dims, |p| {
            let n = IVec3::new(
                SHAFT_CHUNK.x * cells + p.x as i32 - 1,
                lattice.layer0 + p.y as i32 - 1,
                SHAFT_CHUNK.y * cells + p.z as i32 - 1,
            );
            sample_field_lattice(&config, &data, &volume, n).value
        });
        let expected = march_cubes(&whole, UVec3::ONE, dims - UVec3::splat(2));
        assert!(!expected.indices.is_empty());
        assert_eq!(canonical_triangles(&lattice.march()), canonical_triangles(&expected));

        let banded: usize = lattice.bands.iter().map(|&(start, end)| (end - start) as usize).sum();
        let whole_box = (cells * cells) as usize * (dims.y - 3) as usize;
        assert!(banded * 2 < whole_box, "{banded} banded cubes against {whole_box} in the whole box");
    }
}
