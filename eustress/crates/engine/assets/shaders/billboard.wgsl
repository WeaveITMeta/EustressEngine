// ============================================================================
// Billboard shader — DEBUG MODE.
// ============================================================================
// Fragment is hardcoded to output opaque red, ignoring all bindings beyond
// the vertex stage. If red rectangles still don't appear we know the
// pipeline isn't actually being submitted to the GPU (likely cause:
// shader compile error preventing the pipeline cache from producing a
// valid pipeline state).
// ============================================================================

#import bevy_pbr::mesh_view_bindings::view

struct Billboard {
    model: mat4x4<f32>,
}

struct BillboardUv {
    uv_min: vec2<f32>,
    uv_max: vec2<f32>,
    // Depth-bias in metres along the camera-toward direction. Positive
    // pulls the quad toward the camera so it wins the depth test against
    // geometry it intersects (e.g. a mindmap label sitting on its own
    // sphere). Driven by `BillboardGui.z_index * Z_INDEX_METRES_PER_UNIT`
    // (currently 0.5 m per unit — ZIndex 1 clears a 1-stud part).
    z_bias: f32,
    _padding: f32,
}

@group(1) @binding(0)
var<uniform> billboard: Billboard;

@group(1) @binding(1)
var<uniform> billboard_uv: BillboardUv;

@group(2) @binding(0)
var billboard_texture: texture_2d<f32>;
@group(2) @binding(1)
var billboard_sampler: sampler;

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
#ifdef LOCK_ROTATION
    let vertex_position = vec4<f32>(-vertex.position.x, vertex.position.y, vertex.position.z, 1.0);
    let position = view.clip_from_world * billboard.model * vertex_position;
#else
    // Camera-facing quad math.
    //
    // The model matrix carries the billboard's per-axis scale on its
    // diagonal and its world position in the 4th column. If we let the
    // model matrix multiply our camera-aligned `world_offset` directly,
    // the per-axis scale distorts the quad when the camera is tilted —
    // wider on world-X axis than world-Z, so a 45° view-angle of a
    // (4, 1, 1)-scaled billboard turns into a trapezoid instead of a
    // camera-facing rectangle.
    //
    // Fix: extract scale and translation from the model matrix, apply
    // scale to the LOCAL vertex coordinates first (so width/height are
    // in the quad's own 2D frame), THEN rotate to camera plane, THEN
    // add translation. Result: a flat camera-facing rectangle of the
    // intended dimensions regardless of camera angle.
    //
    // WGSL matrix syntax: `m[col].row` (or `m[col][row]`). The legacy
    // `m.x.x` chain that bevy_mod_billboard used compiles on older
    // naga but is rejected by Bevy 0.18's stricter parser.
    let camera_right = normalize(vec3<f32>(
        view.clip_from_world[0].x,
        view.clip_from_world[1].x,
        view.clip_from_world[2].x,
    ));
#ifdef LOCK_Y
    let camera_up = vec3<f32>(0.0, 1.0, 0.0);
#else
    let camera_up = normalize(vec3<f32>(
        view.clip_from_world[0].y,
        view.clip_from_world[1].y,
        view.clip_from_world[2].y,
    ));
#endif

    // Extract scale (magnitude of each model column) and translation.
    // Our setup never rotates the model matrix on the camera-facing
    // path (calculate_billboard_uniform strips rotation), so column
    // magnitudes equal the scale components directly.
    let scale_x = length(billboard.model[0].xyz);
    let scale_y = length(billboard.model[1].xyz);
    let translation = billboard.model[3].xyz;

    // Local quad coords pre-scaled, then rotated to camera basis.
    let world_offset = camera_right * (vertex.position.x * scale_x)
                     + camera_up    * (vertex.position.y * scale_y);

    // Depth-bias: shift the entire quad along the camera-toward
    // direction by `z_bias` metres. This pulls the billboard
    // forward in world space so it wins the depth test against
    // closer-by geometry it's pinned to (e.g. a label sitting on
    // top of its own mindmap sphere), without bypassing the depth
    // test entirely the way `AlwaysOnTop` does. The shift is in
    // world space, so a billboard biased forward still gets
    // properly occluded by geometry that is actually closer to the
    // camera than the biased position.
    //
    // `view.world_position` is the camera's world-space location
    // (provided by `bevy_pbr::mesh_view_bindings::view`).
    let to_camera = normalize(view.world_position - translation);
    let world_pos = translation + world_offset + to_camera * billboard_uv.z_bias;
    let position = view.clip_from_world * vec4<f32>(world_pos, 1.0);
#endif

    var out: VertexOutput;
    out.position = position;
    out.uv = vertex.uv;
    return out;
}

struct Fragment {
    @location(0) uv: vec2<f32>,
};

@fragment
fn fragment(fragment: Fragment) -> @location(0) vec4<f32> {
    // Remap the quad's [0,1]×[0,1] UV onto this billboard's atlas tile.
    let atlas_uv = mix(billboard_uv.uv_min, billboard_uv.uv_max, fragment.uv);
    return textureSample(billboard_texture, billboard_sampler, atlas_uv);
}
