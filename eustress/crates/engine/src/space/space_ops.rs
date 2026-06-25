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
use bevy::prelude::*;
use chrono::Utc;

use crate::space::instance_loader::{
    InstanceDefinition, InstanceMetadata, AssetReference,
    TransformData, InstanceProperties,
};
use crate::space::service_loader::ServiceComponent;
use crate::notifications::NotificationManager;

use eustress_common::{
    AssetIndexManifest, PackageIndexManifest,
    ProjectManifest, ProjectSettingsManifest,
    PublishJournalManifest, PublishManifest,
    SyncManifest, save_toml_file,
};

/// True when `space_root` is a fully-converted `.eustress` world —
/// `header.bin` carries a `migrated_at` stamp, so `world.fjalldb/` is
/// authoritative and there are deliberately no loose service trees on
/// disk. Every disk-regenerating / disk-detecting path checks this and
/// stands down so a migrated world stays clean and DB-sourced.
///
/// Without the `world-db` feature there is no DB and no migration
/// concept, so this is always `false` (legacy disk behaviour intact).
#[cfg(feature = "world-db")]
pub fn space_is_migrated(space_root: &Path) -> bool {
    matches!(
        eustress_worlddb::header::WorldHeader::read(space_root),
        Ok(Some(h)) if h.is_migrated()
    )
}

#[cfg(not(feature = "world-db"))]
pub fn space_is_migrated(_space_root: &Path) -> bool {
    false
}

// ============================================================================
// 1. Service Manifest — EEP service folders created for every new Space
// ============================================================================

