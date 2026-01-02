// Removed: use tauri::State;
use crate::scenes::{SceneManager, SceneData, SceneMetadata, RecentScene};
use crate::parts::PartManager;
use std::sync::Mutex;

/// Global scene manager
pub struct SceneManagerState {
    manager: Mutex<SceneManager>,
    current_scene: Mutex<Option<SceneData>>,
}

impl Default for SceneManagerState {
    fn default() -> Self {
        Self {
            manager: Mutex::new(SceneManager::default()),
            current_scene: Mutex::new(None),
        }
    }
}

/// Create a new blank scene
// Removed tauri::command - now called directly from egui UI
pub fn create_new_scene(
    name: String,
    description: String,
    author: String,
    scene_manager: State<SceneManagerState>,
) -> Result<SceneData, String> {
    let manager = scene_manager.manager.lock().unwrap();
    
    let scene = manager.create_new_scene(name, description, author)
        .map_err(|e| format!("Failed to create scene: {}", e))?;
    
    // Set as current scene
    *scene_manager.current_scene.lock().unwrap() = Some(scene.clone());
    
    Ok(scene)
}

/// Save the current scene
// Removed tauri::command - now called directly from egui UI
pub fn save_current_scene(
    scene_manager: State<SceneManagerState>,
    part_manager: State<PartManager>,
) -> Result<String, String> {
    let mut manager = scene_manager.manager.lock().unwrap();
    let mut current_scene = scene_manager.current_scene.lock().unwrap();
    
    let scene = current_scene.as_mut()
        .ok_or_else(|| "No scene is currently open".to_string())?;
    
    let filepath = manager.save_scene(scene, &part_manager)
        .map_err(|e| format!("Failed to save scene: {}", e))?;
    
    Ok(filepath)
}

/// Save scene as (with new name)
// Removed tauri::command - now called directly from egui UI
pub fn save_scene_as(
    name: String,
    scene_manager: State<SceneManagerState>,
    part_manager: State<PartManager>,
) -> Result<String, String> {
    let mut manager = scene_manager.manager.lock().unwrap();
    let mut current_scene = scene_manager.current_scene.lock().unwrap();
    
    let scene = current_scene.as_mut()
        .ok_or_else(|| "No scene is currently open".to_string())?;
    
    // Update name and create new ID (it's a new scene)
    scene.metadata.name = name;
    scene.metadata.id = uuid::Uuid::new_v4().to_string();
    
    let filepath = manager.save_scene(scene, &part_manager)
        .map_err(|e| format!("Failed to save scene: {}", e))?;
    
    Ok(filepath)
}

/// Load a scene from file
// Removed tauri::command - now called directly from egui UI
pub fn load_scene(
    filepath: String,
    scene_manager: State<SceneManagerState>,
    part_manager: State<PartManager>,
) -> Result<SceneData, String> {
    let mut manager = scene_manager.manager.lock().unwrap();
    
    let scene = manager.load_scene(&filepath)
        .map_err(|e| format!("Failed to load scene: {}", e))?;
    
    // Apply to part manager - parts are cleared and recreated
    for part in &scene.parts {
        part_manager.store_part(part.clone())
            .map_err(|e| format!("Failed to store part: {}", e))?;
    }
    
    // Set as current scene
    *scene_manager.current_scene.lock().unwrap() = Some(scene.clone());
    
    Ok(scene)
}

/// List all available scenes
// Removed tauri::command - now called directly from egui UI
pub fn list_available_scenes(
    scene_manager: State<SceneManagerState>,
) -> Result<Vec<SceneMetadata>, String> {
    let manager = scene_manager.manager.lock().unwrap();
    
    manager.list_scenes()
        .map_err(|e| format!("Failed to list scenes: {}", e))
}

/// Get recent scenes
// Removed tauri::command - now called directly from egui UI
pub fn get_recent_scenes(
    scene_manager: State<SceneManagerState>,
) -> Result<Vec<RecentScene>, String> {
    let manager = scene_manager.manager.lock().unwrap();
    Ok(manager.get_recent_scenes())
}

/// Delete a scene file
// Removed tauri::command - now called directly from egui UI
pub fn delete_scene(
    filepath: String,
    scene_manager: State<SceneManagerState>,
) -> Result<(), String> {
    let mut manager = scene_manager.manager.lock().unwrap();
    
    manager.delete_scene(&filepath)
        .map_err(|e| format!("Failed to delete scene: {}", e))
}

/// Get current scene info
// Removed tauri::command - now called directly from egui UI
pub fn get_current_scene(
    scene_manager: State<SceneManagerState>,
) -> Result<Option<SceneData>, String> {
    let current = scene_manager.current_scene.lock().unwrap();
    Ok(current.clone())
}

/// Update current scene metadata
// Removed tauri::command - now called directly from egui UI
pub fn update_scene_metadata(
    name: Option<String>,
    description: Option<String>,
    scene_manager: State<SceneManagerState>,
) -> Result<SceneData, String> {
    let mut current = scene_manager.current_scene.lock().unwrap();
    
    let scene = current.as_mut()
        .ok_or_else(|| "No scene is currently open".to_string())?;
    
    if let Some(n) = name {
        scene.metadata.name = n;
    }
    if let Some(d) = description {
        scene.metadata.description = d;
    }
    
    Ok(scene.clone())
}
