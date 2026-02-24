# Toolbox System - Prefab-like Mesh Insertion

## Overview

The Toolbox provides a catalog of standard mesh primitives (Block, Ball, Cylinder, etc.) that users can insert into their Space. Instead of spawning entities directly, the Toolbox creates `.glb.toml` instance files that reference shared mesh assets in `assets/meshes/`.

This enables **prefab-like behavior** where:
- One mesh asset → many instances
- Each instance has unique properties (position, color, etc.)
- Updating the mesh asset → all instances update
- File-system-first architecture (no database)

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Toolbox UI (Slint)                        │
│  User clicks "Block" → Triggers ToolboxInsertEvent          │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│              handle_toolbox_inserts System                   │
│  1. Look up mesh in catalog                                  │
│  2. Generate unique instance name                            │
│  3. Create InstanceDefinition                                │
│  4. Write .glb.toml file to Workspace/                       │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                 File Watcher (notify)                        │
│  Detects new .glb.toml file → triggers hot-reload           │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│              instance_loader::spawn_instance                 │
│  1. Load mesh from assets/meshes/                            │
│  2. Apply transform from .toml                               │
│  3. Spawn ECS entity                                         │
└─────────────────────────────────────────────────────────────┘
```

## Standard Mesh Library

Located in `assets/meshes/` (copied from `engine/assets/parts/`):

| Mesh | File | Default Size | Description |
|------|------|--------------|-------------|
| Block | `block.glb` | 4×1×2 | Basic building block |
| Ball | `ball.glb` | 2×2×2 | Round sphere |
| Cylinder | `cylinder.glb` | 2×4×2 | Cylindrical shape |
| Wedge | `wedge.glb` | 2×1×2 | Triangular wedge |
| Corner Wedge | `corner_wedge.glb` | 2×1×2 | Corner wedge |
| Cone | `cone.glb` | 2×4×2 | Cone shape |

All meshes are **unit-scale** (1×1×1 in Blender). The `default_size` is applied via `Transform.scale` in the `.glb.toml` file.

## Toolbox Catalog

Defined in `toolbox/mod.rs`:

```rust
pub struct ToolboxMesh {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub mesh_path: &'static str,
    pub default_size: [f32; 3],
}

pub fn get_mesh_catalog() -> Vec<ToolboxMesh> {
    vec![
        ToolboxMesh {
            id: "block",
            name: "Block",
            description: "Basic building block - the most common part",
            category: "Basic",
            mesh_path: "assets/meshes/block.glb",
            default_size: [4.0, 1.0, 2.0],
        },
        // ... more meshes
    ]
}
```

## Insert Workflow

### User Action
```
1. User clicks "Block" in Toolbox UI
2. UI triggers: ToolboxInsertEvent { mesh_id: "block", position: Vec3::ZERO }
```

### Backend Processing
```rust
fn handle_toolbox_inserts(mut events: EventReader<ToolboxInsertEvent>) {
    for event in events.read() {
        // 1. Find mesh in catalog
        let mesh = catalog.find(|m| m.id == event.mesh_id)?;
        
        // 2. Generate unique name
        let name = generate_unique_name(&space_root, "Block"); // "Block", "Block1", "Block2", ...
        
        // 3. Create instance definition
        let instance = InstanceDefinition {
            asset: AssetReference {
                mesh: "assets/meshes/block.glb",
                scene: "Scene0",
            },
            transform: TransformData {
                position: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [4.0, 1.0, 2.0], // default_size
            },
            properties: InstanceProperties {
                color: [0.5, 0.5, 0.5, 1.0],
                transparency: 0.0,
                anchored: false,
                can_collide: true,
                cast_shadow: true,
                reflectance: 0.0,
            },
            metadata: InstanceMetadata {
                class_name: "Part",
                archivable: true,
                created: "2026-02-24T14:00:00Z",
                last_modified: "2026-02-24T14:00:00Z",
            },
        };
        
        // 4. Write to file
        write_instance_definition("Workspace/Block.glb.toml", &instance)?;
    }
}
```

### File Created
```toml
# Workspace/Block.glb.toml
[asset]
mesh = "assets/meshes/block.glb"
scene = "Scene0"

