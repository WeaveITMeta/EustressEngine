# Eustress Engine Downloads

This directory contains installers for Eustress Engine that are uploaded to Cloudflare R2.

## Directory Structure

```
downloads/
├── studio/                         # Eustress Engine (Creation Tool)
│   ├── windows/
│   │   └── EustressEngine-Setup.exe
│   ├── mac/
│   │   ├── EustressEngine.dmg
│   │   └── EustressEngine-arm64.dmg
│   ├── linux/
│   │   ├── EustressEngine.AppImage
│   │   ├── eustress-engine.deb
│   │   └── eustress-engine.rpm
│   └── redox/
│       └── eustress-engine.tar.gz
├── player/                         # Eustress Player (Experience Runtime)
│   ├── windows/
│   │   └── EustressPlayer-Setup.exe
│   ├── mac/
│   │   └── EustressPlayer.dmg
│   ├── linux/
│   │   └── EustressPlayer.AppImage
│   ├── redox/
│   │   └── eustress-player.tar.gz
│   └── android/
│       └── EustressPlayer.apk      # Sideload only (Play Store is primary)
└── latest.json                     # Version metadata
```

## Upload to R2

### Using Wrangler CLI

```powershell
# Login (one-time)
wrangler login

# Upload Studio files
wrangler r2 object put eustress-downloads/studio/windows/EustressEngine-Setup.exe --file ./studio/windows/EustressEngine-Setup.exe
wrangler r2 object put eustress-downloads/studio/mac/EustressEngine.dmg --file ./studio/mac/EustressEngine.dmg
wrangler r2 object put eustress-downloads/studio/linux/EustressEngine.AppImage --file ./studio/linux/EustressEngine.AppImage
wrangler r2 object put eustress-downloads/studio/redox/eustress-engine.tar.gz --file ./studio/redox/eustress-engine.tar.gz

# Upload Player files
wrangler r2 object put eustress-downloads/player/windows/EustressPlayer-Setup.exe --file ./player/windows/EustressPlayer-Setup.exe
wrangler r2 object put eustress-downloads/player/mac/EustressPlayer.dmg --file ./player/mac/EustressPlayer.dmg
wrangler r2 object put eustress-downloads/player/linux/EustressPlayer.AppImage --file ./player/linux/EustressPlayer.AppImage
wrangler r2 object put eustress-downloads/player/redox/eustress-player.tar.gz --file ./player/redox/eustress-player.tar.gz
wrangler r2 object put eustress-downloads/player/android/EustressPlayer.apk --file ./player/android/EustressPlayer.apk

# Upload metadata
wrangler r2 object put eustress-downloads/latest.json --file ./latest.json
```

### Using Cloudflare Dashboard

1. Go to **R2** → **eustress-downloads** bucket
2. Navigate to the appropriate folder
3. Click **Upload** and select the file

## Building Installers

### Windows (Inno Setup)

1. Build release: `cargo build --release -p eustress-studio`
2. Run Inno Setup with `installer/windows.iss`
3. Output: `windows/EustressEngine-Setup.exe`

### macOS (DMG)

```bash
cargo build --release -p eustress-studio
create-dmg EustressEngine.dmg target/release/EustressEngine.app
```

### Linux (AppImage)

```bash
cargo build --release -p eustress-studio
cargo appimage  # requires cargo-appimage
```

### Redox OS (Studio)

```bash
# Cross-compile for Redox (requires redoxer)
# See: https://gitlab.redox-os.org/redox-os/redoxer
cargo install redoxer
redoxer build --release -p eustress-studio

# Package as tarball
tar -czvf eustress-engine.tar.gz -C target/x86_64-unknown-redox/release eustress-studio
```

### Redox OS (Player)

```bash
# Cross-compile for Redox (requires redoxer)
cargo install redoxer
redoxer build --release -p eustress-player

# Package as tarball
tar -czvf eustress-player.tar.gz -C target/x86_64-unknown-redox/release eustress-player
```

## Updating latest.json

After uploading new installers:

1. Update `version` and `release_date`
2. Calculate SHA256: `certutil -hashfile <file> SHA256` (Windows) or `sha256sum <file>` (Linux/Mac)
3. Update file sizes in bytes
4. Upload `latest.json` to R2

## Download URLs

### Studio (Creation Tool)
- **Windows**: https://downloads.eustress.dev/studio/windows/EustressEngine-Setup.exe
- **macOS (Intel)**: https://downloads.eustress.dev/studio/mac/EustressEngine.dmg
- **macOS (Apple Silicon)**: https://downloads.eustress.dev/studio/mac/EustressEngine-arm64.dmg
- **Linux (AppImage)**: https://downloads.eustress.dev/studio/linux/EustressEngine.AppImage
- **Linux (Deb)**: https://downloads.eustress.dev/studio/linux/eustress-engine.deb
- **Linux (RPM)**: https://downloads.eustress.dev/studio/linux/eustress-engine.rpm
- **Redox OS**: https://downloads.eustress.dev/studio/redox/eustress-engine.tar.gz

### Player (Experience Runtime)
- **Windows**: https://downloads.eustress.dev/player/windows/EustressPlayer-Setup.exe
- **macOS**: https://downloads.eustress.dev/player/mac/EustressPlayer.dmg
- **Linux**: https://downloads.eustress.dev/player/linux/EustressPlayer.AppImage
- **Redox OS**: https://downloads.eustress.dev/player/redox/eustress-player.tar.gz
- **Android (APK)**: https://downloads.eustress.dev/player/android/EustressPlayer.apk
- **Android (Play Store)**: https://play.google.com/store/apps/details?id=dev.eustress.player
- **iOS (App Store)**: https://apps.apple.com/app/eustress-player/id123456789

## Cloudflare Worker (Analytics)

Deploy the download worker for analytics and versioning:

```powershell
cd infrastructure/cloudflare
wrangler deploy
```

The worker provides:
- `/api/latest` - Version info JSON
- `/api/download/:platform` - Download with analytics tracking
- `/api/stats` - Download statistics
