//! Toolbox system - Insert mesh instances from standard library
//!
//! The Toolbox provides a catalog of standard meshes (Block, Ball, Cylinder, etc.)
//! that users can insert into their Space. Instead of spawning entities directly,
//! it creates .glb.toml instance files that reference shared mesh assets.

use bevy::prelude::*;
use std::path::PathBuf;
use std::fs;

use crate::space::instance_loader::{
    InstanceDefinition, AssetReference, TransformData, InstanceProperties, InstanceMetadata,
    write_instance_definition,
};

/// Toolbox mesh catalog entry
#[derive(Debug, Clone)]
pub struct ToolboxMesh {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub mesh_path: &'static str,
    pub default_size: [f32; 3],
}

/// Get the standard mesh catalog
pub fn get_mesh_catalog() -> Vec<ToolboxMesh> {
    vec![
        ToolboxMesh {
            id: "block",
            name: "Block",
            description: "Basic building block - the most common part",
            category: "Basic",
            mesh_path: "assets/parts/block.glb",
            default_size: [4.0, 1.0, 2.0],
        },
        ToolboxMesh {
            id: "ball",
            name: "Ball",
            description: "Round sphere - great for decorations",
            category: "Basic",
            mesh_path: "assets/parts/ball.glb",
            default_size: [2.0, 2.0, 2.0],
        },
        ToolboxMesh {
            id: "cylinder",
            name: "Cylinder",
            description: "Cylindrical shape - pillars and poles",
            category: "Basic",
            mesh_path: "assets/parts/cylinder.glb",
            default_size: [2.0, 4.0, 2.0],
        },
        ToolboxMesh {
            id: "wedge",
            name: "Wedge",
            description: "Triangular wedge - ramps and roofs",
            category: "Basic",
            mesh_path: "assets/parts/wedge.glb",
            default_size: [2.0, 1.0, 2.0],
        },
        ToolboxMesh {
            id: "corner_wedge",
            name: "Corner Wedge",
            description: "Corner wedge - roof corners",
            category: "Basic",
            mesh_path: "assets/parts/corner_wedge.glb",
            default_size: [2.0, 1.0, 2.0],
        },
        ToolboxMesh {
            id: "cone",
            name: "Cone",
            description: "Cone shape - decorative element",
            category: "Basic",
            mesh_path: "assets/parts/cone.glb",
            default_size: [2.0, 4.0, 2.0],
        },
    ]
}

/// Insert a mesh instance by creating a .glb.toml file in a specific target directory.
/// Use this when you already know the directory (e.g. selected folder's path).
pub fn insert_mesh_instance_at(
    target_dir: &PathBuf,
    mesh_id: &str,
    position: [f32; 3],
    instance_name: Option<String>,
) -> Result<PathBuf, String> {
    let catalog = get_mesh_catalog();
    let mesh = catalog.iter()
        .find(|m| m.id == mesh_id)
        .ok_or_else(|| format!("Mesh '{}' not found in catalog", mesh_id))?;

    let base_name = instance_name.unwrap_or_else(|| mesh.name.to_string());
    let now = chrono::Utc::now().to_rfc3339();

    // Generate unique FOLDER NAME within target_dir via the shared
    // `unique_entity_name` helper — gives the first instance the bare
    // base name (`Block/`) and subsequent collisions a short hex
    // suffix (`Block-a3f2/`), matching the convention enforced
    // everywhere else that creates entities. Display name is always
    // the original base, so multiple sibling "Block" entities render
    // identically in the Explorer while staying uniquely addressable
    // on disk.
    let folder_name = crate::space::instance_loader::unique_entity_name(target_dir, &base_name);
    let instance_name = base_name;

    let instance_dir = target_dir.join(&folder_name);
    fs::create_dir_all(&instance_dir)
        .map_err(|e| format!("Failed to create directory {:?}: {}", instance_dir, e))?;

    let instance = crate::space::instance_loader::InstanceDefinition {
        asset: Some(crate::space::instance_loader::AssetReference {
            mesh: mesh.mesh_path.to_string(),
            scene: "Scene0".to_string(),
        }),
        transform: crate::space::instance_loader::TransformData {
            position,
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: mesh.default_size,
        },
        properties: crate::space::instance_loader::InstanceProperties::default(),
        metadata: crate::space::instance_loader::InstanceMetadata {
            class_name: "Part".to_string(),
            archivable: true,
            name: if folder_name != instance_name { Some(instance_name.clone()) } else { None },
            created: now.clone(),
            last_modified: now,
            ..Default::default()
        },
        material: None,
        thermodynamic: None,
        electrochemical: None,
        ui: None,
        attributes: None,
        tags: None,
        parameters: None,
        extra: std::collections::HashMap::new(),
    };

    let toml_path = instance_dir.join("_instance.toml");
    crate::space::instance_loader::write_instance_definition(&toml_path, &instance)?;
    info!("📦 Toolbox: Created instance folder {:?} (display name: {})", instance_dir, instance_name);
    Ok(toml_path)
}

/// Insert a mesh instance by creating a folder with _instance.toml in Workspace
pub fn insert_mesh_instance(
    space_root: &PathBuf,
    mesh_id: &str,
    position: [f32; 3],
    instance_name: Option<String>,
) -> Result<PathBuf, String> {
    let workspace_path = space_root.join("Workspace");
    insert_mesh_instance_at(&workspace_path, mesh_id, position, instance_name)
}

/// Plugin for Toolbox system (mesh catalog + insert_mesh_instance)
/// Insertion is handled inline by drain_slint_actions → InsertPart handler.
pub struct ToolboxPlugin;

impl Plugin for ToolboxPlugin {
    fn build(&self, _app: &mut App) {
        // Catalog and insert_mesh_instance are pure functions — no systems needed.
        // The InsertPart handler in drain_slint_actions calls insert_mesh_instance
        // directly, then spawns the entity inline via instance_loader::spawn_instance.
        info!("🔧 Toolbox: {} mesh primitives available", get_mesh_catalog().len());
    }
}
