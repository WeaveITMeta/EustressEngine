//! # Editor Settings Module
//!
//! Manages persistent editor settings for Eustress Engine.
//!
//! ## Features
//! - **Automatic Loading**: Settings are loaded from `~/.eustress_studio/settings.json` on startup
//! - **Auto-Save**: Settings are automatically saved when modified via Bevy's change detection
//! - **Default Fallback**: If loading fails or no file exists, default settings are used
//! - **Pretty JSON**: Settings are saved in human-readable JSON format
//!
//! ## Settings Persistence
//! - **Location**: `~/.eustress_studio/settings.json`
//! - **Format**: JSON with pretty formatting
//! - **Auto-creation**: Directory is created automatically if it doesn't exist
//!
//! ## Usage
//! Settings are automatically loaded and saved by the `EditorSettingsPlugin`.
//! Modify settings via `ResMut<EditorSettings>` and they will auto-save.

#![allow(dead_code)]

use bevy::prelude::*;
use bevy::gizmos::config::{GizmoConfigStore, DefaultGizmoConfigGroup};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use bevy::log::{info, warn};

/// Global editor settings resource
/// 
/// Automatically persisted to `~/.eustress_studio/settings.json`
#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct EditorSettings {
    /// Grid snap size in world units
    pub snap_size: f32,
    
    /// Enable snapping to grid
    pub snap_enabled: bool,
    
    /// Enable collision-based snapping
    pub collision_snap: bool,
    
    /// Enable surface snapping (raycast to place parts on other parts)
    #[serde(default = "default_surface_snap")]
    pub surface_snap_enabled: bool,
    
    /// Angle snap increment in degrees
    pub angle_snap: f32,
    
    /// Show grid in viewport
    pub show_grid: bool,
    
    /// Grid size
    pub grid_size: f32,
    
    /// Auto-save interval in seconds (0 = disabled)
    pub auto_save_interval: f32,
    
    /// Enable auto-save for scenes
    pub auto_save_enabled: bool,
}

fn default_surface_snap() -> bool {
    true
}

/// Resource to track auto-save state
#[derive(Resource)]
pub struct AutoSaveState {
    /// Timer for auto-save
    pub timer: f32,
    /// Last save time
    pub last_save: Option<std::time::Instant>,
    /// Current scene path (if any)
    pub current_scene_path: Option<PathBuf>,
    /// Has unsaved changes
    pub has_changes: bool,
}

impl Default for AutoSaveState {
    fn default() -> Self {
        Self {
            timer: 0.0,
            last_save: None,
            current_scene_path: None,
            has_changes: false,
        }
    }
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            // Space Grade Ready: 9.80665 / 5 = 1.96133m grid unit
            snap_size: 1.96133,
            snap_enabled: true,
            collision_snap: false,
            surface_snap_enabled: true,
            angle_snap: 15.0,
            show_grid: true,
            // Grid spacing based on SI standard gravity: 9.807m
            grid_size: 9.807,
            auto_save_interval: 300.0, // 5 minutes
            auto_save_enabled: true,
        }
    }
}

impl EditorSettings {
    /// Get the settings file path (~/.eustress_studio/settings.json)
    fn settings_path() -> Option<PathBuf> {
        if let Some(home) = dirs::home_dir() {
            let settings_dir = home.join(".eustress_studio");
            Some(settings_dir.join("settings.json"))
        } else {
            None
        }
    }
    