/// Service folder names that every new Space receives per EEP_SPECIFICATION.md.
/// Order matters: Workspace first so the 3D viewport has a target immediately.
const SERVICE_FOLDERS: &[ServiceFolder] = &[
    ServiceFolder { name: "Workspace",               class: "Workspace",              icon: "workspace",          description: "3D world objects — Parts, Models, Terrain" },
    ServiceFolder { name: "Lighting",                class: "Lighting",               icon: "lighting",           description: "Light sources — Sun, Sky, Atmosphere" },
    ServiceFolder { name: "Players",                 class: "Players",                icon: "players",            description: "Player instances and character models" },
    ServiceFolder { name: "StarterGui",              class: "StarterGui",             icon: "startergui",         description: "UI templates shown to every player" },
    ServiceFolder { name: "StarterPack",             class: "StarterPack",            icon: "starterpack",        description: "Tools given to players on spawn" },
    ServiceFolder { name: "StarterPlayerScripts",    class: "StarterPlayerScripts",   icon: "starterplayer",      description: "Scripts cloned into each player on join" },
    ServiceFolder { name: "StarterCharacterScripts", class: "StarterCharacterScripts",icon: "starterplayer",      description: "Scripts cloned into each character on spawn" },
    ServiceFolder { name: "ReplicatedStorage",       class: "ReplicatedStorage",      icon: "replicatedstorage",  description: "Shared assets visible to server and client" },
    ServiceFolder { name: "ServerStorage",           class: "ServerStorage",          icon: "serverstorage",      description: "Server-only assets hidden from clients" },
    ServiceFolder { name: "ServerScriptService",     class: "ServerScriptService",    icon: "serverscriptservice",description: "Server-side scripts" },
    ServiceFolder { name: "SoulService",             class: "SoulService",            icon: "soulservice",        description: "Soul and Rune scripts (.soul, .rune files)" },
    ServiceFolder { name: "MaterialService",         class: "MaterialService",        icon: "materialservice",    description: "PBR material definitions (.mat.toml files)" },
    ServiceFolder { name: "SoundService",            class: "SoundService",           icon: "soundservice",       description: "Audio — Sound effects and music" },
    ServiceFolder { name: "AdornmentService",        class: "AdornmentService",       icon: "adornmentservice",   description: "Beams, billboards, particles, highlights" },
    ServiceFolder { name: "DataService",             class: "DataService",            icon: "folder",             description: "Data Platform — datasets, series, columns, and runs" },
    ServiceFolder { name: "Teams",                   class: "Teams",                  icon: "teams",              description: "Team definitions and spawn points" },
    ServiceFolder { name: "Chat",                    class: "Chat",                   icon: "chat",               description: "In-game chat system" },
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
    create_dir_all(&space_root.join(".eustress").join("knowledge"))?;
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
    // Copy _service.toml from common/assets/service_templates/<Name>/ so all
    // properties, icons, and descriptions are data-driven from the templates.
    // Common is the canonical asset source — engine no longer ships a sister
    // copy (see 2026-05-12 consolidation).
    let svc_template_dir = eustress_common::service_templates_dir();

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

    // ── Workspace/Baseplate/_instance.toml ──────────────────────────────────
    let baseplate_dir = space_root.join("Workspace").join("Baseplate");
    std::fs::create_dir_all(&baseplate_dir)
        .map_err(|e| format!("Failed to create Baseplate dir: {}", e))?;
    write_file(
        &baseplate_dir.join("_instance.toml"),
        &baseplate_part_toml(),
    )?;

    // ── Workspace/WelcomeCube/_instance.toml ──────────────────────────────
    let cube_dir = space_root.join("Workspace").join("WelcomeCube");
    std::fs::create_dir_all(&cube_dir)
        .map_err(|e| format!("Failed to create WelcomeCube dir: {}", e))?;
    write_file(
        &cube_dir.join("_instance.toml"),
        &welcome_cube_part_toml(),
    )?;

    // ── Lighting children (.instance.toml — picked up by file loader) ───────
    // Copy templates from assets/lighting_templates/ to Lighting/ folder.
    // Files use .instance.toml extension so FileType::from_path returns Toml
    // and the file loader spawns them as ECS entities with Instance components.
    let lighting_template_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("lighting_templates");

    let lighting_children = ["Atmosphere", "Moon", "Sky", "Sun"];
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
        let parts_dir = universe_root.join(".eustress").join("assets").join("parts");
        let _ = std::fs::create_dir_all(&parts_dir);
        let _ = std::fs::create_dir_all(universe_root.join(".eustress").join("assets").join("meshes"));
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
        // Use the LOCAL Transform, not GlobalTransform.
        //
        // Every `_instance.toml` stores `[transform] position/rotation/scale`
        // as values LOCAL to the entity's parent. The loader applies
        // these as the entity's local Transform and lets Bevy compose
        // them with the parent's GlobalTransform. Writing the *global*
        // transform during save made every nested save → reload drift
        // the part by the parent's transform (or compose the parent's
        // rotation a second time) — which is why the user's "neat
        // door" scene came back as a mess after a session close/open.
        // For top-level parts with identity-parent Workspace this was
        // a no-op; any grouped/folder-nested or duplicated-while-
        // parented part accumulated the drift.
        let mut query = world.query::<(
            Entity,
            &eustress_common::classes::Instance,
            &eustress_common::classes::BasePart,
            &Transform,
            Option<&crate::space::instance_loader::InstanceFile>,
            Option<&eustress_common::classes::Part>,
        )>();

        let now = Utc::now().to_rfc3339();

        for (_entity, instance, base_part, local_tf, instance_file, part) in query.iter(world) {
            use eustress_common::classes::ClassName;
            match instance.class_name {
                ClassName::Sky | ClassName::Atmosphere | ClassName::Camera
                | ClassName::Star | ClassName::Moon | ClassName::Clouds => continue,
                _ => {}
            }

            let toml_path = if let Some(inst_file) = instance_file {
                inst_file.toml_path.clone()
            } else {
                // New entity without InstanceFile — create folder structure
                let safe_name = sanitize_filename(&instance.name);
                let part_dir = workspace_dir.join(&safe_name);
                let _ = std::fs::create_dir_all(&part_dir);
                part_dir.join("_instance.toml")
            };

            let t = *local_tf;
            let authoritative_size = base_part.size;

            // Preserve the display-name override when the folder name
            // and instance name don't match — e.g. a second sibling
            // "Block" lives in `Block-a3f2/` with `name = "Block"` in
            // the TOML so the Explorer still renders it as "Block".
            let folder_stem = toml_path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());
            let name_override = match &folder_stem {
                Some(stem) if *stem != instance.name => Some(instance.name.clone()),
                _ => None,
            };

            // Component-authoritative fields (the only ones the ECS owns).
            // TOML scale = BasePart.size (correct in both scale-tool
            // branches; Transform.scale alone pinned legacy parts at 1×1×1).
            let live_transform = TransformData {
                position: [t.translation.x, t.translation.y, t.translation.z],
                rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
                scale: [authoritative_size.x, authoritative_size.y, authoritative_size.z],
            };
            let live_color = {
                let c = base_part.color.to_srgba();
                [c.red, c.green, c.blue, c.alpha]
            };
            // Prefer the live material NAME (preserves custom MaterialService
            // names + Material-Flip edits); fall back to the enum.
            let live_material = if base_part.material_name.is_empty() {
                format!("{:?}", base_part.material)
            } else {
                base_part.material_name.clone()
            };

            // ── LOAD-MERGE (2026-05-22) ─────────────────────────────────
            // Start from the EXISTING on-disk definition and overwrite only
            // the component-authoritative fields. Rebuilding the whole
            // `InstanceDefinition` from components alone (what this path did
            // before) silently dropped every field the ECS does not carry:
            //   * the custom `asset.mesh` — it was re-derived from the
            //     primitive `Part.shape`, so a custom-mesh part (V-Cell)
            //     came back as `parts/block.glb` and rendered as a block;
            //   * the realism `[material]` / `[thermodynamic]` /
            //     `[electrochemical]` sections (V-Cell's Titanium material);
            //   * the metadata audit chain, `attributes`, `tags`, `ui`,
            //     `[extra]`.
            // That was the V-Cell "renders as blocks, lost its material"
            // data loss. Read DISK directly (`load_instance_definition_with_extras`,
            // NOT the active_db funnel) so the merge base is the on-disk
            // TOML, never a stale binary `#bin` cache; the write below then
            // refreshes that cache from the corrected merge.
            let existing = if instance_file.is_some() {
                crate::space::instance_loader::load_instance_definition_with_extras(&toml_path)
                    .map(|(d, _)| d)
                    .ok()
            } else {
                None
            };

            let def = if let Some(mut d) = existing {
                // SCALE GUARD (2026-05-24): for a CUSTOM-mesh part (mesh not
                // under "parts/", e.g. "../meshes/Foo.glb") the TOML `scale`
                // is the user's MULTIPLIER, while `BasePart.size` is the
                // mesh-AABB-derived world size. Writing size→scale here
                // double-applies it on reload and stretches the mesh (the
                // V-Supreme "Save broke the suit" bug). So for custom meshes
                // keep the on-disk scale and only refresh position+rotation;
                // primitives (block.glb etc., size == scale) take the full
                // live_transform. Mirrors the guard in
                // `write_instance_changes_system` (instance_loader.rs).
                let is_custom_mesh = d.asset.as_ref()
                    .map(|a| crate::space::representation::mesh_requires_filesystem(&a.mesh))
                    .unwrap_or(false);
                if is_custom_mesh {
                    d.transform.position = live_transform.position;
                    d.transform.rotation = live_transform.rotation;
                    // d.transform.scale preserved from disk (user multiplier)
                } else {
                    d.transform = live_transform;
                }
                d.properties.color = live_color;
                d.properties.transparency = base_part.transparency;
                d.properties.anchored = base_part.anchored;
                d.properties.can_collide = base_part.can_collide;
                d.properties.cast_shadow = base_part.cast_shadow;
                d.properties.reflectance = base_part.reflectance;
                d.properties.locked = base_part.locked;
                d.properties.material = live_material;
                d.metadata.name = name_override;
                d.metadata.last_modified = now.clone();
                d
            } else {
                // New entity (no on-disk TOML) or unreadable file — build
                // from components. New parts spawned here are primitives, so
                // deriving the mesh from `Part.shape` is correct for them.
                let mesh = part
                    .map(|p| match p.shape {
                        eustress_common::classes::PartType::Block => "parts/block.glb",
                        eustress_common::classes::PartType::Ball => "parts/ball.glb",
                        eustress_common::classes::PartType::Cylinder => "parts/cylinder.glb",
                        eustress_common::classes::PartType::Wedge => "parts/wedge.glb",
                        eustress_common::classes::PartType::CornerWedge => "parts/corner_wedge.glb",
                        eustress_common::classes::PartType::Cone => "parts/cone.glb",
                    })
                    .unwrap_or("parts/block.glb")
                    .to_string();
                let class_name = format!("{:?}", instance.class_name)
                    .trim_start_matches("ClassName::")
                    .to_string();
                InstanceDefinition {
                    nuclear: None,
                    plasma: None,
                    asset: Some(AssetReference {
                        mesh,
                        scene: "Scene0".to_string(),
                    }),
                    transform: live_transform,
                    properties: InstanceProperties {
                        color: live_color,
                        material: live_material,
                        transparency: base_part.transparency,
                        anchored: base_part.anchored,
                        can_collide: base_part.can_collide,
                        cast_shadow: base_part.cast_shadow,
                        reflectance: base_part.reflectance,
                        locked: base_part.locked,
                        physics: None,
                    },
                    metadata: InstanceMetadata {
                        class_name,
                        archivable: instance.archivable,
                        name: name_override,
                        created: String::new(),
                        last_modified: now.clone(),
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
                }
            };

            to_save.push((instance.name.clone(), toml_path, def));
        }
    }

    // Stamp every save with the current user's identity when logged in. The
    // stamp is cheap (~100 bytes) and kept forever — the full chain feeds
    // Bliss attribution and AI "who is capable of what" training data.
    let stamp = world.get_resource::<crate::auth::AuthState>()
        .and_then(crate::space::instance_loader::current_stamp);

    for (name, path, def) in to_save.iter_mut() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match crate::space::instance_loader::write_instance_definition_signed(
            path, def, stamp.as_ref(),
        ) {
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
                if let Err(e) = crate::space::service_loader::save_service_to_file_signed(svc, stamp.as_ref()) {
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

    // Verify and repair: create any missing service folders + knowledge dir
    ensure_space_integrity(space_path);

    // Migrate recordings dir if space was renamed
    migrate_recordings_on_rename(space_path);

    // Ensure Universe-level assets/parts/ has engine default GLBs
    ensure_universe_default_parts(space_path);

    info!("📂 Opening Space: {:?}", space_path);

    // Persist the outgoing space's Output panel to disk before we clear it,
    // and remember which space owns the current buffer. Without this the
    // Output panel keeps logs from the previous space mixed with the new
    // space's logs and nothing ever survives a restart.
    let outgoing_space: Option<std::path::PathBuf> = world
        .get_resource::<crate::space::SpaceRoot>()
        .map(|r| r.0.clone());
    if let Some(ref outgoing) = outgoing_space {
        if let Some(console) = world.get_resource::<crate::ui::slint_ui::OutputConsole>() {
            console.save_to_space(outgoing);
        }
    }
    if let Some(mut console) = world.get_resource_mut::<crate::ui::slint_ui::OutputConsole>() {
        console.clear();
    }

    // Snapshot the outgoing space's center tabs so the user gets the same
    // tab layout back when they navigate to this space again. The Scene
    // tab is rebuilt fresh at index 0 by the restore step.
    //
    // Before snapshotting, stamp each tab's `file_path` from the entity's
    // `LoadedFromFile` component if available. Entity IDs don't survive a
    // world reload, but file paths do — so SoulScript and ParametersEditor
    // tabs that we'd otherwise filter on restore become persistable.
    if let Some(ref outgoing) = outgoing_space {
        // Collect entity → path mapping from the current world.
        let entity_paths: std::collections::HashMap<bevy::prelude::Entity, std::path::PathBuf> = {
            let mut q = world.query::<(bevy::prelude::Entity, &crate::space::file_loader::LoadedFromFile)>();
            q.iter(world)
                .map(|(e, lff)| (e, lff.path.clone()))
                .collect()
        };
        if let Some(mut tab_mgr) = world.get_resource_mut::<crate::ui::center_tabs::CenterTabManager>() {
            for tab in tab_mgr.tabs.iter_mut() {
                if tab.file_path.is_none() {
                    if let Some(entity) = tab.entity {
                        if let Some(path) = entity_paths.get(&entity) {
                            tab.file_path = Some(path.clone());
                        }
                    }
                }
            }
            tab_mgr.snapshot_for_space(outgoing);
        }
    }

    // Clear Workspace/.generated/ — ephemeral entities written by execute_luau.
    // These are transient (generated by scripts) and must not survive a Space
    // switch; the file watcher would re-load them on the next open otherwise.
    let generated_dir = space_path.join("Workspace").join(".generated");
    if generated_dir.exists() {
        let _ = std::fs::remove_dir_all(&generated_dir);
        info!("🗑️ Cleared Workspace/.generated/ generated entities");
    }

    // Despawn ALL Instance entities — including lighting primitives
    // (Sun, Moon, Sky, Atmosphere). Each Space owns its own lighting
    // via Lighting/*.instance.toml files; the file loader + the
    // hydrate_lighting_entities system re-create them with proper
    // DirectionalLight / marker components from the new Space's TOMLs.
    // Discard any frame-budget spill from the OUTGOING world FIRST.
    // Otherwise `drain_pending_spawns` could spawn those queued
    // children moments later, parented to entities we are about to
    // despawn — a flood of dead-`ChildOf` orphans (the 47k-warning
    // storm) and parts detached from their folder.
    crate::space::file_loader::discard_pending_spawns();

    // Despawn children-FIRST, roots last. A flat despawn over all
    // `Instance`s in arbitrary query order despawns a parent (e.g. the
    // Workspace folder, entity 1287) before its 50k benchpart
    // children; Bevy then fires an invalid-`ChildOf` warn+strip for
    // EVERY orphaned child — tens of thousands of WARN lines in one
    // frame plus needless relationship churn. Two passes — entities
    // that HAVE a `ChildOf` (children) before those that don't (roots)
    // — means each child is gone before its parent, so the orphan
    // window never opens. (The benchmark tree is flat: Workspace root →
    // 50k leaf benchparts, so two passes fully eliminate it.)
    let children: Vec<Entity> = {
        let mut q = world.query_filtered::<
            Entity,
            (
                With<eustress_common::classes::Instance>,
                With<bevy::prelude::ChildOf>,
            ),
        >();
        q.iter(world).collect()
    };
    let roots: Vec<Entity> = {
        let mut q = world.query_filtered::<
            Entity,
            (
                With<eustress_common::classes::Instance>,
                Without<bevy::prelude::ChildOf>,
            ),
        >();
        q.iter(world).collect()
    };
    let count = children.len() + roots.len();
    for entity in children {
        world.despawn(entity);
    }
    for entity in roots {
        world.despawn(entity);
    }
    info!("🗑️ Cleared {} existing entities (children-first, no orphan storm)", count);

    if let Some(mut registry) = world.get_resource_mut::<crate::space::SpaceFileRegistry>() {
        *registry = crate::space::SpaceFileRegistry::default();
    }

    // Reset the MaterialRegistry. Without this, the previous space's
    // .mat.toml definitions linger in the name → handle map and the dedup
    // cache holds stale Handle<StandardMaterial> references whose underlying
    // assets were freed when the entities got despawned. Result: parts in
    // the new space that reference a material name shared with the old
    // space (e.g. "Plastic", "Bronze") resolve to a dangling handle and
    // render as the default magenta-or-checker fallback. The file-loader
    // re-populates this from the new space's MaterialService/ on rescan.
    if let Some(mut mat_registry) = world.get_resource_mut::<crate::space::material_loader::MaterialRegistry>() {
        *mat_registry = crate::space::material_loader::MaterialRegistry::default();
    }

    // Reset the camera-locality streaming residency manager. Its state —
    // the `enabled` flag, the resident-cell set, and CRITICALLY the
    // `pending_cores` buffer of raw core bytes already scanned from the
    // OUTGOING Space's `entities` partition — must NOT carry into the new
    // Space. Without this, the next `sys_residency_load` tick drains the
    // previous Space's buffered cores and spawns them here: cross-Space
    // part bleed (a part/model made in Space A appears in Space B). Stale
    // `resident_cells` would also make the new Space's own cells look
    // already-loaded, so they'd never spawn. The incoming Space's boot-load
    // re-decides `enabled` and the manager reloads cells from the correct
    // (now-switched) DB.
    #[cfg(feature = "world-db")]
    if let Some(mut residency) =
        world.get_resource_mut::<crate::space::residency::ResidencyState>()
    {
        *residency = crate::space::residency::ResidencyState::default();
    }
    // Phase 4: clear the non-gated streaming flag + the Explorer's DB-section
    // cache so the virtual "Database (streamed)" section never shows the
    // outgoing Space's classes/rows before the new boot-load re-decides. The
    // boot-load sets the flag true again for a large incoming Space.
    crate::space::active_db::set_streaming_active(false);
    if let Some(mut es) =
        world.get_resource_mut::<crate::ui::slint_ui::UnifiedExplorerState>()
    {
        es.cached_db_classes.clear();
        es.cached_db_pages.clear();
        es.streamed_row_cache.clear();
        es.db_class_id_cache.clear();
        es.expanded_db_classes.clear();
        es.db_cache_valid = false;
    }

    // Bump the load generation and clear any in-flight deferred queue.
    // Any load_deferred_services frame that already popped an entry will
    // see generation != gen.0 on its NEXT iteration and self-discard.
    // Clearing pending here handles the case where we switch again before
    // the first deferred frame even runs.
    if let Some(mut gen) = world.get_resource_mut::<crate::space::file_loader::SpaceLoadGeneration>() {
        gen.0 += 1;
    }
    if let Some(mut deferred) = world.get_resource_mut::<crate::space::file_loader::DeferredServiceLoader>() {
        deferred.pending.clear();
        deferred.priority_done = false;
    }
    // Re-gate write-back: the upcoming rescan re-spawns every entity and
    // re-fires the same mesh-resolve / class-default churn the cold-load
    // path triggers. Without this, switching universes mid-session would
    // race the write-storm bug it was meant to avoid.
    if let Some(mut load) = world.get_resource_mut::<crate::space::file_loader::LoadInProgress>() {
        load.begin();
    }

    world.insert_resource(crate::space::SpaceRoot(space_path.to_path_buf()));
    // Stamp the swappable `space://` asset root IMMEDIATELY (not just via the
    // `Changed<SpaceRoot>` system next frame): the rescan triggered below can
    // begin issuing `space://` mesh loads within this same world-command, and
    // they must resolve against the NEW Space root, not the launch root —
    // otherwise the new Space loads with no meshes (black screen).
    crate::space::space_asset_source::set_space_asset_root(space_path.to_path_buf());

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

    // Load the new space's Output panel buffer. Empty file / missing file =
    // start fresh. Done AFTER SpaceRoot is set so the next push uses the
    // right path for its incremental save.
    if let Some(mut console) = world.get_resource_mut::<crate::ui::slint_ui::OutputConsole>() {
        console.load_from_space(space_path);
    }

    // Restore the incoming space's center tabs from snapshot, or fall back
    // to a fresh Scene-only layout if this is the first visit. Tabs whose
    // entity refs are stale across the world reload are filtered inside
    // restore_for_space — file-based tabs survive verbatim.
    if let Some(mut tab_mgr) = world.get_resource_mut::<crate::ui::center_tabs::CenterTabManager>() {
        tab_mgr.restore_for_space(space_path);
    }

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
    mut decal_materials: ResMut<Assets<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>>,
    space_root: Res<crate::space::SpaceRoot>,
    class_defaults: Option<Res<crate::space::class_defaults::ClassDefaultsRegistry>>,
    mut deferred: ResMut<crate::space::file_loader::DeferredServiceLoader>,
    gen: Res<crate::space::file_loader::SpaceLoadGeneration>,
    mut load_in_progress: ResMut<crate::space::file_loader::LoadInProgress>,
    active_source: Res<crate::space::space_source::ActiveSpaceSource>,
) {
    if !rescan.0 { return; }
    rescan.0 = false;

    let space_path = &space_root.0;
    if !space_path.exists() {
        warn!("Space path does not exist, skipping rescan: {:?}", space_path);
        return;
    }

    warn!(
        target: "eustress_engine::world_db",
        space = %space_path.display(),
        "🔄 apply_space_rescan FIRED — full Space re-scan. If this recurs on a fixed interval it IS the periodic ~2.67s stutter. Now frame-budgeted (streams via drain_pending_spawns) instead of a synchronous 50k freeze."
    );
    // Gate write-back through the rescan's mesh-resolve / class-default churn.
    load_in_progress.begin();
    // Same frame-budget arming as the initial load — without this the
    // rescan re-ran the entire 50k scan+spawn synchronously in one
    // frame (the periodic multi-second stutter the user observed).
    crate::space::file_loader::begin_budgeted_load(gen.0);

    use crate::space::file_loader::{scan_space_directory, FileType, PRIORITY_SERVICES};
    let source_arc = active_source.0.clone();
    let source = source_arc.as_ref();
    let entries = scan_space_directory(source, space_path);
    info!("🔍 Discovered {} top-level entries", entries.len());

    let cd_ref = class_defaults.as_deref();

    // Load priority services immediately, defer the rest
    let mut deferred_entries = Vec::new();
    for entry in entries {
        let is_priority = PRIORITY_SERVICES.iter().any(|s| entry.name == *s);
        if is_priority {
            crate::space::file_loader::rearm_priority_budget();
            match entry.file_type {
                FileType::Directory => {
                    crate::space::file_loader::spawn_directory_entry(
                        &mut commands, &asset_server, &mut meshes, &mut materials,
                        &mut registry, &mut material_registry, &mut mesh_cache, &mut decal_materials, space_path, &entry, None,
                        cd_ref, source,
                    );
                }
                _ => {
                    crate::space::file_loader::spawn_file_entry(
                        &mut commands, &asset_server, &mut meshes, &mut materials,
                        &mut registry, &mut material_registry, &mut mesh_cache, &mut decal_materials, space_path, &entry, None,
                        cd_ref, source,
                    );
                }
            }
        } else {
            deferred_entries.push(entry);
        }
    }

    deferred.pending = deferred_entries;
    deferred.priority_done = true;
    deferred.generation = gen.0;
    info!("📋 Deferred {} services for background loading", deferred.pending.len());
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
            // Create Universe-level directories and copy engine default parts
            let _ = std::fs::create_dir_all(requested_universe_root.join(".eustress").join("assets").join("parts"));
            let _ = std::fs::create_dir_all(requested_universe_root.join(".eustress").join("assets").join("meshes"));
            let _ = std::fs::create_dir_all(requested_universe_root.join(".eustress").join("knowledge"));
            copy_engine_default_parts(&requested_universe_root.join(".eustress").join("assets").join("parts"));

            // Scaffold default Space with full service structure
            let spaces_dir = requested_universe_root.join("Spaces");
            let author = world.get_resource::<crate::auth::AuthState>()
                .and_then(|a| a.user.as_ref())
                .map(|u| u.username.clone())
                .unwrap_or_else(|| "Eustress User".to_string());

            match scaffold_new_space(&spaces_dir, "Space1", &author) {
                Ok(result) => {
                    if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
                        notifs.success(format!("Universe '{}' created with Space1", universe_name));
                    }
                    // Force the Universes panel to rescan immediately
                    if let Some(mut registry) = world.get_resource_mut::<crate::space::UniverseRegistry>() {
                        registry.rescan_requested = true;
                    }
                    info!("🪐 Opening new Universe: {}", universe_name);
                    open_space(world, &result.space_root);
                }
                Err(e) => {
                    warn!("⚠ Space scaffold failed: {} — opening empty universe", e);
                    if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
                        notifs.success(format!("Universe '{}' created (empty)", universe_name));
                    }
                }
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

/// Create a Universe folder with a confirmed name from the UI dialog.
/// This creates the universe directory structure but does NOT automatically create a space.
/// The caller is responsible for creating spaces afterward.
pub fn create_universe_folder(world: &mut World, universe_name: &str) -> Result<PathBuf, String> {
    let workspace_root = crate::space::workspace_root();
    let sanitized_name = sanitize_filename(universe_name);

    if sanitized_name.is_empty() {
        return Err("Universe name cannot be empty".to_string());
    }

    let universe_path = workspace_root.join(&sanitized_name);

    // Check if already exists
    if universe_path.exists() {
        return Err(format!("Universe '{}' already exists", sanitized_name));
    }

    // Create universe directory
    match std::fs::create_dir(&universe_path) {
        Ok(()) => {
            // Create universe-level directories and copy engine default parts
            let _ = std::fs::create_dir_all(universe_path.join(".eustress").join("assets").join("parts"));
            let _ = std::fs::create_dir_all(universe_path.join(".eustress").join("assets").join("meshes"));
            let _ = std::fs::create_dir_all(universe_path.join(".eustress").join("knowledge"));
            copy_engine_default_parts(&universe_path.join(".eustress").join("assets").join("parts"));

            // Create Spaces directory (will contain spaces)
            let _ = std::fs::create_dir(&universe_path.join("Spaces"));

            info!("✓ Universe '{}' created at: {}", sanitized_name, universe_path.display());
            Ok(universe_path)
        }
        Err(e) => {
            Err(format!("Failed to create universe directory: {}", e))
        }
    }
}

/// Create a Space in an existing Universe and open it.
/// This is called after create_universe_folder() has created the universe.
pub fn create_space_in_universe(world: &mut World, universe_path: &Path, space_name: &str) -> Result<PathBuf, String> {
    let sanitized_name = sanitize_filename(space_name);

    if sanitized_name.is_empty() {
        return Err("Space name cannot be empty".to_string());
    }

    let spaces_dir = universe_path.join("Spaces");
    let space_path = spaces_dir.join(&sanitized_name);

    // Check if already exists
    if space_path.exists() {
        return Err(format!("Space '{}' already exists in this Universe", sanitized_name));
    }

    let author = world.get_resource::<crate::auth::AuthState>()
        .and_then(|a| a.user.as_ref())
        .map(|u| u.username.clone())
        .unwrap_or_else(|| "Eustress User".to_string());

    match scaffold_new_space(&spaces_dir, &sanitized_name, &author) {
        Ok(result) => {
            info!("✓ Space '{}' created at: {}", sanitized_name, result.space_root.display());
            // Open the newly created space
            open_space(world, &result.space_root);
            Ok(result.space_root)
        }
        Err(e) => {
            Err(format!("Failed to scaffold space: {}", e))
        }
    }
}

/// Scaffold a Space directory with all standard services and space.toml.
fn scaffold_space(space_root: &Path) {
    let services = [
        ("Workspace", "workspace", "Workspace service — contains all 3D entities"),
        ("Lighting", "lighting", "Lighting service — environment and lights"),
        ("StarterGui", "startergui", "StarterGui service — screen UI elements"),
        ("SoulService", "soulservice", "SoulService — scripts and logic"),
        ("StarterPack", "starterpack", "StarterPack — default player inventory"),
        ("StarterPlayer", "starterplayer", "StarterPlayer — player configuration"),
        ("ReplicatedStorage", "replicatedstorage", "ReplicatedStorage — shared assets"),
        ("ServerStorage", "serverstorage", "ServerStorage — server-only data"),
        ("ServerScriptService", "serverscriptservice", "ServerScriptService — server scripts"),
        ("MaterialService", "materialservice", "MaterialService — custom materials"),
        ("SoundService", "soundservice", "SoundService — audio management"),
    ];

    for (name, id, description) in &services {
        let service_dir = space_root.join(name);
        if std::fs::create_dir_all(&service_dir).is_err() { continue; }

        let toml = format!(
            "[service]\nclass_name = \"{name}\"\nid = \"{id}-service\"\n\n[metadata]\ndescription = \"{description}\"\ncreated = \"{now}\"\n",
            name = name,
            id = id,
            description = description,
            now = chrono::Utc::now().to_rfc3339(),
        );
        let _ = std::fs::write(service_dir.join("_service.toml"), toml);
    }

    let space_name = space_root.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Space1");

    let space_toml = format!(
        "[space]\nname = \"{}\"\nversion = \"0.1.0\"\ncreated = \"{}\"\n",
        space_name,
        chrono::Utc::now().to_rfc3339(),
    );
    let _ = std::fs::write(space_root.join("space.toml"), space_toml);

    info!("📁 Scaffolded Space at {:?} with {} services", space_root, services.len());
}

pub fn new_space(world: &mut World) {
    let current_space_root = world.get_resource::<crate::space::SpaceRoot>().map(|root| root.0.clone());
    let universe_root = resolve_active_universe_root(current_space_root.as_deref());

    // The file dialog opens at the Universe root so the user can type a
    // Space name. The dialog returns e.g. `Universe1/MySpace`, but the
    // actual Space must live under `Universe1/Spaces/MySpace`.
    let Some(requested_space_root) = pick_new_space_root(&universe_root) else {
        info!("🆕 New Space cancelled by user");
        return;
    };

    let space_name = space_name_from_path(&requested_space_root);

    // Validate the picked path is inside the active Universe
    let picked_parent = requested_space_root.parent().map(Path::to_path_buf);
    let is_inside_universe = picked_parent.as_ref().map(|p| {
        // Accept: Universe root directly, or Universe/Spaces/ subdirectory
        p == &universe_root || *p == universe_root.join("Spaces")
    }).unwrap_or(false);

    if !is_inside_universe {
        if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
            notifs.error(format!(
                "New Spaces must be created inside a Universe folder under {}.",
                crate::space::workspace_root().display()
            ));
        }
        return;
    }

    // Always scaffold inside the Spaces/ subdirectory of the Universe
    let spaces_dir = universe_root.join("Spaces");
    let _ = std::fs::create_dir_all(&spaces_dir);

    let author = {
        world.get_resource::<crate::auth::AuthState>()
            .and_then(|a| a.user.as_ref())
            .map(|u| u.username.clone())
            .unwrap_or_else(|| "Eustress User".to_string())
    };

    match scaffold_new_space(&spaces_dir, &space_name, &author) {
        Ok(result) => {
            // Force the Universes panel to rescan immediately so the new
            // Space appears without waiting for the 5-second timer or
            // the file watcher debounce.
            if let Some(mut registry) = world.get_resource_mut::<crate::space::UniverseRegistry>() {
                registry.rescan_requested = true;
            }
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

/// Verify Space has all required service folders, .eustress dirs, and knowledge.
/// Creates any that are missing — non-destructive (never deletes or overwrites).
pub fn ensure_space_integrity(space_root: &Path) {
    // A fully-converted `.eustress` world is DB-authoritative: the
    // service trees, lighting children, materials, `space.toml` and
    // `simulation.toml` all live inside `world.fjalldb/`. Regenerating
    // them on disk here would resurrect exactly the loose files the
    // conversion removed (and they'd come back every load). When the
    // header marks the world migrated, the DB owns integrity — do
    // nothing on disk.
    if space_is_migrated(space_root) {
        debug!(
            "ensure_space_integrity: skipped for migrated .eustress {:?} (DB authoritative, no disk regen)",
            space_root
        );
        return;
    }

    let mut repaired = 0;

    // Ensure .eustress subdirectories
    for subdir in &["local", "knowledge"] {
        let path = space_root.join(".eustress").join(subdir);
        if !path.exists() {
            let _ = std::fs::create_dir_all(&path);
            repaired += 1;
        }
    }

    // Ensure Universe-level knowledge dir
    if let Some(universe_root) = space_root.parent().and_then(|p| p.parent()) {
        let knowledge = universe_root.join(".eustress").join("knowledge");
        if !knowledge.exists() {
            let _ = std::fs::create_dir_all(&knowledge);
            repaired += 1;
        }
    }

    // Ensure all service folders exist with _service.toml
    let svc_template_dir = eustress_common::service_templates_dir();

    for svc in SERVICE_FOLDERS {
        let svc_dir = space_root.join(svc.name);
        if !svc_dir.exists() {
            let _ = std::fs::create_dir_all(&svc_dir);

            // Try template first, fallback to minimal TOML
            let template_path = svc_template_dir.join(svc.name).join("_service.toml");
            if let Ok(content) = std::fs::read_to_string(&template_path) {
                let _ = std::fs::write(svc_dir.join("_service.toml"), &content);
            } else {
                let toml = service_toml(svc.name, svc.class, svc.icon, svc.description);
                let _ = std::fs::write(svc_dir.join("_service.toml"), &toml);
            }
            repaired += 1;
        } else if !svc_dir.join("_service.toml").exists() {
            // Dir exists but missing _service.toml
            let template_path = svc_template_dir.join(svc.name).join("_service.toml");
            if let Ok(content) = std::fs::read_to_string(&template_path) {
                let _ = std::fs::write(svc_dir.join("_service.toml"), &content);
            } else {
                let toml = service_toml(svc.name, svc.class, svc.icon, svc.description);
                let _ = std::fs::write(svc_dir.join("_service.toml"), &toml);
            }
            repaired += 1;
        }
    }

    // Ensure Lighting children (Sun, Moon, Sky, Atmosphere)
    let lighting_dir = space_root.join("Lighting");
    if lighting_dir.exists() {
        // Remove stale Skybox.instance.toml — it used class_name="Sky" which
        // created a duplicate Sky entity. Sky.instance.toml handles everything.
        let stale_skybox = lighting_dir.join("Skybox.instance.toml");
        if stale_skybox.exists() {
            let _ = std::fs::remove_file(&stale_skybox);
            repaired += 1;
        }

        let lighting_template_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("lighting_templates");

        for child in &["Atmosphere", "Moon", "Sky", "Sun"] {
            let child_file = lighting_dir.join(format!("{}.instance.toml", child));
            if !child_file.exists() {
                let template = lighting_template_dir.join(format!("{}.instance.toml", child));
                if let Ok(content) = std::fs::read_to_string(&template) {
                    let _ = std::fs::write(&child_file, &content);
                    repaired += 1;
                }
            }
        }
    }

    // Ensure MaterialService has default material .mat.toml files
    let mat_dir = space_root.join("MaterialService");
    if mat_dir.exists() {
        let mat_template_dir = svc_template_dir.join("MaterialService");
        if mat_template_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&mat_template_dir) {
                for entry in entries.flatten() {
                    let fname = entry.file_name();
                    let fname_str = fname.to_string_lossy();
                    if fname_str.ends_with(".mat.toml") {
                        let dest = mat_dir.join(&fname);
                        if !dest.exists() {
                            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                                let _ = std::fs::write(&dest, &content);
                                repaired += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    // Ensure simulation.toml exists
    let sim_path = space_root.join("simulation.toml");
    if !sim_path.exists() {
        let _ = std::fs::write(&sim_path, simulation_toml());
        repaired += 1;
    }

    // Ensure space.toml exists and name matches folder
    let space_toml_path = space_root.join("space.toml");
    let folder_name = space_root.file_name().and_then(|n| n.to_str()).unwrap_or("Space");
    if !space_toml_path.exists() {
        let _ = std::fs::write(&space_toml_path, space_meta_toml(folder_name, "Eustress User"));
        repaired += 1;
    } else {
        // Sync name in space.toml to match folder name
        if let Ok(content) = std::fs::read_to_string(&space_toml_path) {
            if let Ok(mut doc) = content.parse::<toml::Value>() {
                let needs_update = doc.get("space")
                    .and_then(|s| s.get("name"))
                    .and_then(|n| n.as_str())
                    .map(|n| n != folder_name)
                    .unwrap_or(false);
                if needs_update {
                    if let Some(space) = doc.get_mut("space").and_then(|s| s.as_table_mut()) {
                        space.insert("name".to_string(), toml::Value::String(folder_name.to_string()));
                        if let Ok(new_content) = toml::to_string_pretty(&doc) {
                            let _ = std::fs::write(&space_toml_path, new_content);
                            info!("📝 Updated space.toml name to '{}'", folder_name);
                            repaired += 1;
                        }
                    }
                }
            }
        }
    }

    if repaired > 0 {
        info!("🔧 Space integrity check: repaired {} missing items", repaired);
    }
}

/// If a space was renamed (folder name changed), migrate the recordings directory
/// in the Universe's knowledge/recordings/ to match the new name.
///
/// Uses a `.last_name` file in the space's .eustress/ dir to track the previous name.
fn migrate_recordings_on_rename(space_root: &Path) {
    let current_name = space_root.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Space");

    let last_name_file = space_root.join(".eustress").join(".last_name");

    // Read previous name
    let previous_name = std::fs::read_to_string(&last_name_file).ok();

    // Write current name for next time
    let _ = std::fs::create_dir_all(space_root.join(".eustress"));
    let _ = std::fs::write(&last_name_file, current_name);

    // If name changed, rename recordings dir
    if let Some(prev) = previous_name {
        let prev = prev.trim().to_string();
        if !prev.is_empty() && prev != current_name {
            if let Some(universe_root) = space_root.parent().and_then(|p| p.parent()) {
                let recordings_base = universe_root.join(".eustress").join("knowledge").join("recordings");
                let old_dir = recordings_base.join(&prev);
                let new_dir = recordings_base.join(current_name);

                if old_dir.exists() && !new_dir.exists() {
                    match std::fs::rename(&old_dir, &new_dir) {
                        Ok(_) => info!("📁 Migrated recordings: '{}' → '{}'", prev, current_name),
                        Err(e) => warn!("⚠ Failed to migrate recordings dir: {}", e),
                    }
                }
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

fn service_toml(_name: &str, class: &str, icon: &str, description: &str) -> String {
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
