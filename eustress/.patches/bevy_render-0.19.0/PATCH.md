# Patched bevy_render 0.19.0

A copy of bevy_render 0.19.0 from crates.io with one change. The workspace points at it from `[patch.crates-io]` in `eustress/Cargo.toml`.

## What changed

`create_surfaces` requests an sRGB `view_formats` entry whenever the surface format is not sRGB. `prepare_windows` then builds the swapchain view with that format. Both require `DownlevelFlags::SURFACE_VIEW_FORMATS`.

When the adapter does not have that flag, the surface is configured with an empty `view_formats` list and the swapchain view uses the surface format. Adapters that have the flag keep the original sRGB view.

`prepare_view_targets` also lists an sRGB view format on `Rgba8Unorm` / `Bgra8Unorm` camera targets. That needs `DownlevelFlags::VIEW_FORMATS`. Without the flag the list is empty and the texture keeps its own format.

`SwapchainHold` makes `create_surfaces` and `prepare_windows` skip the swapchain. The Android player sets it on suspend after dropping `WindowSurfaces`, then clears it on resume so the surface is created again. `Surface::get_current_texture` panics (it does not return an error) when the surface is not configured, which is what happened if the GLES device was destroyed while Bevy still held the old swapchain.

`create_surfaces` logs the chosen surface format, view format, and window size. That line is diagnostic for the Android player. It does not change the configuration.

## Why

Android GLES often has no `EGL_KHR_gl_colorspace`, so the only surface formats are non-sRGB and the downlevel flag is absent. `Surface::configure` then fails (`SURFACE_VIEW_FORMATS are required but not supported`) and the next `get_current_texture` panics because the surface was never configured.

Vulkan adapters that support the flag are unchanged. The color space of a GLES frame that cannot use an sRGB view is the surface format itself.
