# Editor Settings Persistence - Implementation Example

## Overview
The editor settings are now automatically loaded from and saved to `~/.eustress_studio/settings.json`.

## File Location
- **Windows**: `C:\Users\<YourUsername>\.eustress_studio\settings.json`
- **Linux/Mac**: `~/.eustress_studio/settings.json`

## Example Settings File
```json
{
  "snap_size": 0.5,
  "snap_enabled": true,
  "collision_snap": false,
  "angle_snap": 15.0,
  "show_grid": true,
  "grid_size": 10.0,
  "auto_save_interval": 300.0
}
```

## How It Works

### 1. On Startup
- Settings are automatically loaded from file
- If file doesn't exist, default settings are used
- Directory is created automatically if needed

### 2. During Runtime
- Modify settings via `ResMut<EditorSettings>`
- Changes are automatically saved when detected
- No manual save calls needed

### 3. Example Usage in Code
```rust
fn change_snap_setting(
    mut settings: ResMut<EditorSettings>,
) {
    // Modify the setting
    settings.snap_size = 1.0;
    settings.snap_enabled = true;
    
    // Settings are automatically saved!
    // No need to call settings.save() manually
}
```

### 4. Manual Save (if needed)
```rust
fn manual_save_example(
    settings: Res<EditorSettings>,
) {
    if let Err(e) = settings.save() {
        eprintln!("Failed to save: {}", e);
    }
}
```

## Settings Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `snap_size` | `f32` | `0.5` | Grid snap size in world units |
| `snap_enabled` | `bool` | `true` | Enable/disable snapping to grid |
| `collision_snap` | `bool` | `false` | Enable collision-based snapping |
| `angle_snap` | `f32` | `15.0` | Angle snap increment in degrees |
| `show_grid` | `bool` | `true` | Show/hide grid in viewport |
| `grid_size` | `f32` | `10.0` | Grid line spacing |
| `auto_save_interval` | `f32` | `300.0` | Auto-save interval (seconds) |

## Error Handling

### Load Errors
- **File Not Found**: Uses defaults, prints info message
- **Parse Error**: Uses defaults, prints warning with error details
- **Read Error**: Uses defaults, prints warning with error details

### Save Errors
- **Directory Creation Failed**: Returns error message
- **Serialization Failed**: Returns error message
- **Write Failed**: Returns error message

All errors are non-fatal and logged to console.

## Testing

### 1. First Run
```bash
cargo run --manifest-path eustress/engine/Cargo.toml
# Output: "ℹ No settings file found. Creating default settings."
```

### 2. Subsequent Runs
```bash
cargo run --manifest-path eustress/engine/Cargo.toml
# Output: "✅ Loaded editor settings from <path>"
```

### 3. Modify Settings in Editor
- Change snap mode (Ctrl+1, Ctrl+2, Ctrl+0)
- Settings automatically save
- Console: "✅ Saved editor settings to <path>"

### 4. Verify Persistence
- Modify a setting
- Restart the application
- Setting should be preserved
