# Eustress Player Downloads

Eustress Player is the lightweight runtime for playing Eustress experiences.

## Download Links

| Platform | Download | Requirements |
|----------|----------|--------------|
| **Windows** | [EustressPlayer-Setup.exe](https://downloads.eustress.dev/player/windows/EustressPlayer-Setup.exe) | Windows 10+ |
| **macOS** | [EustressPlayer.dmg](https://downloads.eustress.dev/player/mac/EustressPlayer.dmg) | macOS 11.0+ |
| **Linux** | [EustressPlayer.AppImage](https://downloads.eustress.dev/player/linux/EustressPlayer.AppImage) | Ubuntu 20.04+ |
| **Web** | [play.eustress.dev](https://play.eustress.dev) | Modern browser (Chrome, Firefox, Safari) |

## Installation

### Windows
1. Download and run `EustressPlayer-Setup.exe`
2. Follow installation wizard
3. `.eustress` files will auto-open with Player

### macOS
1. Download `.dmg` and drag to Applications
2. First launch: Right-click → Open

### Linux
```bash
chmod +x EustressPlayer.AppImage
./EustressPlayer.AppImage
```

### Web (No Install)
Visit [play.eustress.dev](https://play.eustress.dev) and enter a game code or URL.

## System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| **CPU** | 2 cores, 1.5 GHz | 4 cores, 2.5 GHz |
| **RAM** | 4 GB | 8 GB |
| **GPU** | Vulkan 1.1 / WebGPU | Vulkan 1.2 / DirectX 12 |
| **Storage** | 500 MB | 2 GB (cached games) |

## Launching Games

### From URL
```
eustress://play/game-id
```

### From File
Double-click any `.eustress` or `.eup` file.

### From Command Line
```bash
eustress-player --game "game-id"
eustress-player --file "/path/to/game.eustress"
```

## Mobile Downloads

| Platform | Download | Requirements |
|----------|----------|--------------|
| **Android** | [Google Play](https://play.google.com/store/apps/details?id=dev.eustress.player) | Android 10+ |
| **Android (APK)** | [EustressPlayer.apk](https://downloads.eustress.dev/player/android/EustressPlayer.apk) | Android 10+ |
| **iOS** | [App Store](https://apps.apple.com/app/eustress-player/id123456789) | iOS 15+ |

## Building Installers

### Windows (Inno Setup)
```powershell
cargo build --release -p eustress-player
iscc installer/windows/eustress-player.iss
# Output: downloads/player/windows/EustressPlayer-Setup.exe
```

### macOS (DMG)
```bash
cargo build --release -p eustress-player
./scripts/bundle-macos.sh player
create-dmg downloads/player/mac/EustressPlayer.dmg target/release/bundle/EustressPlayer.app
```

### Linux (AppImage)
```bash
cargo build --release -p eustress-player
cargo appimage --release -p eustress-player
mv target/appimage/eustress-player.AppImage downloads/player/linux/EustressPlayer.AppImage
```

### Android (Native Bevy)
```bash
# Install Android NDK and set ANDROID_NDK_HOME
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install cargo-ndk

# Build native libraries
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o crates/player-android/app/src/main/jniLibs build --release -p eustress-player

# Build APK/AAB with Gradle
cd eustress/crates/player-android
./gradlew assembleRelease      # APK for sideload
./gradlew bundleRelease        # AAB for Play Store

mv app/build/outputs/apk/release/app-release.apk ../../../downloads/player/android/EustressPlayer.apk
```

### iOS (Native Bevy)
```bash
# Requires macOS with Xcode
rustup target add aarch64-apple-ios
cargo build --release -p eustress-player --target aarch64-apple-ios

# Build with Xcode
cd eustress/crates/player-ios
xcodebuild -scheme EustressPlayer -configuration Release -archivePath build/EustressPlayer.xcarchive archive
xcodebuild -exportArchive -archivePath build/EustressPlayer.xcarchive -exportPath build -exportOptionsPlist ExportOptions.plist
# Upload .ipa to App Store Connect via Transporter
```

## Upload to Cloudflare R2

```powershell
wrangler r2 object put eustress-downloads/player/windows/EustressPlayer-Setup.exe --file downloads/player/windows/EustressPlayer-Setup.exe
wrangler r2 object put eustress-downloads/player/mac/EustressPlayer.dmg --file downloads/player/mac/EustressPlayer.dmg
wrangler r2 object put eustress-downloads/player/linux/EustressPlayer.AppImage --file downloads/player/linux/EustressPlayer.AppImage
wrangler r2 object put eustress-downloads/player/android/EustressPlayer.apk --file downloads/player/android/EustressPlayer.apk
```

## Difference: Engine vs Player

| Feature | Engine | Player |
|---------|--------|--------|
| Create games | ✅ | ❌ |
| Play games | ✅ | ✅ |
| Edit scripts | ✅ | ❌ |
| File size | ~500 MB | ~100 MB |
| Target users | Developers | Players |
| Mobile support | ❌ | ✅ |
