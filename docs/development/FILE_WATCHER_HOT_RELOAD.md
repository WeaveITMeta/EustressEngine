# File Watcher and Hot-Reload System

## Overview

The file watcher system provides automatic hot-reload functionality for Space files. When files are modified externally (e.g., in Blender, VS Code, or other tools), the engine automatically detects changes and reloads the affected assets.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    notify Crate (OS-level)                   │
│  - macOS: FSEvents                                           │
│  - Windows: ReadDirectoryChangesW                            │
│  - Linux: inotify                                            │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│              notify-debouncer-full (300ms)                   │
│  - Debounces rapid-fire events                              │
│  - Groups related changes                                    │
│  - Prevents duplicate reloads                                │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                 SpaceFileWatcher Resource                    │
│  - Polls events non-blocking                                 │
│  - Filters by file type                                      │
│  - Extracts service from path                                │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│            process_file_changes System                       │
│  - Modified → Hot-reload                                     │
│  - Created → Spawn new entity                                │
│  - Removed → Despawn entity                                  │
└─────────────────────────────────────────────────────────────┘
```

## Supported File Types

### Soul Scripts (.soul, .md)
**Behavior**: Hot-reload markdown source, mark as stale, trigger rebuild

**Example**:
```
1. User edits SoulService/DoorScript.soul in VS Code
2. File watcher detects change
3. Reloads markdown source into SoulScriptData
4. Marks build_status as Stale
5. Triggers automatic rebuild via Soul pipeline
6. New .rune bytecode generated
7. Script hot-reloads without restart
```

**Use Case**: Rapid iteration on game logic without restarting the engine.

### glTF Models (.gltf, .glb)
**Behavior**: Reload scene handle from asset server

**Example**:
```
1. User modifies Workspace/Character.glb in Blender
2. Exports to same file
3. File watcher detects change
4. Reloads SceneRoot with new asset handle
5. Bevy's asset server hot-reloads the mesh
6. Model updates in viewport instantly
```

**Use Case**: Iterate on 3D models without restarting.

### Textures (.png, .jpg, .tga)
**Behavior**: Bevy's built-in hot-reload handles automatically

**Example**:
```
1. User edits textures/grass.png in Photoshop
2. Saves file
3. Bevy's asset server detects change
4. Automatically reloads texture
5. Materials update in real-time
```

**Use Case**: Tweak textures and see results immediately.

## Implementation Details

### Debouncing

**Why**: Text editors often write files multiple times (save → temp file → rename).

**Solution**: 300ms debounce window groups related events.

**Example**:
```
Without debouncing:
  0ms: file.soul modified
  5ms: file.soul modified (temp write)
 10ms: file.soul modified (final write)
 → 3 reload events (wasteful)

With debouncing:
  0ms: file.soul modified
  5ms: file.soul modified (ignored, within 300ms)
 10ms: file.soul modified (ignored, within 300ms)
300ms: Debounce expires → 1 reload event
```

### Service Detection

Files are organized by Roblox-style services:
```
Space1/
├── Workspace/
│   └── Baseplate.glb        → service = "Workspace"
├── SoulService/
│   └── GameManager.soul     → service = "SoulService"
└── StarterGui/
    └── HUD.slint            → service = "StarterGui"
