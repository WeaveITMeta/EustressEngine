# macOS Icon Bundling Guide

## Overview

On macOS, application icons cannot be set at runtime via `winit::window::set_window_icon()`. Instead, the icon must be bundled in the `.app` package structure and referenced in `Info.plist`.

## Build Process

The build script (`crates/engine/build.rs`) automatically generates `assets/icon.icns` from `assets/icon.svg` with all required icon sizes:
- 16x16, 32x32, 64x64, 128x128, 256x256, 512x512, 1024x1024

This happens automatically during `cargo build` if the SVG is newer than the ICNS file.

## Manual Bundling for Development

When running the engine directly via `cargo run`, the icon won't appear in the Dock because the executable isn't bundled as a `.app`. This is expected behavior during development.

## Creating a .app Bundle for Distribution

To create a proper macOS application bundle with the icon:

### 1. Build the release binary
```bash
cargo build --release --bin eustress-engine
```

### 2. Create the .app directory structure
```bash
mkdir -p EustressEngine.app/Contents/MacOS
mkdir -p EustressEngine.app/Contents/Resources
```

### 3. Copy the binary
```bash
cp target/release/eustress-engine EustressEngine.app/Contents/MacOS/
```

### 4. Copy the icon
```bash
cp crates/engine/assets/icon.icns EustressEngine.app/Contents/Resources/
```

### 5. Create Info.plist
Create `EustressEngine.app/Contents/Info.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>eustress-engine</string>
    <key>CFBundleIconFile</key>
    <string>icon.icns</string>
    <key>CFBundleIdentifier</key>
    <string>com.eustressengine.studio</string>
    <key>CFBundleName</key>
    <string>Eustress Engine</string>
    <key>CFBundleDisplayName</key>
    <string>Eustress Engine</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
```

### 6. Launch the app
```bash
open EustressEngine.app
```

The icon should now appear in the Dock and Finder.

## Automated Bundling with cargo-bundle

For automated bundling, consider using `cargo-bundle`:

```bash
cargo install cargo-bundle
```

Add to `Cargo.toml`:
```toml
[package.metadata.bundle]
name = "Eustress Engine"
identifier = "com.eustressengine.studio"
icon = ["crates/engine/assets/icon.icns"]
version = "0.1.0"
resources = ["assets"]
copyright = "Copyright © 2025 EustressEngine Contributors"
category = "Developer Tool"
short_description = "3D game engine and editor"
```

Then run:
```bash
cargo bundle --release
```

## Troubleshooting

### Icon doesn't appear in Dock
- Ensure `icon.icns` is in `Contents/Resources/`
- Verify `Info.plist` has `CFBundleIconFile` set to `icon.icns`
- Try clearing the icon cache: `sudo rm -rf /Library/Caches/com.apple.iconservices.store`
- Restart Finder: `killall Finder`

### Icon appears blurry
- Ensure all icon sizes (16-1024px) are present in the ICNS file
- The build script generates all required sizes automatically

### Development builds don't show icon
- This is expected — icons only work in proper `.app` bundles
- Use the bundling process above for testing icon appearance

## Runtime Behavior

The engine's `set_window_icon()` function in `src/ui/slint_ui.rs` automatically skips icon loading on macOS with an info log message:

```
ℹ️  set_window_icon: skipped on macOS (icon must be bundled in .app package)
```

This is normal and expected behavior.
