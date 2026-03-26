// ============================================================================
// space_ops.rs — Space-level file operations (New, Open, Save)
//
// ## Table of Contents
//   1. Constants & service manifest
//   2. Space scaffolding (New Space: full EEP folder + TOML structure)
//   3. Space save (ECS → TOML files per EEP spec)
//   4. Space open (folder picker → scan + load instances)
//   5. TOML serialization helpers
//   6. Simulation readiness (simulation.toml scaffolding)
//   7. (Cache removed)
// ============================================================================

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use bevy::prelude::*;
use chrono::Utc;

use crate::space::instance_loader::{
    InstanceDefinition, InstanceMetadata, AssetReference,
    TransformData, InstanceProperties, write_instance_definition,
};
use crate::space::service_loader::{ServiceComponent, ServiceDefinition, ServiceProperties, ServiceMetadata};
use crate::notifications::NotificationManager;

use eustress_common::{
    AssetIndexManifest, PackageIndexManifest,
    ProjectManifest, ProjectSettingsManifest,
    PublishJournalManifest, PublishManifest,
    SyncManifest, save_toml_file,
};

// ============================================================================
// 1. Service Manifest — EEP service folders created for every new Space
// ============================================================================

/// Service folder names that every new Space receives per EEP_SPECIFICATION.md.
/// Order matters: Workspace first so the 3D viewport has a target immediately.
const SERVICE_FOLDERS: &[ServiceFolder] = &[
    ServiceFolder { name: "Workspace",               class: "Workspace",              icon: "workspace",          description: "3D world objects — Parts, Models, Terrain" },
    ServiceFolder { name: "MaterialService",         class: "MaterialService",        icon: "materialservice",    description: "PBR material definitions (.mat.toml files)" },
    ServiceFolder { name: "Lighting",                class: "Lighting",               icon: "lighting",           description: "Light sources — Sun, Sky, Atmosphere" },
    ServiceFolder { name: "Players",                 class: "Players",                icon: "players",            description: "Player instances and character models" },
    ServiceFolder { name: "SoulService",             class: "SoulService",            icon: "soulservice",        description: "Soul scripts (.soul files)" },
    ServiceFolder { name: "SoundService",            class: "SoundService",           icon: "soundservice",       description: "Audio — Sound effects and music" },
    ServiceFolder { name: "ServerStorage",           class: "ServerStorage",          icon: "serverstorage",      description: "Server-only assets hidden from clients" },
    ServiceFolder { name: "StarterGui",              class: "StarterGui",             icon: "startergui",         description: "UI templates shown to every player" },
    ServiceFolder { name: "StarterPack",             class: "StarterPack",            icon: "starterpack",        description: "Tools given to players on spawn" },
    ServiceFolder { name: "StarterPlayer",           class: "StarterPlayer",          icon: "starterplayer",      description: "StarterPlayerScripts and StarterCharacterScripts" },
    ServiceFolder { name: "Teams",                   class: "Teams",                  icon: "teams",              description: "Team definitions and spawn points" },
];

struct ServiceFolder {
    name:        &'static str,
    class:       &'static str,
    icon:        &'static str,
    description: &'static str,
}

// ============================================================================
// 2. Space Scaffolding — creates a fresh EEP Space on disk
// ============================================================================

/// Result of a scaffold operation
#[derive(Debug)]
pub struct ScaffoldResult {
    pub space_root: PathBuf,
    pub space_name: String,
}

