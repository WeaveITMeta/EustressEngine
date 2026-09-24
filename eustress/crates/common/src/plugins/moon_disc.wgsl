// Moon disc: the fragment half of `MoonDiscMaterial` (see `moon_disc.rs`).
//
// The quad only has to cover the disc and its halo. Every pixel works out
// where its view ray crosses the moon's sphere from the ray's angle to the
// moon's centre, so the quad's own orientation and texture coordinates are
// never read. The sphere is lit from the sun's real direction with the moon's
// Lommel-Seeliger reflectance, which is flat across a full moon rather than
// darkening toward the limb as a Lambert sphere would. Output is
// premultiplied: the colour is added whole, and alpha is only how much of
// the sky behind the disc it hides (none by day, all of it at night).
//
// The layout of `MoonDiscParams` is checked against its Rust packing by the
// unit tests in `moon_disc.rs`; change both together.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

struct MoonDiscParams {
    // xyz: toward the moon's centre. w: sine of its angular radius.
    moon: vec4<f32>,
    // xyz: toward the sun. w: earthshine, as a fraction of the lit limb.
    sun: vec4<f32>,
    // xyz: the moon's celestial north, perpendicular to `moon`.
    // w: how much the disc hides what is behind it, 0 by day, 1 at night.
    north: vec4<f32>,
    // rgb: pre-tonemap brightness of the full, lit limb. a: halo strength.
    radiance: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> moon_params: MoonDiscParams;

fn hash21(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    var total = 0.0;
    var amplitude = 0.5;
    var q = p;
    for (var i = 0; i < 4; i = i + 1) {
        total = total + amplitude * value_noise(q);
        q = q * 2.03 + vec2<f32>(17.1, 9.7);
        amplitude = amplitude * 0.5;
    }
    return total;
}

// A soft ellipse: 1 inside, easing to 0 at its edge.
fn blob(p: vec2<f32>, center: vec2<f32>, radius: vec2<f32>) -> f32 {
    let d = length((p - center) / radius);
    return 1.0 - smoothstep(0.55, 1.0, d);
}

// The near side's dark lava plains, roughly where they lie: x toward the
// limb that holds Crisium, y toward the moon's north, radius 1 at the limb.
fn maria(p: vec2<f32>) -> f32 {
    var m = blob(p, vec2<f32>(-0.30, 0.42), vec2<f32>(0.30, 0.24));      // Imbrium
    m = max(m, blob(p, vec2<f32>(0.18, 0.40), vec2<f32>(0.17, 0.15)));  // Serenitatis
    m = max(m, blob(p, vec2<f32>(0.33, 0.14), vec2<f32>(0.20, 0.17)));  // Tranquillitatis
    m = max(m, blob(p, vec2<f32>(0.66, 0.30), vec2<f32>(0.11, 0.09)));  // Crisium
    m = max(m, blob(p, vec2<f32>(0.58, -0.10), vec2<f32>(0.12, 0.16))); // Fecunditatis
    m = max(m, blob(p, vec2<f32>(0.36, -0.30), vec2<f32>(0.12, 0.10))); // Nectaris
    m = max(m, blob(p, vec2<f32>(-0.55, 0.05), vec2<f32>(0.28, 0.45))); // Oceanus Procellarum
    m = max(m, blob(p, vec2<f32>(-0.18, -0.32), vec2<f32>(0.20, 0.14))); // Nubium
    m = max(m, blob(p, vec2<f32>(-0.45, -0.38), vec2<f32>(0.13, 0.10))); // Humorum
    m = max(m, blob(p, vec2<f32>(0.02, 0.10), vec2<f32>(0.10, 0.08)));  // Vaporum
    return m;
}

// Surface brightness relative to the highlands.
fn albedo(p: vec2<f32>) -> f32 {
    // Ragged shores, so the plains read as lava rather than as ellipses.
    let shore = fbm(p * 5.0) - 0.5;
    let m = clamp(maria(p + vec2<f32>(shore) * 0.08), 0.0, 1.0);
    // Highland albedo is about 0.12 and the maria about 0.07.
    var a = mix(1.0, 0.58, m);
    // Craters at every scale.
    a = a * (0.86 + 0.28 * fbm(p * 18.0));
    // Bright young ray craters: Tycho in the southern highlands and
    // Copernicus at the edge of the western plains.
    let tycho = p - vec2<f32>(-0.13, -0.72);
    let copernicus = p - vec2<f32>(-0.30, 0.17);
    a = a + 0.30 * exp(-dot(tycho, tycho) / 0.0015);
    a = a + 0.18 * exp(-dot(copernicus, copernicus) / 0.0010);
    return a;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let ray = normalize(in.world_position.xyz - view.world_position);
    let toward_moon = moon_params.moon.xyz;
    let north = moon_params.north.xyz;
    // Right on the disc, as the viewer sees it with the moon's north up.
    let right = normalize(cross(toward_moon, north));
    // Disc coordinates, radius 1 at the limb.
    let p = vec2<f32>(dot(ray, right), dot(ray, north)) / moon_params.moon.w;
    let r = length(p);

    // Derivatives before anything that can discard: they need every pixel
    // of the quad still running.
    let limb_width = max(fwidth(r), 1e-4);
    let horizon_width = max(fwidth(ray.y), 1e-6);

    if (r > 3.2 || dot(ray, toward_moon) <= 0.0) {
        discard;
    }

    let coverage = 1.0 - smoothstep(1.0 - limb_width, 1.0 + limb_width, r);
    // Nothing of the moon below the horizon.
    let above = clamp(ray.y / horizon_width + 0.5, 0.0, 1.0);

    // The point on the sphere under this pixel, `z` toward the viewer.
    let z = sqrt(max(1.0 - dot(p, p), 0.0));
    let normal = normalize(p.x * right + p.y * north - z * toward_moon);
    let to_sun = moon_params.sun.xyz;

    // Lommel-Seeliger, softened over a few degrees at the terminator so a
    // crescent a dozen pixels wide does not alias.
    let facing = dot(normal, to_sun);
    let mu0 = max(facing, 0.0) * smoothstep(-0.02, 0.05, facing);
    let lit = 2.0 * mu0 / (mu0 + max(z, 1e-3));
    // Earthshine lights the night side faintly.
    let earthshine = moon_params.sun.w * (1.0 - smoothstep(0.0, 0.15, facing));
    let surface = moon_params.radiance.rgb
        * albedo(clamp(p, vec2<f32>(-1.0), vec2<f32>(1.0)))
        * (lit + earthshine)
        * coverage;

    // A faint halo, as bright as the moon is full.
    let lit_fraction = 0.5 * (1.0 - dot(toward_moon, to_sun));
    let halo = moon_params.radiance.rgb
        * moon_params.radiance.a
        * lit_fraction
        * exp(-max(r - 1.0, 0.0) * 2.2)
        * (1.0 - coverage);

    var color = vec4<f32>((surface + halo) * above, coverage * moon_params.north.w * above);
#ifdef TONEMAP_IN_SHADER
    color = tone_mapping(color, view.color_grading);
#endif
    return color;
}
