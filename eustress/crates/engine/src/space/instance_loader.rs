//! Instance loader - loads .glb.toml files as entity instances
//!
//! Architecture:
//! - Mesh assets live in assets/meshes/ (shared, reusable)
//! - Instance files (.glb.toml) live in Workspace/ (unique per entity)
//! - Each .toml references a mesh asset and defines instance-specific properties

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Instance definition loaded from .glb.toml file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceDefinition {
    pub asset: AssetReference,
    pub transform: TransformData,
    pub properties: InstanceProperties,
    pub metadata: InstanceMetadata,
}

/// Reference to a shared mesh asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetReference {
    /// Path to mesh file (relative to Space root)
    pub mesh: String,
    /// glTF scene name (usually "Scene0")
    #[serde(default = "default_scene")]
    pub scene: String,
}

fn default_scene() -> String {
    "Scene0".to_string()
}

/// Transform data (position, rotation, scale)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformData {
    pub position: [f32; 3],
    pub rotation: [f32; 4], // Quaternion (x, y, z, w)
    pub scale: [f32; 3],
}

impl Default for TransformData {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

impl From<TransformData> for Transform {
    fn from(data: TransformData) -> Self {
        Transform {
            translation: Vec3::from_array(data.position),
            rotation: Quat::from_xyzw(
                data.rotation[0],
                data.rotation[1],
                data.rotation[2],
                data.rotation[3],
            ),
            scale: Vec3::from_array(data.scale),
        }
    }
}

impl From<Transform> for TransformData {
    fn from(transform: Transform) -> Self {
        Self {
            position: transform.translation.to_array(),
            rotation: [
                transform.rotation.x,
                transform.rotation.y,
                transform.rotation.z,
                transform.rotation.w,
            ],
            scale: transform.scale.to_array(),
        }
    }
}

/// Instance-specific properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceProperties {
    #[serde(default = "default_color")]
    pub color: [f32; 4], // RGBA
    #[serde(default)]
    pub transparency: f32,
    #[serde(default)]
    pub anchored: bool,
    #[serde(default = "default_true")]
    pub can_collide: bool,
    #[serde(default = "default_true")]
    pub cast_shadow: bool,
    #[serde(default)]
    pub reflectance: f32,
}

fn default_color() -> [f32; 4] {
    [0.5, 0.5, 0.5, 1.0]
}

fn default_true() -> bool {
    true
}

impl Default for InstanceProperties {
    fn default() -> Self {
        Self {
            color: default_color(),
            transparency: 0.0,
            anchored: false,
            can_collide: true,
            cast_shadow: true,
            reflectance: 0.0,
        }
    }
}

/// Instance metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceMetadata {
    #[serde(default = "default_class_name")]
    pub class_name: String,
    #[serde(default = "default_true")]
    pub archivable: bool,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub last_modified: String,
}

fn default_class_name() -> String {
    "Part".to_string()
}

impl Default for InstanceMetadata {
    fn default() -> Self {
        Self {
            class_name: default_class_name(),
            archivable: true,
            created: String::new(),
            last_modified: String::new(),
        }
    }
}

/// Component marking an entity as loaded from an instance file
#[derive(Component, Debug, Clone)]
pub struct InstanceFile {
    /// Path to the .glb.toml file
    pub toml_path: PathBuf,
    /// Path to the referenced mesh asset
    pub mesh_path: PathBuf,
    /// Instance name (derived from filename)
    pub name: String,
}

/// Load instance definition from .glb.toml file
pub fn load_instance_definition(toml_path: &Path) -> Result<InstanceDefinition, String> {
    let toml_str = std::fs::read_to_string(toml_path)
        .map_err(|e| format!("Failed to read {}: {}", toml_path.display(), e))?;
    
    let instance: InstanceDefinition = toml::from_str(&toml_str)
        .map_err(|e| format!("Failed to parse {}: {}", toml_path.display(), e))?;
    
    Ok(instance)
}

/// Write instance definition to .glb.toml file
pub fn write_instance_definition(
    toml_path: &Path,
    instance: &InstanceDefinition,
) -> Result<(), String> {
    let toml_str = toml::to_string_pretty(instance)
        .map_err(|e| format!("Failed to serialize instance: {}", e))?;
    
    std::fs::write(toml_path, toml_str)
        .map_err(|e| format!("Failed to write {}: {}", toml_path.display(), e))?;
    
    Ok(())
}

