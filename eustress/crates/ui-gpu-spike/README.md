# ui-gpu-spike

A spike, not part of Studio. It answers one question: can Studio's Slint UI be
drawn by Slint 1.19's GPU renderer on Bevy's own wgpu device, so
`backdrop-blur` works, and what does that cost compared with the software
renderer Studio uses today?

It opens a Bevy 0.19 window with a moving 3D scene and draws a Slint UI over it
with one of two renderers:

- `--renderer software`: Studio's path today, copied from
  `crates/engine/src/ui/slint_ui.rs` (`SoftwareRenderer` with `ReusedBuffer`
  into a CPU staging buffer, the whole buffer copied into a Bevy `Image` when
  anything changed, the `Image` re-uploaded, an orthographic camera composites
  it with premultiplied alpha).
- `--renderer gpu`: `slint::platform::skia_renderer::SkiaWGPU29Renderer`,
  created from Bevy's own wgpu 29 instance, adapter, device and queue, drawing
  straight into the `Image`'s GPU texture with `render_to_texture`. No CPU
  buffer, no upload.

The UI is either Studio's own `StudioWindow` (compiled from
`crates/engine/ui/slint/main.slint`, read only) or `GlassDemo`
(`ui/spike.slint`), which has two `backdrop-blur` panels. With `--scene-under`
the Bevy camera renders into a texture that Slint draws under the UI as an
`Image`, so the blur has the scene to blur.

## Why Skia and not FemtoVG

In Slint 1.19.0 `FemtoVGWGPURenderer` exists only for wgpu 30
(`i-slint-renderer-femtovg` feature `wgpu-30`, exported under
`unstable-wgpu-30`). Bevy 0.19 uses wgpu 29, and the only Slint 1.19 renderer
that renders on a caller's wgpu 29 device is Skia's `SkiaWGPU29Renderer`.

## Build

Everything heavy is behind the `spike` feature, so a workspace-wide build or
test compiles an empty binary here and never adds Skia or wgpu 29 features to
the engine's Slint.

```
cargo build --release -p eustress-ui-gpu-spike --features spike
```

Skia comes from rust-skia's prebuilt binaries (downloaded by `skia-bindings`'
build script). On Windows with Vulkan only the set with svg and textlayout is
published, which is why `Cargo.toml` asks `skia-safe` for those two features.

## Run

`ui-gpu-spike --help` lists the options. `measure.ps1` runs the whole matrix
and appends one JSON line per run to a results file.
