// Sky dome: the fragment half of `SkyDomeMaterial` (see `sky_dome.rs`).
//
// Drawn on the inside of a sphere around the camera when bevy's atmosphere
// is not drawing the sky. Each pixel evaluates Preetham's analytic clear sky
// for two lights, the sun and the moon (each lobe already scaled on the CPU
// so its light on level ground is the scene's `sky_lux`), adds the night
// sky's own glow, the star field turned with the sky, and the sun's
// limb-darkened disc. Below the horizon it shows lit ground. Radiance is in
// cd/m^2 and is scaled by the camera's exposure like every other light.
//
// With a depth prepass the dome is drawn with no depth test and discards
// wherever the scene is; the Rust side sets that up. Over an author's
// skybox (`stars.y` = 0) only the sun's disc is drawn; over bevy's
// atmosphere (`stars.y` = 2) the stars and the sun's disc are added.
//
// The layout of `SkyDomeParams` is checked against its Rust packing by the
// unit tests in `sky_dome.rs`; change both together. `lobe()` has a CPU twin,
// `lobe_radiance`, which the tests exercise.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}

#import bevy_pbr::mesh_view_bindings::globals

#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

// Solar limb darkening, as `LIMB_DARKENING` in `sky_dome.rs`.
const SKY_LIMB_DARKENING: f32 = 0.6;