/// Spawn entity from instance definition
pub fn spawn_instance(
    commands: &mut Commands,
    asset_server: &AssetServer,
    space_root: &Path,
    toml_path: PathBuf,
    instance: InstanceDefinition,
) -> Entity {
    // Resolve mesh path (relative to Space root)
    let mesh_path = space_root.join(&instance.asset.mesh);
    
    // Load scene from asset server
    let scene_handle = asset_server.load(format!("{}#{}", mesh_path.display(), instance.asset.scene));
    
    // Extract instance name from filename
    let name = toml_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .trim_end_matches(".glb")
        .to_string();
    
    // Parse class name
    let class_name = match instance.metadata.class_name.as_str() {
        "Part" => eustress_common::classes::ClassName::Part,
        "Model" => eustress_common::classes::ClassName::Model,
        "MeshPart" => eustress_common::classes::ClassName::MeshPart,
        _ => eustress_common::classes::ClassName::Part,
    };
    
    // Spawn entity with all components
    let entity = commands.spawn((
        SceneRoot(scene_handle),
        Transform::from(instance.transform),
        eustress_common::classes::Instance {
            name: name.clone(),
            class_name,
            archivable: instance.metadata.archivable,
            id: 0,
            ai: false,
        },
        eustress_common::default_scene::PartEntityMarker {
            part_id: name.clone(),
        },
        InstanceFile {
            toml_path: toml_path.clone(),
            mesh_path,
            name: name.clone(),
        },
        Name::new(name.clone()),
    )).id();
    
    info!("📦 Spawned instance '{}' from {:?}", name, toml_path);
    
    entity
}

/// System to load all .glb.toml instance files from Workspace
pub fn load_instance_files_system(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut registry: ResMut<super::file_loader::SpaceFileRegistry>,
) {
    // Get Space root path (TODO: make configurable)
    let space_root = PathBuf::from("C:/Users/miksu/Documents/Eustress/Universe1/spaces/Space1");
    
    if !space_root.exists() {
        warn!("Space path does not exist: {:?}", space_root);
        return;
    }
    
    // Scan Workspace for .glb.toml files
    let workspace_path = space_root.join("Workspace");
    if !workspace_path.exists() {
        return;
    }
    
    let entries = match std::fs::read_dir(&workspace_path) {
        Ok(entries) => entries,
        Err(e) => {
            error!("Failed to read Workspace directory: {}", e);
            return;
        }
    };
    
    for entry in entries.flatten() {
        let path = entry.path();
        
        // Check if it's a .glb.toml file
        if !path.is_file() {
            continue;
        }
        
        let path_str = path.to_string_lossy();
        if !path_str.ends_with(".glb.toml") {
            continue;
        }
        
        // Load instance definition
        match load_instance_definition(&path) {
            Ok(instance) => {
                let entity = spawn_instance(
                    &mut commands,
                    &asset_server,
                    &space_root,
                    path.clone(),
                    instance,
                );
                
                // Register in SpaceFileRegistry
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown")
                    .trim_end_matches(".glb")
                    .to_string();
                
                registry.register(
                    path.clone(),
                    entity,
                    super::file_loader::FileMetadata {
                        path: path.clone(),
                        file_type: super::file_loader::FileType::Toml,
                        service: "Workspace".to_string(),
                        name,
                        size: 0,
                        modified: std::time::SystemTime::now(),
                    },
                );
            }
            Err(e) => {
                error!("Failed to load instance file {:?}: {}", path, e);
            }
        }
    }
}

/// System to write instance changes back to .glb.toml files
pub fn write_instance_changes_system(
    instances: Query<(&Transform, &InstanceFile), Changed<Transform>>,
) {
    for (transform, instance_file) in instances.iter() {
        // Load current instance definition
        let mut instance = match load_instance_definition(&instance_file.toml_path) {
            Ok(inst) => inst,
            Err(e) => {
                error!("Failed to load instance for write-back: {}", e);
                continue;
            }
        };
        
        // Update transform
        instance.transform = TransformData::from(*transform);
        
        // Update last_modified timestamp
        instance.metadata.last_modified = chrono::Utc::now().to_rfc3339();
        
        // Write back to file
        if let Err(e) = write_instance_definition(&instance_file.toml_path, &instance) {
            error!("Failed to write instance: {}", e);
        } else {
            debug!("💾 Wrote transform changes to {:?}", instance_file.toml_path);
        }
    }
}
