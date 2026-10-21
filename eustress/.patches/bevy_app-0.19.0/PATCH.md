# Patched bevy_app 0.19.0

A copy of bevy_app 0.19.0 from crates.io with one change. The workspace points at it from `[patch.crates-io]` in `eustress/Cargo.toml`.

## What changed

`App::finish` and `SubApp::finish` walked `0..plugin_registry.len()` captured once. A plugin whose `finish` called `add_plugins` built those plugins and then skipped their `finish`. The renderer never unpacked its device.

Both loops now walk while the registry grows, so plugins appended during `finish` are finished in the same pass. Nothing else differs from the published crate.

## Why

Bevy 0.19 creates the wgpu instance and device inside `RenderPlugin::build`, which runs before the winit loop. On Android that is before `GameActivity` `onSurfaceCreated`. GLES then has no surface and `request_adapter` fails.

`player-mobile` waits until the primary window has a `RawHandleWrapper` (`winit` `Resumed`, which is `InitWindow` / `onSurfaceCreated`). Its gate plugin's `finish` adds `RenderPlugin` and the render stack only then. That `finish` has to run, or the device stays in `FutureRenderResources` and never reaches the worlds.

Desktop apps do not add plugins from `finish`, so this does not change their plugin order.
