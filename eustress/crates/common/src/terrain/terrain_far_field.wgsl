// Terrain far field: the vertex half of `TerrainFarFieldMaterial` (see
// `far_field.rs`). Its fragment half is `terrain_surface.wgsl`, compiled with
// TERRAIN_FAR_FIELD.
//
// Every far-field level is a flat grid in local XZ, one shared mesh in unit
// spacing that the level's transform scales and places. Each vertex goes to
// world XZ through the mesh's world-from-local, as bevy_pbr's `mesh.wgsl`
// does, is held to the terrain's footprint (so the far field ends where the
// terrain ends: a vertex past the edge folds onto it, and the quads beyond
// collapse to nothing), and is lifted to the terrain height there: the four
// texels of the height texture around it, blended as
// `TerrainData::sample_height` blends its cells. The normal comes from
// central differences of those heights half one of the level's quads apart
// (never less than a raster cell), so a coarse level shades by the slopes it
// draws rather than by cell-scale detail between its vertices.
//
// A level's transform carries its lowering in its translation's Y (level L
// sits (L + 1) sinks plus a share of its quad down, see
// `far_field_level_drop` in `far_field.rs`). The mesh lies at local Y 0
// with no rotation and a unit Y scale, so the world Y the mesh transform
// yields is exactly that lowering, and the stage adds it to the sampled
// height: chunks and finer levels win the depth test where they overlap.
//
// Bindings 106 to 108 sit after the base StandardMaterial's in the material
// bind group; their Rust side is `TerrainFarFieldExtension`. Both structs
// below repeat the ones `terrain_surface.wgsl` declares, and the unit tests
// in `far_field.rs` hold all of them to the Rust packing, so change them
// together.

#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

// Where the height texture lies in the world, matching `sample_height`:
// u = (world_x - world_origin.x) / world_extent.x, texel = u * (width - 1).
// The material map's mapping, since both hold one texel per raster cell.
struct TerrainSurfaceParams {
    world_origin: vec2<f32>,
    world_extent: vec2<f32>,
    cache_size: vec2<u32>,
    flags: u32,
    blend_depth: f32,
}

struct TerrainFarFieldParams {
    // World XZ the near disc the fragment stage leaves to the chunks is
    // centred on, following the scene camera in steps. This stage does not
    // read it: each level's transform carries its own centre.
    camera_xz: vec2<f32>,
    near_radius: f32,
    // Metres each level sits below the one inside it.
    sink: f32,
    // World size of one raster cell: the spacing of the normal's heights.
    cell: vec2<f32>,
    // 1 when a material-map cell whose id_a is "no material" is a hole.
    sparse: u32,
    _pad: u32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(106) var<uniform> terrain_params: TerrainSurfaceParams;
// R32Float: the surface height in world metres, one texel per raster cell.
// Read with textureLoad, since not every adapter filters a 32-bit float.
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var terrain_far_heights: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var<uniform> terrain_far: TerrainFarFieldParams;

// World height of the terrain at `world_xz`: the four texels around it,
// blended as `TerrainData::sample_height` blends its cells.
fn terrain_height_at(world_xz: vec2<f32>) -> f32 {
    let last = max(terrain_params.cache_size, vec2<u32>(1u)) - vec2<u32>(1u);
    let uv = clamp(
        (world_xz - terrain_params.world_origin) / terrain_params.world_extent,
        vec2<f32>(0.0),
        vec2<f32>(1.0),
    );
    let texel = uv * vec2<f32>(last);
    let lo = min(vec2<u32>(floor(texel)), last);
    let hi = min(lo + vec2<u32>(1u), last);
    let f = texel - vec2<f32>(lo);
    let lo_i = vec2<i32>(lo);
    let hi_i = vec2<i32>(hi);
    let h00 = textureLoad(terrain_far_heights, lo_i, 0).r;
    let h10 = textureLoad(terrain_far_heights, vec2<i32>(hi_i.x, lo_i.y), 0).r;
    let h01 = textureLoad(terrain_far_heights, vec2<i32>(lo_i.x, hi_i.y), 0).r;
    let h11 = textureLoad(terrain_far_heights, hi_i, 0).r;
    let row0 = h00 + (h10 - h00) * f.x;
    let row1 = h01 + (h11 - h01) * f.x;
    return row0 + (row1 - row0) * f.y;
}

// The ground's normal at `world_xz`, from the slope between the heights half
// a level quad (`quad`, world size) to either side along X and along Z, and
// at least one raster cell.
fn terrain_normal_at(world_xz: vec2<f32>, quad: vec2<f32>) -> vec3<f32> {
    let spacing = max(max(terrain_far.cell, 0.5 * quad), vec2<f32>(1e-3));
    let along_x = vec2<f32>(spacing.x, 0.0);
    let along_z = vec2<f32>(0.0, spacing.y);
    let slope_x = (terrain_height_at(world_xz + along_x) - terrain_height_at(world_xz - along_x)) / (2.0 * spacing.x);
    let slope_z = (terrain_height_at(world_xz + along_z) - terrain_height_at(world_xz - along_z)) / (2.0 * spacing.y);
    return normalize(vec3<f32>(-slope_x, 1.0, -slope_z));
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;

    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);

#ifdef VERTEX_POSITIONS
    // X and Z where the level puts the vertex, Y the level's lowering.
    let placed = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex_in.position, 1.0));
    let footprint_min = terrain_params.world_origin;
    let xz = clamp(placed.xz, footprint_min, footprint_min + terrain_params.world_extent);
    out.world_position = vec4<f32>(xz.x, terrain_height_at(xz) + placed.y, xz.y, 1.0);
    out.position = position_world_to_clip(out.world_position.xyz);
    // The level's quad: the mesh is in unit spacing, so its transform's X and
    // Z scales are the quad's world size.
    let quad = vec2<f32>(length(world_from_local[0].xyz), length(world_from_local[2].xyz));
    out.world_normal = terrain_normal_at(xz, quad);
#endif

    // The far-field meshes carry positions only (see `far_field.rs`); these
    // pass any other attribute through as `mesh.wgsl` does.
#ifdef VERTEX_UVS_A
    out.uv = vertex_in.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex_in.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex_in.tangent,
        vertex_in.instance_index
    );
#endif
#ifdef VERTEX_COLORS
    out.color = vertex_in.color;
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    // The fragment stage reads the mesh's flags and material slot through it.
    out.instance_index = vertex_in.instance_index;
#endif

#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index,
        world_from_local[3]
    );
#endif

    return out;
}
