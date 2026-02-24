# Space Architecture - File-System-First Simulation Engine

## Overview

Eustress uses a **Universe → Spaces** hierarchy where each Space is a self-contained simulation environment stored as a folder on disk. This enables git-native workflows, sparse checkout for packages, and seamless teleportation between Spaces.

## Directory Structure

```
Documents/Eustress/
└── Universe1/                              ← Universe container (project root)
    ├── .eustress/
    │   ├── universe.toml                   ← Universe metadata
    │   └── cache/                          ← Derived data (gitignored)
    │       ├── shaders/
    │       ├── textures/
    │       └── meshes/
    ├── assets/                             ← Shared assets across all Spaces
    │   ├── textures/
    │   ├── sounds/
    │   └── models/
    └── spaces/
        ├── Space1/                         ← Individual Space (scene)
        │   ├── .eustress/
        │   │   ├── space.toml              ← Space metadata
        │   │   └── packages.toml           ← Package dependencies
        │   ├── Workspace/                  ← Roblox-style service folders
        │   │   ├── Baseplate.glb
        │   │   └── Welcome Cube.glb
        │   ├── Lighting/
        │   │   ├── Sun.glb
        │   │   └── Sky.glb
        │   ├── Players/
        │   ├── ServerStorage/
        │   ├── SoulService/                ← Scripts
        │   ├── SoundService/
        │   ├── StarterCharacterScripts/
        │   ├── StarterGui/
        │   ├── StarterPack/
        │   ├── StarterPlayerScripts/
        │   ├── Teams/
        │   ├── scenes/
        │   │   └── main.gltf               ← Space scene file
        │   └── packages/                   ← Sparse checkouts
        │       ├── @creator/combat-system/
        │       └── @creator/ui-library/
        │
        ├── MyRPG/                          ← Another Space
        │   └── ... (same structure)
        │
        └── CityBuilder/                    ← Another Space
            └── ... (same structure)
```

## Core Concepts

### 1. Universe
- **Container** for multiple Spaces
- **Shared assets** folder for textures, models, sounds used across Spaces
- **Global cache** for derived data (GPU textures, compiled shaders)
- **One Universe per project** (e.g., "My Game", "WATER Project")

### 2. Space
- **Self-contained simulation environment** = One scene = One folder
- **Player-named** (e.g., "Space1", "MyRPG", "City Builder", "Space Station")
- **Roblox-style services** as folders (Workspace, Lighting, Players, etc.)
- **Git-native** - each Space can be its own git repository
- **Teleportation** - seamlessly transition between Spaces at runtime

### 3. Services (Folders)
Each Space contains Roblox-style service folders:

| Service | Purpose | Contains |
|---------|---------|----------|
| **Workspace** | 3D world objects | Parts, models, terrain (.glb files) |
| **Lighting** | Light sources | Sun, Sky, Atmosphere (.glb files) |
| **Players** | Player data | Player instances, character models |
| **ServerStorage** | Server-only assets | Hidden from clients |
| **SoulService** | Scripts | .soul script files |
| **SoundService** | Audio | Sound effects, music (.ogg, .mp3) |
| **StarterCharacterScripts** | Character scripts | Scripts attached to spawned characters |
| **StarterGui** | UI templates | GUI definitions (.slint, .html) |
| **StarterPack** | Starting tools | Tools given to players on spawn |
| **StarterPlayerScripts** | Player scripts | Scripts run on player join |
| **Teams** | Team definitions | Team metadata and spawn points |

### 4. File-System-First Philosophy
- **No proprietary formats** - Everything is standard glTF, TOML, JSON
- **Git-diffable** - All files are text or standard binary formats
- **External editors** - Edit with any tool (Monaco, VS Code, Blender)
- **No import/export** - Files are used directly from disk
- **Cache is ephemeral** - `.eustress/cache/` is fully rebuildable

## Space Metadata (`space.toml`)

