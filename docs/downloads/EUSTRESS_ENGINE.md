# Eustress Engine Downloads

## Download Links

| Platform | Download | Requirements |
|----------|----------|--------------|
| **Windows** | [EustressEngine-Setup.exe](https://downloads.eustress.dev/studio/windows/EustressEngine-Setup.exe) | Windows 10+ |
| **macOS** | [EustressEngine.dmg](https://downloads.eustress.dev/studio/mac/EustressEngine.dmg) | macOS 11.0+ |
| **macOS (Apple Silicon)** | [EustressEngine-arm64.dmg](https://downloads.eustress.dev/studio/mac/EustressEngine-arm64.dmg) | macOS 11.0+ (M1/M2/M3) |
| **Linux (AppImage)** | [EustressEngine.AppImage](https://downloads.eustress.dev/studio/linux/EustressEngine.AppImage) | Ubuntu 20.04+ / Fedora 34+ |
| **Linux (Debian)** | [eustress-engine.deb](https://downloads.eustress.dev/studio/linux/eustress-engine.deb) | Debian 11+ / Ubuntu 20.04+ |
| **Linux (RPM)** | [eustress-engine.rpm](https://downloads.eustress.dev/studio/linux/eustress-engine.rpm) | Fedora 34+ / RHEL 8+ |
| **Redox OS** | [eustress-engine.tar.gz](https://downloads.eustress.dev/studio/redox/eustress-engine.tar.gz) | Redox 0.8.0+ |

## Installation

### Windows
1. Download and run `EustressEngine-Setup.exe`
2. Follow the installation wizard
3. Launch from Start Menu or Desktop shortcut

### macOS
1. Download the `.dmg` file
2. Open and drag Eustress Engine to Applications
3. First launch: Right-click → Open (to bypass Gatekeeper)

### Linux (AppImage)
```bash
chmod +x EustressEngine.AppImage
./EustressEngine.AppImage
```

### Linux (Debian/Ubuntu)
```bash
sudo dpkg -i eustress-engine.deb
sudo apt-get install -f  # Install dependencies
```

### Linux (Fedora/RHEL)
```bash
sudo rpm -i eustress-engine.rpm
```

### Redox OS
```bash
tar -xzvf eustress-engine.tar.gz
./eustress-studio
```

## System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| **CPU** | 4 cores, 2.0 GHz | 8 cores, 3.0 GHz |
| **RAM** | 8 GB | 16 GB |
| **GPU** | Vulkan 1.2 / DirectX 12 | RTX 2060 / RX 5700 |
| **Storage** | 2 GB | 10 GB (with assets) |

## Building Installers

### Windows (Inno Setup)
```powershell
# Build release binary
cargo build --release -p eustress-engine

# Create installer with Inno Setup
# Edit installer/windows/eustress-engine.iss with paths
iscc installer/windows/eustress-engine.iss
# Output: downloads/studio/windows/EustressEngine-Setup.exe
```

### macOS (DMG)
```bash
# Build release
cargo build --release -p eustress-engine

# Create .app bundle (use bundler script)
./scripts/bundle-macos.sh

# Create DMG
create-dmg \
  --volname "Eustress Engine" \
  --window-size 600 400 \
  --icon-size 100 \
  --app-drop-link 450 200 \
  downloads/studio/mac/EustressEngine.dmg \
  target/release/bundle/EustressEngine.app
```

### Linux (AppImage)
```bash
# Build release
cargo build --release -p eustress-studio

# Using cargo-appimage
cargo install cargo-appimage
cargo appimage --release
mv target/appimage/eustress-studio.AppImage downloads/studio/linux/EustressEngine.AppImage
```

### Linux (Debian .deb)
```bash
cargo install cargo-deb
cargo deb -p eustress-studio
mv target/debian/*.deb downloads/studio/linux/eustress-engine.deb
```

### Linux (RPM)
```bash
cargo install cargo-generate-rpm
cargo build --release -p eustress-studio
cargo generate-rpm -p crates/studio
mv target/generate-rpm/*.rpm downloads/studio/linux/eustress-engine.rpm
```

### Redox OS
```bash
cargo install redoxer
redoxer build --release -p eustress-studio
tar -czvf downloads/studio/redox/eustress-engine.tar.gz \
  -C target/x86_64-unknown-redox/release eustress-studio
```

## Upload to Cloudflare R2

```powershell
# Install wrangler (one-time)
npm install -g wrangler
wrangler login

# Upload installers
wrangler r2 object put eustress-downloads/studio/windows/EustressEngine-Setup.exe --file downloads/studio/windows/EustressEngine-Setup.exe
wrangler r2 object put eustress-downloads/studio/mac/EustressEngine.dmg --file downloads/studio/mac/EustressEngine.dmg
wrangler r2 object put eustress-downloads/studio/linux/EustressEngine.AppImage --file downloads/studio/linux/EustressEngine.AppImage
wrangler r2 object put eustress-downloads/studio/linux/eustress-engine.deb --file downloads/studio/linux/eustress-engine.deb
wrangler r2 object put eustress-downloads/studio/linux/eustress-engine.rpm --file downloads/studio/linux/eustress-engine.rpm
wrangler r2 object put eustress-downloads/studio/redox/eustress-engine.tar.gz --file downloads/studio/redox/eustress-engine.tar.gz
wrangler r2 object put eustress-downloads/latest.json --file downloads/latest.json
```

## Verify Download (SHA256)

Check `https://downloads.eustress.dev/latest.json` for current checksums.

```bash
# Linux/macOS
sha256sum EustressEngine-Setup.exe

# Windows PowerShell
Get-FileHash EustressEngine-Setup.exe -Algorithm SHA256
```
