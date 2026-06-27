//! Runtime mesh optimization via meshopt — makes GLB meshes GPU-fast.
//!
//! One system:
//! 1. `optimize_loaded_meshes` — runs once per mesh asset after GLB load
//!    (gated by `on_message::<AssetEvent<Mesh>>` so it only runs on frames
//!    that actually emit mesh events). Applies vertex cache opt, overdraw opt,
//!    and vertex fetch reorder. Cost: ~0.1-2ms per mesh.
//!
//! All optimizations work on standard GLB meshes — no format changes needed.

use bevy::prelude::*;
use bevy::asset::AssetEvent;
use bevy::mesh::{Indices, MeshVertexAttribute, VertexAttributeValues};
use bevy::render::render_resource::PrimitiveTopology;
use meshopt::VertexDataAdapter;

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Runtime mesh optimization and LOD management.
pub struct MeshOptPlugin;

impl Plugin for MeshOptPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OptimizedMeshTracker>()
            // PERF: only run the optimizer on frames that actually emit mesh
            // asset events; on a steady scene there is nothing to do.
            .add_systems(
                Update,
                optimize_loaded_meshes.run_if(on_message::<AssetEvent<Mesh>>),
            );
        // NOTE: the runtime LOD system (lod_switch_system / LodSet / LodCache /
        // build_lod_set / LodEnabled) was DEAD CODE — `LodEnabled` was never
        // inserted on any entity, so the query was always empty — and has been
        // deleted.
    }
}

// ---------------------------------------------------------------------------
// Mesh optimization (runs once per mesh asset on load)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
struct OptimizedMeshTracker {
    processed: std::collections::HashSet<AssetId<Mesh>>,
}

/// Minimum triangle count to bother optimizing. Primitives (cube=12, sphere=~480)
/// and small meshes don't benefit from cache/overdraw reordering.
const MIN_TRIANGLES_FOR_OPTIMIZATION: usize = 500;

/// Watches for newly loaded meshes and runs the meshopt pipeline.
/// Skips engine primitives (parts/*.glb) and small meshes automatically.
fn optimize_loaded_meshes(
    mut mesh_events: MessageReader<AssetEvent<Mesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tracker: ResMut<OptimizedMeshTracker>,
    asset_server: Res<AssetServer>,
) {
    for event in mesh_events.read() {
        let id = match event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => *id,
            _ => continue,
        };
        // Re-optimize on modification (hot-reload)
        if matches!(event, AssetEvent::Modified { .. }) {
            tracker.processed.remove(&id);
        }

        if tracker.processed.contains(&id) {
            continue;
        }

        // Cheap immutable rejects FIRST: before get_path string work and before
        // get_mut (which re-flags Modified and would re-enter the reader).
        let Some(mesh) = meshes.get(id) else { continue };
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            tracker.processed.insert(id);
            continue;
        }
        let tri_count = mesh.indices()
            .map(|idx| match idx {
                Indices::U16(v) => v.len() / 3,
                Indices::U32(v) => v.len() / 3,
            })
            .unwrap_or(0);
        if tri_count < MIN_TRIANGLES_FOR_OPTIMIZATION {
            tracker.processed.insert(id);
            continue;
        }

        if let Some(path) = asset_server.get_path(id) {
            if path.path().to_string_lossy().starts_with("parts/") {
                tracker.processed.insert(id);
                continue;
            }
        }

        let Some(mut mesh) = meshes.get_mut(id) else { continue };
        optimize_mesh_in_place(&mut mesh);
        tracker.processed.insert(id);
    }
}

