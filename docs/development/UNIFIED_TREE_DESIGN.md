# Unified Explorer Tree Design

> **Decision:** Merge ECS entities and filesystem into a single unified tree  
> **Date:** 2026-02-22  
> **Rationale:** Eliminate mode switching, simplify UX, align with file-system-first philosophy

---

## The Problem with Dual-Mode Tabs

**Original Plan:** Explorer panel with two tabs:
- **[Game]** tab → ECS entity tree (Workspace, Lighting, Players)
- **[Files]** tab → Filesystem tree (src/, assets/, docs/)

**Issues:**
1. **Mode switching friction** — Users constantly toggle between tabs
2. **Cognitive overhead** — "Is this a game object or a file?"
3. **Inconsistent with Obsidian philosophy** — Obsidian doesn't separate "notes" from "files"
4. **Breaks drag-and-drop** — Can't drag a texture from Files tab onto an entity in Game tab

---

## The Solution: Single Unified Tree

**One tree showing everything:**

```
📦 Workspace              ← ECS entity (depth 0)
  📷 Camera               ← ECS entity (depth 1)
  🧊 Baseplate            ← ECS entity (depth 1)
  🎲 Welcome Cube         ← ECS entity (depth 1)
💡 Lighting               ← ECS entity (depth 0)
  ☀️ Sun                  ← ECS entity (depth 1)
👥 Players                ← ECS entity (depth 0)
📁 src/                   ← Filesystem directory (depth 0)
  📄 main.rs              ← Filesystem file (depth 1)
  📄 lib.rs               ← Filesystem file (depth 1)
  📁 systems/             ← Filesystem directory (depth 1)
    📄 physics.rs         ← Filesystem file (depth 2)
📁 assets/                ← Filesystem directory (depth 0)
  📁 models/              ← Filesystem directory (depth 1)
    🎨 character.gltf     ← Filesystem file (depth 2)
  📁 textures/            ← Filesystem directory (depth 1)
    🖼️ grass.png          ← Filesystem file (depth 2)
📁 docs/                  ← Filesystem directory (depth 0)
  📝 README.md            ← Filesystem file (depth 1)
```

---

## Technical Implementation

### Unified Data Model

**Slint struct (replaces `EntityNode` and new `FileNode`):**

```slint
export struct TreeNode {
    // Common fields
    id: int,                // Entity ID (for ECS) or hash (for files)
    name: string,           // Display name
    icon: image,            // Icon (class icon or file-type icon)
    depth: int,             // Tree indentation level
    expandable: bool,       // Has children
    expanded: bool,         // Currently expanded
    selected: bool,         // Currently selected
    visible: bool,          // Matches search filter
    
    // Type discriminator
    node-type: string,      // "entity" or "file"
    
    // Entity-specific (when node-type == "entity")
    class-name: string,     // ECS class name
    
    // File-specific (when node-type == "file")
    path: string,           // Absolute path
    is-directory: bool,     // Folder vs file
    extension: string,      // File extension
    size: string,           // Human-readable size ("4.2 KB")
    modified: bool,         // Has unsaved changes (dirty dot)
}
```

### Rust Sync System

**Single system builds both entity and file nodes:**

```rust
pub fn sync_unified_explorer_to_slint(
    slint_context: Option<NonSend<SlintUiState>>,
    explorer_state: Res<UnifiedExplorerState>,
    instances: Query<(Entity, &Instance)>,
    children_query: Query<&Children>,
) {
    let mut nodes = Vec::new();
    
    // 1. Build ECS entity nodes (Workspace, Lighting, Players, etc.)
    build_entity_nodes(&mut nodes, &instances, &children_query, &explorer_state);
    
    // 2. Build filesystem nodes starting at project root
    build_file_nodes(&mut nodes, &explorer_state.project_root, &explorer_state);
    
    // 3. Filter by search query if present
    if !explorer_state.search_query.is_empty() {
        filter_nodes(&mut nodes, &explorer_state.search_query);
    }
    
    // 4. Push to Slint
    let slint_nodes: Vec<TreeNode> = nodes.into_iter().map(|n| n.to_slint()).collect();
    ui.set_explorer_nodes(slint_nodes.into());
}
```

