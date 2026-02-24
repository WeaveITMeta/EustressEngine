# Properties Panel Write-Back Design

## Problem Statement

Currently, the Properties panel can **read** entity properties but cannot **write** changes back to the source files on disk. This breaks the file-system-first philosophy.

**Current Behavior**:
- User edits Transform in Properties panel
- Changes apply to ECS entity (in-memory only)
- File on disk remains unchanged
- Next engine restart → changes lost

**Required Behavior**:
- User edits Transform in Properties panel
- Changes apply to ECS entity (immediate visual feedback)
- Changes written back to source file on disk
- File watcher detects change → validates consistency
- Changes persist across restarts

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Properties Panel (Slint UI)               │
│  User edits: Position, Rotation, Scale, Color, etc.         │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│              PropertyChanged Event (Bevy)                    │
│  { entity, property_name, old_value, new_value }            │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│           write_properties_to_disk System                    │
│  1. Check if entity has LoadedFromFile component            │
│  2. Determine file type (.glb, .soul, .toml, etc.)          │
│  3. Call appropriate writer (glb_writer, toml_writer)       │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                  File Format Writers                         │
│  - GLB: Update node transforms in binary                    │
│  - TOML: Update sidecar metadata file                       │
│  - Soul: Update markdown frontmatter                         │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                  File Watcher (notify)                       │
│  Detects change → validates → confirms write-back           │
└─────────────────────────────────────────────────────────────┘
```

## File Format Strategies

### Strategy 1: GLB Binary Modification (Complex)

**Approach**: Directly modify glTF binary to update node transforms.

**Pros**:
- Single source of truth (no sidecar files)
- Works with any glTF viewer
- Standard format

**Cons**:
- Complex binary parsing
- Risk of corruption
- Must preserve all other data (meshes, materials, animations)

**Implementation**:
```rust
use gltf::Gltf;
use gltf_json::Root;

fn update_glb_transform(path: &Path, node_name: &str, transform: Transform) -> Result<()> {
    // 1. Load GLB
    let (document, buffers, _) = gltf::import(path)?;
    
    // 2. Find node by name
    let node = document.nodes()
        .find(|n| n.name() == Some(node_name))
        .ok_or("Node not found")?;
    
    // 3. Update transform in JSON
    let mut json: Root = document.into_json();
    let node_json = &mut json.nodes[node.index()];
    
    // Convert Transform to TRS
    node_json.translation = Some([
        transform.translation.x,
        transform.translation.y,
        transform.translation.z,
    ]);
    
    let (axis, angle) = transform.rotation.to_axis_angle();
    node_json.rotation = Some([
        axis.x * angle.sin(),
        axis.y * angle.sin(),
        axis.z * angle.sin(),
        angle.cos(),
    ]);
    
    node_json.scale = Some([
        transform.scale.x,
        transform.scale.y,
        transform.scale.z,
    ]);
    
    // 4. Write back to GLB
    write_glb(path, json, buffers)?;
    
    Ok(())
}
```

**Challenges**:
- glTF crate is read-only
- Need custom GLB writer
- Must handle all glTF extensions

### Strategy 2: TOML Sidecar Files (Simple, Recommended)

**Approach**: Store transform overrides in `.toml` sidecar files.

**Pros**:
- Simple text format
- Easy to parse and write
- Non-destructive (original .glb untouched)
- Git-friendly (readable diffs)

**Cons**:
- Two files per asset (.glb + .glb.toml)
- Need to merge at load time

**File Structure**:
```
Workspace/
├── Baseplate.glb           # Original mesh
└── Baseplate.glb.toml      # Transform overrides
```

**TOML Format**:
```toml
# Baseplate.glb.toml
[transform]
position = [0.0, 0.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]  # Quaternion (x, y, z, w)
scale = [1.0, 1.0, 1.0]

[properties]
color = [0.5, 0.5, 0.5, 1.0]  # RGBA
transparency = 0.0
anchored = true
can_collide = true

[metadata]
last_modified = "2026-02-24T13:15:00Z"
modified_by = "user@example.com"
```

**Implementation**:
```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct GlbMetadata {
    transform: TransformData,
    properties: PropertiesData,
    metadata: MetadataInfo,
}

#[derive(Serialize, Deserialize)]
struct TransformData {
    position: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
}

fn write_glb_metadata(glb_path: &Path, transform: Transform) -> Result<()> {
    let toml_path = glb_path.with_extension("glb.toml");
    
    let metadata = GlbMetadata {
        transform: TransformData {
            position: transform.translation.to_array(),
            rotation: [
                transform.rotation.x,
                transform.rotation.y,
                transform.rotation.z,
                transform.rotation.w,
            ],
            scale: transform.scale.to_array(),
        },
        properties: PropertiesData::default(),
        metadata: MetadataInfo {
            last_modified: chrono::Utc::now().to_rfc3339(),
            modified_by: "eustress-engine".to_string(),
        },
    };
    
    let toml_string = toml::to_string_pretty(&metadata)?;
    std::fs::write(toml_path, toml_string)?;
    
    Ok(())
}

