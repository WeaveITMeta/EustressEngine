// Particle cloud: one instanced draw per ParticleSimulation.
//
// Each instance is one particle read from a storage buffer (position in the
// simulation's domain frame, display radius in world metres, linear RGBA).
// The vertex stage expands it to a camera-facing quad in view space; the
// fragment stage turns the quad into a lit sphere and writes the sphere's
// true depth, so particles occlude each other and the scene exactly (no
// sorting, no blending).

#import bevy_pbr::mesh_view_bindings::view

struct Particle {
    position: vec3<f32>,
    radius: f32,
    color: vec4<f32>,
};

struct Cloud {
    // Domain frame to world: rotation and translation of the simulation
    // instance, times its display scale.
    world_from_local: mat4x4<f32>,
    // x: radius multiplier. y, z, w: reserved.
    params: vec4<f32>,
};

@group(1) @binding(0) var<storage, read> particles: array<Particle>;
@group(1) @binding(1) var<uniform> cloud: Cloud;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) center_view: vec3<f32>,
    @location(3) radius: f32,
};

@vertex
fn vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let corner = corners[vertex_index];
    let p = particles[instance_index];
    let radius = p.radius * cloud.params.x;
    let world_center = (cloud.world_from_local * vec4<f32>(p.position, 1.0)).xyz;
    let view_center = (view.view_from_world * vec4<f32>(world_center, 1.0)).xyz;
    let view_pos = view_center + vec3<f32>(corner * radius, 0.0);

    var out: VertexOutput;
    out.clip_position = view.clip_from_view * vec4<f32>(view_pos, 1.0);
    out.uv = corner;
    out.color = p.color;
    out.center_view = view_center;
    out.radius = radius;
    return out;
}

struct FragmentOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let r2 = dot(in.uv, in.uv);
    if (r2 > 1.0) {
        discard;
    }
    // View space: the camera looks down -z, so the visible hemisphere
    // faces +z.
    let normal = vec3<f32>(in.uv, sqrt(1.0 - r2));
    let surface = in.center_view + normal * in.radius;
    let clip = view.clip_from_view * vec4<f32>(surface, 1.0);

    let light = normalize(vec3<f32>(-0.35, 0.55, 0.75));
    let diffuse = max(dot(normal, light), 0.0);
    let half_vector = normalize(light + vec3<f32>(0.0, 0.0, 1.0));
    let specular = pow(max(dot(normal, half_vector), 0.0), 32.0);
    let rgb = in.color.rgb * (0.28 + 0.72 * diffuse) + vec3<f32>(0.22 * specular);

    var out: FragmentOutput;
    out.color = vec4<f32>(rgb, 1.0);
    out.depth = clip.z / clip.w;
    return out;
}