```

The watcher extracts the service from the path to determine entity spawning rules.

### Entity Lifecycle

**Created**:
- New file appears in watched directory
- Spawns entity if `file_type.spawns_entity_in_service(service)`
- Registers in `SpaceFileRegistry`

**Modified**:
- Existing file changes
- Looks up entity via `SpaceFileRegistry`
- Hot-reloads based on file type

**Removed**:
- File deleted
- Despawns entity recursively
- Unregisters from `SpaceFileRegistry`

## Performance

### Non-Blocking
- `poll_events()` uses `try_recv()` (non-blocking)
- Runs in `Update` schedule
- No frame drops from file I/O

### Efficient Filtering
- Only processes supported file types
- Skips directories
- Early-exit for irrelevant events

### Memory Usage
- Watcher uses OS-level APIs (minimal overhead)
- Event channel bounded by debounce window
- No file content caching

## Configuration

### Watch Path
Currently hardcoded to:
```rust
"C:/Users/miksu/Documents/Eustress/Universe1/spaces/Space1"
```

**TODO**: Make configurable via:
- CLI argument: `--space-path <path>`
- Config file: `eustress.toml`
- Environment variable: `EUSTRESS_SPACE_PATH`

### Debounce Duration
Currently: 300ms

**Tuning**:
- Lower (100ms): Faster response, more CPU usage
- Higher (500ms): Slower response, less CPU usage
- Recommended: 200-500ms for most workflows

## Error Handling

### Watcher Creation Failure
```rust
if !space_path.exists() {
    warn!("Space path does not exist, file watcher disabled");
    return; // Engine continues without hot-reload
}
```

### File Read Errors
```rust
Err(e) => {
    error!("Failed to reload Soul script: {}", e);
    // Entity remains in previous state
}
```

### Event Processing Errors
```rust
Err(errors) => {
    for error in errors {
        warn!("File watcher error: {}", error);
    }
    // Continue processing other events
}
```

## Integration with Existing Systems

### Soul Build Pipeline
```rust
// Trigger rebuild on hot-reload
commands.trigger(TriggerBuildEvent {
    entity,
    force: true, // Skip cache, always rebuild
});
```

### Asset Server
```rust
// Reload glTF scene
let scene_handle = asset_server.load(format!("{}#Scene0", path.display()));
commands.entity(entity).insert(SceneRoot(scene_handle));
```

### SpaceFileRegistry
```rust
// Register new file
registry.register(path, entity, metadata);

// Unregister deleted file
registry.unregister(&path);
```

## Future Enhancements

### Phase 2: Advanced Features

**1. Selective Watching**
```rust
// Only watch specific file types
watcher.watch_types(&[FileType::Soul, FileType::Gltf]);
```

**2. Ignore Patterns**
```rust
// Skip temp files, build artifacts
watcher.ignore(&["*.tmp", "*.bak", ".git/*", "target/*"]);
```

**3. Batch Reloads**
```rust
// Group multiple changes into single reload
// E.g., "Save All" in IDE → one reload event
```

**4. Progress UI**
```rust
// Show toast notification on reload
ui.toast("Reloaded DoorScript.soul");
```

### Phase 3: Multiplayer

**Server Authority**:
- Server watches files
- Clients receive reload events
- Ensures consistency across clients

**Example**:
```rust
// Server detects change
watcher.on_change(|path| {
    // Recompile script
    let rune = compile_soul(path);
    
    // Distribute to all clients
    for client in clients {
        send_script_update(client, rune);
    }
});
```

## Troubleshooting

### Hot-Reload Not Working

**Check 1**: Is the file in the watched directory?
```bash
# File must be under Space root
Space1/Workspace/MyModel.glb  ✅
Desktop/MyModel.glb           ❌
```

**Check 2**: Is the file type supported?
```rust
// Supported types in FileType::from_extension()
.soul, .md, .gltf, .glb, .png, .jpg, .tga, etc.
```

**Check 3**: Check console for errors
```
👁 File watcher started for: "C:/Users/.../Space1"
🔄 Hot-reloaded Soul script: "SoulService/DoorScript.soul"
```

### Rapid Reloads

**Symptom**: File reloads multiple times per second

**Cause**: Debounce too low or editor writing multiple times

**Solution**: Increase debounce duration
```rust
Duration::from_millis(500) // Increase from 300ms
```

### Memory Leaks

**Symptom**: Memory usage grows over time

**Cause**: Entities not despawned when files deleted

**Solution**: Check `handle_file_removed()` is called
```rust
commands.entity(entity).despawn_recursive(); // Cleanup
registry.unregister(&path); // Remove from registry
```

## Related Files

- `space/file_watcher.rs` - Core watcher implementation
- `space/file_loader.rs` - Initial file loading
- `space/mod.rs` - Module exports
- `soul/build_pipeline.rs` - Script rebuild on hot-reload
- `Cargo.toml` - notify dependencies

## Dependencies

```toml
notify = { version = "6.1", default-features = false, features = ["macos_fsevent"] }
notify-debouncer-full = "0.3"
```

**Platform Features**:
- macOS: `macos_fsevent` (FSEvents API)
- Windows: Default (ReadDirectoryChangesW)
- Linux: Default (inotify)
