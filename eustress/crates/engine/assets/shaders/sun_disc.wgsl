// Analytical Sun/Moon Disc Shader
// Resolution-independent, pixel-perfect celestial discs with moon phases.

#import bevy_pbr::forward_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material_color: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> material_corona: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> material_params: vec4<f32>; // disc_radius, corona_radius, intensity, phase_angle

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let uv = mesh.uv * 2.0 - vec2(1.0);
    let dist = length(uv);

    let disc_edge = material_params.x;
    let corona_radius = material_params.y;
    let intensity = material_params.z;
    let phase_angle = material_params.w;

    let aa_width = 0.02;
    let disc_alpha = 1.0 - smoothstep(disc_edge - aa_width, disc_edge + aa_width, dist);

    let corona_t = 1.0 - smoothstep(disc_edge, corona_radius, dist);
    let corona_t2 = corona_t * corona_t * corona_t;

    // Phase shadow (moon only — phase_angle < 0 means sun/no phase)
    var phase_mask = 1.0;
    if phase_angle >= 0.0 {
        let phase_cos = cos(phase_angle);
        let nx = uv.x / max(disc_edge, 0.001);
        let shadow = smoothstep(phase_cos - 0.05, phase_cos + 0.05, nx);
        phase_mask = max(1.0 - shadow, 0.03); // Earthshine on dark side
    }

    // Limb darkening
    let limb = 1.0 - 0.3 * pow(dist / max(disc_edge, 0.001), 2.0);

    let disc_color = material_color.rgb * limb * intensity * disc_alpha * phase_mask;
    let corona = material_corona.rgb * corona_t2 * 0.5 * (1.0 - disc_alpha);

    let final_color = disc_color + corona;
    let final_alpha = max(disc_alpha * max(phase_mask, 0.03), corona_t2 * 0.4);

    if final_alpha < 0.001 {
        discard;
    }

    return vec4(final_color, final_alpha);
}
