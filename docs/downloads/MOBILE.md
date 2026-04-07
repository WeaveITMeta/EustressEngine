# Mobile Development

## Supported Platforms

| Platform | Target | Toolchain |
|----------|--------|-----------|
| Android | `aarch64-linux-android` | cargo-ndk + Android NDK |
| iOS | `aarch64-apple-ios` | Xcode + rustup target |

## Building for Android

```bash
# Setup (one-time)
rustup target add aarch64-linux-android
cargo install cargo-ndk
export ANDROID_NDK_HOME=/path/to/android-ndk

# Build
cargo ndk -t arm64-v8a -o crates/player-android/app/src/main/jniLibs \
    build --release -p eustress-client

# Package APK
cd crates/player-android
./gradlew assembleRelease
```

## Building for iOS

```bash
# Setup (macOS only)
rustup target add aarch64-apple-ios

# Build
cargo build --release -p eustress-client --target aarch64-apple-ios

# Xcode
cd crates/player-ios
xcodebuild -scheme EustressPlayer -configuration Release archive
```

## Architecture

Mobile apps use the same `eustress-common` crate as desktop, with platform-specific wrappers for touch input, GPU context, and app lifecycle.

```
eustress-common (shared logic, ECS, networking, GUI)
    ↓
eustress-client (desktop + mobile shared binary)
    ↓
player-android / player-ios (platform wrappers)
```
