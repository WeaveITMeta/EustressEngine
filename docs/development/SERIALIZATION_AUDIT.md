# Serialization Format Audit

> **Status:** Audit Complete + Fixes Applied  
> **Date:** 2026-02-25  
> **Purpose:** Map all serialization formats, clarify the binary + file-system-first relationship, and deprecate legacy formats.

---

## How Binary + File-System-First Work Together

They are **not competing** — they serve different stages of the same pipeline:

```
AUTHORING (Studio)              DISTRIBUTION              PLAYBACK (Client)
──────────────────              ────────────              ─────────────────
.glb.toml per entity            Studio "Publish"          Client downloads binary
.glb shared meshes      ──────► serializes to single ──► from R2 CDN, deserializes,
.soul scripts                   .eustress blob            spawns entities with
(git-diffable, editable)        (zstd, uploaded to R2)    procedural meshes
```

- **File-system-first** = **authoring format**. Human-readable, git-diffable, one file per entity. You edit these in Studio.
- **Binary** = **distribution + internal format**. Machine-optimized, single file. Studio produces these on Publish; client loads them. Also used internally for auto-save and play-mode snapshots.

Think of it as: `.glb.toml` files are **source code**, `.eustress` binary is the **compiled output**, R2 is the **app store**.

---

## Format Map

| Format | Extension | Stage | Used By | Status |
|--------|-----------|-------|---------|--------|
| **TOML** | `.glb.toml` | Authoring | Studio (instance_loader.rs) | ✅ **Active — source of truth** |
| **Binary** | `.eustress` | Distribution + Internal | Studio save/open, auto-save, play-mode, **Client (planned)** | ✅ **Active — primary runtime format** |
| **RON** | `.eustress` / `.eustressengine` (legacy) | Legacy | `--scene` CLI only | ⛔ **Deprecated** |
| **JSON** | `.scene.json` | Legacy | Dead code (Tauri era) | ⛔ **Deprecated** |

### Format Collision — Resolved

`.eustressengine` is **deprecated** — renamed to `.eustress` (unified for Studio + Client). Open path detects format via magic bytes (`EUSTRESS` = binary, else legacy RON text warning). New files are always `.eustress` binary.

---

## Detailed Code Path Analysis

### 1. TOML Instance Loader (`.glb.toml`) — ✅ KEEP

**Files:** `space/instance_loader.rs`  
**Used by:** `default_scene.rs` (startup), `toolbox/mod.rs` (drag-drop from toolbox), `file_loader.rs` (space scanning)

**What it does:**
- Loads per-instance property files that reference shared `.glb` mesh assets
- Supports transform, color, anchored, class_name, and now realism properties (material, thermodynamic, electrochemical)
- Write-back via `write_instance_definition()`

**This is the canonical FS-first path.** Matches the architecture docs exactly. Every entity in a Space has a `.glb.toml` file on disk.

### 2. RON Unified Scene (`.eustress` / `.eustressengine` legacy) — ⚠️ DEPRECATE

**Files:** `eustress_format.rs` (common), `serialization/mod.rs` (`load_unified_scene`, `save_unified_scene`)  
**Used by:** `startup.rs` `load_scene_file()` (CLI `--scene` argument), `eustress_format::new_default_scene()`

**What it does:**
- Serializes entire scene tree (entities + hierarchy + atmosphere + player settings) to RON text
- Uses `eustress_common::scene::Scene` struct with `EntityClass` enum
- `load_unified_scene()` parses RON → Scene → spawns entities

**Problems:**
- RON is not a standard format — no tooling outside Rust
- Duplicates what `.glb.toml` + glTF can do
- Only actually triggered by `--scene` CLI flag (not the UI save/open)
- The `FileEvent::SaveScene` from the UI **does NOT connect to this** — it fires into the void (no system reads `FileEvent` messages!)

### 3. JSON PropertyAccess Scene (`.scene.json`) — ⚠️ DEPRECATE

**Files:** `serialization/scene.rs` (`save_scene`, `load_scene`, `load_scene_from_world`)  
**Used by:** `scenes.rs` SceneManager, `commands/scene_management_commands.rs`

**What it does:**
- Full entity serialization using PropertyAccess trait (reads all component properties via reflection)
- Very thorough — captures every BasePart, Part, Light, Camera, etc. property
- Saves to `Documents/EustressEngine/Scenes/*.scene.json`

**Problems:**
- SceneManager uses Mutex<SceneManager> state pattern from the old Tauri era
- `scene_management_commands.rs` still has `State<SceneManagerState>` — a Tauri pattern, not Bevy
- The actual Slint UI `FileEvent::SaveScene` **never reaches this code** — no system consumes the FileEvent messages
- Has the most thorough entity serialization logic (2600+ lines) but is effectively unreachable from the UI

### 4. Binary Format (`.eustress`) — ✅ KEEP (primary format)

**Files:** `serialization/binary.rs` (2087 lines)  
**Used by:** `play_mode.rs` (snapshot/restore), `editor_settings.rs` (auto-save)

**What it does:**
- Zstd-compressed binary format with string table, varint encoding, chunked entities
- Streaming reader for large scenes (millions of entities)
- `save_binary_scene()` / `load_binary_scene()` / `load_binary_scene_to_world()`

**This actually works well** for its use cases:
- **Play mode:** Snapshots world state before Play, restores on Stop (fast, internal)
- **Auto-save:** Periodic backup to `~/.eustress_studio/autosave/*.eustress`

**Should NOT be the user-facing save format** — it's an internal performance format. Perfect for `.eustress/cache/` derived data.

---

## What Actually Happens When User Clicks Save/Open