    /// Load settings from file or create default
    pub fn load() -> Self {
        if let Some(path) = Self::settings_path() {
            if path.exists() {
                // Try to load from file
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        match serde_json::from_str::<EditorSettings>(&content) {
                            Ok(settings) => {
                                println!("✅ Loaded editor settings from {:?}", path);
                                return settings;
                            }
                            Err(e) => {
                                eprintln!("⚠ Failed to parse settings file: {}. Using defaults.", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("⚠ Failed to read settings file: {}. Using defaults.", e);
                    }
                }
            } else {
                println!("ℹ No settings file found. Creating default settings.");
            }
        } else {
            eprintln!("⚠ Could not determine home directory. Using default settings.");
        }
        
        // Return default settings if loading failed
        Self::default()
    }
    
    /// Save settings to file
    pub fn save(&self) -> Result<(), String> {
        let path = Self::settings_path()
            .ok_or_else(|| "Could not determine home directory".to_string())?;
        
        // Create directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create settings directory: {}", e))?;
        }
        
        // Serialize settings to JSON with pretty formatting
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize settings: {}", e))?;
        
        // Write to file
        fs::write(&path, json)
            .map_err(|e| format!("Failed to write settings file: {}", e))?;
        
        println!("✅ Saved editor settings to {:?}", path);
        Ok(())
    }
    
    /// Apply snap to a value
    pub fn apply_snap(&self, value: f32) -> f32 {
        if self.snap_enabled && self.snap_size > 0.0 {
            (value / self.snap_size).round() * self.snap_size
        } else {
            value
        }
    }
    
    /// Apply snap to a vector
    pub fn apply_snap_vec3(&self, value: Vec3) -> Vec3 {
        if self.snap_enabled {
            Vec3::new(
                self.apply_snap(value.x),
                self.apply_snap(value.y),
                self.apply_snap(value.z),
            )
        } else {
            value
        }
    }
    
    /// Apply angle snap
    pub fn apply_angle_snap(&self, angle_degrees: f32) -> f32 {
        if self.snap_enabled && self.angle_snap > 0.0 {
            (angle_degrees / self.angle_snap).round() * self.angle_snap
        } else {
            angle_degrees
        }
    }
}

/// Plugin to manage editor settings
pub struct EditorSettingsPlugin;

impl Plugin for EditorSettingsPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(EditorSettings::load())
            .init_resource::<AutoSaveState>()
            .add_systems(Startup, setup_grid_gizmo_config)
            .add_systems(Update, draw_grid_overlay)
            .add_systems(Update, auto_save_settings)
            .add_systems(Update, auto_save_scene_system);
    }
}

/// Auto-save settings when they change
fn auto_save_settings(
    settings: Res<EditorSettings>,
) {
    // Save when settings are modified
    if settings.is_changed() && !settings.is_added() {
        if let Err(e) = settings.save() {
            eprintln!("❌ Failed to save editor settings: {}", e);
        }
    }
}

/// Configure gizmos - tool handles should render on top
fn setup_grid_gizmo_config(
    mut config_store: ResMut<GizmoConfigStore>,
) {
    let (config, _) = config_store.config_mut::<DefaultGizmoConfigGroup>();
    
    // Set depth_bias to render gizmos (tool handles, selection boxes) on top of objects
    // This ensures Move/Scale/Rotate handles are always visible
    config.depth_bias = -1.0;
}