```toml
[space]
name = "Space1"
version = "1.0.0"
description = "Default starter Space"
author = "Player Name"
created = "2026-02-24T12:00:00Z"

[spawn]
# Default spawn point for players
position = [0.0, 5.0, 0.0]
rotation = [0.0, 0.0, 0.0]

[physics]
gravity = -9.81
air_density = 1.225

[rendering]
skybox = "Lighting/Sky.glb"
ambient_light = [0.1, 0.1, 0.1]

[packages]
# Dependencies from other Spaces or remote sources
# See packages.toml for full manifest
```

## Package System (Sparse Checkout)

### `packages.toml` Format

```toml
# Git-like sparse checkout for cross-Space code reuse
# Similar to Roblox packages but file-system-first

[[package]]
name = "combat-system"
source = "git+https://github.com/creator/combat-system.git"
version = "1.2.0"
path = "packages/@creator/combat-system"
sparse = true  # Only checkout specific files

[[package]]
name = "ui-library"
source = "local:../SharedLibrary/ui"
version = "2.0.0"
path = "packages/@creator/ui-library"

[[package]]
name = "terrain-tools"
source = "r2://my-bucket/packages/terrain-tools-v1.0.0.pak"
version = "1.0.0"
path = "packages/@creator/terrain-tools"
cached = true  # Load from .pak file
```

### Package Sources

1. **Git repositories** - `git+https://...` (standard git clone)
2. **Local paths** - `local:../path` (symlink or copy)
3. **Cloudflare R2** - `r2://bucket/path.pak` (compressed .pak files)
4. **HTTP** - `https://cdn.example.com/package.pak` (direct download)

## .pak Files (Distribution Format)

### Structure
```
package.pak (LZ4 compressed archive)
├── manifest.toml           ← Package metadata
├── Workspace/
│   └── models/
│       └── weapon.glb
├── SoulService/
│   └── combat.soul
└── assets/
    └── textures/
        └── weapon_diffuse.png
```

### Loading from Cloudflare R2
```rust
// Load remote Space from .pak file
let space_url = "https://r2.example.com/spaces/MyRPG-v1.0.0.pak";
engine.load_remote_space(space_url).await?;

// Teleport to remote Space
player.teleport_to_space("MyRPG").await?;
```

## Space Creation

### CLI Tool
```bash
# Create new Space in current Universe
eustress space create "My RPG"

# Create new Universe with default Space
eustress universe create "My Game" --space "Lobby"

# Clone Space from template
eustress space clone "Space1" "Space2"

# Export Space to .pak file
eustress space pack "MyRPG" --output MyRPG-v1.0.0.pak

# Publish Space to Cloudflare R2
eustress space publish "MyRPG" --bucket my-bucket
```

### Programmatic API
```rust
use eustress::space::{SpaceBuilder, Universe};

// Create new Space
let space = SpaceBuilder::new("My RPG")
    .in_universe("Universe1")
    .with_baseplate(true)
    .with_welcome_cube(true)
    .with_services(&["Workspace", "Lighting", "Players"])
    .create()?;

// Load Space
let universe = Universe::open("C:/Users/miksu/Documents/Eustress/Universe1")?;
let space = universe.load_space("Space1")?;
```

## Space Teleportation

### Seamless Transitions
```rust
// Teleport player to another Space in same Universe
player.teleport_to_space("CityBuilder").await?;

// Teleport to remote Space (loads from .pak)
player.teleport_to_remote_space("r2://bucket/SpaceStation.pak").await?;

// Bring items across Spaces
player.inventory.transfer_to_space("MyRPG", vec![sword, shield]).await?;
```

### Implementation
1. **Save player state** (position, inventory, health)
2. **Unload current Space** (despawn entities, free memory)
3. **Load target Space** (from disk or .pak file)
4. **Restore player state** (spawn at target spawn point)
5. **Transition effect** (fade, loading screen)

## Git Integration

