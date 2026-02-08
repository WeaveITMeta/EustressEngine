# Bevy Integration Needs Work

## Issue Title
**Slint-Bevy Integration: Skia ICU Conflicts on Windows & Resource Initialization Order**

---

## Summary

The Slint UI framework integration with Bevy 0.17 is blocked by multiple issues on Windows:
1. **Skia ICU symbol conflicts** prevent using Slint's GPU-accelerated wgpu backend
2. **Resource initialization order issues** cause runtime panics due to systems requiring resources without `Option<T>` wrappers

---

## What Was Attempted

### Approach 1: Slint-First Architecture with Shared wgpu (FAILED)

Following the [official Slint+Bevy example](https://github.com/slint-ui/slint/tree/master/examples/bevy), we attempted to:
- Have Slint own the native window and event loop
- Share wgpu device, queue, and instance between Slint and Bevy
- Render Bevy's 3D scene to a texture displayed in Slint's viewport

**Configuration:**
```toml
[dependencies]
slint = { version = "1.10", default-features = false, features = [
    "compat-1-2",
    "unstable-wgpu-26",  # Match Bevy 0.17's wgpu version
] }
```

**Result:** Linker errors on Windows due to duplicate ICU symbols from Skia:
```
error LNK2005: icudt74_dat already defined in icudt.lib(icudt74_dat.obj)
error LNK2005: u_errorName_74 already defined in icuuc74.lib(utypes.obj)
... (hundreds of duplicate symbols)
```

**Root Cause:** Slint's default renderer uses Skia, which bundles ICU. When linking with Bevy (which may also use ICU indirectly), duplicate symbol errors occur. The `rust-lld` linker is stricter about this than MSVC's default linker.

### Approach 2: FemtoVG Renderer (FAILED)

Attempted to use FemtoVG renderer to avoid Skia:
```toml
slint = { version = "1.10", default-features = false, features = [
    "renderer-femtovg",
    "unstable-wgpu-26",
] }
```

**Result:** Same ICU conflicts - FemtoVG still pulls in Skia dependencies.

### Approach 3: Software Renderer Overlay (PARTIAL SUCCESS)

Reverted to Bevy-first architecture with Slint software renderer:
```toml
slint = { version = "1.10", default-features = false, features = [
    "renderer-software",
    "compat-1-2",
] }
```

**Result:** Builds successfully, but revealed systemic issues with resource initialization order.

---

## Current Blockers

### 1. Resource Initialization Order Panics

Many systems in the codebase require `Res<T>` or `ResMut<T>` without `Option<T>` wrappers:

```rust
// This panics if StudioState isn't initialized yet
fn handle_select_drag(
    studio_state: Res<StudioState>,  // Should be Option<Res<StudioState>>
    ...
) { ... }
```

**Affected Systems:**
- `handle_select_drag` in `select_tool.rs`
- `handle_box_selection` in `select_tool.rs`
- `handle_spawn_part_events` in `spawn_events.rs`
- `apply_ui_actions` in `world_view.rs`
- `handle_window_close_request` in `slint_ui.rs` and `mod.rs`
- Many others...

**Temporary Workaround:**
```rust
// Set custom error handler that warns instead of panicking
app.set_error_handler(bevy::ecs::error::warn);
```

### 2. Slint Rendering Not Implemented

The `SlintUiPlugin` currently only initializes resources but doesn't actually render the Slint UI. The software renderer overlay approach requires:
- Initializing Slint with software renderer
- Rendering to a pixel buffer each frame
- Compositing the buffer onto Bevy's window as an overlay

---

## Best Case Scenario: Multi-Threaded Architecture

For optimal performance, the integration should use a **multi-threaded approach** where Slint and Bevy run in parallel:

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Main Thread                               │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                    Slint Event Loop                          ││
│  │  - Owns native window                                        ││
│  │  - Handles all UI events (mouse, keyboard, touch)            ││
│  │  - GPU-accelerated rendering via wgpu                        ││
│  │  - Displays Bevy texture in viewport area                    ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
                              │
                    Shared wgpu Device/Queue
                    Async Channels (smol/tokio)
                              │
┌─────────────────────────────────────────────────────────────────┐
│                       Bevy Thread                                │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                    Bevy App::run()                           ││
│  │  - Headless mode (no window)                                 ││
│  │  - Renders 3D scene to shared texture                        ││
│  │  - Runs at independent frame rate                            ││
│  │  - Receives input events from Slint via channel              ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

### Performance Benefits

| Aspect | Single-Thread | Multi-Thread |
|--------|---------------|--------------|
| **Frame Rate** | Limited by slower of UI/3D | Independent rates (UI@60fps, 3D@144fps) |
| **Input Latency** | Blocked during 3D render | Immediate UI response |
| **CPU Utilization** | Single core | Multi-core parallelism |
| **GPU Utilization** | Sequential | Parallel command submission |
| **Responsiveness** | UI freezes during heavy 3D | UI always responsive |

### Implementation Requirements

1. **Shared wgpu Resources:**
   ```rust
   // Slint provides these to Bevy
   let (device, queue, instance) = slint_window.require_wgpu_26();
   ```

2. **Render-to-Texture:**
   ```rust
   // Bevy renders to a shared texture
   let render_texture = device.create_texture(&TextureDescriptor {
       size: Extent3d { width: viewport_width, height: viewport_height, depth_or_array_layers: 1 },
       format: TextureFormat::Rgba8UnormSrgb,
       usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
       ...
   });
   ```

3. **Async Communication:**
   ```rust
   // Input events from Slint to Bevy
   let (input_tx, input_rx) = smol::channel::unbounded::<InputEvent>();
   
   // Texture ready signal from Bevy to Slint
   let (texture_tx, texture_rx) = smol::channel::bounded::<TextureHandle>(1);
   ```

4. **Thread Synchronization:**
   ```rust
   // Bevy thread
   std::thread::spawn(move || {
       let mut app = App::new();
       app.add_plugins(DefaultPlugins.build().disable::<WinitPlugin>());
       app.insert_resource(SharedRenderTexture(render_texture));
       app.run();
   });
   
   // Main thread - Slint event loop
   slint::run_event_loop();
   ```

---

## Proposed Solutions

### Short-Term (Workaround)

1. ✅ Use `app.set_error_handler(bevy::ecs::error::warn)` to prevent panics
2. ⬜ Fix critical systems to use `Option<Res<T>>` pattern
3. ⬜ Implement basic software renderer overlay for Slint UI

### Medium-Term (Proper Fix)

1. ⬜ Audit all systems and convert to `Option<Res<T>>` pattern
2. ⬜ Implement proper plugin initialization order
3. ⬜ Add run conditions to skip systems when resources unavailable

### Long-Term (Optimal)

1. ⬜ Resolve Skia ICU conflicts (upstream Slint issue or custom build)
2. ⬜ Implement multi-threaded architecture with shared wgpu
3. ⬜ Add proper input forwarding from Slint to Bevy
4. ⬜ Implement viewport resize handling

---

## Environment

- **OS:** Windows 11
- **Rust:** 1.84.0 (stable)
- **Bevy:** 0.17.3
- **Slint:** 1.10.x
- **wgpu:** 26.x (via Bevy)
- **Linker:** MSVC (rust-lld disabled due to stricter symbol checking)

---

## Related Issues

- Slint ICU bundling: https://github.com/nickkuk/slint/issues/XXX (if exists)
- Bevy resource validation: https://github.com/bevyengine/bevy/issues/XXX (if exists)

---

## Files Modified

- `crates/engine/src/main.rs` - Added error handler, reordered plugins
- `crates/engine/src/ui/slint_ui.rs` - Fixed Option wrappers
- `crates/engine/src/ui/spawn_events.rs` - Fixed Option wrappers
- `crates/engine/src/ui/world_view.rs` - Fixed Option wrappers
- `crates/engine/src/select_tool.rs` - Fixed Option wrappers
- `crates/engine/Cargo.toml` - Switched to software renderer
- `.cargo/config.toml` - Disabled rust-lld linker
