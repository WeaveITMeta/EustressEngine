// Analytical Sun Disc Shader
// Renders a resolution-independent, pixel-perfect sun/moon disc.
// Applied to a billboard quad that faces the camera at the sun position.

#import bevy_pbr::forward_io::VertexOutput

struct SunDiscMaterial {
    color: vec4<f32>,       // Sun core color (warm white-yellow)
    corona_color: vec4<f32>, // Corona glow color
    disc_radius: f32,       // Normalized disc radius (0-1 in UV space)
    corona_radius: f32,     // Normalized corona radius
    intensity: f32,         // Brightness multiplier
    _padding: f32,
};

@group(2) @binding(0) var<uniform> material: SunDiscMaterial;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // UV centered at (0,0), range [-1, 1]
    let uv = in.uv * 2.0 - vec2(1.0);
    let dist = length(uv);

    // Disc: smooth anti-aliased circle
    let disc_edge = material.disc_radius;
    let aa_width = 0.02; // Anti-aliasing width in UV space
    let disc_alpha = 1.0 - smoothstep(disc_edge - aa_width, disc_edge + aa_width, dist);

    // Limb darkening: center is brighter, edge is dimmer (realistic solar limb)
    let limb = 1.0 - 0.3 * pow(dist / disc_edge, 2.0);

    // Corona glow: soft falloff beyond the disc
    let corona_t = 1.0 - smoothstep(disc_edge, material.corona_radius, dist);
    let corona_t2 = corona_t * corona_t * corona_t; // Cubic falloff

    // Composite: disc + corona
    let disc_color = material.color.rgb * limb * material.intensity * disc_alpha;
    let corona = material.corona_color.rgb * corona_t2 * 0.5 * (1.0 - disc_alpha);

    let final_color = disc_color + corona;
    let final_alpha = max(disc_alpha, corona_t2 * 0.4);

    // Discard fully transparent pixels (no depth write for sky)
    if final_alpha < 0.001 {
        discard;
    }

    return vec4(final_color, final_alpha);
}
