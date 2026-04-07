# Eustress Engine Downloads

## How to Download

1. Sign in at [eustress.dev/login](https://eustress.dev/login)
2. Go to [eustress.dev/download](https://eustress.dev/download)
3. Click your platform button — the download starts automatically

Downloads require authentication. Your identity is verified via KYC for platform safety.

## Platforms

| Platform | Format | Requirements |
|----------|--------|-------------|
| **Windows** | `.zip` (portable) | Windows 10+ x64 |
| **macOS (Apple Silicon)** | `.dmg` | macOS 13+ ARM64 |
| **Linux** | `.tar.gz` | Ubuntu 22.04+ x64, Vulkan 1.2 |

## Installation

### Windows
1. Extract the `.zip` to any folder
2. Run `eustress-engine.exe`
3. Optional: pin to taskbar

### macOS
1. Open the `.dmg`
2. Drag `Eustress Engine.app` to Applications
3. First launch: Right-click → Open (bypasses Gatekeeper)

### Linux
1. Extract: `tar xzf eustress-engine-*.tar.gz`
2. Run installer: `chmod +x install.sh && ./install.sh`
3. Launch from application menu or run `eustress-engine`

The installer copies the binary to `~/.local/bin/`, adds a `.desktop` entry, and installs icons for GNOME/KDE/XFCE.

## System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| **CPU** | 4 cores, 2.0 GHz | 8+ cores, 3.0 GHz |
| **RAM** | 8 GB | 16 GB |
| **GPU** | Vulkan 1.2 / DirectX 12 | RTX 3060 / RX 6700 |
| **Storage** | 2 GB | 10 GB (with projects) |

## Updates

Eustress Engine checks for updates automatically on startup. When a new version is available, an update button appears in the status bar. Click to download, verify, and restart.

See [UPDATE.md](UPDATE.md) for full update system documentation.

## Version Check

Current version: **Help → About Eustress Engine**

Latest version: fetched from `releases.eustress.dev/latest.json`

## Building from Source

```bash
git clone https://github.com/WeaveITMeta/EustressEngine.git
cd EustressEngine/eustress
cargo build --release --package eustress-engine
```

## Release Distribution

Releases are built automatically via GitHub Actions on tag push and uploaded to Cloudflare R2. See [UPDATE.md](UPDATE.md) for the CI/CD pipeline details.
