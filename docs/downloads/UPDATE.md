# Eustress Engine — Update System

## Overview

Eustress Engine uses an in-app self-update system powered by Cloudflare R2 for distribution and GitHub Actions for automated cross-platform builds. When a new version is available, an update button appears in the engine's status bar. Clicking it downloads the new binary, replaces the running executable, and restarts.

## Architecture

```
Developer pushes git tag (e.g., v0.3.0)
    ↓
GitHub Actions triggers (release.yml)
    ├─ Build Windows x64 (.exe + .zip)
    ├─ Build macOS ARM64 (.app + .dmg)
    ├─ Build macOS x64 (.app + .dmg)
    ├─ Build Linux x64 (.tar.gz + .AppImage)
    ↓
Upload artifacts to Cloudflare R2 (eustress-releases bucket)
    ├─ v0.3.0/eustress-engine-0.3.0-windows-x64.zip
    ├─ v0.3.0/eustress-engine-0.3.0-macos-arm64.dmg
    ├─ v0.3.0/eustress-engine-0.3.0-linux-x64.tar.gz
    ↓
Update latest.json manifest
    ↓
Running engines fetch latest.json on startup
    ↓
Compare against compiled-in version
    ↓
Show "Update Available" button if newer
    ↓
User clicks → download → replace exe → restart
```

## Version Manifest (latest.json)

Hosted at: `https://releases.eustress.dev/latest.json`

```json
{
  "version": "0.3.0",
  "date": "2026-04-07",
  "channel": "stable",
  "min_version": "0.1.0",
  "changelog_url": "https://eustress.dev/changelog#v0.3.0",
  "changelog": "Electrochemical simulation, KYC verification, shared scripting runtime",
  "platforms": {
    "windows-x64": {
      "url": "https://releases.eustress.dev/v0.3.0/eustress-engine-0.3.0-windows-x64.zip",
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      "size_bytes": 85000000,
      "installer": false
    },
    "macos-arm64": {
      "url": "https://releases.eustress.dev/v0.3.0/eustress-engine-0.3.0-macos-arm64.dmg",
      "sha256": "abc123...",
      "size_bytes": 82000000,
      "installer": true
    },
    "macos-x64": {
      "url": "https://releases.eustress.dev/v0.3.0/eustress-engine-0.3.0-macos-x64.dmg",
      "sha256": "def456...",
      "size_bytes": 83000000,
      "installer": true
    },
    "linux-x64": {
      "url": "https://releases.eustress.dev/v0.3.0/eustress-engine-0.3.0-linux-x64.tar.gz",
      "sha256": "789ghi...",
      "size_bytes": 80000000,
      "installer": false
    }
  }
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `version` | string | Semantic version (MAJOR.MINOR.PATCH) |
| `date` | string | Release date (ISO 8601) |
| `channel` | string | `stable`, `beta`, or `nightly` |
| `min_version` | string | Minimum version that can auto-update (older must reinstall) |
| `changelog_url` | string | Full changelog page URL |
| `changelog` | string | One-line summary for the update notification |
| `platforms.{platform}.url` | string | Direct download URL from R2 |
| `platforms.{platform}.sha256` | string | SHA-256 hash for integrity verification |
| `platforms.{platform}.size_bytes` | number | Download size for progress display |
| `platforms.{platform}.installer` | bool | Whether the artifact is an installer (DMG) vs portable (ZIP/tar.gz) |

### Platform Keys

| Key | OS | Architecture | Artifact |
|-----|-------|--------------|----------|
| `windows-x64` | Windows 10/11 | x86_64 | `.zip` containing `eustress-engine.exe` |
| `macos-arm64` | macOS 13+ | Apple Silicon | `.dmg` containing `Eustress Engine.app` |
| `macos-x64` | macOS 13+ | Intel | `.dmg` containing `Eustress Engine.app` |
| `linux-x64` | Ubuntu 22.04+ | x86_64 | `.tar.gz` containing `eustress-engine` binary |

## Engine-Side Update Flow

### 1. Check on Startup

```rust
// Runs once at startup (async, non-blocking)
// Compare env!("CARGO_PKG_VERSION") against latest.json
```

The engine fetches `latest.json` in a background thread within 5 seconds of startup. If the fetch fails (offline, timeout), the update check is silently skipped.

### 2. Version Comparison

Uses semantic versioning comparison:
- `0.3.0` > `0.2.1` → update available
- `0.3.0` == `0.3.0` → up to date
- Current version < `min_version` → force update (cannot skip)

### 3. Update Button

When an update is available, the status bar (top-right, next to "Cloud sync ready") shows:

```
[⬆ Update to v0.3.0]  Cloud sync ready  Bliss 0.00  ● Simbuilder
```

The button is:
- Green text on dark background
- Pulsing glow animation to draw attention
- Tooltip shows changelog summary and download size

### 4. Download + Replace

When the user clicks the update button:

1. **Confirm dialog**: "Update Eustress Engine to v0.3.0? (85 MB download). The engine will restart after updating."
2. **Download**: HTTP GET to the platform-specific URL with progress bar in the status bar
3. **Verify**: SHA-256 hash check against manifest
4. **Extract**: Unzip/untar to a temp directory
5. **Replace**:
   - **Windows**: Rename running `.exe` to `.exe.old`, copy new `.exe` in place, spawn new process, exit current
   - **macOS**: Replace `.app` bundle contents, restart via `open -n`
   - **Linux**: Replace binary (atomic rename), exec() into new process
6. **Cleanup**: On next startup, delete `.exe.old` / old binary

### 5. Rollback

If the new version fails to start (crash within 10 seconds):
- **Windows**: `.exe.old` is still present — user can manually rename back
- **macOS/Linux**: Previous version is in the system trash / `/tmp`

## Cloudflare R2 Bucket Layout

```
eustress-releases/
├── latest.json                                          # Version manifest
├── beta.json                                            # Beta channel manifest
├── v0.3.0/
│   ├── eustress-engine-0.3.0-windows-x64.zip
│   ├── eustress-engine-0.3.0-macos-arm64.dmg
│   ├── eustress-engine-0.3.0-macos-x64.dmg
│   └── eustress-engine-0.3.0-linux-x64.tar.gz
├── v0.2.1/
│   ├── eustress-engine-0.2.1-windows-x64.zip
│   └── ...
└── archive/                                             # Older versions
    └── ...