fn load_glb_with_metadata(glb_path: &Path) -> (Handle<Scene>, Transform) {
    let scene = asset_server.load(format!("{}#Scene0", glb_path.display()));
    
    // Check for sidecar metadata
    let toml_path = glb_path.with_extension("glb.toml");
    let transform = if toml_path.exists() {
        let toml_str = std::fs::read_to_string(toml_path).ok()?;
        let metadata: GlbMetadata = toml::from_str(&toml_str).ok()?;
        
        Transform {
            translation: Vec3::from_array(metadata.transform.position),
            rotation: Quat::from_xyzw(
                metadata.transform.rotation[0],
                metadata.transform.rotation[1],
                metadata.transform.rotation[2],
                metadata.transform.rotation[3],
            ),
            scale: Vec3::from_array(metadata.transform.scale),
        }
    } else {
        Transform::default()
    };
    
    (scene, transform)
}
```

### Strategy 3: Hybrid Approach (Best of Both)

**Approach**: Use sidecar for transforms, modify GLB for materials/colors.

**Rationale**:
- Transforms change frequently → sidecar (fast, safe)
- Materials/colors change rarely → GLB (portable)

**Implementation**: Combine Strategy 1 and 2 based on property type.

## Recommended Implementation: Strategy 2 (TOML Sidecar)

### Phase 1: Basic Transform Write-Back

**Files to Create**:
1. `space/metadata_writer.rs` - TOML sidecar writer
2. `space/metadata_loader.rs` - TOML sidecar loader
3. `ui/properties_writeback.rs` - Properties panel integration

**System Flow**:
```rust
// 1. User edits property in UI
slint_ui.on_property_changed(|entity_id, property, value| {
    commands.trigger(PropertyChangedEvent {
        entity: Entity::from_raw(entity_id),
        property: property.to_string(),
        value: value.clone(),
    });
});

// 2. Apply to ECS entity
fn apply_property_changes(
    mut events: EventReader<PropertyChangedEvent>,
    mut transforms: Query<&mut Transform>,
) {
    for event in events.read() {
        if event.property == "Position" {
            if let Ok(mut transform) = transforms.get_mut(event.entity) {
                transform.translation = event.value.as_vec3();
            }
        }
    }
}

// 3. Write to disk
fn write_properties_to_disk(
    mut events: EventReader<PropertyChangedEvent>,
    file_entities: Query<&LoadedFromFile>,
) {
    for event in events.read() {
        if let Ok(loaded) = file_entities.get(event.entity) {
            match loaded.file_type {
                FileType::Gltf => {
                    write_glb_metadata(&loaded.path, event)?;
                }
                FileType::Soul => {
                    write_soul_frontmatter(&loaded.path, event)?;
                }
                _ => {}
            }
        }
    }
}
```

### Phase 2: Undo/Redo Support

**Challenge**: User edits property → writes to disk → file watcher reloads → infinite loop?

**Solution**: Undo stack with write-back suppression.

```rust
#[derive(Resource)]
struct UndoStack {
    history: Vec<PropertyChange>,
    current_index: usize,
    suppress_writeback: bool,
}

impl UndoStack {
    fn push(&mut self, change: PropertyChange) {
        // Truncate future if we're in the middle
        self.history.truncate(self.current_index + 1);
        self.history.push(change);
        self.current_index += 1;
    }
    
    fn undo(&mut self) -> Option<&PropertyChange> {
        if self.current_index > 0 {
            self.current_index -= 1;
            self.suppress_writeback = true;
            Some(&self.history[self.current_index])
        } else {
            None
        }
    }
    
    fn redo(&mut self) -> Option<&PropertyChange> {
        if self.current_index < self.history.len() - 1 {
            self.current_index += 1;
            self.suppress_writeback = true;
            Some(&self.history[self.current_index])
        } else {
            None
        }
    }
}

fn write_properties_to_disk(
    undo_stack: Res<UndoStack>,
    // ...
) {
    if undo_stack.suppress_writeback {
        return; // Skip write-back during undo/redo
    }
    
    // Normal write-back...
}
```

### Phase 3: Conflict Resolution

**Scenario**: User edits in Properties panel while external tool modifies file.

**Detection**:
```rust
#[derive(Component)]
struct FileMetadataCache {
    last_known_hash: String,
    last_modified: SystemTime,
}