/// Full meshopt pipeline on a Bevy Mesh — mutates vertex and index buffers in place.
fn optimize_mesh_in_place(mesh: &mut Mesh) {
    // Extract indices
    let Some(raw_indices) = mesh.indices() else { return };
    let mut indices: Vec<u32> = match raw_indices {
        Indices::U16(v) => v.iter().map(|&i| i as u32).collect(),
        Indices::U32(v) => v.clone(),
    };
    if indices.len() < 3 { return; }

    // Extract positions
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).cloned() else {
        return;
    };
    let vertex_count = positions.len();
    if vertex_count == 0 { return; }

    // Build position byte slice for VertexDataAdapter
    let pos_bytes: Vec<u8> = bytemuck::cast_slice::<[f32; 3], u8>(&positions).to_vec();
    let pos_stride = std::mem::size_of::<[f32; 3]>();

    // 1. Vertex cache optimization (Tom Forsyth algorithm)
    //    Reorders indices for maximum GPU vertex cache reuse.
    //    Typically 1.5-3× improvement in vertex shading throughput.
    meshopt::optimize_vertex_cache_in_place(&mut indices, vertex_count);

    // 2. Overdraw optimization
    //    Reorders triangles so occluders render first → depth test rejects hidden frags.
    if let Ok(adapter) = VertexDataAdapter::new(&pos_bytes, pos_stride, 0) {
        meshopt::optimize_overdraw_in_place(&mut indices, &adapter, 1.05);
    }

    // 3. Vertex fetch optimization
    //    Reorders vertex buffer so vertices are accessed sequentially.
    //    Reduces GPU L2 cache pressure significantly.
    let remap = meshopt::optimize_vertex_fetch_remap(&indices, vertex_count);
    indices = meshopt::remap_index_buffer(Some(&indices), vertex_count, &remap);

    // Apply remap to all vertex attributes
    reorder_f32x3(mesh, Mesh::ATTRIBUTE_POSITION, &remap, vertex_count);
    reorder_f32x3(mesh, Mesh::ATTRIBUTE_NORMAL, &remap, vertex_count);
    reorder_f32x2(mesh, Mesh::ATTRIBUTE_UV_0, &remap, vertex_count);
    reorder_f32x4(mesh, Mesh::ATTRIBUTE_TANGENT, &remap, vertex_count);
    // Joint indices/weights for skeletal meshes
    reorder_u16x4(mesh, Mesh::ATTRIBUTE_JOINT_INDEX, &remap, vertex_count);
    reorder_f32x4(mesh, Mesh::ATTRIBUTE_JOINT_WEIGHT, &remap, vertex_count);

    // Write optimized indices
    if vertex_count <= u16::MAX as usize + 1 {
        mesh.insert_indices(Indices::U16(indices.iter().map(|&i| i as u16).collect()));
    } else {
        mesh.insert_indices(Indices::U32(indices));
    }
}

// ---------------------------------------------------------------------------
// Vertex attribute reordering helpers
// ---------------------------------------------------------------------------

fn apply_remap<T: Clone + Default>(data: &[T], remap: &[u32], count: usize) -> Vec<T> {
    let mut out = vec![T::default(); count];
    for (old_idx, &new_idx) in remap.iter().enumerate() {
        let ni = new_idx as usize;
        if ni < count && old_idx < data.len() {
            out[ni] = data[old_idx].clone();
        }
    }
    out
}

fn reorder_f32x3(mesh: &mut Mesh, attr: MeshVertexAttribute, remap: &[u32], count: usize) {
    if let Some(VertexAttributeValues::Float32x3(data)) = mesh.attribute(attr).cloned() {
        mesh.insert_attribute(attr, apply_remap(&data, remap, count));
    }
}

fn reorder_f32x2(mesh: &mut Mesh, attr: MeshVertexAttribute, remap: &[u32], count: usize) {
    if let Some(VertexAttributeValues::Float32x2(data)) = mesh.attribute(attr).cloned() {
        mesh.insert_attribute(attr, apply_remap(&data, remap, count));
    }
}

fn reorder_f32x4(mesh: &mut Mesh, attr: MeshVertexAttribute, remap: &[u32], count: usize) {
    if let Some(VertexAttributeValues::Float32x4(data)) = mesh.attribute(attr).cloned() {
        mesh.insert_attribute(attr, apply_remap(&data, remap, count));
    }
}

fn reorder_u16x4(mesh: &mut Mesh, attr: MeshVertexAttribute, remap: &[u32], count: usize) {
    if let Some(VertexAttributeValues::Uint16x4(data)) = mesh.attribute(attr).cloned() {
        mesh.insert_attribute(attr, VertexAttributeValues::Uint16x4(apply_remap(&data, remap, count)));
    }
}

// NOTE: The runtime LOD system (LodEnabled, LodSet, LodCache, lod_switch_system,
// build_lod_set) was removed — it was dead code. `LodEnabled` was never inserted
// on any entity, so `lod_switch_system` always iterated an empty query. The
// distance-based render LOD strategy lives elsewhere (residency + VisibilityRange).
