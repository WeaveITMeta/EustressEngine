# Eustress Engine — Start Guide

## Prerequisites

- **Rust** (stable, latest) — `rustup update stable`
- **Git** — required for fetching Slint from source

## Quick Start — Engine (Studio)

```powershell
cd eustress
cargo run -p eustress-engine
```

First build takes ~5–10 minutes (Bevy 0.18 + Slint UI). Subsequent builds are incremental (~30s).

### With a scene file

```powershell
cargo run -p eustress-engine -- --scene path/to/scene.eustress
```

### Release mode (much faster runtime)

```powershell
cargo run -p eustress-engine --release
```

## Quick Start — Client (Player)

```powershell
cd eustress
cargo run -p eustress-client
```

## What Happens on Startup

1. **Camera** spawns at `(10, 8, 10)` looking at origin
2. **Workspace scan** — loads every `.glb.toml` from:
   ```
   C:/Users/.../Documents/Eustress/Universe1/spaces/Space1/Workspace/
   ```
   - `Baseplate.glb.toml` loads first (ground plane)
   - All other `.glb.toml` files load alphabetically (parts, etc.)
   - Each `.glb.toml` references a shared mesh asset (`assets/meshes/block.glb`, etc.)
   - If no `Baseplate.glb.toml` exists, a programmatic fallback baseplate spawns
3. **Lighting** — Sky, Atmosphere, Sun, Moon entities spawn via .toml files.
4. **Slint UI** — Explorer panel, Properties panel, Toolbox, Console

## Run Only (no run)

```powershell
cd eustress

# Engine only
cargo run -p eustress-engine # --release for double clicking .eustress files.

```

## Export Eustress Engine for Market

AAA TODO

## Project Layout

```
eustress/
├── crates/
│   ├── engine/          # Studio — 3D editor, Slint UI, serialization
│   ├── client/          # Player — downloads .eustress from R2, renders
│   ├── common/          # Shared types, realism, scene format, networking
│   ├── web/             # Website (Leptos)
│   ├── backend/         # API server
│   ├── server/          # Game server
│   ├── mcp/             # MCP protocol server
│   └── forge/           # Asset forge pipeline
└── Cargo.toml           # Workspace root
```

## Ports

| Service        | Port  | Notes                    |
|---------------|-------|--------------------------|
| Engine (Studio)| —     | Native desktop window    |
| Client (Player)| —     | Native desktop window    |
| Backend API    | 7000  | REST API                 |
| Web (Leptos)   | 3000  | SSR website              |
| MCP Server     | 7100  | MCP protocol             |

## Troubleshooting

- **Slint build failures** — ensure `git` is available (Slint is fetched from git)
- **No meshes visible** — `.glb` asset files may not exist yet; engine uses procedural fallbacks
- **V-Cell not loading** — check `Workspace/` directory has `.glb.toml` files