```

### R2 Bucket Configuration

```
Bucket name: eustress-releases
Custom domain: releases.eustress.dev
Public access: Read-only (via custom domain)
CORS: Allow GET from *.eustress.dev and localhost
Cache-Control: latest.json → max-age=300 (5 min), artifacts → max-age=31536000 (1 year)
```

## GitHub Actions CI/CD Pipeline

### Trigger

```yaml
on:
  push:
    tags:
      - 'v*'  # e.g., v0.3.0, v0.3.0-beta.1
```

### Jobs

#### 1. Build Windows

```yaml
build-windows:
  runs-on: windows-latest
  steps:
    - uses: actions/checkout@v4
      with: { submodules: recursive }
    - uses: dtolnay/rust-toolchain@stable
    - run: cargo build --release --package eustress-engine
    - run: |
        mkdir dist
        cp target/release/eustress-engine.exe dist/
        cp -r eustress/crates/engine/assets dist/assets
        cd dist && 7z a ../eustress-engine-${{ github.ref_name }}-windows-x64.zip *
    - uses: actions/upload-artifact@v4
      with:
        name: windows-x64
        path: eustress-engine-*-windows-x64.zip
```

#### 2. Build macOS (ARM64 + x64)

```yaml
build-macos:
  runs-on: macos-14  # ARM64 runner
  strategy:
    matrix:
      target: [aarch64-apple-darwin, x86_64-apple-darwin]
  steps:
    - uses: actions/checkout@v4
      with: { submodules: recursive }
    - uses: dtolnay/rust-toolchain@stable
      with: { targets: ${{ matrix.target }} }
    - run: cargo build --release --package eustress-engine --target ${{ matrix.target }}
    - run: |
        # Create .app bundle and .dmg
        ./scripts/create-macos-bundle.sh ${{ matrix.target }} ${{ github.ref_name }}
    - uses: actions/upload-artifact@v4
      with:
        name: macos-${{ matrix.target }}
        path: eustress-engine-*-macos-*.dmg
