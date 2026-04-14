// Moon Phase Shader (standalone Material)
// Renders a disc with correct illumination based on sun-moon elongation angle.
// The terminator (lit/dark boundary) is an ellipse: x = cos(phase) * sqrt(1 - y²)

#import bevy_pbr::forward_io::VertexOutput

struct MoonPhaseUniforms {
    // cos(elongation_angle): -1 = full moon, 0 = quarter, 1 = new moon
    cos_phase: f32,
    // +1 = waxing (lit on right), -1 = waning (lit on left)
    waxing_sign: f32,
    _padding1: f32,
    _padding2: f32,
};

@group(2) @binding(0)
var<uniform> uniforms: MoonPhaseUniforms;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Map UV from [0,1] to [-1,1] centered on disc
    let uv = in.uv * 2.0 - 1.0;
    let r = length(uv);

    // Disc clipping with soft edge
    if r > 1.0 {
        discard;
    }
    let edge = 1.0 - smoothstep(0.95, 1.0, r);

    // Moon surface base color (pale silver)
    let moon_color = vec3<f32>(0.85, 0.87, 0.92);

    // Terminator: boundary between lit and dark halves
    // The lit region satisfies: x * waxing_sign < cos_phase * sqrt(1 - y²)
    let y_term = sqrt(max(1.0 - uv.y * uv.y, 0.0));
    let terminator = uv.x * uniforms.waxing_sign - uniforms.cos_phase * y_term;

    // Smooth transition at the terminator (antialiased edge)
    let lit = 1.0 - smoothstep(-0.03, 0.03, terminator);

    // Lit side: bright moon surface. Dark side: faint earthshine
    let earthshine = 0.05;
    let brightness = mix(earthshine, 1.0, lit);

    // Subtle limb darkening for realism
    let limb = 1.0 - 0.15 * r * r;

    let final_color = moon_color * brightness * limb;
    return vec4<f32>(final_color, edge * 0.95);
}
