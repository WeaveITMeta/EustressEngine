# Eustress Player Downloads

The lightweight runtime for playing Eustress experiences.

## How to Download

1. Sign in at [eustress.dev/login](https://eustress.dev/login)
2. Go to [eustress.dev/download-player](https://eustress.dev/download-player)
3. Click your platform button

## Platforms

| Platform | Format | Requirements |
|----------|--------|-------------|
| **Windows** | `.exe` installer | Windows 10+ |
| **macOS** | `.dmg` | macOS 13+ |
| **Linux** | `.AppImage` | Ubuntu 22.04+ |
| **Android** | `.apk` / Google Play | Android 10+ |
| **iOS** | App Store | iOS 15+ |
| **Web** | [play.eustress.dev](https://play.eustress.dev) | Modern browser with WebGPU |

## System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| **CPU** | 2 cores, 1.5 GHz | 4 cores, 2.5 GHz |
| **RAM** | 4 GB | 8 GB |
| **GPU** | Vulkan 1.1 / WebGPU | Vulkan 1.2 / DirectX 12 |
| **Storage** | 500 MB | 2 GB (cached games) |

## Launching Games

```bash
# From URL
eustress://play/game-id

# From file
eustress-player --file "/path/to/game.eustress"

# From command line
eustress-player --game "game-id"
```

## Engine vs Player

| Feature | Engine | Player |
|---------|--------|--------|
| Create experiences | Yes | No |
| Play experiences | Yes | Yes |
| Edit scripts | Yes | No |
| Size | ~150 MB | ~50 MB |
| Target users | Developers | Players |
| Mobile support | No | Yes |
