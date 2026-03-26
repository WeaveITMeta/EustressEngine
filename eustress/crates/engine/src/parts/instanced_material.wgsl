// MaterialExtension shader for per-instance colors
// Multiplies StandardMaterial base_color with ColorExtension.color uniform

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
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

// ColorExtension uniform — group(2) binding(0) matches AsBindGroup #[uniform(0)]
struct ColorExtension {
    color: vec4<f32>,
}

@group(2) @binding(0)
var<uniform> color_extension: ColorExtension;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    // Get the standard PBR input
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Multiply base color with per-instance color from extension
    pbr_input.material.base_color = pbr_input.material.base_color * color_extension.color;

    // Alpha discard
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

    return out;
}
