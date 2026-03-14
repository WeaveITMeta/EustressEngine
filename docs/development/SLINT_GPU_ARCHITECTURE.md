# Slint GPU Architecture Migration

## Status: Planned

## Problem Statement

The current architecture suffers from persistent UI flickering because Slint is embedded backwards:

```
Current: Bevy owns window → Slint software-renders to CPU buffer → CPU→GPU upload → fullscreen quad overlay
```

Every `ui.set_*()` call marks Slint regions dirty. The software renderer repaints dirty regions into a CPU pixel buffer every frame. That buffer is uploaded to a GPU texture. A second Bevy camera (order=100) renders it as a fullscreen quad on top of the 3D scene. The "viewport" is a transparent hole in the Slint UI — the 3D bleeds through from underneath.

This is the `bevy-hosts-slint` example pattern, designed for simple HUDs, not a full studio application.

## Target Architecture

```
Target: Slint owns window (winit + FemtoVG-wgpu) → GPU renders UI natively
        Bevy runs headless → renders 3D to wgpu::Texture → Slint displays it as Image component
```

### Diagram

```
┌─────────────────────────────────────────────────────────┐
│  Slint OWNS the window (winit + FemtoVG-wgpu renderer)  │
│  GPU-native rendering — no CPU buffer, no texture upload │
│                                                          │
│  ┌──────────┐  ┌────────────────────┐  ┌─────────────┐  │
│  │ Explorer  │  │     Viewport       │  │ Properties  │  │
│  │  (Slint)  │  │   (Slint Image)    │  │   (Slint)   │  │
│  │           │  │                    │  │             │  │
│  │           │  │  Bevy 3D renders   │  │             │  │
│  │           │  │  into this texture │  │             │  │
│  └──────────┘  └────────────────────┘  └─────────────┘  │
│  ┌──────────────────────────────────────────────────┐    │
│  │ Ribbon Toolbar (Slint)                            │    │
│  └──────────────────────────────────────────────────┘    │
│  ┌──────────────────────────────────────────────────┐    │
│  │ Output / Workshop / Asset Manager (Slint)         │    │
│  └──────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
```

## Technical Foundation

### Shared wgpu Device (wgpu 27)

Bevy 0.18 uses **wgpu 27.0.1**. Slint (master) supports **`unstable-wgpu-27`** feature flag which exposes `slint::wgpu_27` module and `slint::Image::try_from(wgpu::Texture)`.

Both Bevy and Slint share the same `wgpu::Device` and `wgpu::Queue`, enabling zero-copy texture sharing.

### Key Slint Application Programming Interfaces

```rust
// Slint creates the wgpu device via its backend
slint::BackendSelector::new()
    .require_wgpu_27(slint::wgpu_27::WGPUConfiguration::default())
    .select()?;

// Extract device/queue from Slint's rendering notifier
app.window().set_rendering_notifier(move |state, graphics_api| {
    if let (slint::RenderingState::RenderingSetup, slint::GraphicsAPI::WGPU27 { device, queue, .. }) = (state, graphics_api) {
        // Pass device + queue to Bevy's headless renderer
    }
})?;

// Import Bevy's render target into Slint
let slint_image = slint::Image::try_from(bevy_render_texture).unwrap();
ui.set_viewport_image(slint_image);
```

### Bevy Headless Rendering

Bevy renders its 3D scene to an offscreen `wgpu::Texture` (render target) without creating its own window. This is Bevy's standard `RenderToTexture` pattern used for minimaps, security cameras, and similar features.

## Cargo.toml Changes

```toml
# Before (current)
slint = { git = "https://github.com/slint-ui/slint", branch = "master", features = [
    "std", "compat-1-2", "renderer-software", "renderer-femtovg"
]}

# After (target)
slint = { git = "https://github.com/slint-ui/slint", branch = "master", features = [
    "std", "compat-1-2", "renderer-femtovg-wgpu", "unstable-wgpu-27", "backend-winit"
]}
```

## What Gets Deleted