fn detect_conflicts(
    file_entities: Query<(&LoadedFromFile, &FileMetadataCache)>,
) {
    for (loaded, cache) in file_entities.iter() {
        let current_hash = hash_file(&loaded.path);
        
        if current_hash != cache.last_known_hash {
            warn!("Conflict detected: {} modified externally", loaded.path.display());
            // Show conflict resolution UI
        }
    }
}
```

**Resolution Options**:
1. **Keep Engine Changes** - Overwrite file
2. **Keep File Changes** - Reload entity
3. **Merge** - Apply both (if possible)
4. **Show Diff** - Let user decide

## Multi-Selection Write-Back

### Batch Editing

**UI Flow**:
```
1. User selects 3 cubes in Explorer
2. Properties panel shows "3 entities selected"
3. User changes Position.Y to 5.0
4. All 3 cubes move to Y=5.0
5. All 3 .glb.toml files updated
```

**Implementation**:
```rust
#[derive(Resource)]
struct MultiSelection {
    entities: Vec<Entity>,
}

fn apply_batch_property_changes(
    events: EventReader<PropertyChangedEvent>,
    selection: Res<MultiSelection>,
    mut transforms: Query<&mut Transform>,
    file_entities: Query<&LoadedFromFile>,
) {
    for event in events.read() {
        // Apply to all selected entities
        for &entity in &selection.entities {
            if let Ok(mut transform) = transforms.get_mut(entity) {
                // Apply change
                if event.property == "Position.Y" {
                    transform.translation.y = event.value.as_f32();
                }
            }
            
            // Write to disk
            if let Ok(loaded) = file_entities.get(entity) {
                write_glb_metadata(&loaded.path, transform)?;
            }
        }
    }
}
```

### Relative vs Absolute

**Absolute**: Set all to same value
```
Cube1.Position.Y = 5.0
Cube2.Position.Y = 5.0
Cube3.Position.Y = 5.0
```

**Relative**: Offset by delta
```
Cube1.Position.Y += 2.0  (3.0 → 5.0)
Cube2.Position.Y += 2.0  (1.0 → 3.0)
Cube3.Position.Y += 2.0  (4.0 → 6.0)
```

**UI Toggle**:
```slint
HorizontalLayout {
    Text { text: "Position.Y"; }
    LineEdit { value: "5.0"; }
    CheckBox { text: "Relative"; checked: false; }
}
```

## Performance Considerations

### Debounced Writes

**Problem**: User drags slider → 60 writes/second → file system thrashing.

**Solution**: Debounce writes (500ms after last change).

```rust
#[derive(Resource)]
struct PendingWrites {
    changes: HashMap<Entity, PropertyChange>,
    last_change: Instant,
}

fn debounced_write_to_disk(
    mut pending: ResMut<PendingWrites>,
    time: Res<Time>,
) {
    if pending.last_change.elapsed() > Duration::from_millis(500) {
        // Flush all pending writes
        for (entity, change) in pending.changes.drain() {
            write_to_disk(entity, change);
        }
    }
}
```

### Async I/O

**Problem**: Writing large files blocks main thread.

**Solution**: Offload to background thread.

```rust
use tokio::task;

async fn write_glb_metadata_async(path: PathBuf, metadata: GlbMetadata) {
    task::spawn_blocking(move || {
        let toml_string = toml::to_string_pretty(&metadata)?;
        std::fs::write(path, toml_string)?;
        Ok(())
    }).await
}
```

## Testing Strategy

### Unit Tests
```rust
#[test]
fn test_write_glb_metadata() {
    let temp_dir = tempdir()?;
    let glb_path = temp_dir.path().join("test.glb");
    
    let transform = Transform::from_xyz(1.0, 2.0, 3.0);
    write_glb_metadata(&glb_path, transform)?;
    
    let toml_path = glb_path.with_extension("glb.toml");
    assert!(toml_path.exists());
    
    let metadata: GlbMetadata = toml::from_str(&std::fs::read_to_string(toml_path)?)?;
    assert_eq!(metadata.transform.position, [1.0, 2.0, 3.0]);
}
```

### Integration Tests
```rust
#[test]
fn test_roundtrip_write_and_reload() {
    // 1. Load entity from .glb
    let entity = load_glb("Workspace/Cube.glb");
    
    // 2. Modify transform
    transform.translation.y = 10.0;
    
    // 3. Write to disk
    write_properties_to_disk(entity);
    
    // 4. Reload
    let reloaded = load_glb("Workspace/Cube.glb");
    
    // 5. Verify
    assert_eq!(reloaded.translation.y, 10.0);
}
```

## Migration Path

### Step 1: Read-Only Sidecar Support
- Load .glb.toml if exists
- Apply overrides to entities
- No write-back yet

### Step 2: Write-Back for Transform Only
- Implement write_glb_metadata()
- Hook up to Properties panel
- Test with single entity

### Step 3: Multi-Selection
- Implement batch editing
- Add relative/absolute toggle
- Test with multiple entities

### Step 4: Full Property Support
- Add color, material, etc.
- Implement conflict resolution
- Add undo/redo

## Related Files

- `space/metadata_writer.rs` (new)
- `space/metadata_loader.rs` (new)
- `space/file_loader.rs` (update to load sidecars)
- `ui/properties.rs` (update to trigger write-back)
- `ui/explorer.rs` (update for multi-selection)