/// Create a brand-new Space at `parent_dir/<space_name>/` following the full
/// EEP_SPECIFICATION.md folder + TOML structure, then return the root path.
///
/// Layout produced:
/// ```
/// <space_name>/
/// ├── .eustress/
/// │   ├── project.toml
/// │   ├── settings.toml
/// │   ├── sync.toml
/// │   ├── asset-index.toml
/// │   ├── package-index.toml
/// │   ├── publish.toml
/// │   ├── publish-journal.toml
/// ├── .eustress/local/
/// ├── Workspace/
/// │   ├── _service.toml
/// │   └── Baseplate.part.toml
/// ├── Lighting/
/// │   ├── _service.toml
/// │   ├── Sky.sky.toml
/// │   └── Atmosphere.atmosphere.toml
/// ├── Players/  … (+ 7 more service folders)
/// ├── src/                (empty, for Soul scripts)
/// (Note: assets/ lives at Universe level, not Space level)
/// ├── space.toml          (space metadata)
/// ├── simulation.toml     (simulation readiness)
/// └── .gitignore
/// ```
pub fn scaffold_new_space(
    parent_dir: &Path,
    space_name: &str,
    author: &str,
) -> Result<ScaffoldResult, String> {
    let space_root = parent_dir.join(space_name);
    if space_root.exists() {
        return Err(format!(
            "Space '{}' already exists at {:?}",
            space_name, space_root
        ));
    }

    // ── Top-level directories ──────────────────────────────────────────────
    create_dir_all(&space_root)?;
    create_dir_all(&space_root.join(".eustress").join("local"))?;
    create_dir_all(&space_root.join("src"))?;

    // Ensure Universe-level assets/parts/ has engine default GLBs
    ensure_universe_default_parts(&space_root);

    let now = Utc::now().to_rfc3339();

    // ── .eustress/project.toml ─────────────────────────────────────────────
    save_manifest(
        &space_root.join(".eustress").join("project.toml"),
        &ProjectManifest::new(space_name, author, &now),
    )?;

    // ── .eustress/settings.toml ────────────────────────────────────────────
    save_manifest(
        &space_root.join(".eustress").join("settings.toml"),
        &ProjectSettingsManifest::default(),
    )?;

    // ── .eustress/sync.toml ────────────────────────────────────────────────
    save_manifest(
        &space_root.join(".eustress").join("sync.toml"),
        &SyncManifest::default(),
    )?;

    // ── .eustress/asset-index.toml ─────────────────────────────────────────
    save_manifest(
        &space_root.join(".eustress").join("asset-index.toml"),
        &AssetIndexManifest::default(),
    )?;

    // ── .eustress/package-index.toml ───────────────────────────────────────
    save_manifest(
        &space_root.join(".eustress").join("package-index.toml"),
        &PackageIndexManifest::default(),
    )?;

    // ── .eustress/publish.toml ──────────────────────────────────────────────
    save_manifest(
        &space_root.join(".eustress").join("publish.toml"),
        &PublishManifest::default(),
    )?;

    // ── .eustress/publish-journal.toml ─────────────────────────────────────
    save_manifest(
        &space_root.join(".eustress").join("publish-journal.toml"),
        &PublishJournalManifest::new(&now),
    )?;

    // ── .gitignore ─────────────────────────────────────────────────────────
    write_file(&space_root.join(".gitignore"), GITIGNORE)?;

    // ── space.toml (Space metadata) ────────────────────────────────────────
    write_file(&space_root.join("space.toml"), &space_meta_toml(space_name, author))?;

    // ── simulation.toml (simulation readiness) ────────────────────────────
    write_file(&space_root.join("simulation.toml"), &simulation_toml())?;

    // ── Service folders ────────────────────────────────────────────────────
    // Copy _service.toml from assets/service_templates/<Name>/ so all
    // properties, icons, and descriptions are data-driven from the templates.
    let svc_template_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("service_templates");

    for svc in SERVICE_FOLDERS {
        let svc_dir = space_root.join(svc.name);
        create_dir_all(&svc_dir)?;

        let template_path = svc_template_dir.join(svc.name).join("_service.toml");
        if let Ok(content) = std::fs::read_to_string(&template_path) {
            write_file(&svc_dir.join("_service.toml"), &content)?;
        } else {
            // Fallback: generate minimal _service.toml so service is always discovered
            warn!("⚠️ Service template not found for '{}' at {:?}, using fallback", svc.name, template_path);
            write_file(
                &svc_dir.join("_service.toml"),
                &service_toml(svc.name, svc.class, svc.icon, svc.description),
            )?;
        }
    }

    // ── Workspace/Baseplate.part.toml ──────────────────────────────────────
    write_file(
        &space_root.join("Workspace").join("Baseplate.part.toml"),
        &baseplate_part_toml(),
    )?;

    // ── Workspace/WelcomeCube.part.toml ────────────────────────────────────
    write_file(
        &space_root.join("Workspace").join("WelcomeCube.part.toml"),
        &welcome_cube_part_toml(),
    )?;

    // ── Lighting children (.instance.toml — picked up by file loader) ───────
    // Copy templates from assets/lighting_templates/ to Lighting/ folder.
    // Files use .instance.toml extension so FileType::from_path returns Toml
    // and the file loader spawns them as ECS entities with Instance components.
    let lighting_template_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("lighting_templates");

    let lighting_children = ["Atmosphere", "Moon", "Sky", "Sun", "Skybox"];
    for child_name in &lighting_children {
        let template_path = lighting_template_dir.join(format!("{}.instance.toml", child_name));
        let target_path = space_root.join("Lighting").join(format!("{}.instance.toml", child_name));

        if let Ok(content) = std::fs::read_to_string(&template_path) {
            write_file(&target_path, &content)?;
        } else {
            warn!("⚠️ Lighting template not found: {:?}", template_path);
            // Fallback: minimal instance toml so the entity still spawns
            write_file(&target_path, &format!(
                "# {} - Auto-generated fallback\n[metadata]\nclass_name = \"{}\"\narchivable = true\n\n[properties]\n",
                child_name, child_name
            ))?;
        }
    }

    info!(
        "✅ New Space '{}' scaffolded at {:?}",
        space_name, space_root
    );
    Ok(ScaffoldResult {
        space_root,
        space_name: space_name.to_string(),
    })
}