struct SkyDomeParams {
    // xyz: toward the sun. w: the disc's angular radius, radians.
    sun: vec4<f32>,
    // rgb: the disc's radiance, cd/m^2.
    sun_disc: vec4<f32>,
    // The sun's lobe. x: zenith luminance over F_Y(0, theta_s), cd/m^2.
    // y, z: zenith chromaticity over F_x(0, theta_s) and F_y(0, theta_s).
    // w: 1 when the lobe lights anything.
    sun_zenith: vec4<f32>,
    // rgb: the sun's lobe's tint, luminance 1.
    sun_tint: vec4<f32>,
    // xyz: toward the moon.
    moon: vec4<f32>,
    moon_zenith: vec4<f32>,
    moon_tint: vec4<f32>,
    // A, B, C, D of the Perez luminance distribution.
    perez_lum: vec4<f32>,
    // A, B, C, D of the x chromaticity distribution.
    perez_x: vec4<f32>,
    // A, B, C, D of the y chromaticity distribution.
    perez_y: vec4<f32>,
    // E of the luminance, x and y distributions.
    perez_e: vec4<f32>,
    // rgb: the ground below the horizon, cd/m^2.
    ground: vec4<f32>,
    // rgb: the night sky's own glow overhead, cd/m^2. w: horizon brightening.
    night_sky: vec4<f32>,
    // x: the star field's gain. y: 1 draws the sky, 0 only the sun's disc,
    // 2 only the stars.
    stars: vec4<f32>,
    // Columns of the rotation from the world into the star map.
    star_x: vec4<f32>,
    star_y: vec4<f32>,
    star_z: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> dome: SkyDomeParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var star_map: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var star_sampler: sampler;

// The Perez distribution F(theta, gamma), the horizon held just above
// cos(theta) = 0 where the first factor is finite.
fn perez(c: vec4<f32>, e: f32, cos_theta: f32, gamma: f32) -> f32 {
    let ct = max(cos_theta, 0.01);
    let cg = cos(gamma);
    return (1.0 + c.x * exp(c.y / ct)) * (1.0 + c.z * exp(c.w * gamma) + e * cg * cg);
}

// Linear RGB, cd/m^2, of one lobe of the sky toward `ray`.
fn lobe(ray: vec3<f32>, toward: vec3<f32>, zenith: vec4<f32>, tint: vec3<f32>) -> vec3<f32> {
    if (zenith.w <= 0.0) {
        return vec3<f32>(0.0);
    }
    let cos_theta = ray.y;
    let gamma = acos(clamp(dot(ray, toward), -1.0, 1.0));
    let lum = zenith.x * perez(dome.perez_lum, dome.perez_e.x, cos_theta, gamma);
    let cx = zenith.y * perez(dome.perez_x, dome.perez_e.y, cos_theta, gamma);
    let cy = max(zenith.z * perez(dome.perez_y, dome.perez_e.z, cos_theta, gamma), 0.0001);
    let big_x = cx / cy * lum;
    let big_z = (1.0 - cx - cy) / cy * lum;
    let rgb = vec3<f32>(
        3.2406 * big_x - 1.5372 * lum - 0.4986 * big_z,
        -0.9689 * big_x + 1.8758 * lum + 0.0415 * big_z,
        0.0557 * big_x - 0.2040 * lum + 1.0570 * big_z
    );
    return max(rgb, vec3<f32>(0.0)) * tint;
}

// The star field toward `ray`, turned with the sky, faded near the horizon,
// and twinkling: each star flickers on its own phase and rate, more low in
// the sky where its light crosses more air.
fn stars(ray: vec3<f32>) -> vec3<f32> {
    let star_rotation = mat3x3<f32>(dome.star_x.xyz, dome.star_y.xyz, dome.star_z.xyz);
    // Bevy's skybox convention: rotate, then mirror z.
    let star_dir = (star_rotation * ray) * vec3<f32>(1.0, 1.0, -1.0);
    let star = textureSampleLevel(star_map, star_sampler, star_dir, 0.0).rgb;
    let cell = floor(normalize(star_dir) * 640.0);
    let phase = fract(sin(dot(cell, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
    let rate = 1.5 + 2.5 * fract(phase * 7.13);
    let low = 1.0 - clamp(ray.y, 0.0, 1.0);
    let depth = 0.10 + 0.40 * low * low;
    let twinkle = 1.0 + depth * sin(globals.time * rate * 6.2831853 + phase * 6.2831853);
    return star * dome.stars.x * twinkle * smoothstep(0.0, 0.10, ray.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let ray = normalize(in.world_position.xyz - view.world_position);
    let sun_dir = dome.sun.xyz;

    // Derivatives before anything that can discard: they need every pixel
    // of the quad.
    let from_sun = acos(clamp(dot(ray, sun_dir), -1.0, 1.0));
    let edge = max(fwidth(from_sun), 0.000001);

#ifdef DEPTH_PREPASS
    // The scene is here: the sky is behind it.
    if (prepass_depth(in.position, 0u) > 0.0) {
        discard;
    }
#endif

    // The sun's disc, limb-darkened, hidden by the ground.
    let radius = dome.sun.w;
    let above = smoothstep(-0.002, 0.002, ray.y);
    // No disc at all when the Sky hides it or the Sun is off (radius 0).
    let coverage = select(0.0, (1.0 - smoothstep(radius - edge, radius + edge, from_sun)) * above, radius > 0.0);
    let q = clamp(from_sun / max(radius, 0.000001), 0.0, 1.0);
    let limb = 1.0 - SKY_LIMB_DARKENING * (1.0 - sqrt(max(1.0 - q * q, 0.0)));
    let disc = dome.sun_disc.rgb * limb * coverage;

    var color = disc;
    var alpha = coverage;
    if (dome.stars.y > 1.5) {
        // Over bevy's atmosphere: the stars' light and the sun's disc, added.
        color = stars(ray) + disc;
        alpha = 0.0;
    } else if (dome.stars.y > 0.5) {
        // Above the horizon: the two lobes, the night glow and the stars.
        let up = max(ray.y, 0.0);
        let sky_ray = normalize(vec3<f32>(ray.x, up, ray.z));
        var sky = lobe(sky_ray, sun_dir, dome.sun_zenith, dome.sun_tint.rgb)
            + lobe(sky_ray, dome.moon.xyz, dome.moon_zenith, dome.moon_tint.rgb);
        let low = 1.0 - up;
        sky = sky + dome.night_sky.rgb * (1.0 + dome.night_sky.w * low * low);
        sky = sky + stars(ray);
        // Below it, lit ground, blended across a sliver of horizon.
        let ground_mix = 1.0 - smoothstep(-0.02, 0.0, ray.y);
        color = mix(sky, dome.ground.rgb, ground_mix) + disc;
        alpha = 1.0;
    }

    // Premultiplied: the light added, and how much of what is behind it hides.
    var result = vec4<f32>(color * view.exposure, alpha);
#ifdef TONEMAP_IN_SHADER
    result = tone_mapping(result, view.color_grading);
#endif
    return result;
}