### Per-Space Repositories
```bash
cd Universe1/spaces/Space1
git init
git add .
git commit -m "Initial Space setup"
git remote add origin https://github.com/user/space1.git
git push -u origin main
```

### Collaborative Workflow
```bash
# Developer A: Create combat system
cd Universe1/spaces/CombatDemo
git checkout -b feature/new-weapon
# ... edit files ...
git commit -am "Add laser sword"
git push origin feature/new-weapon

# Developer B: Review and merge
git pull origin feature/new-weapon
# ... test in engine ...
git merge feature/new-weapon
```

### Package as Git Submodule
```bash
cd Universe1/spaces/MyRPG
git submodule add https://github.com/creator/combat-system.git packages/@creator/combat-system
git commit -m "Add combat system package"
```

## Advantages Over Traditional Engines

| Feature | Eustress | Unity | Unreal | Roblox |
|---------|----------|-------|--------|--------|
| **Project Format** | Folder | `.unity` binary | `.uproject` + binary | Cloud database |
| **Git-Friendly** | ✅ Native | ⚠️ YAML mode | ❌ Merge hell | ❌ No git |
| **External Editors** | ✅ Any tool | ❌ Inspector only | ❌ Editor only | ❌ Studio only |
| **Sparse Checkout** | ✅ Built-in | ❌ Manual | ❌ Manual | ✅ Packages |
| **Remote Loading** | ✅ .pak from R2 | ⚠️ AssetBundles | ⚠️ Pak files | ✅ Cloud |
| **Teleportation** | ✅ Seamless | ❌ Scene load | ❌ Level streaming | ✅ TeleportService |

## Migration Path

### Phase 1: File Operations (Current)
- ✅ Universe1/spaces/Space1 structure
- ✅ Baseplate.glb and Welcome Cube.glb
- ✅ Explorer shows Space folder structure
- ✅ Startup loads from .glb files

### Phase 2: Space Creation (Next)
- [ ] `eustress space create` CLI command
- [ ] SpaceBuilder API
- [ ] space.toml metadata format
- [ ] Default service folder generation

### Phase 3: Package System
- [ ] packages.toml parser
- [ ] Git sparse checkout integration
- [ ] Local package symlinks
- [ ] Package dependency resolution

### Phase 4: Remote Loading
- [ ] .pak file format (LZ4 compression)
- [ ] Cloudflare R2 integration
- [ ] HTTP .pak loader
- [ ] Package cache management

### Phase 5: Teleportation
- [ ] Space transition system
- [ ] Player state persistence
- [ ] Inventory transfer
- [ ] Loading screen UI

## Security Considerations

### Sandboxing
- Scripts run in isolated WASM sandbox
- File access restricted to Space folder
- Network requests require permission

### Package Verification
- SHA-256 checksums for .pak files
- GPG signatures for published packages
- Dependency vulnerability scanning

### Remote Loading
- HTTPS only for .pak downloads
- Content-Type validation
- Size limits (max 500MB per .pak)

## Performance

### Lazy Loading
- Only load visible Spaces
- Stream assets on-demand
- Unload unused Spaces after timeout

### Caching Strategy
- `.eustress/cache/` for derived data
- Package cache in `~/.eustress/packages/`
- R2 .pak files cached locally

### Memory Management
- Max 3 Spaces loaded simultaneously
- Automatic unload after 5 minutes inactive
- Shared assets deduplicated across Spaces

## Future Enhancements

### Multiplayer Spaces
- Multiple players in same Space
- Cross-Space chat and trading
- Shared persistent Spaces

### Procedural Spaces
- Generate Spaces from seed
- Infinite universe exploration
- Dynamic Space creation

### AI-Generated Content
- Generate Spaces from text prompts
- Populate Spaces with AI models
- Procedural quest generation

---

**Status**: Phase 1 Complete (2026-02-24)  
**Next**: Implement Space creation CLI tool
