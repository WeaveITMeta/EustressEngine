// Volumetric clouds: the fragment half of `CloudMaterial` (see
// `volumetric_clouds.rs`).
//
// Drawn on the inside of a sphere around the camera. Each pixel marches its
// view ray through the cloud layer, a shell over the curved planet, stopping
// at scene geometry when a depth prepass is available. Density comes from
// two tiling noise volumes whose values are quantiles, so `coverage` is the
// fraction of the layer that is cloud. Each sample is lit by the key light
// (the sun, or the moon at night) through a short march toward it, and by
// sky light from above and ground light from below. Lights arrive in lux and
// sky light in cd/m^2, already through the atmosphere, and the result is
// scaled by the camera's exposure like every other light in the scene.
//
// Output is premultiplied: the light the clouds add, and in alpha how much
// of the sky behind them they hide. Distant cloud fades into the sky the
// atmosphere drew behind it.
//
// The layout of `CloudParams` is checked against its Rust packing by the
// unit tests in `volumetric_clouds.rs`; change both together.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
    utils::interleaved_gradient_noise,
}

#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

const CLOUD_PI: f32 = 3.14159265358979;

struct CloudParams {
    // x: base altitude, y: top altitude, z: planet radius (metres),
    // w: march limit (metres).
    layer: vec4<f32>,
    // x: coverage, y: extinction per metre at full density,
    // z: layer type (0 cumulus, 1 cirrus, 2 stratus, 3 cumulonimbus,
    // 4 altocumulus), w: detail erosion.
    shape: vec4<f32>,
    // x: 1 / shape tile, y: 1 / detail tile, z: 1 / weather tile,
    // w: weather contrast.
    scales: vec4<f32>,
    // xy: base-shape wind offset, zw: weather wind offset (metres).
    wind: vec4<f32>,
    // xyz: detail wind offset (metres). w: fade distance (metres).
    detail_wind: vec4<f32>,
    // xy: coverage bias direction, z: strength, w: mode (0 toward a
    // direction, 1 the horizon, 2 overhead).
    bias: vec4<f32>,
    // xyz: toward the key light. w: 1 while there is one.
    light_direction: vec4<f32>,
    // rgb: the key light at the cloud base, lux.
    light_base: vec4<f32>,
    // rgb: the key light at the cloud top, lux.
    light_top: vec4<f32>,
    // rgb: sky radiance on the cloud tops, cd/m^2.
    ambient_top: vec4<f32>,
    // rgb: light from below on the cloud bases, cd/m^2.
    ambient_bottom: vec4<f32>,
    // rgb: cloud tint. a: diffusion strength.
    albedo: vec4<f32>,
    // x: most primary steps, y: light steps, z: powder strength,
    // w: forward scattering g.
    quality: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> clouds: CloudParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var shape_noise: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var shape_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var detail_noise: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var detail_sampler: sampler;

fn remap01(v: f32, lo: f32, hi: f32) -> f32 {
    return clamp((v - lo) / max(hi - lo, 1e-5), 0.0, 1.0);
}

// Distances along a ray to the sphere `shell` metres above the ground, for a
// ray starting `altitude` metres up and climbing with sine `mu`: (near, far),
// both negative when it misses. `c` is formed from the altitudes so the
// planet's radius never cancels against itself in f32.
fn ray_shell(altitude: f32, mu: f32, shell: f32, planet: f32) -> vec2<f32> {
    let b = (planet + altitude) * mu;
    let c = (altitude - shell) * (2.0 * planet + altitude + shell);
    let disc = b * b - c;
    if (disc < 0.0) {
        return vec2<f32>(-1.0, -1.0);
    }
    let s = sqrt(disc);
    // `q` takes the sign that adds rather than cancels.
    let q = select(-b + s, -b - s, b > 0.0);
    if (abs(q) < 1e-3) {
        return vec2<f32>(0.0, 0.0);
    }
    let t = c / q;
    return vec2<f32>(min(t, q), max(t, q));
}

// Altitude above the curved ground `t` metres along the ray. The planet's
// curvature term is written so it never subtracts two huge numbers.
fn altitude_along(altitude: f32, mu: f32, t: f32, planet: f32) -> f32 {
    let along = planet + altitude + t * mu;
    let x = t * t * (1.0 - mu * mu);
    return altitude + t * mu + x / (sqrt(along * along + x) + along);
}

// How cloud is distributed through the layer's depth, by layer type.
fn height_profile(h: f32) -> f32 {
    let kind = clouds.shape.z;
    if (kind < 0.5) {
        // Cumulus: flat bases, cores rising into rounded tops.
        return smoothstep(0.0, 0.12, h) * (1.0 - smoothstep(0.45, 1.0, h));
    } else if (kind < 1.5) {
        // Cirrus: a thin sheet.
        return smoothstep(0.3, 0.5, h) * (1.0 - smoothstep(0.5, 0.7, h));
    } else if (kind < 2.5) {
        // Stratus: a low, flat deck.
        return smoothstep(0.0, 0.1, h) * (1.0 - smoothstep(0.25, 0.45, h));
    } else if (kind < 3.5) {
        // Cumulonimbus: towers to the top of the layer.
        return smoothstep(0.0, 0.06, h) * (1.0 - smoothstep(0.85, 1.0, h));
    }
    // Altocumulus: a mid-level band of small puffs.
    return smoothstep(0.3, 0.42, h) * (1.0 - smoothstep(0.55, 0.72, h));
}

// The authored coverage mode: more cloud toward a direction, around the
// horizon or overhead.
fn coverage_bias(pos: vec3<f32>) -> f32 {
    let strength = clouds.bias.z;
    if (strength <= 0.0) {
        return 1.0;
    }
    let offset = pos.xz - view.world_position.xz;
    let dist = length(offset);
    var weight = 0.0;
    if (clouds.bias.w < 0.5) {
        weight = dot(offset / max(dist, 1.0), clouds.bias.xy);
    } else if (clouds.bias.w < 1.5) {
        weight = clamp(dist / 20000.0, 0.0, 1.0) * 2.0 - 1.0;
    } else {
        weight = 1.0 - clamp(dist / 12000.0, 0.0, 1.0) * 2.0;
    }
    return clamp(1.0 + weight * strength, 0.0, 2.0);
}

// Coverage at `pos`: the authored value, moved by the drifting weather and
// the coverage mode.
fn coverage_at(pos: vec3<f32>) -> f32 {
    let uvw = vec3<f32>(
        (pos.x + clouds.wind.z) * clouds.scales.z,
        0.37,
        (pos.z + clouds.wind.w) * clouds.scales.z
    );
    let weather = textureSampleLevel(shape_noise, shape_sampler, uvw, 0.0).g;
    return clamp(clouds.shape.x * coverage_bias(pos) + (weather - 0.5) * clouds.scales.w, 0.0, 1.0);
}

// Cloud density, 0..1, at `pos`, `h` of the way up the layer.
fn cloud_density(pos: vec3<f32>, h: f32, coverage: f32, detailed: bool) -> f32 {
    let profile = height_profile(h);
    if (profile <= 0.0 || coverage <= 0.001) {
        return 0.0;
    }
    var p = vec3<f32>(pos.x + clouds.wind.x, pos.y, pos.z + clouds.wind.y) * clouds.scales.x;
    if (clouds.shape.z > 0.5 && clouds.shape.z < 1.5) {
        // Cirrus is drawn out into streaks.
        p = p * vec3<f32>(0.3, 2.0, 1.4);
    }
    let base = textureSampleLevel(shape_noise, shape_sampler, p, 0.0).r;
    var d = remap01(base * profile, 1.0 - coverage, 1.0);
    if (detailed && d > 0.0) {
        let q = (pos + clouds.detail_wind.xyz) * clouds.scales.y;
        let detail = textureSampleLevel(detail_noise, detail_sampler, q, 0.0).r;
        // Torn, wispy bases; billowing tops.
        let erosion = mix(1.0 - detail, detail, clamp(h * 3.0, 0.0, 1.0)) * clouds.shape.w;
        d = remap01(d, erosion, 1.0);
    }
    return d;
}

fn henyey_greenstein(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    let denom = max(1.0 + g2 - 2.0 * g * cos_theta, 1e-4);
    return (1.0 - g2) / (4.0 * CLOUD_PI * denom * sqrt(denom));
}

// A strong forward lobe (the silver lining around a cloud in front of the
// sun) and a weaker backward one (the lit faces seen with the sun behind).
fn cloud_phase(cos_theta: f32, g: f32) -> f32 {
    return mix(henyey_greenstein(cos_theta, -0.25), henyey_greenstein(cos_theta, g), 0.72);
}

// Optical depth from `pos` toward the key light, through the base shape
// only: steps that grow as they go, covering the layer in a handful.
fn light_optical_depth(pos: vec3<f32>, altitude: f32, coverage: f32) -> f32 {
    let base = clouds.layer.x;
    let thickness = max(clouds.layer.y - clouds.layer.x, 1.0);
    let to_light = clouds.light_direction.xyz;
    var stride = thickness * 0.035;
    var travelled = 0.0;
    var depth = 0.0;
    let count = i32(clouds.quality.y);
    for (var j = 0; j < count; j = j + 1) {
        let s = travelled + stride * 0.5;
        let h = (altitude + to_light.y * s - base) / thickness;
        if (h < 0.0 || h > 1.0) {
            break;
        }
        depth = depth + cloud_density(pos + to_light * s, h, coverage, false) * stride;
        travelled = travelled + stride;
        stride = stride * 1.7;
    }
    return depth * clouds.shape.y;
}

// Distance to the scene geometry behind this pixel, or "none".
fn scene_distance(frag_coord: vec4<f32>) -> f32 {
#ifdef DEPTH_PREPASS
    let depth = prepass_depth(frag_coord, 0u);
    if (depth <= 0.0) {
        return 1.0e30;
    }
    let uv = (frag_coord.xy - view.viewport.xy) / view.viewport.zw;
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let p = view.view_from_clip * ndc;
    return length(p.xyz / p.w);
#else
    return 1.0e30;
#endif
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let ray = normalize(in.world_position.xyz - view.world_position);
    let altitude = view.world_position.y;
    let mu = ray.y;
    let base = clouds.layer.x;
    let top = clouds.layer.y;
    let planet = clouds.layer.z;

    // The stretch of the ray inside the layer.
    let hit_base = ray_shell(altitude, mu, base, planet);
    let hit_top = ray_shell(altitude, mu, top, planet);
    var t_start = 0.0;
    var t_end = 0.0;
    if (altitude < base) {
        // Below: in through the base, out through the top. A ray heading
        // down never gets there.
        if (mu <= 0.0) {
            discard;
        }
        t_start = hit_base.y;
        t_end = hit_top.y;
    } else if (altitude <= top) {
        // Inside: from here to wherever the ray leaves.
        t_end = hit_top.y;
        if (hit_base.x > 0.0) {
            t_end = min(t_end, hit_base.x);
        }
    } else {
        // Above: in through the top, out through the base or the top again.
        if (hit_top.x <= 0.0) {
            discard;
        }
        t_start = hit_top.x;
        t_end = select(hit_top.y, hit_base.x, hit_base.x > 0.0);
    }
    t_end = min(t_end, min(clouds.layer.w, scene_distance(in.position)));
    if (t_end <= t_start) {
        discard;
    }

    let span = t_end - t_start;
    let steps = clamp(ceil(span / 60.0), 16.0, clouds.quality.x);
    let dt = span / steps;
    // A fixed per-pixel offset: breaks up banding without the frame-to-frame
    // shimmer a moving one would need temporal filtering to hide.
    let jitter = interleaved_gradient_noise(in.position.xy, 0u);

    let to_light = clouds.light_direction.xyz;
    let lit = clouds.light_direction.w > 0.5;
    let cos_theta = dot(ray, to_light);
    let g = clouds.quality.w;
    // Single scattering, then two octaves of approximate multiple
    // scattering, each reaching deeper and scattering less sharply forward,
    // then diffusion through the cloud's depth.
    let phase0 = cloud_phase(cos_theta, g);
    let phase1 = cloud_phase(cos_theta, g * 0.5);
    let phase2 = cloud_phase(cos_theta, g * 0.25);
    let diffusion = clouds.albedo.a / (4.0 * CLOUD_PI);
    // Powder: thin cloud has not yet scattered much light back out, so the
    // rims and crevices of a face lit from the viewer's side read darker.
    let powder_view = clouds.quality.z * (0.5 - 0.5 * cos_theta);

    var transmittance = 1.0;
    var radiance = vec3<f32>(0.0);
    var weighted_distance = 0.0;
    var weight = 0.0;
    let count = i32(steps);
    for (var i = 0; i < count; i = i + 1) {
        let t = t_start + (f32(i) + jitter) * dt;
        let sample_altitude = altitude_along(altitude, mu, t, planet);
        let h = (sample_altitude - base) / (top - base);
        if (h < 0.0 || h > 1.0) {
            continue;
        }
        let pos = view.world_position + ray * t;
        let coverage = coverage_at(pos);
        let density = cloud_density(pos, h, coverage, true);
        if (density <= 0.0) {
            continue;
        }
        let sigma = density * clouds.shape.y;

        var key = vec3<f32>(0.0);
        if (lit) {
            let depth = light_optical_depth(pos, sample_altitude, coverage);
            let scatter = phase0 * exp(-depth)
                + 0.5 * phase1 * exp(-depth * 0.5)
                + 0.25 * phase2 * exp(-depth * 0.25)
                + diffusion * exp(-depth * 0.12);
            let powder = 1.0 - powder_view * exp(-depth * 2.0);
            key = mix(clouds.light_base.rgb, clouds.light_top.rgb, h) * scatter * powder;
        }
        let ambient = mix(clouds.ambient_bottom.rgb, clouds.ambient_top.rgb, sqrt(h));
        let source = (key + ambient) * clouds.albedo.rgb * sigma;

        // Integrated exactly across the step, so a coarse step through a
        // dense core neither loses energy nor invents it.
        let step_transmittance = exp(-sigma * dt);
        radiance = radiance + transmittance * (source - source * step_transmittance) / max(sigma, 1e-7);
        let absorbed = transmittance * (1.0 - step_transmittance);
        weighted_distance = weighted_distance + absorbed * t;
        weight = weight + absorbed;
        transmittance = transmittance * step_transmittance;
        if (transmittance < 0.01) {
            transmittance = 0.0;
            break;
        }
    }
    if (weight <= 1e-5) {
        discard;
    }

    // Aerial perspective: distant cloud dissolves into the sky the
    // atmosphere has already drawn behind it.
    let mean_distance = weighted_distance / weight;
    let fade_distance = clouds.detail_wind.w;
    let fade = exp(-(mean_distance * mean_distance) / (fade_distance * fade_distance));
    var color = vec4<f32>(radiance * view.exposure * fade, (1.0 - transmittance) * fade);
#ifdef TONEMAP_IN_SHADER
    color = tone_mapping(color, view.color_grading);
#endif
    return color;
}