/// Copy engine default part GLBs (block, ball, wedge, etc.) into a target directory.
/// Skips files that already exist so user modifications are preserved.
pub fn copy_engine_default_parts(target_parts_dir: &Path) {
    let engine_parts_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("parts");

    if !engine_parts_dir.exists() {
        warn!("Engine parts directory not found at {:?}", engine_parts_dir);
        return;
    }

    let Ok(entries) = std::fs::read_dir(&engine_parts_dir) else { return };
    for entry in entries.flatten() {
        let src = entry.path();
        if src.extension().and_then(|e| e.to_str()) == Some("glb") {
            let Some(file_name) = src.file_name() else { continue };
            let dest = target_parts_dir.join(file_name);
            if !dest.exists() {
                if let Err(e) = std::fs::copy(&src, &dest) {
                    warn!("Failed to copy {:?} → {:?}: {}", src, dest, e);
                } else {
                    info!("📦 Copied default part {:?} → {:?}", file_name, dest);
                }
            }
        }
    }
}

/// Ensure the Universe-level assets/parts/ directory exists and has engine defaults.
/// Called at Space load time to handle existing Universes that predate this feature.
pub fn ensure_universe_default_parts(space_root: &Path) {
    if let Some(universe_root) = crate::space::universe_root_for_path(space_root) {
        let parts_dir = universe_root.join("assets").join("parts");
        let _ = std::fs::create_dir_all(&parts_dir);
        let _ = std::fs::create_dir_all(universe_root.join("assets").join("meshes"));
        copy_engine_default_parts(&parts_dir);
    }
}

pub fn resolve_active_universe_root(current_space_root: Option<&Path>) -> PathBuf {
    if let Some(space_root) = current_space_root {
        if let Some(universe_root) = crate::space::universe_root_for_path(space_root) {
            return universe_root;
        }
    }

    crate::space::first_universe_root().unwrap_or_else(crate::space::workspace_root)
}

pub fn pick_new_universe_root(initial_dir: &Path) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("New Universe — enter the new Universe folder name")
        .set_directory(initial_dir)
        .set_file_name("New Universe")
        .save_file()
}

