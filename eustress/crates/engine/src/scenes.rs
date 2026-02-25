//! DEPRECATED: Tauri-era JSON scene manager.
//!
//! This module uses `.scene.json` files and a Mutex-based SceneManager pattern
//! from the old Tauri frontend. It is NOT connected to the Slint UI and is
//! effectively dead code.
//!
//! Use instead:
//! - Binary format: `serialization::save_binary_scene` / `load_binary_scene_to_world`
//! - TOML instances: `space::instance_loader` for per-entity `.glb.toml` files
//!
//! This module will be removed in a future release.

use serde::{Serialize, Deserialize};
use std::path::PathBuf;
use std::fs;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use crate::parts::{PartManager, PartData};

/// Scene metadata and configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub version: String,
    pub thumbnail: Option<String>, // Base64 encoded image or path
}

/// Complete scene data including all parts and settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneData {
    pub metadata: SceneMetadata,
    pub parts: Vec<PartData>,
    pub camera: CameraData,
    pub lighting: LightingData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraData {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub fov: f32,
    pub mode: String, // "orbit", "free", "firstPerson"
}

impl Default for CameraData {
    fn default() -> Self {
        Self {
            position: [0.0, 10.0, -10.0],
            rotation: [0.0, 0.0, 0.0],
            fov: 70.0,
            mode: "orbit".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightingData {
    pub sun_position: [f32; 3],
    pub sun_intensity: f32,
    pub sun_color: [f32; 3],
    pub ambient_color: [f32; 3],
    pub ambient_brightness: f32,
}

impl Default for LightingData {
    fn default() -> Self {
        Self {
            sun_position: [50.0, 100.0, 50.0],
            sun_intensity: 1000.0,
            sun_color: [1.0, 0.9, 0.8],
            ambient_color: [1.0, 1.0, 1.0],
            ambient_brightness: 0.3,
        }
    }
}

/// Recent scene entry for the home page
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentScene {
    pub id: String,
    pub name: String,
    pub path: String,
    pub thumbnail: Option<String>,
    pub last_opened: DateTime<Utc>,
}

/// Scene manager for save/load operations
pub struct SceneManager {
    scenes_dir: PathBuf,
    recent_scenes: Vec<RecentScene>,
}

impl SceneManager {
    pub fn new() -> Result<Self, std::io::Error> {
        // Get user's documents directory
        let documents_dir = dirs::document_dir()
            .ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not find documents directory"
            ))?;
        
        let scenes_dir = documents_dir.join("EustressEngine").join("Scenes");
        
        // Create scenes directory if it doesn't exist
        if !scenes_dir.exists() {
            fs::create_dir_all(&scenes_dir)?;
        }
        
        Ok(Self {
            scenes_dir,
            recent_scenes: Vec::new(),
        })
    }
    
    /// Create a new blank scene
    pub fn create_new_scene(
        &self,
        name: String,
        description: String,
        author: String,
    ) -> Result<SceneData, Box<dyn std::error::Error>> {
        let metadata = SceneMetadata {
            id: Uuid::new_v4().to_string(),
            name,
            description,
            author,
            created_at: Utc::now(),
            modified_at: Utc::now(),
            version: "1.0.0".to_string(),
            thumbnail: None,
        };
        
        Ok(SceneData {
            metadata,
            parts: Vec::new(),
            camera: CameraData::default(),
            lighting: LightingData::default(),
        })
    }
    
    /// Save a scene to disk
    pub fn save_scene(
        &mut self,
        scene: &mut SceneData,
        part_manager: &PartManager,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // Update modified time
        scene.metadata.modified_at = Utc::now();
        
        // Get all parts from manager
        scene.parts = part_manager.list_parts()?;
        
        // Create filename from scene name
        let filename = format!("{}.scene.json", scene.metadata.name.replace(" ", "_"));
        let filepath = self.scenes_dir.join(&filename);
        
        // Serialize to JSON
        let json = serde_json::to_string_pretty(scene)?;
        
        // Write to file
        fs::write(&filepath, json)?;
        
        // Add to recent scenes
        self.add_recent_scene(RecentScene {
            id: scene.metadata.id.clone(),
            name: scene.metadata.name.clone(),
            path: filepath.to_string_lossy().to_string(),
            thumbnail: scene.metadata.thumbnail.clone(),
            last_opened: Utc::now(),
        });
        
        Ok(filepath.to_string_lossy().to_string())
    }
    
    /// Load a scene from disk
    pub fn load_scene(
        &mut self,
        filepath: &str,
    ) -> Result<SceneData, Box<dyn std::error::Error>> {
        // Read file
        let json = fs::read_to_string(filepath)?;
        
        // Deserialize
        let scene: SceneData = serde_json::from_str(&json)?;
        
        // Add to recent scenes
        self.add_recent_scene(RecentScene {
            id: scene.metadata.id.clone(),
            name: scene.metadata.name.clone(),
            path: filepath.to_string(),
            thumbnail: scene.metadata.thumbnail.clone(),
            last_opened: Utc::now(),
        });
        
        Ok(scene)
    }
    
    /// List all available scenes
    pub fn list_scenes(&self) -> Result<Vec<SceneMetadata>, Box<dyn std::error::Error>> {
        let mut scenes = Vec::new();
        
        // Read all .scene.json files
        for entry in fs::read_dir(&self.scenes_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    if name.ends_with(".scene.json") {
                        // Load metadata only
                        if let Ok(json) = fs::read_to_string(&path) {
                            if let Ok(scene) = serde_json::from_str::<SceneData>(&json) {
                                scenes.push(scene.metadata);
                            }
                        }
                    }
                }
            }
        }
        
        // Sort by modified date (newest first)
        scenes.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
        
        Ok(scenes)
    }
    
    /// Get recent scenes
    pub fn get_recent_scenes(&self) -> Vec<RecentScene> {
        self.recent_scenes.clone()
    }
    
    /// Add a scene to recent list
    fn add_recent_scene(&mut self, scene: RecentScene) {
        // Remove if already exists
        self.recent_scenes.retain(|s| s.id != scene.id);
        
        // Add to front
        self.recent_scenes.insert(0, scene);
        
        // Keep only last 10
        self.recent_scenes.truncate(10);
        
        // TODO: Persist recent scenes to disk
    }
    
    /// Delete a scene
    pub fn delete_scene(&mut self, filepath: &str) -> Result<(), Box<dyn std::error::Error>> {
        fs::remove_file(filepath)?;
        
        // Remove from recent scenes
        self.recent_scenes.retain(|s| s.path != filepath);
        
        Ok(())
    }
    
    /// Apply loaded scene to PartManager
    pub fn apply_scene_to_manager(
        &self,
        scene: &SceneData,
        part_manager: &mut PartManager,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Clear existing parts
        let existing_parts = part_manager.list_parts()?;
        for part in existing_parts {
            part_manager.delete_part(part.id)?;
        }
        
        // Add all parts from scene
        for part in &scene.parts {
            part_manager.store_part(part.clone())?;
        }
        
        Ok(())
    }
}

impl Default for SceneManager {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self {
            scenes_dir: PathBuf::from("./scenes"),
            recent_scenes: Vec::new(),
        })
    }
}