[transform]
position = [0.0, 0.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [4.0, 1.0, 2.0]

[properties]
color = [0.5, 0.5, 0.5, 1.0]
transparency = 0.0
anchored = false
can_collide = true
cast_shadow = true
reflectance = 0.0

[metadata]
class_name = "Part"
archivable = true
created = "2026-02-24T14:00:00Z"
last_modified = "2026-02-24T14:00:00Z"
```

### Automatic Spawn
```
1. File watcher detects new file: Workspace/Block.glb.toml
2. Triggers: FileChangeEvent { path, file_type: Toml, change_type: Created }
3. handle_file_created() calls instance_loader::spawn_instance()
4. Entity appears in viewport immediately
```

## Unique Name Generation

The Toolbox ensures unique names by appending numbers:

```rust
fn generate_unique_name(space_root: &PathBuf, base_name: &str) -> String {
    // Check if "Block.glb.toml" exists
    if !exists("Workspace/Block.glb.toml") {
        return "Block";
    }
    
    // Try "Block1.glb.toml", "Block2.glb.toml", etc.
    for i in 1..1000 {
        let candidate = format!("{}{}", base_name, i);
        if !exists(format!("Workspace/{}.glb.toml", candidate)) {
            return candidate;
        }
    }
    
    // Fallback with timestamp
    format!("{}_{}", base_name, Utc::now().timestamp())
}
```

**Example**:
```
Insert Block → "Block.glb.toml"
Insert Block → "Block1.glb.toml"
Insert Block → "Block2.glb.toml"
```

## Prefab-like Behavior

### Scenario: 10 Blocks from 1 Mesh

**Step 1**: User inserts 10 blocks via Toolbox

**Result**:
```
assets/meshes/
└── block.glb (1.7 KB, loaded once)

Workspace/
├── Block.glb.toml (0.5 KB)
├── Block1.glb.toml (0.5 KB)
├── Block2.glb.toml (0.5 KB)
├── ...
└── Block9.glb.toml (0.5 KB)
```

**Memory Usage**:
- 1 mesh loaded: 1.7 KB
- 10 instances: 10 × ~200 bytes = 2 KB (ECS components)
- Total: ~4 KB (vs 17 KB if each had separate mesh)

### Scenario: Update Mesh Asset

**Step 1**: User edits `assets/meshes/block.glb` in Blender (adds beveled edges)

**Step 2**: File watcher detects change

**Step 3**: All 10 instances automatically update with new mesh

**No manual work required!**

## Integration with Existing Systems

### Toolbox UI (Legacy egui - to be modernized)

The existing Toolbox UI in `ui/toolbox.rs.disabled` provides:
- Grid layout with icons
- Search functionality
- Category filtering
- Drag-and-drop support

**Current State**: Disabled (egui-based)

**Migration Plan**:
1. Create Slint-based Toolbox panel
2. Wire up to `ToolboxInsertEvent`
3. Add mesh preview thumbnails
4. Support drag-to-viewport positioning

### File Watcher Integration

The file watcher automatically detects new `.glb.toml` files:

```rust
fn handle_file_created(event: &FileChangeEvent) {
    if event.file_type == FileType::Toml {
        // Load instance definition
        let instance = load_instance_definition(&event.path)?;
        
        // Spawn entity
        let entity = spawn_instance(commands, asset_server, &space_root, event.path, instance);
        
        // Register in SpaceFileRegistry
        registry.register(event.path, entity, metadata);
    }
}
```

**No polling required** - OS-level file events via `notify` crate.

### Properties Panel Integration

When user edits an instance in Properties panel:
1. Transform changes apply to ECS entity (immediate visual feedback)
2. `write_instance_changes_system` writes back to `.glb.toml`
3. File watcher validates consistency

## Comparison to Other Engines

| Feature | Eustress Toolbox | Unity | Unreal | Roblox |
|---------|------------------|-------|--------|--------|
| Prefab System | ✅ .glb.toml | ✅ Prefabs | ✅ Blueprints | ❌ Clones |
| File-System-First | ✅ | ❌ | ❌ | ❌ |
| Git-Friendly | ✅ Text | ❌ Binary | ❌ Binary | ❌ Binary |
| Asset Reuse | ✅ Shared mesh | ✅ | ✅ | ❌ |
| Hot-Reload | ✅ | ✅ | ✅ | ✅ |
| Unique Naming | ✅ Auto | Manual | Manual | Auto |

**Unique Advantages**:
- Only engine with TOML-based prefab system
- File-system-first (no project database)
- Automatic unique naming
- Instant hot-reload via file watcher

## Usage Examples

### Example 1: Build a Platform

```rust
// User clicks "Block" 3 times in Toolbox
// Result: 3 files created automatically

// Workspace/Block.glb.toml
[transform]
position = [0.0, 0.0, 0.0]
scale = [10.0, 1.0, 10.0]

// Workspace/Block1.glb.toml
[transform]
position = [15.0, 0.0, 0.0]
scale = [10.0, 1.0, 10.0]

// Workspace/Block2.glb.toml
[transform]
position = [-15.0, 0.0, 0.0]
scale = [10.0, 1.0, 10.0]
```

### Example 2: Create Color Variants

```rust
// User inserts 3 balls, then edits colors in Properties panel

// Workspace/Ball.glb.toml
[properties]
color = [1.0, 0.0, 0.0, 1.0]  # Red

// Workspace/Ball1.glb.toml
[properties]
color = [0.0, 1.0, 0.0, 1.0]  # Green

// Workspace/Ball2.glb.toml
[properties]
color = [0.0, 0.0, 1.0, 1.0]  # Blue
```

### Example 3: Programmatic Insertion

```rust
// Trigger from script or command
commands.trigger(ToolboxInsertEvent {
    mesh_id: "cylinder".to_string(),
    position: Vec3::new(5.0, 10.0, 0.0),
    instance_name: Some("Pillar".to_string()),
});

// Creates: Workspace/Pillar.glb.toml
```

## Future Enhancements

### Phase 2: Custom Meshes

Allow users to add custom meshes to Toolbox:

```toml
# toolbox_custom.toml
[[mesh]]
id = "tree"
name = "Oak Tree"
category = "Nature"
mesh_path = "assets/meshes/custom/oak_tree.glb"
default_size = [3.0, 8.0, 3.0]
```

### Phase 3: Mesh Variants

Support multiple variants of the same mesh:

```rust
ToolboxMesh {
    id: "block_smooth",
    name: "Block (Smooth)",
    mesh_path: "assets/meshes/block_smooth.glb",
    // ...
}
```

### Phase 4: Drag-to-Position

Allow dragging from Toolbox directly into viewport:

```rust
// User drags "Block" from Toolbox
// Raycast to find position in 3D space
// Create instance at raycast hit point
```

### Phase 5: Thumbnails

Generate preview thumbnails for each mesh:

```
assets/meshes/
├── block.glb
├── block.png (thumbnail)
├── ball.glb
├── ball.png (thumbnail)
```

## Related Files

- `toolbox/mod.rs` - Toolbox system implementation
- `space/instance_loader.rs` - Instance file loading and spawning
- `space/file_watcher.rs` - Hot-reload on file changes
- `ui/toolbox.rs.disabled` - Legacy egui Toolbox UI (to be modernized)
- `engine/assets/parts/` - Standard mesh library (source)
- `Space1/assets/meshes/` - Standard mesh library (deployed)

## Testing

**Manual Test**:
1. Run engine
2. Trigger `ToolboxInsertEvent` (via command or UI)
3. Check `Workspace/` for new `.glb.toml` file
4. Verify entity spawns in viewport
5. Edit `.glb.toml` externally → entity updates
6. Edit entity in Properties panel → `.glb.toml` updates

**Automated Test** (future):
```rust
#[test]
fn test_toolbox_insert() {
    let space_root = PathBuf::from("test_space");
    
    // Insert block
    let path = insert_mesh_instance(&space_root, "block", [0.0, 0.0, 0.0], None)?;
    
    // Verify file exists
    assert!(path.exists());
    
    // Verify content
    let instance = load_instance_definition(&path)?;
    assert_eq!(instance.asset.mesh, "assets/meshes/block.glb");
    assert_eq!(instance.transform.scale, [4.0, 1.0, 2.0]);
}
```
