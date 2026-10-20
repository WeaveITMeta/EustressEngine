// Water surface: the fragment half of `WaterSurfaceMaterial` (see `water.rs`).
//
// Every water pixel measures how deep the water is under it: its own world
// height less the terrain height the terrain height texture holds there
// (read with the bilinear footprint `TerrainData::sample_height` uses).
// Shallow water is a clear tint with a band of foam along the shore; the
// colour and opacity deepen to the deep colour with depth; pixels over ground
// that stands above the water are dropped, so the waterline follows the
// ground between raster cells. Detail ripples perturb the normal: they drift
// slowly on still water and run along the mesh's flow (UV_1, world XZ metres
// per second) on a river, blended over two phases so a long run of time never
// stretches them; still water's drift is carried the same way, so nothing
// moves by an offset that grows with the clock (which wraps every hour). The
// result goes through Bevy's standard PBR lighting, fog included, alpha
// blended.
//
// Bindings 100 and 101 sit after the base StandardMaterial's in the material
// bind group. Their Rust side is `WaterSurfaceExtension`; the layout of
// `WaterSurfaceParams` below is checked against the Rust packing by the unit
// tests there, so change both together.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::globals,
}

#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions::visibility_range_dither
#endif

#ifdef OIT_ENABLED
#import bevy_core_pipeline::oit::oit_draw
#endif