pub fn pick_new_space_root(initial_dir: &Path) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("New Space — choose the Universe folder and enter the new Space folder name")
        .set_directory(initial_dir)
        .set_file_name("New Space")
        .save_file()
}

// ============================================================================
// 3. Space Save — write all ECS entities back to their TOML files
// ============================================================================

/// Save the entire current Space: serialize every `Instance` + `BasePart` entity
/// that has an `InstanceFile` component back to its `.part.toml` on disk.
/// Entities without `InstanceFile` (runtime-spawned, default scene) are written
/// to `Workspace/<name>.part.toml` as new files.
pub fn save_space(world: &mut World) {
    let space_root = match world.get_resource::<crate::space::SpaceRoot>() {
        Some(sr) => sr.0.clone(),
        None => {
            warn!("Cannot save — no SpaceRoot resource set");
            return;
        }
    };

    ensure_manifest_set(&space_root, None, None);

    let workspace_dir = space_root.join("Workspace");
    let _ = std::fs::create_dir_all(&workspace_dir);

    let mut saved = 0usize;
    let mut errors = 0usize;
    let mut to_save: Vec<(String, PathBuf, InstanceDefinition)> = Vec::new();

    {
        let mut query = world.query::<(
            Entity,
            &eustress_common::classes::Instance,
            &eustress_common::classes::BasePart,
            &GlobalTransform,
            Option<&crate::space::instance_loader::InstanceFile>,
            Option<&eustress_common::classes::Part>,
        )>();

        let now = Utc::now().to_rfc3339();

        for (_entity, instance, base_part, global_tf, instance_file, part) in query.iter(world) {
            use eustress_common::classes::ClassName;
            match instance.class_name {
                ClassName::Sky | ClassName::Atmosphere | ClassName::Camera
                | ClassName::Star | ClassName::Moon | ClassName::Clouds => continue,
                _ => {}
            }

            let toml_path = if let Some(inst_file) = instance_file {
                inst_file.toml_path.clone()
            } else {
                workspace_dir.join(format!("{}.part.toml", sanitize_filename(&instance.name)))
            };

            let t = global_tf.compute_transform();
            let mesh = part.map(|p| {
                match p.shape {
                    eustress_common::classes::PartType::Block => "parts/block.glb",
                    eustress_common::classes::PartType::Ball => "parts/ball.glb",
                    eustress_common::classes::PartType::Cylinder => "parts/cylinder.glb",
                    eustress_common::classes::PartType::Wedge => "parts/wedge.glb",
                    eustress_common::classes::PartType::CornerWedge => "parts/corner_wedge.glb",
                    eustress_common::classes::PartType::Cone => "parts/cone.glb",
                }
            }).unwrap_or("parts/block.glb").to_string();

            let class_name = format!("{:?}", instance.class_name)
                .trim_start_matches("ClassName::")
                .to_string();

            let def = InstanceDefinition {
                asset: Some(AssetReference {
                    mesh,
                    scene: "Scene0".to_string(),
                }),
                transform: TransformData {
                    position: [t.translation.x, t.translation.y, t.translation.z],
                    rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
                    scale: [t.scale.x, t.scale.y, t.scale.z],
                },
                properties: InstanceProperties {
                    color: {
                        let c = base_part.color.to_srgba();
                        [c.red, c.green, c.blue, c.alpha]
                    },
                    material: format!("{:?}", base_part.material),
                    transparency: base_part.transparency,
                    anchored: base_part.anchored,
                    can_collide: base_part.can_collide,
                    cast_shadow: true,
                    reflectance: base_part.reflectance,
                    locked: base_part.locked,
                },
                metadata: InstanceMetadata {
                    class_name,
                    archivable: instance.archivable,
                    created: String::new(),
                    last_modified: now.clone(),
                },
                material: None,
                thermodynamic: None,
                electrochemical: None,
                ui: None,
                extra: std::collections::HashMap::new(),
            };

            to_save.push((instance.name.clone(), toml_path, def));
        }
    }

    for (name, path, def) in &to_save {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match write_instance_definition(path, def) {
            Ok(()) => {
                saved += 1;
                debug!("💾 Saved '{}' → {:?}", name, path);
            }
            Err(e) => {
                errors += 1;
                error!("❌ Failed to save '{}': {}", name, e);
            }
        }
    }

    {
        let mut svc_query = world.query::<&ServiceComponent>();
        let services: Vec<ServiceComponent> = svc_query.iter(world).cloned().collect();
        for svc in &services {
            if svc.toml_path != PathBuf::new() {
                if let Err(e) = crate::space::service_loader::save_service_to_file(svc) {
                    error!("❌ Failed to save service {}: {}", svc.class_name, e);
                    errors += 1;
                } else {
                    saved += 1;
                }
            }
        }
    }

    if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
        if errors == 0 {
            notifs.success(format!("Space saved — {} files written", saved));
        } else {
            notifs.warning(format!(
                "Space saved with {} errors ({} files written)",
                errors, saved
            ));
        }
    }

    info!("💾 Space save complete: {} saved, {} errors", saved, errors);
}

