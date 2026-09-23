// Textured terrain surface: the fragment half of `TerrainSurfaceMaterial`
// (see `surface_material.rs`).
//
// Every terrain pixel reads the material map around it (the same bilinear
// footprint `TerrainData::sample_height` uses), keeps the four strongest
// material slots, sharpens their blend by height, samples each slot's layer
// of the terrain texture arrays (planar on flat ground, triplanar on slopes)
// and hands the result to Bevy's standard PBR lighting, so shadows, fog,
// atmosphere and clustered decals behave exactly as on a StandardMaterial.
//
// Bindings 100 to 106 sit after the base StandardMaterial's in the material
// bind group. Their Rust side is `TerrainSurfaceExtension`; the layouts of
// `TerrainSlotRecord` and `TerrainSurfaceParams` below are checked against
// the Rust packing by the unit tests there, so change both together.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    pbr_types,
    mesh_view_bindings::view,
    decal::clustered::apply_decals,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions::visibility_range_dither
#endif

// How one material slot is drawn. 256 of them, indexed by slot id.
struct TerrainSlotRecord {
    // Multiplier on the layer's albedo (the slot's tint), or the flat colour
    // when `layer` is negative. Linear RGB.
    albedo: vec3<f32>,
    // Texture-array layer, -1 to draw the slot flat.
    layer: i32,
    // Texture repeats per metre of ground (1 / the slot's tiling).
    repeats_per_metre: f32,
    // Perceptual roughness of a flat slot.
    roughness: f32,
    // Multiplier on the ORM roughness channel of a textured slot.
    roughness_scale: f32,
    metallic: f32,
    // Multiplier that brings the layer's mean albedo luminance to 0.5, so
    // every layer's height proxy centres on the same value.
    height_scale: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

// Where the material map lies in the world, matching `sample_height`:
// u = (world_x - world_origin.x) / world_extent.x, texel = u * (width - 1).
struct TerrainSurfaceParams {
    world_origin: vec2<f32>,
    world_extent: vec2<f32>,
    cache_size: vec2<u32>,
    // Bit 0: the texture arrays are bound. Clear, the material draws the
    // vertex colours as the StandardMaterial would.
    flags: u32,
    // How far, in blended weight plus height, a slot may trail the leading
    // one and still show: smaller is a sharper, more height-driven edge.
    blend_depth: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var terrain_albedo: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var terrain_normal: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var terrain_orm: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var terrain_sampler: sampler;
// RGBA8 unorm, one texel per raster cell: [id_a, id_b, blend_b, 0] / 255.
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var terrain_material_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var<storage, read> terrain_slots: array<TerrainSlotRecord, 256>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var<uniform> terrain_params: TerrainSurfaceParams;

const TERRAIN_FLAG_TEXTURED: u32 = 1u;
// "No material": an unused id_b, or a cell nothing has painted.
const SLOT_NONE: u32 = 255u;
// What a point with no material around it draws, as the CPU mesher does.
const GRASS_SLOT: u32 = 0u;
// Distinct slots four two-material cells plus one brick material can name;
// the length of the `SlotCandidates` arrays.
const MAX_CANDIDATES: u32 = 9u;
// Slots blended per pixel.
const MAX_LAYERS: u32 = 4u;
// Exponent on the geometric normal's components that narrows the triplanar
// blend zone, and the weight under which a projection is skipped: on
// ground flatter than about 20 degrees only the top-down projection is left.
const TRIPLANAR_SHARPNESS: f32 = 4.0;
const TRIPLANAR_MIN_WEIGHT: f32 = 0.02;
// The floor Bevy's lighting clamps perceptual roughness to anyway.
const MIN_PERCEPTUAL_ROUGHNESS: f32 = 0.089;
const LUMINANCE: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

// ---------------------------------------------------------------------------
// Material map
// ---------------------------------------------------------------------------

struct SlotCandidates {
    slots: array<u32, 9>,
    weights: array<f32, 9>,
    count: u32,
}

fn add_candidate(candidates: ptr<function, SlotCandidates>, slot: u32, weight: f32) {
    if slot >= SLOT_NONE || !(weight > 0.0) {
        return;
    }
    let count = (*candidates).count;
    for (var i = 0u; i < count; i += 1u) {
        if (*candidates).slots[i] == slot {
            (*candidates).weights[i] += weight;
            return;
        }
    }
    if count < MAX_CANDIDATES {
        (*candidates).slots[count] = slot;
        (*candidates).weights[count] = weight;
        (*candidates).count = count + 1u;
    }
}

// One material-map cell at `texel`, weighted `weight`. Mirrors
// `material_cell_weights`: an unused or zero-weight id_b is skipped, and a
// cell whose id_a is "no material" gives id_b the whole weight.
fn add_material_cell(candidates: ptr<function, SlotCandidates>, texel: vec2<i32>, weight: f32) {
    if !(weight > 0.0) {
        return;
    }
    let cell = vec4<u32>(round(textureLoad(terrain_material_map, texel, 0) * 255.0));
    let a_none = cell.x == SLOT_NONE;
    let b_none = cell.y == SLOT_NONE || cell.z == 0u || cell.y == cell.x;
    if a_none && b_none {
        return;
    }
    if a_none {
        add_candidate(candidates, cell.y, weight);
        return;
    }
    if b_none {
        add_candidate(candidates, cell.x, weight);
        return;
    }
    let blend_b = f32(cell.z) / 255.0;
    add_candidate(candidates, cell.x, weight * (1.0 - blend_b));
    add_candidate(candidates, cell.y, weight * blend_b);
}

// The bilinear slot weights of the material map at `world_xz`, scaled by
// `weight`: the same four cells and fractions `material_weights_at_uv`
// reads on the CPU.
fn add_material_map(candidates: ptr<function, SlotCandidates>, world_xz: vec2<f32>, weight: f32) {
    let size = terrain_params.cache_size;
    if size.x == 0u || size.y == 0u || !(weight > 0.0) {
        return;
    }
    let last = size - vec2<u32>(1u);
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
    add_material_cell(candidates, lo_i, weight * (1.0 - f.x) * (1.0 - f.y));
    add_material_cell(candidates, vec2<i32>(hi_i.x, lo_i.y), weight * f.x * (1.0 - f.y));
    add_material_cell(candidates, vec2<i32>(lo_i.x, hi_i.y), weight * (1.0 - f.x) * f.y);
    add_material_cell(candidates, hi_i, weight * f.x * f.y);
}

// ---------------------------------------------------------------------------
// Sampling one slot
// ---------------------------------------------------------------------------

// The pixel's world position, geometric normal, triplanar weights (for the
// planes facing X, Y and Z) and world-position derivatives. Every texture
// read is a textureSampleGrad off these derivatives, taken once in uniform
// control flow, because the reads sit in per-pixel branches and loops where
// implicit derivatives are not allowed.
struct SurfaceFrame {
    position: vec3<f32>,
    normal: vec3<f32>,
    projection: vec3<f32>,
    dpdx: vec3<f32>,
    dpdy: vec3<f32>,
}

struct PlaneSample {
    albedo: vec3<f32>,
    // Tangent-space normal, unpacked to -1..1.
    normal: vec3<f32>,
    orm: vec3<f32>,
}

struct LayerSample {
    albedo: vec3<f32>,
    height: f32,
    normal: vec3<f32>,
    occlusion: f32,
    perceptual_roughness: f32,
    metallic: f32,
}

fn triplanar_weights(normal: vec3<f32>) -> vec3<f32> {
    var w = pow(abs(normal), vec3<f32>(TRIPLANAR_SHARPNESS));
    w = w / max(w.x + w.y + w.z, 1e-6);
    w = select(w, vec3<f32>(0.0), w < vec3<f32>(TRIPLANAR_MIN_WEIGHT));
    return w / max(w.x + w.y + w.z, 1e-6);
}

fn sample_plane(layer: i32, uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> PlaneSample {
    var s: PlaneSample;
    s.albedo = textureSampleGrad(terrain_albedo, terrain_sampler, uv, layer, ddx, ddy).rgb;
    s.normal = textureSampleGrad(terrain_normal, terrain_sampler, uv, layer, ddx, ddy).rgb * 2.0 - 1.0;
    s.orm = textureSampleGrad(terrain_orm, terrain_sampler, uv, layer, ddx, ddy).rgb;
    return s;
}

// Slot `slot` at this pixel. A textured slot is projected in world space:
// the top-down plane maps u to +X and v to +Z, the side planes map u along
// the wall (+Z facing X, +X facing Z) and v down (-Y), so images stand
// upright on cliffs. Each projection's normal-map sample is whiteout-blended
// onto the geometric normal in that projection's frame. The maps follow the
// OpenGL convention (texture-gen `field::normal_map`): x points along +u and
// y points up the image, which is -v because rows run down the texture.
fn sample_slot(slot: u32, frame: SurfaceFrame) -> LayerSample {
    let record = terrain_slots[slot];
    var out: LayerSample;
    if record.layer < 0 {
        out.albedo = record.albedo;
        out.height = 0.5;
        out.normal = frame.normal;
        out.occlusion = 1.0;
        out.perceptual_roughness = max(record.roughness, MIN_PERCEPTUAL_ROUGHNESS);
        out.metallic = record.metallic;
        return out;
    }

    let scale = record.repeats_per_metre;
    let p = frame.position * scale;
    let dx = frame.dpdx * scale;
    let dy = frame.dpdy * scale;
    let n = frame.normal;
    let w = frame.projection;
    var albedo = vec3<f32>(0.0);
    var orm = vec3<f32>(0.0);
    var normal = vec3<f32>(0.0);

    if w.y > 0.0 {
        // u = +X, v = +Z; the plane's normal is Y.
        let s = sample_plane(record.layer, p.xz, dx.xz, dy.xz);
        albedo += s.albedo * w.y;
        orm += s.orm * w.y;
        normal += vec3<f32>(s.normal.x + n.x, abs(s.normal.z) * n.y, n.z - s.normal.y) * w.y;
    }
    if w.x > 0.0 {
        // u = +Z, v = -Y; the plane's normal is X.
        let s = sample_plane(record.layer, vec2<f32>(p.z, -p.y), vec2<f32>(dx.z, -dx.y), vec2<f32>(dy.z, -dy.y));
        albedo += s.albedo * w.x;
        orm += s.orm * w.x;
        normal += vec3<f32>(abs(s.normal.z) * n.x, n.y + s.normal.y, s.normal.x + n.z) * w.x;
    }
    if w.z > 0.0 {
        // u = +X, v = -Y; the plane's normal is Z.
        let s = sample_plane(record.layer, vec2<f32>(p.x, -p.y), vec2<f32>(dx.x, -dx.y), vec2<f32>(dy.x, -dy.y));
        albedo += s.albedo * w.z;
        orm += s.orm * w.z;
        normal += vec3<f32>(s.normal.x + n.x, n.y + s.normal.y, abs(s.normal.z) * n.z) * w.z;
    }

    out.albedo = albedo * record.albedo;
    // The raw texture's luminance, before the tint, so a dark tint does not
    // sink a slot under its neighbours.
    out.height = saturate(dot(albedo, LUMINANCE) * record.height_scale);
    out.normal = select(n, normalize(normal), dot(normal, normal) > 1e-8);
    out.occlusion = orm.r;
    out.perceptual_roughness = clamp(orm.g * record.roughness_scale, MIN_PERCEPTUAL_ROUGHNESS, 1.0);
    out.metallic = record.metallic;
    return out;
}

// ---------------------------------------------------------------------------
// The blended surface
// ---------------------------------------------------------------------------

struct SurfaceResult {
    base_color: vec3<f32>,
    perceptual_roughness: f32,
    metallic: f32,
    occlusion: f32,
    normal: vec3<f32>,
}

// The material-map slots at this pixel (sharing the weight left over by a
// brick material, `brick_weight` of `brick_slot`), cut to the strongest
// four, sampled and height-blended.
fn terrain_surface(frame: SurfaceFrame, brick_slot: u32, brick_weight: f32) -> SurfaceResult {
    var candidates: SlotCandidates;
    add_material_map(&candidates, frame.position.xz, 1.0 - brick_weight);
    add_candidate(&candidates, brick_slot, brick_weight);
    if candidates.count == 0u {
        add_candidate(&candidates, GRASS_SLOT, 1.0);
    }

    // The strongest four, strongest first; ties keep the earlier candidate.
    var slots = array<u32, 4>(SLOT_NONE, SLOT_NONE, SLOT_NONE, SLOT_NONE);
    var weights = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    var layers = 0u;
    var total = 0.0;
    for (var k = 0u; k < MAX_LAYERS; k += 1u) {
        var best = MAX_CANDIDATES;
        var best_weight = 0.0;
        for (var i = 0u; i < candidates.count; i += 1u) {
            if candidates.weights[i] > best_weight {
                best = i;
                best_weight = candidates.weights[i];
            }
        }
        if best == MAX_CANDIDATES {
            break;
        }
        slots[k] = candidates.slots[best];
        weights[k] = best_weight;
        candidates.weights[best] = 0.0;
        total += best_weight;
        layers = k + 1u;
    }

    // Height blend: a slot shows where its weight plus its height comes
    // within `blend_depth` of the leader's, so bright stones of one material
    // poke through the other along a boundary instead of the two fading
    // evenly. A slot alone in its cell is untouched.
    var samples: array<LayerSample, 4>;
    var peak = -1.0e9;
    for (var k = 0u; k < layers; k += 1u) {
        weights[k] = weights[k] / max(total, 1e-6);
        samples[k] = sample_slot(slots[k], frame);
        peak = max(peak, weights[k] + samples[k].height);
    }
    let threshold = peak - max(terrain_params.blend_depth, 1e-4);
    var blend = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    var blend_total = 0.0;
    for (var k = 0u; k < layers; k += 1u) {
        blend[k] = max(weights[k] + samples[k].height - threshold, 0.0);
        blend_total += blend[k];
    }

    var result: SurfaceResult;
    var normal = vec3<f32>(0.0);
    for (var k = 0u; k < layers; k += 1u) {
        let b = blend[k] / max(blend_total, 1e-6);
        result.base_color += samples[k].albedo * b;
        result.perceptual_roughness += samples[k].perceptual_roughness * b;
        result.metallic += samples[k].metallic * b;
        result.occlusion += samples[k].occlusion * b;
        normal += samples[k].normal * b;
    }
    result.normal = select(frame.normal, normalize(normal), dot(normal, normal) > 1e-8);
    return result;
}

@fragment
fn fragment(
    vertex_output: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var in = vertex_output;

    // Taken first, while control flow is still uniform (see SurfaceFrame).
    let dpdx_world = dpdx(in.world_position.xyz);
    let dpdy_world = dpdy(in.world_position.xyz);

#ifdef VISIBILITY_RANGE_DITHER
    visibility_range_dither(in.position, in.visibility_range_dither);
#endif

    // The StandardMaterial inputs: vertex colour, view vector, geometric
    // normal, SSAO and the base material's flags.
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    if (terrain_params.flags & TERRAIN_FLAG_TEXTURED) != 0u {
        // The vertex colour's alpha is the baked AO, slope and macro shade
        // alone (its RGB is the vertex-colour fallback's swatch times it).
        var shade = 1.0;
#ifdef VERTEX_COLORS
        shade = in.color.a;
#endif

        // Volumetric chunks carry their brick material in UV_1 as
        // [slot + 1, 1], and [0, 0] where the material map applies (see
        // `brick_material_uv`). The mesher gives every triangle at most one
        // brick slot, so `x / y` is that slot + 1 across it; a ratio off a
        // whole number is ignored rather than rounded to whatever slot lies
        // nearest, a backstop that keeps any leftover sweep to a thin band.
        var brick_slot = SLOT_NONE;
        var brick_weight = 0.0;
#ifdef VERTEX_UVS_B
        let brick = in.uv_b;
        if brick.y > 0.001 {
            let ratio = brick.x / brick.y;
            let code = round(ratio);
            if code >= 1.0 && code <= 255.0 && abs(ratio - code) < 0.01 {
                brick_slot = u32(code) - 1u;
                brick_weight = saturate(brick.y);
            }
        }
#endif

        var frame: SurfaceFrame;
        frame.position = in.world_position.xyz;
        frame.normal = normalize(pbr_input.world_normal);
        frame.projection = triplanar_weights(frame.normal);
        // The view's mip bias (TAA, upscalers) as a derivative scale, the
        // explicit-gradient form of the bias StandardMaterial samples with.
        let gradient_scale = exp2(view.mip_bias);
        frame.dpdx = dpdx_world * gradient_scale;
        frame.dpdy = dpdy_world * gradient_scale;

        let surface = terrain_surface(frame, brick_slot, brick_weight);
        pbr_input.material.base_color = vec4<f32>(surface.base_color * shade, 1.0);
        pbr_input.material.perceptual_roughness = surface.perceptual_roughness;
        pbr_input.material.metallic = surface.metallic;
        // Multiplied in, so screen-space AO the camera adds still applies.
        pbr_input.diffuse_occlusion *= surface.occlusion;
        pbr_input.N = surface.normal;
        pbr_input.clearcoat_N = surface.normal;
    }

    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    apply_decals(&pbr_input);

#ifdef PREPASS_PIPELINE
    // Deferred: the gbuffer takes the inputs, lighting runs later.
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    if (pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    // Fog, and in-shader tonemapping for cameras without HDR.
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

    return out;
}
