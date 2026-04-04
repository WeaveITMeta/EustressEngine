//! Runtime mesh optimization via meshopt — makes GLB meshes GPU-fast.
//!
//! Two systems:
//! 1. `optimize_loaded_meshes` — runs once per mesh asset after GLB load.
//!    Applies vertex cache opt, overdraw opt, and vertex fetch reorder.
//!    Cost: ~0.1-2ms per mesh, runs during asset loading (not per-frame).
//!
//! 2. `lod_switch_system` — per-frame LOD switching based on camera distance.
//!    Generates simplified LOD meshes lazily on first proximity, caches them.
//!
//! All optimizations work on standard GLB meshes — no format changes needed.

use bevy::prelude::*;
use bevy::asset::AssetEvent;
use bevy::mesh::{Indices, MeshVertexAttribute, VertexAttributeValues};
use bevy::render::render_resource::PrimitiveTopology;
use meshopt::VertexDataAdapter;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Runtime mesh optimization and LOD management.
pub struct MeshOptPlugin;

impl Plugin for MeshOptPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OptimizedMeshTracker>()
            .init_resource::<LodCache>()
            .add_systems(Update, optimize_loaded_meshes)
            .add_systems(Update, lod_switch_system.after(optimize_loaded_meshes));
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

        // Skip engine primitives — already optimized at source
        if let Some(path) = asset_server.get_path(id) {
            let path_str = path.path().to_string_lossy();
            if path_str.starts_with("parts/") {
                tracker.processed.insert(id);
                continue;
            }
        }

        let Some(mesh) = meshes.get_mut(id) else { continue };
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            continue;
        }

        // Skip small meshes — optimization overhead isn't worth it
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

        optimize_mesh_in_place(mesh);
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

// ---------------------------------------------------------------------------
// Runtime LOD system
// ---------------------------------------------------------------------------

/// Component to enable LOD switching on an entity.
/// Add this alongside `Mesh3d` for automatic distance-based LOD.
#[derive(Component)]
pub struct LodEnabled {
    /// The original (full-detail) mesh handle. LOD system swaps Mesh3d
    /// to simplified versions and back based on camera distance.
    pub original: Handle<Mesh>,
}

#[derive(Debug, Clone)]
struct LodSet {
    original: Handle<Mesh>,
    /// (handle, world_error) pairs, coarsest last.
    levels: Vec<(Handle<Mesh>, f32)>,
}

#[derive(Resource, Default)]
struct LodCache {
    sets: HashMap<AssetId<Mesh>, LodSet>,
}

/// Per-frame: pick the coarsest LOD whose screen-space error < 1 pixel.
fn lod_switch_system(
    mut lod_cache: ResMut<LodCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    camera_query: Query<(&GlobalTransform, &Projection), With<Camera3d>>,
    mut lod_query: Query<(&GlobalTransform, &mut Mesh3d, &LodEnabled)>,
) {
    let Some((cam_tf, projection)) = camera_query.iter().next() else { return };
    let cam_pos = cam_tf.translation();
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };
    // screen_err = world_error * proj_scale / distance
    // We want screen_err < 1px → use coarser LOD
    let proj_scale = 1080.0 / (2.0 * (fov * 0.5).tan());

    for (entity_tf, mut mesh3d, lod) in &mut lod_query {
        let distance = cam_pos.distance(entity_tf.translation()).max(0.1);
        let original_id = lod.original.id();

        // Lazily generate LODs (clone mesh to release immutable borrow before mutating)
        if !lod_cache.sets.contains_key(&original_id) {
            let Some(original_mesh) = meshes.get(original_id).cloned() else { continue };
            let set = build_lod_set(&original_mesh, &lod.original, &mut meshes);
            lod_cache.sets.insert(original_id, set);
        }

        let Some(set) = lod_cache.sets.get(&original_id) else { continue };

        // Find coarsest acceptable LOD
        let mut best = &set.original;
        for (handle, world_error) in &set.levels {
            let screen_err = world_error * proj_scale / distance;
            if screen_err < 1.0 {
                best = handle;
            } else {
                break;
            }
        }

        if mesh3d.0.id() != best.id() {
            mesh3d.0 = best.clone();
        }
    }
}

/// Generate LOD1 (50%), LOD2 (25%), LOD3 (12.5%) from original mesh.
fn build_lod_set(
    original: &Mesh,
    original_handle: &Handle<Mesh>,
    meshes: &mut Assets<Mesh>,
) -> LodSet {
    let mut levels = Vec::new();

    let Some(raw_indices) = original.indices() else {
        return LodSet { original: original_handle.clone(), levels };
    };
    let indices: Vec<u32> = match raw_indices {
        Indices::U16(v) => v.iter().map(|&i| i as u32).collect(),
        Indices::U32(v) => v.clone(),
    };
    let Some(VertexAttributeValues::Float32x3(positions)) = original.attribute(Mesh::ATTRIBUTE_POSITION) else {
        return LodSet { original: original_handle.clone(), levels };
    };

    let vertex_count = positions.len();
    let pos_bytes: &[u8] = bytemuck::cast_slice(positions.as_slice());
    let pos_stride = std::mem::size_of::<[f32; 3]>();

    let Ok(adapter) = VertexDataAdapter::new(pos_bytes, pos_stride, 0) else {
        return LodSet { original: original_handle.clone(), levels };
    };

    let original_tri_count = indices.len() / 3;

    for ratio in [0.5f32, 0.25, 0.125] {
        let target = ((original_tri_count as f32 * ratio) as usize).max(4) * 3;

        let mut error = 0.0f32;
        let simplified = meshopt::simplify(
            &indices,
            &adapter,
            target,
            f32::MAX,
            meshopt::SimplifyOptions::LockBorder,
            Some(&mut error),
        );

        // Skip if simplification didn't meaningfully reduce
        if simplified.len() >= indices.len() * 9 / 10 {
            continue;
        }

        let mut lod_mesh = original.clone();
        if vertex_count <= u16::MAX as usize + 1 {
            lod_mesh.insert_indices(Indices::U16(simplified.iter().map(|&i| i as u16).collect()));
        } else {
            lod_mesh.insert_indices(Indices::U32(simplified));
        }

        let handle = meshes.add(lod_mesh);
        levels.push((handle, error));
    }

    LodSet { original: original_handle.clone(), levels }
}
