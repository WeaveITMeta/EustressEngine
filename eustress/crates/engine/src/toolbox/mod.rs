//! Toolbox system - Insert mesh instances from standard library
//!
//! The Toolbox provides a catalog of standard meshes (Block, Ball, Cylinder, etc.)
//! that users can insert into their Space. Instead of spawning entities directly,
//! it creates .glb.toml instance files that reference shared mesh assets.

use bevy::prelude::*;
use std::path::PathBuf;

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
///
/// Forwards to [`insert_mesh_instance_with_class`] with `class_name = "Part"` so
/// existing callers that just want a plain mesh part keep working unchanged.
pub fn insert_mesh_instance_at(
    target_dir: &PathBuf,
    mesh_id: &str,
    position: [f32; 3],
    instance_name: Option<String>,
) -> Result<PathBuf, String> {
    insert_mesh_instance_with_class(target_dir, mesh_id, position, instance_name, "Part")
}

/// Same as [`insert_mesh_instance_at`] but with a caller-supplied
/// `class_name` written into the generated `_instance.toml`.
///
/// Needed so the Toolbox / Insert-menu can spawn mesh-backed subclasses
/// of `Part` (`Seat`, `VehicleSeat`, `SpawnLocation`, `UnionOperation`, …)
/// as their real class without forking a parallel file-writer. Every
/// path still produces the same folder + `_instance.toml` layout; only
/// the `[metadata] class_name` differs.
pub fn insert_mesh_instance_with_class(
    target_dir: &PathBuf,
    mesh_id: &str,
    position: [f32; 3],
    instance_name: Option<String>,
    class_name: &str,
) -> Result<PathBuf, String> {
    let catalog = get_mesh_catalog();
    let mesh = catalog.iter()
        .find(|m| m.id == mesh_id)
        .ok_or_else(|| format!("Mesh '{}' not found in catalog", mesh_id))?;

    let base_name = instance_name.unwrap_or_else(|| mesh.name.to_string());

    // Route through the canonical pipeline: copy the class template,
    // patch transform + asset_mesh from the catalog entry, let the
    // file_watcher pick it up. The Part template has no `[asset]`
    // section by default — `asset_mesh` injects one so the toolbox
    // entry's specific mesh wins over any class-level default.
    let overrides = eustress_common::instance_create::InstanceOverrides {
        display_name: Some(base_name.clone()),
        position: Some(position),
        scale: Some(mesh.default_size),
        asset_mesh: Some(mesh.mesh_path.to_string()),
        ..Default::default()
    };

    match eustress_common::instance_create::create_instance(
        target_dir,
        class_name,
        Some(&base_name),
        overrides,
    ) {
        Ok(created) => {
            info!(
                "📦 Toolbox: created instance folder {:?} (display name: {})",
                created.folder_path, base_name,
            );
            Ok(created.toml_path)
        }
        Err(e) => Err(format!("toolbox create_instance: {}", e)),
    }
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