**Key insight:** Both entity and file nodes use the same `TreeNode` struct with a `node-type` discriminator. The tree rendering logic doesn't care about the type — it just renders depth, icon, name, and expand/collapse state.

---

## Benefits

### 1. **Zero Mode Switching**
- No tabs to toggle
- Everything visible at once
- Natural mental model: "The Explorer shows my project"

### 2. **Unified Search**
- One search bar filters both entities AND files
- Search "Camera" → finds both the Camera entity and camera.rs file
- Ctrl+Shift+F can search entity names + file contents

### 3. **Seamless Drag-and-Drop**
- Drag `grass.png` from assets/ onto `Baseplate` entity
- Drag `character.gltf` into Workspace to spawn
- No tab switching required

### 4. **File-System-First Philosophy**
- Aligns with Obsidian's approach: "Your vault is just a folder"
- Eustress project = folder with entities + files
- No artificial separation between "game content" and "project files"

### 5. **Simpler Code**
- One `TreeNode` struct instead of `EntityNode` + `FileNode`
- One sync system instead of two separate systems
- One set of callbacks: `on-select-node`, `on-expand-node`, `on-open-node`

---

## User Experience Flow

### Opening a File
1. User sees unified tree with entities and files
2. Double-click `main.rs` in src/ folder
3. Eustress detects `.rs` extension → opens **Code Frame** with Monaco Editor
4. File content loads, syntax highlighting activates

### Opening an Entity
1. User sees unified tree
2. Double-click `Welcome Cube` entity
3. Eustress detects entity type → focuses **Scene Frame** (already open)
4. Properties panel updates to show cube's transform/material

### Drag-and-Drop Asset
1. User drags `grass.png` from assets/textures/
2. Drops onto `Baseplate` entity in tree
3. Eustress applies texture to baseplate material
4. Scene updates immediately

---

## Comparison to Other Engines

| Engine | ECS Tree | File Tree | Unified? |
|--------|----------|-----------|----------|
| **Unity** | Hierarchy panel | Project panel (separate) | ❌ |
| **Unreal** | Outliner | Content Browser (separate) | ❌ |
| **Godot** | Scene tree | FileSystem dock (separate) | ❌ |
| **Roblox Studio** | Explorer (instances only) | None (cloud-based) | ❌ |
| **Eustress** | **Single unified tree** | **Integrated** | **✅** |

No other engine merges these views. This is a **unique competitive advantage**.

---

## Implementation Phases

### Phase 0: Foundation ✅ COMPLETE
- Architecture document created
- 75 SVG icons imported (49 file types + 26 folders)
- Icon system designed

### Phase 1: Unified Tree (5-7 days)
- Rename `EntityNode` → `TreeNode` with unified fields
- Create `UnifiedExplorerState` resource
- Rewrite `sync_explorer_to_slint` → `sync_unified_explorer_to_slint`
- Build entity nodes + file nodes in single flat list
- Update callbacks to handle both types
- Add file watcher for live updates

### Phase 2+: Enhanced Features
- Monaco Editor integration
- Media viewers (image, video, audio)
- File operations (create, rename, delete, move)
- Advanced search (ripgrep content search)
- VS Code keybindings

---

## Open Questions (Resolved)

### Q: Should filesystem come before or after entities?
**A:** Entities first, then filesystem. Rationale:
- Entities are the "game content" — primary focus
- Filesystem is "project infrastructure" — secondary
- Matches mental model: "I'm building a game, which happens to have source files"

### Q: How deep should filesystem scan go by default?
**A:** Respect `expanded_dirs` state. Only scan directories the user has expanded. Prevents performance issues with node_modules/ or target/ directories.

### Q: What about .gitignore files?
**A:** Honor .gitignore by default. Add toggle in settings to show ignored files (grayed out).

### Q: How to handle very large projects (10k+ files)?
**A:** 
- Virtual scrolling in Slint (only render visible nodes)
- Lazy loading (don't scan unexpanded directories)
- Debounced file watcher (batch filesystem events)
- Search indexing with ripgrep for fast content search

---

## Conclusion

The unified tree design eliminates artificial boundaries between game entities and project files. It's simpler to implement, easier to use, and aligns with the file-system-first philosophy that prevents vendor lock-in.

**Next step:** Implement Phase 1 — rename structs, create unified sync system, test with real project directory.