### Save Scene (Ctrl+S)
```
User clicks Save → Slint callback → SlintAction::SaveScene → FileEvent::SaveScene 
→ ... NOTHING. No system reads FileEvent messages.
```

**Save is broken.** The event is written but never consumed. The old egui ribbon.rs (now `.disabled`) had inline handlers, but the Slint migration only wires up event dispatch without a consumer system.

### Open Scene (Ctrl+O)  
```
User clicks Open → Slint callback → SlintAction::OpenScene → FileEvent::OpenScene
→ ... NOTHING. Same problem.
```

### Auto-Save (background)
```
auto_save_scene_system → save_binary_scene() → ~/.eustress_studio/autosave/*.eustress
```
**This works.** Binary format, internal only.

### Play Mode Snapshot
```
Enter Play → save_binary_scene() → temp file → binary data in memory
Exit Play  → load_binary_scene_to_world() → restore
```
**This works.** Binary format, internal only.

### Default Scene (startup)
```
Engine starts → default_scene.rs → load .glb.toml files from Universe1/spaces/Space1/Workspace/
```
**This works.** TOML + glTF, file-system-first.

---

## TOML-Compatible Classes

All `ClassName` variants that can be represented in `.glb.toml` instance files today:

| Class | TOML Support | Notes |
|-------|-------------|-------|
| `Part` | ✅ Full | transform, color, anchored, can_collide, etc. |
| `MeshPart` | ✅ Full | Same as Part + custom mesh reference |
| `AdvancedPart` | ✅ Full | Part + material, thermodynamic, electrochemical sections |
| `Model` | 🟡 Partial | Would need children references |
| `Folder` | 🟡 Partial | Metadata only, no geometry |
| `Camera` | ❌ Not yet | Would need FOV, projection, camera_type fields |
| `PointLight` | ❌ Not yet | Would need intensity, color, range fields |
| `SpotLight` | ❌ Not yet | Would need angle, intensity, color fields |
| `DirectionalLight` | ❌ Not yet | Would need direction, intensity, color fields |
| `Sound` | ❌ Not yet | Would need audio source, volume fields |
| `Sky` / `Atmosphere` | ❌ Not yet | Would need environment properties |
| `Humanoid` | ❌ Not yet | Complex — character controller state |
| UI classes | ❌ Not yet | BillboardGui, ScreenGui, etc. |

---

## Recommended Architecture (Aligned with FILE_SYSTEM_FIRST.md)

### Keep
1. **`.glb.toml`** — Authoring format. Instance definitions (per-entity, on-disk, FS-first) → extend to all classes
2. **Binary `.eustress`** — Distribution + internal format. Studio Save/Open, auto-save, play-mode, **Client playback via R2 CDN**

### Deprecated (all marked `#[deprecated]` in code) — ✅ DONE  
3. **RON unified scene** — `eustress_format.rs`, `serialization/mod.rs` (`load_unified_scene`, `save_unified_scene`)
4. **JSON `.scene.json`** — `serialization/scene.rs`, `scenes.rs`, `commands/scene_management_commands.rs`
5. **Client RON+JSON loader** — `client/src/systems/scene_loader.rs` (`scene_loader_system`)

### Fixes Applied — ✅ DONE
6. **Wire up FileEvent consumer** — `engine/src/ui/file_event_handler.rs` (drain → execute two-phase pattern). Save/Open from Slint UI now uses binary format.
7. **Disambiguate `.eustress`** — Open path detects format via magic bytes (`EUSTRESS` = binary, else legacy text warning).

### Remaining Migration Steps
1. **Extract binary format parser to `common`** — Move `BinaryEntityData`, `FileHeader`, `StringTable`, `deserialize_entity` to `eustress_common::serialization::binary_format` so both engine and client can read the same format
2. **Client binary scene loader** — New system in `client/src/systems/scene_loader.rs` that reads `BinaryEntityData` → procedural meshes (replaces deprecated RON/JSON loader)
3. **Publish flow** — `FileEvent::Publish` → walk workspace `.glb.toml` files → `save_binary_scene` → `S3Client::upload` to R2 → backend `sync_experience` API
4. Extend `.glb.toml` to support all ClassName variants (lights, cameras, etc.)
5. Add Ctrl+S / Ctrl+O keyboard shortcut wiring (currently only File menu triggers FileEvent)
6. Implement glTF 2.0 + EXT_eustress scene export/import (per Phase 1 in FILE_SYSTEM_FIRST.md)

---

## File Reference

| File | Format | Lines | Purpose |
|------|--------|-------|---------|
| `space/instance_loader.rs` | TOML | ~420 | FS-first instance loading |
| `serialization/mod.rs` | RON | 88 | Unified scene load/save wrappers |
| `serialization/scene.rs` | JSON | 2606 | PropertyAccess scene serialization |
| `serialization/binary.rs` | Binary | 2087 | Zstd binary scene format |
| `eustress_format.rs` (common) | RON | 447 | .eustress format constants + deprecated RON loader |
| `scenes.rs` | JSON | 278 | SceneManager (Tauri-era) |
| `startup.rs` | RON | 917 | CLI scene loading |
| `default_scene.rs` | TOML | 215 | Default scene from .glb.toml |
| `play_mode.rs` | Binary | ~1100 | Play mode snapshot/restore |
| `editor_settings.rs` | Binary | ~525 | Auto-save |
| `toolbox/mod.rs` | TOML | 169 | Drag-drop instance creation |
| `ui/file_event_handler.rs` | Binary | ~300 | **NEW** FileEvent consumer (Save/Open/New) |
| `client/systems/scene_loader.rs` | RON+JSON | 549 | **DEPRECATED** Client scene loading |
| `common/assets/s3.rs` | — | ~540 | S3/R2 upload/download client |