// ============================================================================
// 4. Space Open — pick a Space folder and reload it
// ============================================================================

/// Show a folder picker for opening a Space directory.
/// Returns the chosen directory path, or None if cancelled.
pub fn pick_space_folder() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Open Space — select the Space folder")
        .set_directory(crate::space::workspace_root())
        .pick_folder()
}

/// Switch the engine to a new Space root directory.
/// Clears all current `Instance` entities and triggers a fresh scan via `SpaceRoot`.
pub fn open_space(world: &mut World, space_path: &Path) {
    if !space_path.exists() || !space_path.is_dir() {
        error!("❌ Not a valid Space directory: {:?}", space_path);
        if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
            notifs.error(format!("Not a valid Space directory: {}", space_path.display()));
        }
        return;
    }

    let author = world.get_resource::<crate::auth::AuthState>()
        .and_then(|a| a.user.as_ref())
        .map(|u| u.username.clone());
    ensure_manifest_set(space_path, Some(&space_name_from_path(space_path)), author.as_deref());

    // Ensure Universe-level assets/parts/ has engine default GLBs
    ensure_universe_default_parts(space_path);

    info!("📂 Opening Space: {:?}", space_path);

    let to_despawn: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, With<eustress_common::classes::Instance>>();
        q.iter(world).collect()
    };
    let count = to_despawn.len();
    for entity in to_despawn {
        world.despawn(entity);
    }
    info!("🗑️ Cleared {} existing entities", count);

    if let Some(mut registry) = world.get_resource_mut::<crate::space::SpaceFileRegistry>() {
        *registry = crate::space::SpaceFileRegistry::default();
    }

    world.insert_resource(crate::space::SpaceRoot(space_path.to_path_buf()));

    let space_name = space_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".to_string());

    if let Some(mut scene_file) = world.get_resource_mut::<crate::ui::SceneFile>() {
        scene_file.name = space_name.clone();
        scene_file.path = Some(space_path.to_path_buf());
        scene_file.modified = false;
    }

    world.insert_resource(SpaceRescanNeeded(true));

    if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
        notifs.success(format!("Opened Space: {}", space_name));
    }

    info!("✅ Space '{}' loaded from {:?}", space_name, space_path);
}

/// Resource that signals the file loader to re-scan the current SpaceRoot.
#[derive(Resource, Default)]
pub struct SpaceRescanNeeded(pub bool);