/// Draw grid overlay in viewport - follows camera on X/Z plane
/// Origin axes (red X, blue Z) stay fixed at world origin
fn draw_grid_overlay(
    mut gizmos: Gizmos,
    settings: Res<EditorSettings>,
    camera_query: Query<&Transform, With<Camera3d>>,
) {
    if !settings.show_grid {
        return;
    }
    
    // Get camera position to center grid around it
    let camera_pos = camera_query.iter().next()
        .map(|t| t.translation)
        .unwrap_or(Vec3::ZERO);
    
    let grid_size = settings.grid_size;  // Spacing between grid lines
    let grid_half_extent = 100.0;  // How far the grid extends from camera
    let grid_divisions = (grid_half_extent * 2.0 / grid_size) as i32;
    
    // Snap grid center to grid increments so lines don't jitter when camera moves
    let grid_center_x = (camera_pos.x / grid_size).round() * grid_size;
    let grid_center_z = (camera_pos.z / grid_size).round() * grid_size;
    
    // Render grid slightly above ground (y = 0.01) to prevent z-fighting
    let grid_y = 0.01;
    
    // Grid line color
    let grid_color = Color::srgba(0.4, 0.4, 0.4, 0.5);
    
    // Draw grid lines centered around camera position
    for i in 0..=grid_divisions {
        let offset = (i as f32 * grid_size) - grid_half_extent;
        
        // Lines parallel to X axis (running along X, at different Z positions)
        let z_pos = grid_center_z + offset;
        gizmos.line(
            Vec3::new(grid_center_x - grid_half_extent, grid_y, z_pos),
            Vec3::new(grid_center_x + grid_half_extent, grid_y, z_pos),
            grid_color,
        );
        
        // Lines parallel to Z axis (running along Z, at different X positions)
        let x_pos = grid_center_x + offset;
        gizmos.line(
            Vec3::new(x_pos, grid_y, grid_center_z - grid_half_extent),
            Vec3::new(x_pos, grid_y, grid_center_z + grid_half_extent),
            grid_color,
        );
    }
    
    // ════════════════════════════════════════════════════════════════════════
    // Origin axes - ALWAYS at world origin (0, 0, 0)
    // These extend far enough to be visible from anywhere
    // ════════════════════════════════════════════════════════════════════════
    
    let axis_extent = 10000.0;  // Very long so always visible
    let origin_y = 0.02;  // Slightly above grid to render on top
    
    // X-axis (RED) - runs along X at Z=0
    gizmos.line(
        Vec3::new(-axis_extent, origin_y, 0.0),
        Vec3::new(axis_extent, origin_y, 0.0),
        Color::srgba(1.0, 0.2, 0.2, 0.9),
    );
    
    // Z-axis (BLUE) - runs along Z at X=0
    gizmos.line(
        Vec3::new(0.0, origin_y, -axis_extent),
        Vec3::new(0.0, origin_y, axis_extent),
        Color::srgba(0.2, 0.2, 1.0, 0.9),
    );
    
    // Small Y-axis indicator at origin (GREEN)
    gizmos.line(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 5.0, 0.0),
        Color::srgba(0.2, 1.0, 0.2, 0.9),
    );
}

// ============================================================================
// Auto-Save Scene System
// ============================================================================

/// System to auto-save the current scene at regular intervals
fn auto_save_scene_system(
    time: Res<Time>,
    settings: Res<EditorSettings>,
    mut auto_save: ResMut<AutoSaveState>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
    // Query for entities to save
    parts_query: Query<(Entity, &crate::classes::Instance, Option<&crate::classes::BasePart>, Option<&crate::classes::Part>)>,
) {
    // Skip if auto-save is disabled
    if !settings.auto_save_enabled || settings.auto_save_interval <= 0.0 {
        return;
    }
    
    // Update timer
    auto_save.timer += time.delta_secs();
    
    // Check if it's time to auto-save
    if auto_save.timer >= settings.auto_save_interval {
        auto_save.timer = 0.0;
        
        // Get auto-save directory
        let auto_save_dir = if let Some(home) = dirs::home_dir() {
            home.join(".eustress_studio").join("autosave")
        } else {
            return;
        };
        
        // Create directory if needed
        if let Err(e) = fs::create_dir_all(&auto_save_dir) {
            warn!("Failed to create auto-save directory: {}", e);
            return;
        }
        
        // Generate filename with timestamp - use binary .eustressengine format
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("autosave_{}.eustressengine", timestamp);
        let save_path = auto_save_dir.join(&filename);
        
        // Count entities
        let entity_count = parts_query.iter().count();
        
        if entity_count == 0 {
            // Nothing to save
            return;
        }
        
        // Build scene data and save as binary .eustressengine format
        let scene = build_auto_save_scene(&parts_query);
        
        // Save to binary format using the unified scene serialization
        match save_auto_save_binary(&save_path, &scene) {
            Ok(_) => {
                auto_save.last_save = Some(std::time::Instant::now());
                notifications.info(format!("Auto-saved ({} entities)", entity_count));
                info!("✅ Auto-saved to {:?}", save_path);
                
                // Clean up old auto-saves (keep last 5)
                cleanup_old_autosaves(&auto_save_dir, 5);
            }
            Err(e) => {
                warn!("Failed to write auto-save: {}", e);
            }
        }
    }
}

