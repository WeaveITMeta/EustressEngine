# Eustress Engine - Monorepo

A batteries-included Rust game engine and editor built with **Bevy** + **Slint**.

<p align="center">
  <img src="docs/marketing/screenshot.png" alt="Eustress Engine studio — a live V-Cell battery simulation in the native 3D viewport, with the scene Explorer, real-time Properties, and the built-in AI Workshop assistant" width="900">
</p>

<p align="center"><em>The Eustress Engine studio in action — a native Bevy 3D viewport running a live <strong>V-Cell</strong> battery-monitor simulation, alongside the scene Explorer, real-time Properties, and the built-in <strong>AI Workshop</strong> assistant, all in a single Rust + Slint window.</em></p>

## What's Inside

- **Engine** - Desktop 3D editor/studio for scene creation
- **Client** - Generative player/renderer with AI enhancements
- **Common** - Shared scene format, types, and utilities
- **Utilities** - Camera controllers, networking, and more

## Prerequisites

- Rust (latest stable)
- Cargo

## Quick Start

### Run the Engine (Studio)
```bash
cd eustress
cargo run --bin eustress-engine

# Or use the helper script
.\build-and-run.ps1 engine
```

### Run the Client (Player)
```bash
cd eustress
cargo run --bin eustress-client

# Or use the helper script
.\build-and-run.ps1 client
```

### Check All Crates
```bash
cargo check --workspace
```

## Building for Production

```bash
cd eustress

# Build engine
cargo build --bin eustress-engine --release

# Build client
cargo build --bin eustress-client --release

# Build everything
cargo build --workspace --release
```

Binaries output to: `eustress/target/release/`

## Architecture

- **Engine**: Bevy 0.18
- **UI**: Slint (declarative GUI)
- **Language**: 100% Rust
- **Platform**: Desktop only (Windows, macOS, Linux)

## Features

- **Integrated 3D Viewport**: Native Bevy rendering with Slint overlay
- **Scene Hierarchy**: Explorer panel with part tree
- **Properties Editor**: Real-time entity property editing
- **Transform Tools**: Move, Rotate, Scale with visual gizmos
- **Console Output**: Real-time logging and debugging
- **Zero Overhead**: Direct Rust function calls, no IPC

## Project Structure

```
eustress/                     # Workspace root
├── Cargo.toml               # Workspace configuration
├── rust-toolchain.toml      # Rust version pinning
├── build-and-run.ps1        # Build helper script
├── crates/
│   ├── common/              # Shared scene format & types
│   │   ├── src/
│   │   │   ├── scene.rs    # Scene definitions
│   │   │   ├── types.rs    # Common types
│   │   │   └── utils.rs    # Utilities
│   │   └── Cargo.toml
│   ├── engine/              # Desktop editor/studio
│   │   ├── src/
│   │   │   ├── main.rs     # Entry point
│   │   │   ├── ui/         # Slint panels
│   │   │   ├── parts.rs    # Part management
│   │   │   └── rendering.rs
│   │   └── Cargo.toml
│   ├── client/              # Generative player/renderer
│   │   ├── src/
│   │   │   └── main.rs     # Client entry
│   │   └── Cargo.toml
│   ├── bevy-camera-controller/  # Camera utilities
│   └── bevy-webtransport/       # Networking
└── assets/                  # Shared assets
```

See [DESKTOP_EGUI.md](DESKTOP_EGUI.md) for detailed documentation.