/// Bevy system: if SpaceRescanNeeded is set, trigger a full re-scan by
/// re-running the file loader system logic directly.
pub fn apply_space_rescan(
    mut rescan: ResMut<SpaceRescanNeeded>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<crate::space::SpaceFileRegistry>,
    mut material_registry: ResMut<crate::space::material_loader::MaterialRegistry>,
    mut mesh_cache: ResMut<crate::space::instance_loader::PrimitiveMeshCache>,
    space_root: Res<crate::space::SpaceRoot>,
    class_defaults: Option<Res<crate::space::class_defaults::ClassDefaultsRegistry>>,
) {
    if !rescan.0 { return; }
    rescan.0 = false;

    let space_path = &space_root.0;
    if !space_path.exists() {
        warn!("Space path does not exist, skipping rescan: {:?}", space_path);
        return;
    }

    info!("🔄 Re-scanning Space at {:?}", space_path);

    use crate::space::file_loader::{scan_space_directory, FileType};
    let entries = scan_space_directory(space_path);
    info!("🔍 Discovered {} top-level entries", entries.len());

    let cd_ref = class_defaults.as_deref();
    for entry in &entries {
        match entry.file_type {
            FileType::Directory => {
                crate::space::file_loader::spawn_directory_entry(
                    &mut commands, &asset_server, &mut meshes, &mut materials,
                    &mut registry, &mut material_registry, &mut mesh_cache, space_path, entry, None,
                    cd_ref,
                );
            }
            _ => {
                crate::space::file_loader::spawn_file_entry(
                    &mut commands, &asset_server, &mut meshes, &mut materials,
                    &mut registry, &mut material_registry, &mut mesh_cache, space_path, entry, None,
                    cd_ref,
                );
            }
        }
    }
}

// ============================================================================
// 5. New Space — scaffold + switch to it
// ============================================================================

pub fn new_universe(world: &mut World) {
    let workspace_root = crate::space::workspace_root();

    let Some(requested_universe_root) = pick_new_universe_root(&workspace_root) else {
        info!("🪐 New Universe cancelled by user");
        return;
    };

    let Some(parent_dir) = requested_universe_root.parent() else {
        if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
            notifs.error("Failed to resolve the workspace root for the new Universe.");
        }
        return;
    };

    if parent_dir != workspace_root.as_path() {
        if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
            notifs.error(format!(
                "New Universes must be created directly under {}.",
                workspace_root.display()
            ));
        }
        return;
    }

    let universe_name = space_name_from_path(&requested_universe_root);
    if requested_universe_root.exists() {
        if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
            notifs.error(format!("Universe '{}' already exists.", universe_name));
        }
        return;
    }

    match std::fs::create_dir(&requested_universe_root) {
        Ok(()) => {
            // Create Universe-level asset directories and copy engine default parts
            let _ = std::fs::create_dir_all(requested_universe_root.join("assets").join("parts"));
            let _ = std::fs::create_dir_all(requested_universe_root.join("assets").join("meshes"));
            copy_engine_default_parts(&requested_universe_root.join("assets").join("parts"));

            if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
                notifs.success(format!("Universe created: {}", universe_name));
            }
        }
        Err(e) => {
            error!("❌ Failed to create Universe: {}", e);
            if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
                notifs.error(format!("Failed to create Universe: {}", e));
            }
        }
    }
}

pub fn new_space(world: &mut World) {
    let current_space_root = world.get_resource::<crate::space::SpaceRoot>().map(|root| root.0.clone());
    let initial_dir = resolve_active_universe_root(current_space_root.as_deref());

    let Some(requested_space_root) = pick_new_space_root(&initial_dir) else {
        info!("🆕 New Space cancelled by user");
        return;
    };

    let Some(parent_dir) = requested_space_root.parent().map(Path::to_path_buf) else {
        if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
            notifs.error("Failed to resolve the target Universe folder for the new Space.");
        }
        return;
    };

    let workspace_root = crate::space::workspace_root();
    if parent_dir == workspace_root || parent_dir.parent() != Some(workspace_root.as_path()) {
        if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
            notifs.error(format!(
                "New Spaces must be created directly inside a Universe folder under {}.",
                workspace_root.display()
            ));
        }
        return;
    }

    let space_name = space_name_from_path(&requested_space_root);
    let author = {
        world.get_resource::<crate::auth::AuthState>()
            .and_then(|a| a.user.as_ref())
            .map(|u| u.username.clone())
            .unwrap_or_else(|| "Eustress User".to_string())
    };

    match scaffold_new_space(&parent_dir, &space_name, &author) {
        Ok(result) => {
            open_space(world, &result.space_root);
        }
        Err(e) => {
            error!("❌ Failed to scaffold new Space: {}", e);
            if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
                notifs.error(format!("Failed to create Space: {}", e));
            }
        }
    }
}

