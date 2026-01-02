# Bevy Camera Controller

**Multi-mode camera controller for Bevy with desktop, mobile, and touch support**

## Features

- ✅ **Flycam Mode** - WASD + mouse look for free flight
- ✅ **RTS Mode** - Pan, zoom, rotate for strategy games
- ✅ **Touch Support** - Mobile gestures (pinch zoom, pan, rotate)
- ✅ **Smooth Transitions** - Switch between modes seamlessly
- ✅ **Configurable** - All controls and speeds customizable

## Usage

```rust
use bevy::prelude::*;
use bevy_camera_controller::{CameraControllerPlugin, CameraController, CameraMode};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(CameraControllerPlugin)
        .run();
}

// Setup camera with specific mode
fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        CameraController {
            mode: CameraMode::Flycam,
            ..default()
        },
    ));
}

// Switch modes at runtime
fn switch_mode(
    keys: Res<ButtonInput<KeyCode>>,
    mut controllers: Query<&mut CameraController>,
) {
    if keys.just_pressed(KeyCode::F1) {
        for mut controller in &mut controllers {
            controller.mode = CameraMode::Flycam;
        }
    }
    if keys.just_pressed(KeyCode::F2) {
        for mut controller in &mut controllers {
            controller.mode = CameraMode::RTS;
        }
    }
}
```

## Camera Modes

### Flycam
- WASD - Move horizontally
- Q/E - Move up/down
- Mouse - Look around
- Shift - Run
- Scroll - Adjust speed

### RTS
- WASD / Arrow Keys - Pan camera
- Mouse drag - Pan camera
- Scroll - Zoom in/out
- Right mouse drag - Rotate around focus
- Edge scrolling (optional)

### Touch (Mobile)
- One finger drag - Pan
- Two finger pinch - Zoom
- Two finger drag - Pan
- Two finger rotate - Rotate around focus

## Configuration

```rust
CameraController {
    enabled: true,
    mode: CameraMode::Flycam,
    
    // Flycam settings
    fly_speed: 5.0,
    fly_run_speed: 15.0,
    fly_sensitivity: 1.0,
    
    // RTS settings
    rts_pan_speed: 10.0,
    rts_zoom_speed: 5.0,
    rts_rotate_speed: 1.0,
    rts_min_zoom: 5.0,
    rts_max_zoom: 100.0,
    rts_edge_scroll_margin: 50.0, // pixels from edge
    
    // Touch settings
    touch_pan_speed: 1.0,
    touch_zoom_speed: 0.5,
    touch_rotate_speed: 1.0,
    
    ..default()
}
```

## Platform Support

- ✅ Windows, macOS, Linux (desktop)
- ✅ Web (WASM)
- ✅ iOS, Android (via Tauri Mobile)
- ✅ Touch screens on all platforms
