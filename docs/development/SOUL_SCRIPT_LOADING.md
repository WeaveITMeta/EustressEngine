# Soul Script Loading Pipeline

## Overview

Soul scripts are **Markdown files** (`.soul` or `.md`) that get compiled to **Rune bytecode** (`.rune`) in the cache directory. The dynamic file loader automatically discovers and loads these scripts from the `SoulService` folder.

## File Flow

```
SoulService/MyScript.soul (.md)
    ↓ (discovered by file loader)
    ↓ (loaded as SoulScriptData entity)
    ↓ (user triggers build or auto-build)
    ↓ (Claude API generates Rune code)
    ↓ (validated by Rune engine)
    ↓ (cached as .rune bytecode)
.eustress/cache/soul/MyScript.rune
    ↓ (executed by Rune runtime)
```

## Architecture

### 1. File Discovery (`space/file_loader.rs`)

The `scan_space_directory()` function:
- Scans all Roblox-style service folders
- Detects `.soul` and `.md` files in `SoulService/`
- Creates `FileMetadata` for each discovered script

### 2. Entity Creation

When a `.soul` file is discovered:
```rust
commands.spawn((
    Instance {
        name: "MyScript",
        class_name: ClassName::SoulScript,
        ...
    },
    SoulScriptData {
        source: markdown_content,  // Raw .md content
        dirty: false,
        ast: None,
        generated_code: None,
        build_status: NotBuilt,
        errors: Vec::new(),
    },
    LoadedFromFile {
        path: PathBuf::from("SoulService/MyScript.soul"),
        file_type: FileType::Soul,
        service: "SoulService",
    },
    Name::new("MyScript"),
))
```

### 3. Compilation Pipeline (`soul/build_pipeline.rs`)

The Soul build pipeline automatically:
1. **Parses** markdown to AST (optional - can skip to direct generation)
2. **Generates** Rune code via Claude API
3. **Validates** Rune syntax
4. **Auto-fixes** common errors (MissingLocal, etc.)
5. **Caches** compiled `.rune` bytecode
6. **Executes** via Rune runtime

### 4. Cache Structure

```
.eustress/cache/soul/
├── MyScript.rune          # Compiled bytecode
├── AnotherScript.rune
└── error_tracker.json     # Error patterns for auto-fix
```

## File Extensions

| Extension | Type | Purpose |
|-----------|------|---------|
| `.soul` | Source | Markdown script (preferred) |
| `.md` | Source | Markdown script (alternative) |
| `.rune` | Compiled | Cached bytecode (generated) |

## Integration with Existing Systems

### Properties Panel
- Edits `.soul` file directly on disk
- Marks `SoulScriptData.dirty = true` on changes
- Triggers rebuild when saved

### Explorer
- Shows `.soul` files in `SoulService` folder
- Icon: 📜 (script icon)
- Double-click opens script editor

### Build Triggers
- **Manual**: User clicks "Build" button
- **Auto**: File watcher detects changes
- **Command Bar**: Natural language → Rune generation

## Example: Creating a Soul Script

### 1. Create File
```bash
# In Space folder
SoulService/RotateCube.soul
```

### 2. Write Markdown
```markdown
# Rotate Cube

Rotate the Welcome Cube continuously around the Y axis at 45 degrees per second.

## Requirements
- Find entity named "Welcome Cube"
- Apply rotation every frame
- Use smooth interpolation
```

### 3. Automatic Loading
- Engine discovers `RotateCube.soul` on startup
- Creates `SoulScript` entity
- Shows in Explorer under `SoulService`

### 4. Build & Execute
- User triggers build (or auto-build)
- Claude generates Rune code
- Validates and caches as `.rune`
- Executes in Rune runtime

## Hot Reload

When `.soul` file changes externally:
1. File watcher detects modification
2. Reloads markdown source
3. Marks `dirty = true`
4. Triggers rebuild if auto-build enabled
5. Updates `.rune` cache
6. Hot-reloads running script

## Error Handling

### Compilation Errors
- Tracked in `error_tracker.json`
- Auto-fix patterns applied
- Up to 10 fix iterations
- Telemetry reports failures

### Runtime Errors
- Caught by Rune runtime
- Displayed in Output panel
- Script stops execution
- User can fix and rebuild

## API Surface

Soul scripts have access to:
- `soul::wait(seconds)` - Yield execution
- `soul::spawn()` - Create entities
- `soul::find()` - Query entities
- `soul::set_property()` - Modify components
- `soul::play_sound()` - Audio playback

See `soul/scope.rs` for full API documentation.

## Future Enhancements

### Phase 2
- [ ] File watcher with `notify` crate
- [ ] Incremental compilation
- [ ] Source maps for debugging
- [ ] Breakpoint support

### Phase 3
- [ ] Multi-file scripts with imports
- [ ] Package system for shared libraries
- [ ] Visual script editor (node-based)
- [ ] Performance profiling

## Related Files

- `space/file_loader.rs` - File discovery and loading
- `soul/build_pipeline.rs` - Compilation pipeline
- `soul/rune_api.rs` - Rune execution
- `soul/claude_client.rs` - Code generation
- `soul/error_tracker.rs` - Auto-fix system
