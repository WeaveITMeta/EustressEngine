// Analytical Sun/Moon Disc Shader
// Renders resolution-independent, pixel-perfect celestial discs.
// Supports moon phases via phase_angle uniform.

#import bevy_pbr::forward_io::VertexOutput

struct SunDiscMaterial {
    color: vec4<f32>,        // Disc core color
    corona_color: vec4<f32>, // Corona/glow color
    disc_radius: f32,        // Normalized disc radius (0-1 in UV space)
    corona_radius: f32,      // Normalized corona outer radius
    intensity: f32,          // Brightness multiplier
    phase_angle: f32,        // Moon phase: 0=full, PI=new, <0 = no phase (sun)
};

@group(2) @binding(0) var<uniform> material: SunDiscMaterial;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv * 2.0 - vec2(1.0);
    let dist = length(uv);

    // Disc edge with anti-aliasing
    let disc_edge = material.disc_radius;
    let aa_width = 0.02;
    let disc_alpha = 1.0 - smoothstep(disc_edge - aa_width, disc_edge + aa_width, dist);

    // Corona glow beyond the disc
    let corona_t = 1.0 - smoothstep(disc_edge, material.corona_radius, dist);
    let corona_t2 = corona_t * corona_t * corona_t;

    // Phase shadow (moon only — phase_angle < 0 means sun/no phase)
    var phase_mask = 1.0;
    if material.phase_angle >= 0.0 {
        // Phase rendering: the terminator is an ellipse whose x-scale = cos(phase_angle)
        // phase_angle: 0 = full (all lit), PI = new (all dark)
        // The shadow boundary runs vertically — lit side is determined by
        // which direction the sun is relative to the moon.
        let phase_cos = cos(material.phase_angle);

        // In UV space, the terminator is at x = phase_cos * disc_radius
        // Left of terminator = lit, right = shadow (for waxing)
        // We use the normalized x coordinate within the disc
        let nx = uv.x / disc_edge; // -1 to 1 within disc

        // Illuminated fraction: smoothstep across the terminator
        // phase_cos > 0 = gibbous (> half lit), < 0 = crescent (< half lit)
        let terminator = phase_cos;
        let shadow = smoothstep(terminator - 0.05, terminator + 0.05, nx);

        // At phase_angle 0 (full): cos=1, terminator=1, everything < 1 = lit
        // At phase_angle PI (new): cos=-1, terminator=-1, everything > -1 = shadow
        phase_mask = 1.0 - shadow;

        // Earthshine: faint illumination of the dark side (~3% of full)
        phase_mask = max(phase_mask, 0.03);
    }

    // Limb darkening (realistic — center brighter than edge)
    let limb = 1.0 - 0.3 * pow(dist / max(disc_edge, 0.01), 2.0);

    // Composite
    let disc_color = material.color.rgb * limb * material.intensity * disc_alpha * phase_mask;
    let corona = material.corona_color.rgb * corona_t2 * 0.5 * (1.0 - disc_alpha);

    let final_color = disc_color + corona;
    let final_alpha = max(disc_alpha * max(phase_mask, 0.03), corona_t2 * 0.4);

    if final_alpha < 0.001 {
        discard;
    }

    return vec4(final_color, final_alpha);
}