struct WaterSurfaceParams {
    // Linear RGB of water at the shore, and its opacity there.
    shallow_color: vec4<f32>,
    // Linear RGB of deep water, and its opacity.
    deep_color: vec4<f32>,
    // Linear RGB of the shore foam, and how strongly it covers the water.
    foam_color: vec4<f32>,
    // Where the height texture lies in the world, matching `sample_height`:
    // u = (world_x - terrain_origin.x) / terrain_extent.x, texel = u * (width - 1).
    terrain_origin: vec2<f32>,
    terrain_extent: vec2<f32>,
    height_size: vec2<u32>,
    // Bit 0: the height texture is bound. Clear, every pixel is
    // `fallback_depth` deep.
    flags: u32,
    fallback_depth: f32,
    // Depth, metres, at which the deep colour and opacity are reached.
    deep_depth: f32,
    // Depth, metres, by which the shore foam has faded out.
    foam_depth: f32,
    // Ripple cells per metre of the coarser ripple octave.
    detail_repeats: f32,
    // Slope the ripples give the normal.
    normal_strength: f32,
    // Metres per second still water's ripples drift.
    drift_speed: f32,
    // Seconds a flowing pattern runs before it is blended into a fresh one.
    flow_period: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var water_terrain_height: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> water_params: WaterSurfaceParams;

const WATER_FLAG_TERRAIN_HEIGHT: u32 = 1u;
// The last few centimetres of depth fade to nothing, so the waterline does
// not alias against the ground.
const SHORE_FADE: f32 = 0.05;
// Foam is rougher than open water.
const FOAM_ROUGHNESS: f32 = 0.6;
// The finer ripple octave: this many times the coarser one's frequency, at
// this share of its slope.
const FINE_OCTAVE_SCALE: f32 = 2.3;
const FINE_OCTAVE_WEIGHT: f32 = 0.5;
const TAU: f32 = 6.28318530718;

// ---------------------------------------------------------------------------
// Terrain height
// ---------------------------------------------------------------------------

// World height of the terrain at `world_xz`: the four texels around it,
// blended as `TerrainData::sample_height` blends its cells.
fn terrain_height_at(world_xz: vec2<f32>) -> f32 {
    let size = water_params.height_size;
    let last = size - vec2<u32>(1u);
    let uv = clamp(
        (world_xz - water_params.terrain_origin) / water_params.terrain_extent,
        vec2<f32>(0.0),
        vec2<f32>(1.0),
    );
    let texel = uv * vec2<f32>(last);
    let lo = min(vec2<u32>(floor(texel)), last);
    let hi = min(lo + vec2<u32>(1u), last);
    let f = texel - vec2<f32>(lo);
    let lo_i = vec2<i32>(lo);
    let hi_i = vec2<i32>(hi);
    let h00 = textureLoad(water_terrain_height, lo_i, 0).r;
    let h10 = textureLoad(water_terrain_height, vec2<i32>(hi_i.x, lo_i.y), 0).r;
    let h01 = textureLoad(water_terrain_height, vec2<i32>(lo_i.x, hi_i.y), 0).r;
    let h11 = textureLoad(water_terrain_height, hi_i, 0).r;
    let row0 = h00 + (h10 - h00) * f.x;
    let row1 = h01 + (h11 - h01) * f.x;
    return row0 + (row1 - row0) * f.y;
}

// ---------------------------------------------------------------------------
// Ripples
// ---------------------------------------------------------------------------

// An integer hash (PCG), so the pattern holds up kilometres from the origin
// where a sine hash loses its precision.
fn hash_u32(x: u32) -> u32 {
    let state = x * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// The unit gradient at lattice point `cell`.
fn lattice_gradient(cell: vec2<i32>) -> vec2<f32> {
    // WGSL will not mix `^` with `*` unparenthesized.
    let h = hash_u32((bitcast<u32>(cell.x) * 1597334677u) ^ hash_u32(bitcast<u32>(cell.y)));
    let angle = f32(h) * (TAU / 4294967296.0);
    return vec2<f32>(cos(angle), sin(angle));
}

// Gradient noise at `p` with its analytic derivative: (value, d/dx, d/dy).
fn gradient_noise(p: vec2<f32>) -> vec3<f32> {
    let i = floor(p);
    let f = p - i;
    // Quintic fade and its derivative.
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let c = vec2<i32>(i);
    let ga = lattice_gradient(c);
    let gb = lattice_gradient(c + vec2<i32>(1, 0));
    let gc = lattice_gradient(c + vec2<i32>(0, 1));
    let gd = lattice_gradient(c + vec2<i32>(1, 1));
    let va = dot(ga, f);
    let vb = dot(gb, f - vec2<f32>(1.0, 0.0));
    let vc = dot(gc, f - vec2<f32>(0.0, 1.0));
    let vd = dot(gd, f - vec2<f32>(1.0, 1.0));
    let value = va + u.x * (vb - va) + u.y * (vc - va) + u.x * u.y * (va - vb - vc + vd);
    let derivative = ga + u.x * (gb - ga) + u.y * (gc - ga) + u.x * u.y * (ga - gb - gc + gd)
        + du * (u.yx * (va - vb - vc + vd) + vec2<f32>(vb, vc) - va);
    return vec3<f32>(value, derivative);
}

// Slope (dh/dx, dh/dz) of the ripples at `world_xz`, the coarse octave
// carried `coarse_offset` metres and the fine one `fine_offset` metres.
fn ripple_slope(world_xz: vec2<f32>, coarse_offset: vec2<f32>, fine_offset: vec2<f32>) -> vec2<f32> {
    let repeats = water_params.detail_repeats;
    let coarse = gradient_noise((world_xz - coarse_offset) * repeats).yz;
    let fine = gradient_noise((world_xz - fine_offset) * (repeats * FINE_OCTAVE_SCALE) + vec2<f32>(17.0, 31.0)).yz;
    return (coarse + fine * FINE_OCTAVE_WEIGHT) * water_params.normal_strength;
}

// Seconds one copy of a moving pattern runs before it is blended out.
fn flow_period() -> f32 {
    return max(water_params.flow_period, 0.1);
}

// Where the two copies of a moving pattern are through `flow_period()`, and
// the first copy's weight: (phase_a, phase_b, weight_a). The weight falls to
// zero as a copy's phase wraps, so its jump back is never seen.
fn flow_phases(time: f32) -> vec3<f32> {
    let period = flow_period();
    let phase_a = fract(time / period);
    let phase_b = fract(time / period + 0.5);
    return vec3<f32>(phase_a, phase_b, 1.0 - abs(1.0 - 2.0 * phase_a));
}

// The ripples' slope on water flowing at `flow` (world XZ m/s). Two copies
// of the pattern run half a period apart; each is carried for one period,
// then jumps back while its weight is zero. Both copies are carried by the
// flow plus each octave's own drift (the octaves drift in different
// directions, so still water shimmers rather than slides), and never by an
// offset that grows with the clock: Bevy's `globals.time` wraps every 3600 s,
// and such an offset would jump at the wrap.
fn flowing_slope(world_xz: vec2<f32>, flow: vec2<f32>, time: f32) -> vec2<f32> {
    let period = flow_period();
    let phases = flow_phases(time);
    let coarse_v = flow - water_params.drift_speed * vec2<f32>(1.0, 0.6);
    let fine_v = flow - water_params.drift_speed * vec2<f32>(-0.8, 1.1) / FINE_OCTAVE_SCALE;
    let t_a = phases.x * period;
    let t_b = phases.y * period;
    let slope_a = ripple_slope(world_xz, coarse_v * t_a, fine_v * t_a);
    let slope_b = ripple_slope(world_xz, coarse_v * t_b, fine_v * t_b);
    return slope_a * phases.z + slope_b * (1.0 - phases.z);
}

@fragment
fn fragment(
    vertex_output: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var in = vertex_output;

#ifdef VISIBILITY_RANGE_DITHER
    visibility_range_dither(in.position, in.visibility_range_dither);
#endif

    // The StandardMaterial inputs: view vector, geometric normal, the base
    // material's roughness, reflectance and flags.
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    var depth = water_params.fallback_depth;
    if (water_params.flags & WATER_FLAG_TERRAIN_HEIGHT) != 0u {
        depth = in.world_position.y - terrain_height_at(in.world_position.xz);
    }
    // Ground above the water: no water here.
    if !(depth > 0.0) {
        discard;
    }

    var flow = vec2<f32>(0.0);
#ifdef VERTEX_UVS_B
    flow = in.uv_b;
#endif
    let time = globals.time;
    let slope = flowing_slope(in.world_position.xz, flow, time);
    let geometric = normalize(pbr_input.world_normal);
    let normal = normalize(geometric + vec3<f32>(-slope.x, 0.0, -slope.y));

    // Colour and opacity by depth.
    let deep_t = smoothstep(0.0, max(water_params.deep_depth, 1e-3), depth);
    var color = mix(water_params.shallow_color, water_params.deep_color, deep_t);

    // Shore foam: a band along the waterline, broken up by a finer pattern
    // that runs with the flow, blended over the same two phases as the
    // ripples so the clock's wrap moves it nowhere.
    let band = 1.0 - smoothstep(0.0, max(water_params.foam_depth, 1e-3), depth);
    let period = flow_period();
    let phases = flow_phases(time);
    let foam_scale = water_params.detail_repeats * 3.0;
    let breakup_a = gradient_noise((in.world_position.xz - flow * (phases.x * period)) * foam_scale).x;
    let breakup_b = gradient_noise((in.world_position.xz - flow * (phases.y * period)) * foam_scale).x;
    let breakup = saturate(0.55 + (breakup_a * phases.z + breakup_b * (1.0 - phases.z)) * 1.6);
    let foam = saturate(band * breakup * water_params.foam_color.a);
    color = vec4<f32>(mix(color.rgb, water_params.foam_color.rgb, foam), max(color.a, foam));
    color.a *= smoothstep(0.0, SHORE_FADE, depth);

    pbr_input.material.base_color = color;
    pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, FOAM_ROUGHNESS, foam);
    pbr_input.N = normal;
    pbr_input.clearcoat_N = normal;

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Fog, and in-shader tonemapping for cameras without HDR.
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);

#ifdef OIT_ENABLED
    // With order-independent transparency the fragment is only drawn by the
    // resolve pass.
    oit_draw(in.position, out.color);
    discard;
#endif

    return out;
}
