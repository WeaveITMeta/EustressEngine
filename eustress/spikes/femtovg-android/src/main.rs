fn main() {
    // FemtoVG's wgpu renderer is the first pattern-(a) attempt. On Android
    // Slint cfg's this crate out, so this reference should fail to resolve.
    let _ = std::any::type_name::<slint::platform::femtovg_renderer::FemtoVGWGPURenderer>();
}