fn ensure_manifest_set(space_root: &Path, preferred_name: Option<&str>, preferred_author: Option<&str>) {
    let project_dir = space_root.join(".eustress");
    let _ = std::fs::create_dir_all(project_dir.join("local"));

    let now = Utc::now().to_rfc3339();
    let space_name = preferred_name
        .map(|value| value.to_string())
        .unwrap_or_else(|| space_name_from_path(space_root));
    let author = preferred_author.unwrap_or("Eustress User");

    ensure_manifest_file(
        &project_dir.join("project.toml"),
        &ProjectManifest::new(&space_name, author, &now),
    );
    ensure_manifest_file(
        &project_dir.join("settings.toml"),
        &ProjectSettingsManifest::default(),
    );
    ensure_manifest_file(
        &project_dir.join("sync.toml"),
        &SyncManifest::default(),
    );
    ensure_manifest_file(
        &project_dir.join("asset-index.toml"),
        &AssetIndexManifest::default(),
    );
    ensure_manifest_file(
        &project_dir.join("package-index.toml"),
        &PackageIndexManifest::default(),
    );
    ensure_manifest_file(
        &project_dir.join("publish.toml"),
        &PublishManifest::default(),
    );
    ensure_manifest_file(
        &project_dir.join("publish-journal.toml"),
        &PublishJournalManifest::new(&now),
    );
}

fn ensure_manifest_file<T: serde::Serialize>(path: &Path, value: &T) {
    if path.exists() {
        return;
    }

    if let Err(e) = save_manifest(path, value) {
        warn!("Failed to initialize manifest {:?}: {}", path, e);
    }
}

fn space_name_from_path(space_path: &Path) -> String {
    space_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".to_string())
}

/// Returns the default simulation.toml content — also used by file_event_handler
/// to ensure every saved Space has a simulation.toml for play-mode readiness.
pub fn default_simulation_toml() -> &'static str {
    SIMULATION_TOML_CONTENT
}

const SIMULATION_TOML_CONTENT: &str = "# Simulation configuration -- SIMULATION_SYSTEM.md\n\
# Controls tick-based time compression for physics and product simulations.\n\
\n\
[simulation]\n\
tick_rate_hz = 60.0\n\
time_scale = 1.0\n\
max_ticks_per_frame = 10\n\
auto_start = false\n\
\n\
[simulation.recording]\n\
enabled = false\n\
output_dir = \".eustress/local/recordings\"\n\
format = \"both\"\n\
auto_export = false\n\
\n\
# [[watchpoints]]\n\
# name = \"voltage\"\n\
# label = \"Cell Voltage\"\n\
# unit = \"V\"\n\
# interval = 1\n\
# color = \"#4CAF50\"\n\
\n\
# [[breakpoints]]\n\
# name = \"low_soc\"\n\
# variable = \"soc\"\n\
# comparison = \"<\"\n\
# threshold = 20.0\n\
# one_shot = false\n\
\n\
# [[tests]]\n\
# name = \"cycle_life_test\"\n\
# script = \"src/cycle_life_test.soul\"\n\
# time_scale = 7200000.0\n\
# max_time_s = 7200000.0\n";

fn simulation_toml() -> String {
    SIMULATION_TOML_CONTENT.to_string()
}

