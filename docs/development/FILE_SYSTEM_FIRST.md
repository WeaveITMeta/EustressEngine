# File-System-First Engine Architecture

> **Status:** Accepted  
> **Author:** Eustress Engine Team  
> **Date:** 2026-02-23  
> **Priority:** P0 — Foundational Architecture Decision  

---

## Table of Contents

1. [Decision](#1-decision)
2. [The Obsidian Parallel](#2-the-obsidian-parallel)
3. [What Binary Formats Buy You (And Why We Don't Need Them)](#3-what-binary-formats-buy-you)
4. [Project Structure](#4-project-structure)
5. [The .eustress/ Directory](#5-the-eustress-directory)
6. [Scene Format: glTF 2.0 + EXT_eustress](#6-scene-format)
7. [Asset Resolution](#7-asset-resolution)
8. [Derived Asset Cache](#8-derived-asset-cache)
9. [File Watching](#9-file-watching)
10. [Distribution Packaging](#10-distribution-packaging)
11. [Migration Path](#11-migration-path)
12. [Competitive Analysis](#12-competitive-analysis)

---

## 1. Decision

**Eustress is a file-system-first engine.** Every asset, script, scene, and configuration file lives as a real file on disk in a standard format. There is no proprietary asset database, no import step that transforms source files into internal representations, and no binary project file.

### Core Rules

1. **Opening a folder = opening a project** (like VS Code, Obsidian — not Unity's `.unityproject`)
2. **The Unified Explorer shows your actual filesystem** (not a virtual asset database)
3. **Scripts are editable in any editor** (Monaco, VS Code, Neovim, Helix)
4. **Scenes export to standard formats** (glTF 2.0 with extensions)
5. **No proprietary asset pipeline** — if you import a PNG, it stays a PNG
6. **Derived data is cached, never authoritative** — delete `.eustress/cache/` and rebuild from source
7. **Every file is git-diffable** — text formats for everything except binary media (which stays as-is)

---

## 2. The Obsidian Parallel

Obsidian proved that a file-system-first approach can win against database-backed competitors (Notion, Roam). The parallels are exact:

| Obsidian | Eustress |
|----------|----------|
| Your vault is just a folder of `.md` files | Your project is just a folder of assets + scenes |
| No import step — drop a file in, it appears | No import step — drop a PNG in, it's usable |
| Any markdown editor works | Any text editor works for scripts/configs |
| `.obsidian/` stores settings + cache | `.eustress/` stores settings + cache |
| Sync via git, Dropbox, iCloud | Sync via git (text scenes are diffable) |
| Plugins extend, never lock in | Plugins extend, never lock in |
| Delete `.obsidian/` — your notes survive | Delete `.eustress/` — your project survives |

### The Mesh Parallel

In Obsidian, each note is a file. In Eustress, **each mesh is a file**:

```
assets/
  models/
    character.gltf          ← The mesh IS this file. Not imported into a binary.
    character.bin           ← glTF binary buffer (vertex data)
    environment.gltf
    props/
      chair.glb             ← Single-file glTF (embedded binary)
      table.glb
  textures/
    grass.png               ← The texture IS this PNG. Not converted.
    brick_diffuse.png
    brick_normal.png
  audio/
    footstep.ogg            ← The audio IS this OGG. Not re-encoded.
```

The engine reads these files directly. No shadow copies. No asset database mapping UUIDs to files. The **path is the identifier**.

---

## 3. What Binary Formats Buy You

### Analysis: Binary vs. File-System-First

| Capability | Binary Approach | FS-First Alternative | Limitation? |
|---|---|---|---|
| Fast load times | Pre-baked binary blobs | Memory-mapped files + lazy loading + LZ4 streaming | **None** |
| Asset deduplication | Content-addressed binary store | Symlinks / hardlinks / content-addressed `.eustress/cache/` | **None** |
| Cross-references | Internal ID tables | Relative paths (the path IS the reference) | **None** |
| Texture compression | Bake to GPU format (BC7/ASTC) | Keep source PNG, generate GPU textures in `.eustress/cache/` | **None** |
| Mesh optimization | Bake LODs + vertex reorder | Keep source glTF, generate optimized meshes in `.eustress/cache/` | **None** |
| Scene serialization | Proprietary binary scene | glTF 2.0 + `EXT_eustress` (JSON readable + binary buffers) | **None** |
| Incremental builds | Dependency graph in binary DB | File mtime + content hash (notify crate) | **None** |
| Atomic saves | Single binary write | Write `.tmp` → rename atomically | **None** |
| Version control | Binary diff tools needed | **Native git diff/merge** | **FS-first wins** |
| Collaboration | Custom merge servers | **Standard git workflows** | **FS-first wins** |
| Editor agnostic | Locked to engine editor | **Any editor works** | **FS-first wins** |
| Portability | Engine-specific format | **Standard formats everywhere** | **FS-first wins** |

### Verdict

Binary formats provide **zero capabilities** that cannot be achieved with filesystem + cache. The advantages of FS-first are overwhelming. The only trade-off is distribution packaging, which is a build step (see [Section 10](#10-distribution-packaging)).

---

## 4. Project Structure

An Eustress project is a directory. Opening it in Eustress Studio = opening the project.

### Minimal Project

```
my-game/                        ← This IS the project. Open this folder.
├── .eustress/                  ← Engine metadata (gitignored cache, user settings)
│   ├── settings.toml           ← Editor preferences (window layout, theme)
│   ├── project.toml            ← Project metadata (name, version, engine version)
│   ├── cache/                  ← Derived assets (gitignored, fully rebuildable)
│   │   ├── textures/           ← GPU-compressed textures (BC7/ASTC)
│   │   ├── meshes/             ← Optimized vertex buffers
│   │   └── manifest.json       ← Cache manifest (source hash → cached path)
│   └── local/                  ← User-local state (gitignored)
│       ├── recent.json         ← Recently opened files
│       └── breakpoints.json    ← Debug breakpoints
├── scenes/                     ← Scene files (glTF 2.0 + EXT_eustress)
│   ├── main.gltf               ← Main scene (JSON, git-diffable)
│   ├── main.bin                ← Scene binary buffers (vertex data, etc.)
│   └── lobby.gltf              ← Another scene
├── src/                        ← Soul scripts
│   ├── main.soul               ← Entry point script
│   ├── player_controller.soul
│   └── ui/
│       └── health_bar.soul
├── assets/                     ← Raw assets (images, models, audio)
│   ├── models/
│   │   ├── character.gltf
│   │   └── character.bin
│   ├── textures/
│   │   ├── grass.png
│   │   └── brick.png
│   └── audio/
│       └── music.ogg
├── docs/                       ← Documentation (optional)
│   └── README.md
└── .gitignore                  ← Ignores .eustress/cache/, .eustress/local/
```

### Default .gitignore

```gitignore
# Eustress derived data (fully rebuildable from source)
.eustress/cache/
.eustress/local/

# OS noise
.DS_Store
Thumbs.db
desktop.ini
```

### What Gets Committed to Git

Everything except `.eustress/cache/` and `.eustress/local/`. This means:

- ✅ `scenes/*.gltf` — diffable JSON scene files
- ✅ `scenes/*.bin` — binary buffers (git LFS recommended for large ones)
- ✅ `src/*.soul` — scripts (fully diffable)
- ✅ `assets/**` — raw assets (git LFS for large binaries)
- ✅ `.eustress/project.toml` — project metadata
- ✅ `.eustress/settings.toml` — shared editor settings (team can override locally)
- ❌ `.eustress/cache/` — derived, rebuildable
- ❌ `.eustress/local/` — user-specific state

---

## 5. The .eustress/ Directory

Like `.obsidian/`, `.git/`, `.vscode/` — a metadata directory that the engine manages.

### 5.1 project.toml

```toml
[project]
name = "My Game"
version = "0.1.0"
engine_version = "0.16.1"
description = "A cool game built with Eustress"
author = "Developer Name"

[build]
entry_scene = "scenes/main.gltf"
entry_script = "src/main.soul"

[features]
physics = true
networking = false
vr = false
```

### 5.2 settings.toml

```toml
[editor]
theme = "eustress-dark"
font_size = 13
font_family = "Cascadia Code"
tab_size = 4
minimap = true
word_wrap = false

[layout]
left_panel_width = 260
right_panel_width = 300
output_height = 200
show_explorer = true
show_properties = true
show_output = true

[viewport]
snap_enabled = true
snap_size = 1.0
grid_visible = true
grid_size = 4.0

[explorer]
show_hidden_files = false
exclude_patterns = ["target/", "node_modules/", "*.tmp"]
```

### 5.3 cache/ Directory

All derived assets. Fully rebuildable. Gitignored.

```
.eustress/cache/
├── manifest.json               ← Maps source_hash → cached_path
├── textures/
│   ├── a1b2c3d4.bc7            ← GPU-compressed texture (keyed by content hash)
│   └── e5f6g7h8.astc
├── meshes/
│   ├── i9j0k1l2.mesh           ← Optimized vertex buffer
│   └── m3n4o5p6.mesh
└── thumbnails/
    ├── grass_png.thumb.webp     ← Explorer thumbnail
    └── character_gltf.thumb.webp
```

### Cache Invalidation

```rust
/// Cache entry in manifest.json
struct CacheEntry {
    source_path: PathBuf,       // Relative path to source file
    source_hash: u64,           // xxHash of source file content
    source_mtime: SystemTime,   // Last modified time (fast check)
    cached_path: PathBuf,       // Relative path in cache/
    cached_at: SystemTime,      // When cache was generated
    engine_version: String,     // Engine version that generated this
}
```

**Strategy:** Check mtime first (fast). If mtime changed, check content hash (accurate). If hash matches, update mtime in manifest. If hash differs, regenerate cached asset.

---

## 6. Scene Format

### Why glTF 2.0

glTF is the "JPEG of 3D" — an open, widely-supported standard. By using glTF as our scene format:

1. **Any 3D tool can open our scenes** (Blender, Maya, three.js, Babylon.js)
2. **JSON-based** — git-diffable, human-readable
3. **Extensible** — custom extensions for ECS data
4. **Binary buffers** — efficient storage for vertex data, animations
5. **Ecosystem** — validators, viewers, converters already exist

### EXT_eustress Extension

We extend glTF with `EXT_eustress` to store ECS-specific data that standard glTF doesn't cover:

```json
{
  "asset": {
    "version": "2.0",
    "generator": "Eustress Engine 0.16.1"
  },
  "extensionsUsed": ["EXT_eustress"],
  "extensions": {
    "EXT_eustress": {
      "version": "1.0",
      "scene_metadata": {
        "name": "Main Scene",
        "description": "The main game scene",
        "gravity": [0, -9.81, 0],
        "ambient_color": [0.1, 0.1, 0.15]
      },
      "services": {
        "workspace": { "class": "Workspace" },
        "lighting": { "class": "Lighting" },
        "players": { "class": "Players" }
      }
    }
  },
  "nodes": [
    {
      "name": "Camera",
      "camera": 0,
      "translation": [0, 10, 20],
      "rotation": [-0.2, 0, 0, 0.98],
      "extensions": {
        "EXT_eustress": {
          "class": "Camera",
          "properties": {
            "field_of_view": 70.0,
            "camera_type": "Scriptable"
          },
          "scripts": ["src/camera_controller.soul"],
          "parent_service": "workspace"
        }
      }
    },
    {
      "name": "Baseplate",
      "mesh": 0,
      "translation": [0, -0.5, 0],
      "scale": [100, 1, 100],
      "extensions": {
        "EXT_eustress": {
          "class": "Part",
          "properties": {
            "anchored": true,
            "can_collide": true,
            "material": "SmoothPlastic",
            "color": [0.388, 0.373, 0.384],
            "transparency": 0.0
          },
          "parent_service": "workspace"
        }
      }
    },
    {
      "name": "Sun",
      "extensions": {
        "EXT_eustress": {
          "class": "Sun",
          "properties": {
            "brightness": 1.0,
            "color": [1.0, 0.95, 0.85],
            "direction": [-0.5, -1.0, -0.3]
          },
          "parent_service": "lighting"
        }
      }
    }
  ],
  "meshes": [
    {
      "name": "Baseplate",
      "primitives": [
        {
          "attributes": { "POSITION": 0, "NORMAL": 1 },
          "indices": 2,
          "material": 0
        }
      ]
    }
  ]
}
```

### What Standard glTF Handles Natively

- Node hierarchy (parent/child)
- Transforms (translation, rotation, scale)
- Meshes + materials (PBR)
- Cameras
- Lights (KHR_lights_punctual)
- Animations
- Textures + images
- Skins + morph targets

### What EXT_eustress Adds

- ECS class names (Part, Model, SoulScript, etc.)
- Eustress-specific properties (anchored, can_collide, material enum, etc.)
- Script references (relative paths to .soul files)
- Service hierarchy (Workspace, Lighting, Players)
- Physics properties (density, friction, elasticity)
- Sound emitters, particle systems, beams
- GUI elements (ScreenGui, BillboardGui)

### Scene Save/Load Flow

```
Save:
  ECS World → glTF JSON + binary buffers → scenes/main.gltf + scenes/main.bin
  
Load:
  scenes/main.gltf → parse JSON → create ECS entities with components
  scenes/main.bin  → memory-map → vertex/index buffers → GPU upload
```

---

## 7. Asset Resolution

### Path-Based References

All asset references are **relative paths from the project root**:

```json
{
  "name": "Character",
  "mesh": 0,
  "extensions": {
    "EXT_eustress": {
      "class": "MeshPart",
      "properties": {
        "mesh_source": "assets/models/character.gltf",
        "texture_diffuse": "assets/textures/character_diffuse.png",
        "texture_normal": "assets/textures/character_normal.png"
      },
      "scripts": ["src/character_controller.soul"]
    }
  }
}
```

### Resolution Rules

1. **Relative paths** resolve from project root (the folder you opened)
2. **No UUIDs** — the path IS the identifier
3. **Renaming a file** = updating references (find-and-replace in JSON scenes)
4. **Moving a file** = same as rename (update paths)
5. **Deleting a file** = broken reference (engine warns, shows placeholder)

### Reference Integrity

When a file is renamed/moved via the Explorer:

```rust
/// Update all references to a moved/renamed file
fn update_asset_references(
    project_root: &Path,
    old_path: &Path,
    new_path: &Path,
) -> Result<usize> {
    let old_rel = old_path.strip_prefix(project_root)?;
    let new_rel = new_path.strip_prefix(project_root)?;
    let old_str = old_rel.to_string_lossy();
    let new_str = new_rel.to_string_lossy();
    
    let mut updated = 0;
    
    // Scan all .gltf scene files for references
    for scene_path in glob(project_root.join("**/*.gltf")) {
        let content = fs::read_to_string(&scene_path)?;
        if content.contains(&*old_str) {
            let new_content = content.replace(&*old_str, &*new_str);
            fs::write(&scene_path, new_content)?;
            updated += 1;
        }
    }
    
    // Scan .soul scripts for require/import references
    for script_path in glob(project_root.join("**/*.soul")) {
        let content = fs::read_to_string(&script_path)?;
        if content.contains(&*old_str) {
            let new_content = content.replace(&*old_str, &*new_str);
            fs::write(&script_path, new_content)?;
            updated += 1;
        }
    }
    
    Ok(updated)
}
```

---

## 8. Derived Asset Cache

### What Gets Cached

| Source Format | Cached Format | Why |
|---|---|---|
| `.png`, `.jpg`, `.exr` | `.bc7` / `.astc` (GPU compressed) | GPU can't read PNG directly; compression is slow |
| `.gltf` mesh data | `.mesh` (optimized vertex buffer) | Vertex reordering for GPU cache efficiency |
| `.gltf` animations | `.anim` (sampled keyframes) | Pre-sampled for runtime interpolation |
| `.soul` scripts | `.soulc` (compiled bytecode) | Compilation is slow; bytecode is fast |
| `.md` documents | `.html` (rendered markdown) | Avoid re-rendering each frame |
| Large textures | `.thumb.webp` (thumbnails) | Explorer preview without loading full texture |

### Cache Is Never Authoritative

The source file is **always** the truth. The cache is a performance optimization.

```
Source (authoritative):  assets/textures/grass.png
Cache (derived):         .eustress/cache/textures/a1b2c3d4.bc7

Delete the cache → engine regenerates from source on next load.
Modify the source → cache invalidated, regenerated on next load.
```

### Lazy Cache Generation

Caches are generated **on demand**, not eagerly:

1. Engine needs `grass.png` as a GPU texture
2. Check cache manifest: is there a valid `.bc7` for this content hash?
3. **Cache hit:** Load `.bc7` directly → GPU upload (fast)
4. **Cache miss:** Load `grass.png` → compress to BC7 → save to cache → GPU upload
5. Next time: cache hit (fast path)

This means:
- First load of a new project is slower (cache cold)
- Subsequent loads are fast (cache warm)
- `eustress cache build` CLI command can pre-warm the cache

---

## 9. File Watching

### notify Crate Integration

The `notify` crate watches the project directory for changes:

```rust
use notify::{Watcher, RecursiveMode, Event, EventKind};

/// File watcher resource
#[derive(Resource)]
pub struct ProjectWatcher {
    watcher: RecommendedWatcher,
    events: Arc<Mutex<Vec<FileChangeEvent>>>,
}

#[derive(Debug, Clone)]
pub enum FileChangeEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
}
```

### What Happens on File Change

| Change | Engine Response |
|---|---|
| Script modified externally | Reload script, recompile if needed |
| Texture modified | Invalidate cache, reload texture |
| Scene modified externally | Prompt: "Scene changed on disk. Reload?" |
| New file created | Appears in Explorer immediately |
| File deleted | Disappears from Explorer, warn if referenced |
| File renamed | Update Explorer, update references in scenes |

### Hot Reload

Scripts and shaders support hot reload:

```
External editor saves player_controller.soul
  → notify fires Modified event
  → Engine detects .soul extension
  → Recompile script
  → Hot-swap running script instance
  → Console: "♻️ Reloaded: src/player_controller.soul"
```

---

## 10. Distribution Packaging

Development is file-system-first. Distribution uses a packed archive.

### The .eep Archive (Eustress Engine Package)

For shipping a game to end users, we pack the project into a read-only archive:

```
eustress pack --output my-game.eep

my-game.eep contents:
├── manifest.json           ← File index + content hashes
├── scenes/
│   └── main.gltf.lz4      ← LZ4-compressed scene
├── assets/
│   ├── textures/           ← GPU-compressed textures (from cache)
│   ├── meshes/             ← Optimized meshes (from cache)
│   └── audio/              ← Source audio (already compressed)
├── scripts/
│   └── *.soulc             ← Compiled bytecode (from cache)
└── metadata.toml           ← Project metadata
```

### Key Distinction

```
Development workflow:  Raw files → edit freely → git commit
                       (file-system-first, no binary database)

Release workflow:      Raw files → eustress pack → .eep archive
                       (packed for distribution, read-only)

Runtime:               .eep archive → memory-mapped → fast loading
                       OR raw files → lazy cache → same result
```

The engine can load from **either** a raw project directory or a packed `.eep` archive. The API is the same:

```rust
/// Asset loader that works with both raw files and packed archives
pub trait AssetSource {
    fn read(&self, path: &str) -> Result<Vec<u8>>;
    fn exists(&self, path: &str) -> bool;
    fn list_dir(&self, path: &str) -> Result<Vec<String>>;
}

/// Raw filesystem (development)
pub struct FileSystemSource { root: PathBuf }

/// Packed archive (distribution)  
pub struct ArchiveSource { archive: MemoryMappedFile }
```

---

## 11. Migration Path

### From Current .eustressengine Binary Format

The current binary format (`serialization/binary.rs`) and RON format (`serialization/scene.rs`) need a migration path:

1. **Phase 1:** Add glTF export alongside existing formats
   - `File → Export As → glTF 2.0` menu option
   - Existing save/load continues to work

2. **Phase 2:** Make glTF the default save format
   - New projects default to `scenes/*.gltf`
   - Existing projects can convert via `eustress migrate`

3. **Phase 3:** Deprecate binary/RON scene formats
   - Keep read support for legacy files
   - All new saves go to glTF

### Migration Command

```bash
# Convert existing project to file-system-first layout
eustress migrate ./my-old-project

# What it does:
# 1. Creates .eustress/ directory
# 2. Converts .eustressengine scenes → .gltf scenes
# 3. Extracts embedded assets to assets/ directory
# 4. Generates project.toml
# 5. Creates .gitignore
```

---

## 12. Competitive Analysis

### How Other Engines Handle Assets

| Engine | Asset Model | Scene Format | FS-First? | Git-Friendly? |
|---|---|---|---|---|
| **Unity** | Asset database (`.meta` files + Library/) | Proprietary YAML (huge diffs) | ❌ | ⚠️ Painful |
| **Unreal** | Content browser (`.uasset` binary) | Proprietary binary | ❌ | ❌ Terrible |
| **Godot** | Resource system (`.tres`/`.tscn` text) | Custom text format | ⚠️ Partial | ✅ Good |
| **Roblox** | Cloud-first binary (`.rbxl`) | Proprietary XML/binary | ❌ | ❌ |
| **Bevy** | Asset server (loads from `assets/`) | RON scenes | ✅ Yes | ✅ Good |
| **Eustress** | **Filesystem-first (loads from project root)** | **glTF 2.0 + extensions** | **✅ Full** | **✅ Native** |

### Why Eustress Wins

1. **Godot is close** but uses custom formats (`.tres`/`.tscn`) — not standard
2. **Bevy is close** but RON scenes aren't interoperable with 3D tools
3. **Unity/Unreal** are fundamentally database-backed — can't escape it
4. **Eustress uses glTF** — the universal 3D interchange format, readable by every 3D tool

### The Obsidian Lesson

Obsidian didn't win by having the best editor. It won by having **the simplest mental model**: your vault is a folder of markdown files. Everything else is optional.

Eustress follows the same principle: **your project is a folder of standard files.** The engine is a viewer/editor for that folder. Delete the engine, your files survive. Switch engines, your assets transfer. That's the promise.

---

## Summary

| Question | Answer |
|---|---|
| What can binary formats do that FS-first can't? | **Nothing.** |
| What can FS-first do that binary can't? | Git diff, any-editor, no lock-in, standard formats |
| Is there a performance cost? | First load is slower (cold cache). After that, identical. |
| How do we ship games? | `eustress pack` → `.eep` archive (build step, not dev workflow) |
| Scene format? | glTF 2.0 + `EXT_eustress` (JSON + binary buffers) |
| Asset references? | Relative paths from project root |
| Cache? | `.eustress/cache/` — derived, gitignored, fully rebuildable |
| File watching? | `notify` crate — hot reload scripts, live update textures |

**Decision: Proceed with file-system-first. No limitations found.**
