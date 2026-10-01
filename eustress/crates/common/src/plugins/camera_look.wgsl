// The view grade: the fragment half of the pass in `camera_look.rs`.
//
// Runs after tonemapping on every view camera with a `ViewGrade`, the way
// Roblox's ColorCorrectionEffect works on the finished image. The math is in
// display space (sRGB-encoded), where Roblox's Brightness and Contrast are
// defined: Brightness is added, Contrast scales the distance from mid grey,
// Saturation the distance from the Rec. 709 luma, and TintColor multiplies
// the result.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct ViewGrade {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    unused: f32,
    tint: vec4<f32>,
}

@group(0) @binding(0) var screen: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> grade: ViewGrade;

fn to_display(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((max(c, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let source = textureSample(screen, screen_sampler, in.uv);
    var c = to_display(clamp(source.rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    c = c + vec3<f32>(grade.brightness);
    c = (c - vec3<f32>(0.5)) * grade.contrast + vec3<f32>(0.5);
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    c = mix(vec3<f32>(luma), c, grade.saturation);
    c = c * grade.tint.rgb;
    return vec4<f32>(to_linear(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0))), source.a);
}