```

#### 3. Build Linux

```yaml
build-linux:
  runs-on: ubuntu-22.04
  steps:
    - uses: actions/checkout@v4
      with: { submodules: recursive }
    - run: |
        sudo apt-get update
        sudo apt-get install -y libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
    - uses: dtolnay/rust-toolchain@stable
    - run: cargo build --release --package eustress-engine
    - run: |
        mkdir dist
        cp target/release/eustress-engine dist/
        cp -r eustress/crates/engine/assets dist/assets
        tar czf eustress-engine-${{ github.ref_name }}-linux-x64.tar.gz -C dist .
    - uses: actions/upload-artifact@v4
      with:
        name: linux-x64
        path: eustress-engine-*-linux-x64.tar.gz
```

#### 4. Upload to R2 + Update Manifest

```yaml
upload-r2:
  needs: [build-windows, build-macos, build-linux]
  runs-on: ubuntu-latest
  steps:
    - uses: actions/download-artifact@v4
    - name: Compute SHA256 hashes
      run: |
        for f in **/*.zip **/*.dmg **/*.tar.gz; do
          sha256sum "$f" >> checksums.txt
        done
    - name: Upload to R2
      env:
        AWS_ACCESS_KEY_ID: ${{ secrets.R2_ACCESS_KEY }}
        AWS_SECRET_ACCESS_KEY: ${{ secrets.R2_SECRET_KEY }}
        AWS_ENDPOINT_URL: https://${{ secrets.CF_ACCOUNT_ID }}.r2.cloudflarestorage.com
      run: |
        VERSION=${{ github.ref_name }}
        for f in **/*.zip **/*.dmg **/*.tar.gz; do
          aws s3 cp "$f" "s3://eustress-releases/${VERSION}/$(basename $f)"
        done
    - name: Generate and upload latest.json
      run: |
        python3 scripts/generate-manifest.py \
          --version ${{ github.ref_name }} \
          --checksums checksums.txt \
          --output latest.json
        aws s3 cp latest.json s3://eustress-releases/latest.json \
          --cache-control "max-age=300"
```

## Manual Upload (Without CI)

For quick releases without the full CI pipeline:

```bash
# 1. Build
cargo build --release --package eustress-engine

# 2. Package (Windows example)
mkdir dist && cp target/release/eustress-engine.exe dist/
cd dist && 7z a ../eustress-engine-v0.3.0-windows-x64.zip * && cd ..

# 3. Compute hash
sha256sum eustress-engine-v0.3.0-windows-x64.zip

# 4. Upload to R2
npx wrangler r2 object put eustress-releases/v0.3.0/eustress-engine-v0.3.0-windows-x64.zip \
  --file eustress-engine-v0.3.0-windows-x64.zip

# 5. Update latest.json (edit version, url, sha256, size_bytes)
npx wrangler r2 object put eustress-releases/latest.json \
  --file latest.json \
  --cache-control "max-age=300"
```

## Security

- **SHA-256 verification**: Every download is hash-checked against the manifest before replacement
- **HTTPS only**: All downloads via `https://releases.eustress.dev`
- **No code execution during download**: The new binary is written to disk, verified, then the current process exits and the new one starts
- **Manifest integrity**: `latest.json` is served from Cloudflare's edge with short TTL (5 minutes) — cache poisoning window is minimal
- **Rollback safety**: Old binary preserved as `.old` until new version starts successfully

## User Experience

### Update Available
```
┌─────────────────────────────────────────────────────────┐
│ ⬆ Update to v0.3.0  │ Cloud sync ready │ Bliss 0.00    │
└─────────────────────────────────────────────────────────┘
```

### Downloading
```
┌─────────────────────────────────────────────────────────┐
│ ⬇ Downloading v0.3.0 ████████░░ 72%  │ Bliss 0.00     │
└─────────────────────────────────────────────────────────┘
```

### Ready to Restart
```
┌─────────────────────────────────────────────────────────┐
│ ✓ Update ready — Click to restart    │ Bliss 0.00      │
└─────────────────────────────────────────────────────────┘
```

## Channels

| Channel | Manifest | Update Frequency | Audience |
|---------|----------|------------------|----------|
| `stable` | `latest.json` | Monthly releases | All users |
| `beta` | `beta.json` | Weekly builds | Opt-in testers |
| `nightly` | `nightly.json` | Daily builds | Developers |

Users select their channel in Settings. Default is `stable`.
