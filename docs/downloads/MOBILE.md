# Mobile Development Guide

## Crate Structure

Mobile apps share code with the desktop client via the `common` crate:

```
eustress/crates/
├── common/                    # Shared game logic, ECS, networking
│   ├── src/
│   │   ├── lib.rs
│   │   ├── game/             # Game state, entities
│   │   ├── network/          # Multiplayer client
│   │   ├── assets/           # Asset loading
│   │   └── input/            # Abstract input (touch/mouse/keyboard)
│   └── Cargo.toml
│
├── player/                    # Desktop player (Windows/Mac/Linux)
│   ├── src/main.rs
│   └── Cargo.toml            # depends on common
│
├── player-android/            # Android wrapper
│   ├── app/
│   │   ├── src/main/
│   │   │   ├── java/dev/eustress/player/
│   │   │   │   └── MainActivity.java
│   │   │   ├── jniLibs/      # Built .so files go here
│   │   │   └── AndroidManifest.xml
│   │   └── build.gradle
│   ├── build.gradle
│   └── settings.gradle
│
├── player-ios/                # iOS wrapper
│   ├── EustressPlayer/
│   │   ├── AppDelegate.swift
│   │   ├── Info.plist
│   │   └── EustressPlayer.entitlements
│   ├── EustressPlayer.xcodeproj/
│   └── ExportOptions.plist
│
└── player-mobile/             # Shared mobile Rust code
    ├── src/
    │   ├── lib.rs
    │   ├── touch.rs          # Touch input handling
    │   ├── gestures.rs       # Pinch, swipe, tap
    │   └── ui.rs             # Mobile-specific UI
    └── Cargo.toml            # depends on common
```

## Cargo.toml Dependencies

### common/Cargo.toml
```toml
[package]
name = "eustress-common"
version = "0.1.0"

[features]
default = []
mobile = ["bevy/android_shared_stdcxx"]

[dependencies]
bevy = { version = "0.14", default-features = false, features = [
    "bevy_asset",
    "bevy_render",
    "bevy_winit",
    "bevy_core_pipeline",
    "bevy_pbr",
    "bevy_sprite",
    "bevy_text",
    "bevy_ui",
] }
```

### player-mobile/Cargo.toml
```toml
[package]
name = "eustress-player-mobile"
version = "0.1.0"

[lib]
crate-type = ["cdylib", "staticlib"]

[dependencies]
eustress-common = { path = "../common", features = ["mobile"] }
bevy = { version = "0.14", features = ["android_shared_stdcxx"] }

[target.'cfg(target_os = "android")'.dependencies]
ndk-glue = "0.7"
android_logger = "0.13"

[target.'cfg(target_os = "ios")'.dependencies]
# iOS-specific deps
```

## Touch Input System

```rust
// common/src/input/mod.rs
pub enum InputAction {
    // Abstract actions work on all platforms
    Move(Vec2),
    Look(Vec2),
    Jump,
    Interact,
    Menu,
}

// player-mobile/src/touch.rs
pub fn touch_to_input(touches: &Touches) -> Vec<InputAction> {
    let mut actions = vec![];
    
    // Virtual joystick (left side of screen)
    if let Some(touch) = touches.iter().find(|t| t.position().x < 200.0) {
        let delta = touch.position() - touch.start_position();
        actions.push(InputAction::Move(delta.normalize_or_zero()));
    }
    
    // Look (right side drag)
    if let Some(touch) = touches.iter().find(|t| t.position().x > 600.0) {
        actions.push(InputAction::Look(touch.delta()));
    }
    
    actions
}
```

## Mobile UI Components

```rust
// player-mobile/src/ui.rs
use bevy::prelude::*;

pub struct MobileUiPlugin;

impl Plugin for MobileUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_mobile_ui)
           .add_systems(Update, handle_touch_buttons);
    }
}

fn setup_mobile_ui(mut commands: Commands) {
    // Virtual joystick
    commands.spawn((
        NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                left: Val::Px(20.0),
                bottom: Val::Px(20.0),
                width: Val::Px(150.0),
                height: Val::Px(150.0),
                ..default()
            },
            background_color: Color::rgba(1.0, 1.0, 1.0, 0.3).into(),
            ..default()
        },
        VirtualJoystick,
    ));
    
    // Action buttons (right side)
    // Jump, Interact, Menu buttons...
}
```

## Building for Mobile

### Android
```bash
# One-time setup
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install cargo-ndk

# Set NDK path
export ANDROID_NDK_HOME=/path/to/android-ndk

# Build
cargo ndk -t arm64-v8a -t armeabi-v7a -o crates/player-android/app/src/main/jniLibs \
    build --release -p eustress-player-mobile

# Package
cd crates/player-android
./gradlew assembleRelease
```

### iOS
```bash
# One-time setup (macOS only)
rustup target add aarch64-apple-ios

# Build
cargo build --release -p eustress-player-mobile --target aarch64-apple-ios

# Copy to Xcode project
cp target/aarch64-apple-ios/release/libeustress_player_mobile.a crates/player-ios/

# Build with Xcode
cd crates/player-ios
xcodebuild -scheme EustressPlayer -configuration Release archive
```

## Store Assets Required

### Google Play
- **Icon**: 512x512 PNG
- **Feature Graphic**: 1024x500 PNG
- **Screenshots**: Min 2, recommended 8 (phone + tablet)
- **Short Description**: 80 chars max
- **Full Description**: 4000 chars max
- **Privacy Policy URL**: Required

### App Store
- **Icon**: 1024x1024 PNG (no alpha)
- **Screenshots**: 6.7", 6.5", 5.5" iPhone + iPad sizes
- **App Preview Video**: Optional, 15-30 sec
- **Description**: 4000 chars max
- **Keywords**: 100 chars max
- **Privacy Policy URL**: Required
- **Support URL**: Required

## Downloads Directory Structure

```
downloads/
├── windows/                   # Engine desktop
├── mac/
├── linux/
├── redox/
├── player/                    # Player apps
│   ├── windows/
│   │   └── EustressPlayer-Setup.exe
│   ├── mac/
│   │   └── EustressPlayer.dmg
│   ├── linux/
│   │   └── EustressPlayer.AppImage
│   └── android/
│       └── EustressPlayer.apk    # Sideload APK (not on Play Store)
└── latest.json
```

**Note**: iOS apps cannot be distributed outside App Store. Android APK is for sideloading only; primary distribution is via Google Play.
