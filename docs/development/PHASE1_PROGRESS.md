# Phase 1 Progress: Unified Explorer Single Tree

> **Status:** In Progress (60% Complete)  
> **Date:** 2026-02-22  
> **Goal:** Merge ECS entities and filesystem into one unified tree in the Explorer panel

---

## ✅ Completed Tasks

### 1. Slint UI Data Model ✅
**File:** `e:/Workspace/EustressEngine/eustress/crates/engine/ui/slint/explorer.slint`

- **Renamed `EntityNode` → `TreeNode`** with unified fields
- Added `node-type: string` discriminator ("entity" or "file")
- Added entity-specific fields: `class-name`
- Added file-specific fields: `path`, `is-directory`, `extension`, `size`, `modified`
- **Updated `ExplorerPanel`** to use single `tree-nodes` property (removed `workspace-entities`, `lighting-entities`, `entities`)
- **Updated callbacks** to handle both types:
  - `on-select-node(id, node-type)`
  - `on-expand-node(id, node-type)`
  - `on-collapse-node(id, node-type)`
  - `on-open-node(id, node-type)` — NEW for double-click
- Changed search placeholder from "Search entities..." to "Search..."
- Simplified tree rendering: single `for node in root.tree-nodes` loop

### 2. File Icon System ✅
**File:** `e:/Workspace/EustressEngine/eustress/crates/engine/src/ui/file_icons.rs`

Created comprehensive icon mapping module with:
- **`load_file_icon(extension)`** — Maps 80+ file extensions to SVG icons
  - Tier 1: rust, lua, js, ts, python, json, toml, yaml, html, css, markdown, image, video, audio, pdf, git, docker, etc.
  - Tier 2: go, c, cpp, java, kotlin, swift, zig, ruby, sass, less, react, vue, cmake, shader, proto, dll, etc.
- **`load_folder_icon(dir_name, expanded)`** — Maps folder names to specialized icons
  - Special folders: src, assets, docs, test, config, dist, scripts, lib, target, .git, .github, .vscode, images
  - Returns `folder.svg` or `folder-open.svg` for generic folders
- **Helper functions:**
  - `is_directory(path)` — Check if path is directory
  - `get_extension(path)` — Extract file extension
  - `get_stem(path)` — Get filename without extension
  - `get_dir_name(path)` — Get directory name
  - `format_file_size(bytes)` — Human-readable size (B, KB, MB, GB, TB)
- **Tests included** for all major functions

**Module registered** in `e:/Workspace/EustressEngine/eustress/crates/engine/src/ui/mod.rs`

### 3. Unified Explorer State ✅
**File:** `e:/Workspace/EustressEngine/eustress/crates/engine/src/ui/slint_ui.rs`

Created `UnifiedExplorerState` resource replacing `ExplorerState` + `ExplorerExpanded`:

```rust
pub struct UnifiedExplorerState {
    pub selected: SelectedItem,              // Entity | File | None
    pub expanded_entities: HashSet<Entity>,  // Expanded ECS entities
    pub expanded_dirs: HashSet<PathBuf>,     // Expanded directories
    pub search_query: String,                // Search filter
    pub project_root: PathBuf,               // Project directory
    pub file_cache: FileTreeCache,           // Cached filesystem scan
    pub dirty: bool,                         // Needs refresh
}

pub enum SelectedItem {
    Entity(Entity),
    File(PathBuf),
    None,
}

pub struct FileTreeCache {
    pub nodes: Vec<FileNodeData>,
    pub last_scan: Instant,
}

pub struct FileNodeData {
    pub path: PathBuf,
    pub name: String,
    pub is_directory: bool,
    pub extension: String,
    pub size: u64,
    pub modified: SystemTime,
}
```

**Updated all references:**
- Replaced `ExplorerExpanded` + `ExplorerState` with `UnifiedExplorerState` in both plugin implementations
- Updated `handle_explorer_toggle` to use `state.expanded_entities`
- Updated `sync_explorer_to_slint` to use `UnifiedExplorerState`
- Updated `sync_properties_to_slint` to match on `SelectedItem::Entity`
- Updated `DrainResources` struct to remove `explorer_expanded`

