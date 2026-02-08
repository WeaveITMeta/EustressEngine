# Eustress Engine - Slint UI Migration

This directory contains the Slint-based UI for Eustress Engine Studio, replacing the previous egui implementation.

## Overview

Slint is a declarative UI framework that provides:
- **Better aesthetics**: Modern, polished look with smooth animations
- **Declarative syntax**: UI defined in `.slint` files, logic in Rust
- **WGPU integration**: Native support for embedding 3D viewports
- **Better accessibility**: Built-in keyboard navigation
- **Responsive layouts**: Automatic resizing and panel management

## Directory Structure

```
ui/slint/
├── main.slint           # Root window and layout
├── theme.slint          # Colors, fonts, and shared components
├── ribbon.slint         # Top toolbar with menus and tools
├── explorer.slint       # Entity hierarchy tree
├── properties.slint     # Property editor panel
├── output.slint         # Log console
├── command_bar.slint    # Command palette (Ctrl+Shift+P)
├── toolbox.slint        # Part insertion tools
├── asset_manager.slint  # Asset browser
├── collaboration.slint  # Real-time collaboration
├── soul_panel.slint     # Soul script management
├── script_editor.slint  # Code editor
├── notifications.slint  # Toast messages
├── view_selector.slint  # Camera view modes
├── ai_generation.slint  # AI generation queue
├── publish.slint        # Publish dialog
├── login.slint          # SSO authentication
├── history_panel.slint  # Undo/redo history
└── context_menu.slint   # Right-click menus
```

## Migration Guide

### Step 1: Enable Slint UI

In `main.rs`, replace the egui plugin with Slint:

```rust
// Before (egui)
.add_plugins(EguiPlugin::default())
.add_plugins(StudioUiPlugin { ... })

// After (Slint)
.add_plugins(slint_ui::SlintUiPlugin { ... })
```

### Step 2: Remove egui Dependencies (Optional)

Once migration is complete, remove from `Cargo.toml`:
```toml
# Remove these
bevy_egui = { workspace = true }
egui_extras = { workspace = true }
egui-notify = { workspace = true }
egui_dock = { workspace = true }
```

### Step 3: Update Event Handlers

Slint uses callbacks instead of immediate-mode rendering:

```rust
// egui (immediate mode)
if ui.button("Save").clicked() {
    save_scene();
}

// Slint (callbacks)
ui.on_save_scene(|| {
    save_scene();
});
```

## WGPU Viewport Integration

The Bevy 3D viewport is embedded in the Slint UI using shared WGPU textures:

1. **Bevy renders to offscreen texture**
2. **Slint displays texture in viewport Rectangle**
3. **Input events are forwarded from Slint to Bevy**

See `src/ui/slint_ui.rs` for implementation details.

## Theming

The theme is defined in `theme.slint`:

- **Dark mode** (default): Professional dark colors
- **Light mode**: Available via `dark-theme: false`

To customize colors, edit the `Theme` global in `theme.slint`.

## Adding New Panels

1. Create a new `.slint` file in this directory
2. Import it in `main.slint`
3. Add the component to the layout
4. Bind callbacks in `slint_ui.rs`

Example:
```slint
// my_panel.slint
import { Theme } from "theme.slint";

export component MyPanel inherits Rectangle {
    in property <string> data: "";
    callback on-action();
    
    background: Theme.panel-background;
    // ... UI definition
}
```

## Build Requirements

- Rust 1.75+
- Slint 1.12
- slint-build 1.12 (build dependency)

The `.slint` files are compiled at build time by `build.rs`.

## Troubleshooting

### "Failed to compile Slint UI"
- Check `.slint` file syntax
- Ensure all imports are correct
- Run `cargo clean` and rebuild

### UI not updating
- Ensure properties are bound correctly
- Check that callbacks are registered in `setup_slint_ui`
- Verify Bevy resources are synced in `sync_bevy_to_slint`

### Viewport not rendering
- Check WGPU texture sharing setup
- Verify render graph configuration
- Ensure viewport Rectangle has correct dimensions