fn space_meta_toml(space_name: &str, author: &str) -> String {
    let now = Utc::now().to_rfc3339();
    format!(
        r#"# EEP Space metadata
[space]
name = "{space_name}"
author = "{author}"
version = "0.1.0"
created_with = "Eustress Engine"

[metadata]
created = "{now}"
last_modified = "{now}"
"#,
        space_name = space_name,
        author = author,
        now = now,
    )
}

fn service_toml(name: &str, class: &str, icon: &str, description: &str) -> String {
    let now = Utc::now().to_rfc3339();
    format!(
        r#"# EEP _service.toml — marks this folder as a Service container.
[service]
class_name = "{class}"
icon = "{icon}"
description = "{description}"
can_have_children = true

[metadata]
id = "{class_lower}-service"
created = "{now}"
last_modified = "{now}"
"#,
        class = class,
        class_lower = class.to_lowercase(),
        icon = icon,
        description = description,
        now = now,
    )
}

fn baseplate_part_toml() -> String {
    let now = Utc::now().to_rfc3339();
    format!(
        r#"# EEP Part instance — Baseplate
[metadata]
class_name = "Part"
archivable = true
created = "{now}"
last_modified = "{now}"

[asset]
mesh = "parts/block.glb"
scene = "Scene0"

[transform]
position = [0.0, -0.5, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [512.0, 1.0, 512.0]

[properties]
color = [0.388, 0.373, 0.384, 1.0]
transparency = 0.0
reflectance = 0.1
anchored = true
can_collide = true
locked = true
"#,
        now = now,
    )
}

fn welcome_cube_part_toml() -> String {
    let now = Utc::now().to_rfc3339();
    format!(
        r#"# EEP Part instance — Welcome Cube
[metadata]
class_name = "Part"
archivable = true
created = "{now}"
last_modified = "{now}"

[asset]
mesh = "parts/block.glb"
scene = "Scene0"

[transform]
position = [0.0, 2.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [4.0, 4.0, 4.0]

[properties]
color = [0.388, 0.706, 1.0, 1.0]
transparency = 0.0
reflectance = 0.2
anchored = true
can_collide = true
locked = false
"#,
        now = now,
    )
}

fn sky_toml() -> String {
    let now = Utc::now().to_rfc3339();
    format!(
        r#"# EEP Sky instance
[instance]
name = "Sky"
class_name = "Sky"
archivable = true
ai = false

[metadata]
id = "sky-001"
created = "{now}"
last_modified = "{now}"

[sky]
skybox_texture = ""
celestial_bodies_shown = true
sun_angular_velocity = 0.0
"#,
        now = now,
    )
}

fn atmosphere_toml() -> String {
    let now = Utc::now().to_rfc3339();
    format!(
        r#"# EEP Atmosphere instance
[instance]
name = "Atmosphere"
class_name = "Atmosphere"
archivable = true
ai = false

[metadata]
id = "atmosphere-001"
created = "{now}"
last_modified = "{now}"

[atmosphere]
density = 0.395
offset = 0.25
color = [0.784, 0.859, 1.0, 1.0]
decay = 1.0
glare = 0.0
haze = 0.0
"#,
        now = now,
    )
}

const GITIGNORE: &str = r#"# Eustress — gitignore
# User-local state — not committed
.eustress/local/

# OS artifacts
.DS_Store
Thumbs.db
desktop.ini

# Rust build artifacts (if scripts are compiled in-tree)
target/
"#;

// ============================================================================
// 7. (Cache removed — Bevy World is the sole runtime source of truth)

// ============================================================================
// Utilities
// ============================================================================

fn create_dir_all(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|e| format!("Failed to create directory {:?}: {}", path, e))
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content)
        .map_err(|e| format!("Failed to write {:?}: {}", path, e))
}

/// Save a manifest file using eustress-common's save_toml_file function.
fn save_manifest<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    save_toml_file(value, path)
        .map_err(|e| format!("Failed to write {:?}: {}", path, e))
}

/// Strip characters that are illegal in file system names.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}