| Current Component | Purpose | Replacement |
|---|---|---|
| `BevyWindowAdapter` | Custom Slint platform adapter | Slint's built-in winit backend |
| `SoftwareRenderer` + `ReusedBuffer` | CPU pixel rendering | FemtoVG-wgpu GPU renderer |
| `render_slint_to_texture` system | CPU→GPU texture upload | `slint::Image::try_from(wgpu::Texture)` |
| `SlintScene` entity + `SlintOverlaySprite` | Fullscreen quad overlay | Slint `Image` component |
| Camera order=100 | Overlay camera for UI quad | Not needed — Slint renders directly |
| `forward_input_to_slint` system | Manual input forwarding | Slint's native winit input handling |
| `handle_window_resize` system | Manual resize dispatch | Slint's native window resize |
| `bytemuck::cast_slice_mut` pixel conversion | CPU pixel format conversion | Not needed — GPU native |

## What Gets Added

| New Component | Purpose |
|---|---|
| `slint::BackendSelector::require_wgpu_27()` | Initialize Slint with shared wgpu device |
| `set_rendering_notifier` callback | Extract wgpu device/queue for Bevy |
| Bevy `RenderToTexture` camera | Render 3D scene to offscreen texture |
| Slint `Image` component for viewport | Display Bevy's render target |
| `TouchArea` in viewport | Forward viewport clicks/drags to Bevy |
| Viewport input bridge system | Route Slint viewport events → Bevy input |

## Migration Phases

### Phase 1: Feature Flags
- Change `Cargo.toml` to `renderer-femtovg-wgpu` + `unstable-wgpu-27` + `backend-winit`
- Remove `renderer-software` and `renderer-femtovg` (OpenGL)
- Verify compilation

### Phase 2: Slint Owns Window
- Remove `BevyWindowAdapter`, `SlintPlatform`, manual platform setup
- Let Slint create the window via its winit backend
- Bevy runs headless (no `WindowPlugin` or custom window creation)
- Extract `wgpu::Device` and `wgpu::Queue` from Slint's rendering notifier

### Phase 3: Shared Texture
- Create Bevy `RenderToTexture` camera targeting an offscreen wgpu texture
- Import that texture into Slint via `slint::Image::try_from()`
- Add Slint `Image` component in the viewport area of the layout
- Update texture reference each frame (or when Bevy finishes rendering)

### Phase 4: Input Routing
- Remove `forward_input_to_slint` — Slint handles all input natively
- Add `TouchArea` in Slint viewport to capture mouse events
- Bridge viewport events back to Bevy's input system
- Camera controller reads from bridged events instead of raw winit

### Phase 5: Cleanup
- Delete `render_slint_to_texture`, `SlintScene`, overlay camera
- Delete `handle_window_resize` manual dispatch
- Delete `SlintCursorState` (Slint manages cursor natively)
- Remove `bytemuck` dependency for pixel conversion
- Update all documentation

## Benefits

- **Zero flickering** — Slint GPU renderer only redraws changed regions, no CPU→GPU upload
- **Better performance** — GPU-native UI rendering, zero CPU pixel work
- **Simpler code** — delete ~500 lines of adapter/overlay/forwarding code
- **Proper input** — native winit input handling, no manual forwarding
- **Future-proof** — Slint's GPU renderer is actively developed; software renderer is for embedded

## Risks

- **wgpu version coupling** — `unstable-wgpu-27` is not stable Slint Application Programming Interface. When Bevy upgrades to wgpu 28, switch to `unstable-wgpu-28`.
- **Bevy headless rendering** — less commonly used path, may have edge cases
- **Texture sharing** — must ensure Bevy and Slint agree on texture format and lifecycle
- **Input bridge** — viewport mouse events need careful coordinate transformation

## Stopgap Fixes (Applied)

While this migration is planned, the following fixes reduce flickering in the current software renderer architecture:

1. `sync_workshop_to_slint` — added `pipeline.is_changed()` early return (was setting 7 properties + 2 models every frame with zero change detection)
2. `sync_center_tabs_to_slint` — added change detection for `set_scene_tab_name()` (was set every frame)
3. `sync_center_tabs_to_slint` — added change detection for `set_active_tab_type()` (was set every frame)
4. `sync_bevy_to_slint` — increased FPS change threshold from 0.05 to 1.0 (was triggering updates on normal fluctuation)
5. `sync_bevy_to_slint` — increased frame time threshold from 0.05 to 0.5
6. `forward_input_to_slint` — only dispatch `PointerMoved` when position actually changes (was sent every frame, causing hover state re-evaluation)
7. Removed unused `last_toolbar_hash` field from `StudioState`