---

## 🚧 Remaining Tasks

### 4. Rewrite `sync_explorer_to_slint` → `sync_unified_explorer_to_slint` ⏳
**Current state:** Function signature updated, but still builds separate entity lists

**Needs:**
1. Rename function to `sync_unified_explorer_to_slint`
2. Build single flat `Vec<TreeNode>` instead of separate workspace/lighting/other lists
3. Add filesystem scanning logic after entity nodes
4. Use `file_icons::load_file_icon()` and `load_folder_icon()` for file nodes
5. Respect `expanded_dirs` for directory depth
6. Push single `tree-nodes` model to Slint (not separate workspace-entities/lighting-entities)

### 5. Update Explorer Callbacks ⏳
**File:** `e:/Workspace/EustressEngine/eustress/crates/engine/src/ui/slint_ui.rs`

**Needs:**
- Wire new callbacks in `setup_slint_overlay`:
  - `on-select-node(id, node-type)` → handle both entity and file selection
  - `on-expand-node(id, node-type)` → expand entity or directory
  - `on-collapse-node(id, node-type)` → collapse entity or directory
  - `on-open-node(id, node-type)` → open file in appropriate tab or focus entity
- Update `SlintAction` enum with new variants for file operations
- Add filesystem scanning on expand/collapse

### 6. Update `main.slint` ⏳
**File:** `e:/Workspace/EustressEngine/eustress/crates/engine/ui/slint/main.slint`

**Needs:**
- Import `TreeNode` struct from `explorer.slint`
- Update property bindings to use new callback signatures
- Remove old `workspace-entities`, `lighting-entities` properties
- Add `tree-nodes` property binding

### 7. Add File Watching ⏳
**Needs:**
- Add `notify` crate to `Cargo.toml`
- Create file watcher system to detect external changes
- Mark `UnifiedExplorerState.dirty = true` on filesystem events
- Refresh cache on next sync

### 8. Testing ⏳
**Needs:**
- Build and verify no compilation errors
- Test unified tree shows entities + filesystem
- Test expand/collapse for both entities and directories
- Test file icon mapping correctness
- Test search filtering across both types
- Test double-click to open files

---

## Architecture Summary

### Before (Dual-Mode)
```
ExplorerPanel
  [Game Tab]
    - workspace-entities: [EntityNode]
    - lighting-entities: [EntityNode]
    - entities: [EntityNode]
  [Files Tab]
    - file-nodes: [FileNode]
```

### After (Unified)
```
ExplorerPanel
  tree-nodes: [TreeNode]
    - node-type: "entity" | "file"
    - Common: id, name, icon, depth, expandable, expanded, selected, visible
    - Entity: class-name
    - File: path, is-directory, extension, size, modified
```

### Sync Flow
```
sync_unified_explorer_to_slint:
  1. Build ECS entity nodes (Workspace, Lighting, Players, etc.)
  2. Append filesystem nodes (src/, assets/, docs/, etc.)
  3. Filter by search query
  4. Push single Vec<TreeNode> to Slint
```

---

## Files Modified

1. ✅ `eustress/crates/engine/ui/slint/explorer.slint` — TreeNode struct, unified callbacks
2. ✅ `eustress/crates/engine/src/ui/file_icons.rs` — NEW icon mapping module
3. ✅ `eustress/crates/engine/src/ui/mod.rs` — Registered file_icons module
4. ✅ `eustress/crates/engine/src/ui/slint_ui.rs` — UnifiedExplorerState, updated references
5. ⏳ `eustress/crates/engine/ui/slint/main.slint` — Pending callback updates
6. ⏳ `eustress/crates/engine/Cargo.toml` — Pending notify crate

---

## Next Steps

1. **Complete `sync_unified_explorer_to_slint` rewrite** — Add filesystem scanning logic
2. **Wire callbacks** — Handle both entity and file interactions
3. **Update main.slint** — Use new TreeNode model
4. **Add file watching** — Live filesystem updates
5. **Test end-to-end** — Verify unified tree works correctly

**Estimated completion:** 2-3 hours of focused work remaining