/// Build a scene struct from current entities for auto-save
fn build_auto_save_scene(
    parts_query: &Query<(Entity, &crate::classes::Instance, Option<&crate::classes::BasePart>, Option<&crate::classes::Part>)>,
) -> crate::serialization::Scene {
    use crate::serialization::{Scene, SceneMetadata, EntityData};
    use std::collections::HashMap;
    
    let mut entities = Vec::new();
    
    for (entity, instance, basepart, part) in parts_query.iter() {
        let mut properties: HashMap<String, serde_json::Value> = HashMap::new();
        
        // Add Instance properties
        properties.insert("Name".to_string(), serde_json::json!(instance.name));
        properties.insert("Archivable".to_string(), serde_json::json!(instance.archivable));
        
        // Add BasePart properties if present
        if let Some(bp) = basepart {
            properties.insert("Position".to_string(), serde_json::json!([
                bp.cframe.translation.x,
                bp.cframe.translation.y,
                bp.cframe.translation.z
            ]));
            properties.insert("Size".to_string(), serde_json::json!([
                bp.size.x,
                bp.size.y,
                bp.size.z
            ]));
            let srgba = bp.color.to_srgba();
            properties.insert("Color".to_string(), serde_json::json!([
                srgba.red,
                srgba.green,
                srgba.blue
            ]));
            properties.insert("Transparency".to_string(), serde_json::json!(bp.transparency));
            properties.insert("Anchored".to_string(), serde_json::json!(bp.anchored));
            properties.insert("CanCollide".to_string(), serde_json::json!(bp.can_collide));
        }
        
        // Add Part properties if present
        if let Some(p) = part {
            properties.insert("Shape".to_string(), serde_json::json!(format!("{:?}", p.shape)));
        }
        
        entities.push(EntityData {
            id: entity.to_bits() as u32,
            class: format!("{:?}", instance.class_name),
            parent: None, // TODO: Track parent relationships
            properties,
            children: vec![],
            attributes: std::collections::HashMap::new(),
            tags: vec![],
            parameters: None,
        });
    }
    
    Scene {
        format: "eustress_propertyaccess".to_string(),
        metadata: SceneMetadata {
            name: "Auto-Save".to_string(),
            description: "Automatically saved scene".to_string(),
            author: "Eustress Engine".to_string(),
            created: chrono::Local::now().to_rfc3339(),
            modified: chrono::Local::now().to_rfc3339(),
            engine_version: "0.1.0".to_string(),
        },
        entities,
        global_sources: None,
        domain_configs: None,
        global_variables: None,
    }
}

/// Save auto-save scene to binary .eustressengine format
fn save_auto_save_binary(path: &PathBuf, scene: &crate::serialization::Scene) -> std::io::Result<()> {
    use std::io::{BufWriter, Write};
    
    let file = fs::File::create(path)?;
    let mut writer = BufWriter::new(file);
    
    // Write magic bytes for .eustressengine format
    writer.write_all(b"EUST")?;
    
    // Write version (u32 little-endian)
    writer.write_all(&1u32.to_le_bytes())?;
    
    // Serialize scene to binary using bincode
    let scene_bytes = bincode::serialize(scene)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    
    // Write compressed data length (u64 little-endian)
    writer.write_all(&(scene_bytes.len() as u64).to_le_bytes())?;
    
    // Write scene data (could add zstd compression here later)
    writer.write_all(&scene_bytes)?;
    
    writer.flush()?;
    Ok(())
}

/// Clean up old auto-save files, keeping only the most recent N
fn cleanup_old_autosaves(dir: &PathBuf, keep_count: usize) {
    let mut files: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension()
                    .map(|ext| ext == "eustressengine")
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => return,
    };
    
    // Sort by modification time (newest first)
    files.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });
    
    // Remove old files beyond keep_count
    for file in files.into_iter().skip(keep_count) {
        if let Err(e) = fs::remove_file(file.path()) {
            warn!("Failed to remove old auto-save: {}", e);
        }
    }
}
