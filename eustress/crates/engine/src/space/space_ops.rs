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
    ServiceFolder { name: "Workspace",               class: "Workspace",              icon: "workspace",          description: "3D world objects - Parts, Models, Terrain" },
    ServiceFolder { name: "Lighting",                class: "Lighting",               icon: "lighting",           description: "Light sources - Sun, Sky, Atmosphere" },
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
    ServiceFolder { name: "SoundService",            class: "SoundService",           icon: "soundservice",       description: "Audio - Sound effects and music" },
    ServiceFolder { name: "PhysicsService",          class: "PhysicsService",         icon: "physicsservice",     description: "Solver settings and which physics domains step" },
    ServiceFolder { name: "AdornmentService",        class: "AdornmentService",       icon: "adornmentservice",   description: "Beams, billboards, particles, highlights" },
    ServiceFolder { name: "DataService",             class: "DataService",            icon: "dataservice",        description: "Data Platform - datasets, series, columns, and runs" },
    ServiceFolder { name: "Website",                 class: "Website",                 icon: "website",            description: "Values a website reads - References baked into a published manifest" },
    ServiceFolder { name: "ExperimentService",       class: "ExperimentService",      icon: "experimentservice", description: "Designs, their parts, the laws wiring them, and every run over them" },
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
    let lighting_template_dir = crate::resource_root()
        .join("assets")
        .join("lighting_templates");

    // Clouds is a visible, editable fair-weather layer: with no Clouds object
    // a Space has no clouds (Roblox's rule), so a new Space gets one. The
    // repair below leaves it out, so a deleted Clouds stays deleted.
    let lighting_children = ["Atmosphere", "Clouds", "Moon", "Sky", "Sun"];
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
    // The primitive part meshes ship in common's assets, which both apps ship.
    let engine_parts_dir = eustress_common::assets_dir().join("parts");

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
pub fn save_space(world: &mut World) -> SaveReport {
    let space_root = match world.get_resource::<crate::space::SpaceRoot>() {
        Some(sr) => sr.0.clone(),
        None => {
            warn!("Cannot save: no SpaceRoot resource set");
            return SaveReport::default();
        }
    };

    ensure_manifest_set(&space_root, None, None);

    let workspace_dir = space_root.join("Workspace");
    let _ = std::fs::create_dir_all(&workspace_dir);

    // Every part and tag edit not yet on disk: the edit writer's queue, in
    // order, then whatever is still marked changed (`persist_pending_edits`).
    let pending = persist_pending_edits(world);
    let mut saved = pending.written;
    let unchanged = pending.unchanged;
    let mut errors = pending.errors;
    let mut paths = pending.paths;

    // Services changed since the last save. Without the tracker every one.
    let dirty_services: Option<std::collections::HashSet<Entity>> = world
        .get_resource_mut::<SaveDirty>()
        .map(|mut d| std::mem::take(&mut d.services));
    let stamp = world.get_resource::<crate::auth::AuthState>()
        .and_then(crate::space::instance_loader::current_stamp);

    {
        // Services too: only those changed since the last save.
        let mut svc_query = world.query::<(Entity, &ServiceComponent)>();
        let services: Vec<(Entity, ServiceComponent)> = svc_query
            .iter(world)
            .filter(|(e, _)| dirty_services.as_ref().is_none_or(|d| d.contains(e)))
            .map(|(e, svc)| (e, svc.clone()))
            .collect();
        let mut retry_services: Vec<Entity> = Vec::new();
        for (entity, svc) in &services {
            if svc.toml_path != PathBuf::new() {
                if let Err(e) = crate::space::service_loader::save_service_to_file_signed(svc, stamp.as_ref()) {
                    error!("❌ Failed to save service {}: {}", svc.class_name, e);
                    errors += 1;
                    retry_services.push(*entity);
                } else {
                    saved += 1;
                    paths.push(svc.toml_path.clone());
                }
            }
        }
        if !retry_services.is_empty() {
            if let Some(mut d) = world.get_resource_mut::<SaveDirty>() {
                d.services.extend(retry_services);
            }
        }
    }

    if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
        if errors == 0 {
            notifs.success(format!("Space saved: {} files written", saved));
        } else {
            notifs.warning(format!(
                "Space saved with {} errors ({} files written)",
                errors, saved
            ));
        }
    }

    info!(
        "💾 Space save complete: {} written, {} already up to date, {} errors",
        saved, unchanged, errors
    );
    SaveReport { written: saved, unchanged, errors, paths }
}

/// What a `save_space` did: files written (parts, tags and attributes, and
/// services), parts whose file already matched, failures (left due for the
/// next save), and the paths written. A database checkpoint pushes those
/// paths into its tree before dumping, ahead of the watcher.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveReport {
    pub written: usize,
    pub unchanged: usize,
    pub errors: usize,
    pub paths: Vec<PathBuf>,
}

/// Parts changed since the last save: the ones `save_space` reads and
/// writes. Filled every frame by `track_save_dirty`, emptied by each save.
/// A part edited in the same frame as a save joins the set after it, so it
/// goes out with the next save.
#[derive(Resource, Default)]
pub struct SaveDirty {
    entities: std::collections::HashSet<Entity>,
    services: std::collections::HashSet<Entity>,
    /// File-backed entities whose tags or attributes changed.
    attrs: std::collections::HashSet<Entity>,
    /// `PartSave::fingerprint` of what was last handed to the writer, per
    /// part: a part marked changed with the same values is not written again.
    fingerprints: std::collections::HashMap<Entity, u64>,
}

/// Record every part whose pose, part properties or instance changed, and
/// every service whose properties changed; `save_part` then writes only the
/// parts that differ from their files.
///
/// A Space opening is not an edit. Every part and service it spawns matches
/// its file, and the class-default backfill after the spawn marks them all
/// again, so both are left out: counting them made the first save after an
/// open read every part's file and rewrite every `_service.toml`, and with
/// autosave calling `save_space` that cost landed on a timer. A part spawned
/// with no file is always kept, since only a save writes it.
pub fn track_save_dirty(
    mut dirty: ResMut<SaveDirty>,
    load_in_progress: Option<Res<crate::space::file_loader::LoadInProgress>>,
    changed_services: Query<(Entity, Ref<ServiceComponent>), Changed<ServiceComponent>>,
    changed_attrs: Query<
        (
            Entity,
            Option<Ref<eustress_common::attributes::Tags>>,
            Option<Ref<eustress_common::attributes::Attributes>>,
        ),
        (
            With<crate::space::instance_loader::InstanceFile>,
            Or<(
                Changed<eustress_common::attributes::Tags>,
                Changed<eustress_common::attributes::Attributes>,
            )>,
        ),
    >,
    changed: Query<
        (
            Entity,
            Ref<Transform>,
            bevy::ecs::query::Has<crate::space::instance_loader::InstanceFile>,
        ),
        (
            With<eustress_common::classes::BasePart>,
            // Parts streamed from the binary cores persist through the binary
            // mirror, and residency spawns and despawns them all the time. A
            // selected one loses the marker; `save_space` skips it by path.
            Without<eustress_common::classes::ColdStreamed>,
            Or<(
                Changed<Transform>,
                Changed<eustress_common::classes::BasePart>,
                Changed<eustress_common::classes::Instance>,
            )>,
        ),
    >,
) {
    let loading = load_in_progress.is_some_and(|l| l.active);
    dirty.entities.extend(
        changed
            .iter()
            .filter(|(_, transform, has_file)| !*has_file || !(loading || transform.is_added()))
            .map(|(entity, ..)| entity),
    );
    dirty.services.extend(
        changed_services
            .iter()
            .filter(|(_, service)| !(loading || service.is_added()))
            .map(|(entity, _)| entity),
    );
    // Tags and attributes arrive with the spawn too; only later edits count.
    dirty.attrs.extend(
        changed_attrs
            .iter()
            .filter(|(_, tags, attrs)| {
                !(loading
                    || tags.as_ref().is_some_and(|t| t.is_added())
                    || attrs.as_ref().is_some_and(|a| a.is_added()))
            })
            .map(|(entity, ..)| entity),
    );
}

/// A part's live values, gathered on the main thread for `save_part`.
struct PartSave {
    entity: Entity,
    name: String,
    toml_path: PathBuf,
    has_file: bool,
    translation: Vec3,
    rotation: Quat,
    size: Vec3,
    color: [f32; 4],
    material: String,
    transparency: f32,
    anchored: bool,
    can_collide: bool,
    cast_shadow: bool,
    reflectance: f32,
    locked: bool,
    destructible: bool,
    archivable: bool,
    class_name: String,
    mesh: &'static str,
    name_override: Option<String>,
    /// The name the loader gives the file when it names none
    /// (`loader_fallback_name`).
    stem: Option<String>,
}

/// Whether two definitions agree on every field `save_space` writes: the
/// test for "this part has not changed since its file was written".
fn saved_fields_match(a: &InstanceDefinition, b: &InstanceDefinition) -> bool {
    fn close(x: &[f32], y: &[f32], tolerance: f32) -> bool {
        x.len() == y.len()
            && x.iter().zip(y).all(|(p, q)| (p - q).abs() <= tolerance * p.abs().max(q.abs()).max(1.0))
    }
    // Colours are stored as 0 to 255 integers: equal within half a step.
    let colour_step = 0.5 / 255.0;
    close(&a.transform.position, &b.transform.position, 1e-5)
        && same_rotation(a.transform.rotation, b.transform.rotation)
        && close(&a.transform.scale, &b.transform.scale, 1e-5)
        && close(&a.properties.color, &b.properties.color, colour_step)
        && close(
            &[a.properties.transparency, a.properties.reflectance],
            &[b.properties.transparency, b.properties.reflectance],
            1e-5,
        )
        && a.properties.anchored == b.properties.anchored
        && a.properties.can_collide == b.properties.can_collide
        && a.properties.cast_shadow == b.properties.cast_shadow
        && a.properties.locked == b.properties.locked
        && a.properties.material == b.properties.material
        && a.metadata.name == b.metadata.name
}

/// Largest turn, in radians, that still counts as the same rotation: about
/// 0.06 degrees. Well above what a file's short quaternion loses (four
/// digits: about 1e-4 rad) and well below any turn a person makes.
const SAME_ROTATION_RADIANS: f64 = 1e-3;

/// Whether two stored rotations turn a part the same way. They are compared
/// as rotations, not number by number: the loader normalises every rotation
/// it reads (`sanitize_rot`), and a file written with a short quaternion
/// (`[0.0, -0.1693, 0.0, 0.9856]`) is not unit length, so the live rotation
/// never equals the file's numbers even when nothing moved. Comparing the
/// numbers made every rotated part look edited once anything touched it
/// after a load, and the edit writer rewrote and re-stamped them all. `q`
/// and `-q` are the same rotation. A zero or non-finite quaternion matches
/// only itself.
fn same_rotation(a: [f32; 4], b: [f32; 4]) -> bool {
    fn unit(q: [f32; 4]) -> Option<[f64; 4]> {
        let v = q.map(f64::from);
        let length = v.iter().map(|c| c * c).sum::<f64>().sqrt();
        (length.is_finite() && length > 1e-9).then(|| v.map(|c| c / length))
    }
    match (unit(a), unit(b)) {
        (Some(x), Some(y)) => {
            let dot = x.iter().zip(y.iter()).map(|(p, q)| p * q).sum::<f64>().abs().min(1.0);
            // The angle between the two rotations is 2 * acos(|dot|).
            2.0 * dot.acos() <= SAME_ROTATION_RADIANS
        }
        _ => a == b,
    }
}

/// A part's live values, ready for `save_part`, or `None` for a part the
/// save leaves alone (a baked part, a sky or light singleton).
fn part_save_for(
    entity: Entity,
    instance: &eustress_common::classes::Instance,
    base_part: &eustress_common::classes::BasePart,
    local_tf: &Transform,
    instance_file: Option<&crate::space::instance_loader::InstanceFile>,
    part: Option<&eustress_common::classes::Part>,
    workspace_dir: &Path,
) -> Option<PartSave> {
    use eustress_common::classes::ClassName;
    // A part streamed from the database's binary cores carries a
    // synthetic `__bin_` path with no file behind it, and its edits
    // persist through the binary mirror. Written here, it would become
    // a real `__bin_` folder on disk that loads as a second copy.
    if instance_file.is_some_and(|f| is_synthetic_core_path(&f.toml_path)) {
        return None;
    }
    match instance.class_name {
        ClassName::Sky | ClassName::Atmosphere | ClassName::Camera
        | ClassName::Star | ClassName::Moon | ClassName::Clouds => return None,
        _ => {}
    }

    let toml_path = if let Some(inst_file) = instance_file {
        inst_file.toml_path.clone()
    } else {
        // New entity without InstanceFile: its own folder.
        workspace_dir.join(sanitize_filename(&instance.name)).join("_instance.toml")
    };

    // Preserve the display-name override when the folder name
    // and instance name don't match: a second sibling "Block"
    // lives in `Block-a3f2/` with `name = "Block"` in the TOML so
    // the Explorer still renders it as "Block".
    let stem = loader_fallback_name(&toml_path);
    let name_override = match &stem {
        Some(stem) if *stem != instance.name => Some(instance.name.clone()),
        _ => None,
    };

    let color = {
        let c = base_part.color.to_srgba();
        [c.red, c.green, c.blue, c.alpha]
    };
    // Prefer the live material NAME (preserves custom MaterialService
    // names + Material-Flip edits); fall back to the enum.
    let material = if base_part.material_name.is_empty() {
        format!("{:?}", base_part.material)
    } else {
        base_part.material_name.clone()
    };
    // A new part is a primitive, so its mesh comes from `Part.shape`.
    let mesh = part
        .map(|p| match p.shape {
            eustress_common::classes::PartType::Block => "parts/block.glb",
            eustress_common::classes::PartType::Ball => "parts/ball.glb",
            eustress_common::classes::PartType::Cylinder => "parts/cylinder.glb",
            eustress_common::classes::PartType::Wedge => "parts/wedge.glb",
            eustress_common::classes::PartType::CornerWedge => "parts/corner_wedge.glb",
            eustress_common::classes::PartType::Cone => "parts/cone.glb",
        })
        .unwrap_or("parts/block.glb");
    let class_name = format!("{:?}", instance.class_name)
        .trim_start_matches("ClassName::")
        .to_string();

    Some(PartSave {
        entity,
        name: instance.name.clone(),
        toml_path,
        has_file: instance_file.is_some(),
        translation: local_tf.translation,
        rotation: local_tf.rotation,
        // TOML scale = BasePart.size (correct in both scale-tool
        // branches; Transform.scale alone pinned legacy parts at 1x1x1).
        size: base_part.size,
        color,
        material,
        transparency: base_part.transparency,
        anchored: base_part.anchored,
        can_collide: base_part.can_collide,
        cast_shadow: base_part.cast_shadow,
        reflectance: base_part.reflectance,
        locked: base_part.locked,
        destructible: base_part.destructible,
        archivable: instance.archivable,
        class_name,
        mesh,
        name_override,
        stem,
    })
}

/// Save one part: the file's definition with the live values merged in,
/// written only when they differ from what the file holds. `Ok(true)` when
/// written, `Ok(false)` when the file already matched. Runs on a worker
/// thread.
fn save_part(
    p: &PartSave,
    stamp: Option<&crate::space::instance_loader::CreatorStamp>,
    now: &str,
) -> Result<bool, String> {
    // LOAD-MERGE: start from the EXISTING on-disk definition and overwrite
    // only the component-authoritative fields. Rebuilding the whole
    // `InstanceDefinition` from components alone dropped every field the ECS
    // does not carry: a custom `asset.mesh` (V-Cell came back as blocks), the
    // realism `[material]` / `[thermodynamic]` / `[electrochemical]` sections,
    // the metadata audit chain, `attributes`, `tags`, `ui`, `[extra]`. Read
    // DISK directly (NOT the active_db funnel) so the merge base is the
    // on-disk TOML, never a stale binary `#bin` cache, and heal it in memory
    // as the loader does. The disk heal (`load_instance_definition_with_extras`)
    // wrote the file back whenever its canonical form differed, so a save
    // with no edits rewrote every file before comparing anything.
    // A part that had a file whose file is gone was trashed, moved or
    // renamed after its values were gathered. Writing now would bring the
    // old file back, so the write is dropped.
    if p.has_file && !p.toml_path.is_file() {
        return Ok(false);
    }
    let existing = if p.has_file {
        std::fs::read_to_string(&p.toml_path)
            .ok()
            .and_then(|text| crate::space::instance_loader::load_instance_definition_from_str(&text).ok())
    } else {
        None
    };

    let mut def = if let Some(mut d) = existing {
        let before = d.clone();
        // SCALE GUARD: for a CUSTOM-mesh part (mesh not under "parts/") the
        // TOML `scale` is the user's MULTIPLIER, while `BasePart.size` is the
        // mesh-AABB-derived world size; writing size into scale stretches the
        // mesh on reload. Custom meshes keep the on-disk scale. Mirrors the
        // guard in `write_instance_changes_system` (instance_loader.rs).
        let is_custom_mesh = d.asset.as_ref()
            .map(|a| crate::space::representation::mesh_requires_filesystem(&a.mesh))
            .unwrap_or(false);
        // The live pose and, for a primitive, its size, in the file's own
        // unit (`set_authored_transform`): a Space imported in feet stays in
        // feet.
        crate::space::instance_loader::set_authored_transform(
            &mut d,
            p.translation,
            p.rotation,
            (!is_custom_mesh).then_some(p.size),
        );
        d.properties.color = p.color;
        d.properties.transparency = p.transparency;
        d.properties.anchored = p.anchored;
        d.properties.can_collide = p.can_collide;
        d.properties.cast_shadow = p.cast_shadow;
        d.properties.reflectance = p.reflectance;
        d.properties.locked = p.locked;
        d.properties.material = p.material.clone();
        // A file that still names the part keeps its own `name` entry; only a
        // name that differs from the one the file gives is written.
        if d.metadata.name.as_deref().or(p.stem.as_deref()) != Some(p.name.as_str()) {
            d.metadata.name = p.name_override.clone();
        }
        if saved_fields_match(&before, &d) {
            return Ok(false);
        }
        d.metadata.last_modified = now.to_string();
        d
    } else {
        // New entity (no on-disk TOML) or unreadable file: build from
        // components.
        InstanceDefinition {
            plasma: None,
            asset: Some(AssetReference {
                mesh: p.mesh.to_string(),
                scene: "Scene0".to_string(),
            }),
            transform: TransformData {
                position: p.translation.to_array(),
                rotation: p.rotation.to_array(),
                scale: p.size.to_array(),
            },
            properties: InstanceProperties {
                color: p.color,
                material: p.material.clone(),
                transparency: p.transparency,
                anchored: p.anchored,
                can_collide: p.can_collide,
                cast_shadow: p.cast_shadow,
                reflectance: p.reflectance,
                locked: p.locked,
                physics: None,
                respect_gltf_materials: false,
                // Persist the destructible opt-in so a part authored as
                // destructible stays destructible across a save/load.
                destructible: p.destructible,
            },
            metadata: InstanceMetadata {
                class_name: p.class_name.clone(),
                archivable: p.archivable,
                name: p.name_override.clone(),
                created: String::new(),
                last_modified: now.to_string(),
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

    // Only a new part gets a folder made for it.
    if !p.has_file {
        if let Some(parent) = p.toml_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    crate::space::instance_loader::write_instance_definition_signed(&p.toml_path, &mut def, stamp)?;
    Ok(true)
}

// ============================================================================
// Persisting edits as they happen
// ============================================================================
//
// Every edit reaches disk shortly after it settles, not only at the next save:
// `track_save_dirty` records what changed, `persist_edits` gathers it on the
// main thread about three times a second, and one background thread writes it
// in order (`save_part`, `save_attributes`). A save, a Space switch and exit
// first finish that queue (`persist_pending_edits`), so a queued job can never
// overwrite something newer.

/// How often, at most, `persist_edits` hands a batch to the writer, in
/// seconds. A held nudge key or a rolled wheel writes at this rate.
const PERSIST_INTERVAL_SECS: f64 = 0.3;

/// An entity's tags and attributes as a file patch, gathered on the main thread.
struct AttrSave {
    entity: Entity,
    toml_path: PathBuf,
    tags: Option<Vec<String>>,
    attrs: Option<std::collections::HashMap<String, toml::Value>>,
}

/// One batch of edits: values read from the World on the main thread,
/// written later by the writer thread or by a save that takes the queue over.
struct PersistJob {
    /// The Space the edits belong to: a snapshot revert of it drops them.
    space_root: PathBuf,
    parts: Vec<PartSave>,
    attrs: Vec<AttrSave>,
    stamp: Option<crate::space::instance_loader::CreatorStamp>,
    now: String,
}

/// True while a snapshot revert of `space_root` is being applied: its files
/// are about to be, or have just been, restored, and the World that made the
/// queued edits is being replaced. Edits of that Space are dropped, never
/// written: the revert's safety snapshot already holds them, and writing them
/// once the revert finishes would put the reverted state back.
fn restore_pending(space_root: &Path) -> bool {
    crate::space::checkpoint::restore_pending(space_root)
}

/// Drop every queued job of a Space being reverted; `true` if any went.
fn drop_reverted_jobs(state: &mut PersistState) -> bool {
    let before = state.jobs.len();
    state.jobs.retain(|job| !restore_pending(&job.space_root));
    let dropped = before - state.jobs.len();
    if dropped > 0 {
        info!("Snapshot revert in progress: dropped {} queued edit batches of the reverted Space", dropped);
    }
    dropped > 0
}

/// What writing one or more jobs did.
#[derive(Debug, Default)]
pub struct PersistOutcome {
    pub written: usize,
    pub unchanged: usize,
    pub errors: usize,
    /// Every file written.
    pub paths: Vec<PathBuf>,
    failed_parts: Vec<Entity>,
    failed_attrs: Vec<Entity>,
}

impl PersistOutcome {
    fn absorb(&mut self, other: PersistOutcome) {
        self.written += other.written;
        self.unchanged += other.unchanged;
        self.errors += other.errors;
        self.paths.extend(other.paths);
        self.failed_parts.extend(other.failed_parts);
        self.failed_attrs.extend(other.failed_attrs);
    }
}

/// The writer's queue. `in_flight` is true only while the writer holds the
/// git commit lock and writes a job; `failed_*` carry writes it could not
/// make back to the main thread, which marks them due again.
struct PersistState {
    jobs: std::collections::VecDeque<PersistJob>,
    in_flight: bool,
    worker_started: bool,
    failed_parts: Vec<Entity>,
    failed_attrs: Vec<Entity>,
}

struct PersistQueue {
    state: std::sync::Mutex<PersistState>,
    changed: std::sync::Condvar,
}

static PERSIST: PersistQueue = PersistQueue {
    state: std::sync::Mutex::new(PersistState {
        jobs: std::collections::VecDeque::new(),
        in_flight: false,
        worker_started: false,
        failed_parts: Vec::new(),
        failed_attrs: Vec::new(),
    }),
    changed: std::sync::Condvar::new(),
};

fn persist_state() -> std::sync::MutexGuard<'static, PersistState> {
    PERSIST.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Queue a job for the writer thread, starting the thread on first use. If
/// it cannot start, the job waits in the queue for the next save.
fn enqueue_persist_job(job: PersistJob) {
    let mut state = persist_state();
    state.jobs.push_back(job);
    if !state.worker_started {
        match std::thread::Builder::new().name("eustress-persist".into()).spawn(persist_worker) {
            Ok(_) => state.worker_started = true,
            Err(e) => warn!("Edit writer did not start ({e}); edits are written at the next save"),
        }
    }
    drop(state);
    PERSIST.changed.notify_all();
}

/// The writer thread: one job at a time, in queue order, each under the git
/// commit lock so no commit's `git add` stages half a batch. It never waits
/// on that lock: while someone else holds it, the job stays queued, so a save
/// holding the lock can take the queue over (`flush_persist_queue`) instead
/// of waiting on a writer that waits on it.
fn persist_worker() {
    loop {
        {
            let mut state = persist_state();
            while state.jobs.is_empty() {
                state = PERSIST.changed.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            if drop_reverted_jobs(&mut state) && state.jobs.is_empty() {
                continue;
            }
        }
        let guard = match crate::editor_settings::GIT_COMMIT_LOCK.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(25));
                continue;
            }
        };
        let job = {
            let mut state = persist_state();
            let job = state.jobs.pop_front();
            state.in_flight = job.is_some();
            job
        };
        let Some(job) = job else {
            drop(guard);
            continue;
        };
        let outcome = run_persist_job(&job);
        drop(guard);
        {
            let mut state = persist_state();
            state.in_flight = false;
            state.failed_parts.extend(outcome.failed_parts);
            state.failed_attrs.extend(outcome.failed_attrs);
        }
        PERSIST.changed.notify_all();
    }
}

/// Finish the writer's queue on the calling thread: wait for the job being
/// written, then write every job still queued, in order. Safe to call while
/// holding the git commit lock (autosave does): a job is only in flight
/// while the writer holds that lock itself.
pub fn flush_persist_queue() -> PersistOutcome {
    let stolen: Vec<PersistJob> = {
        let mut state = persist_state();
        while state.in_flight {
            state = PERSIST.changed.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        drop_reverted_jobs(&mut state);
        state.jobs.drain(..).collect()
    };
    let mut total = PersistOutcome::default();
    for job in &stolen {
        total.absorb(run_persist_job(job));
    }
    total
}

/// Write one job: its parts in parallel (distinct files), then its tags and
/// attributes, so no file is written by two threads at once.
fn run_persist_job(job: &PersistJob) -> PersistOutcome {
    use rayon::prelude::*;
    let mut out = PersistOutcome::default();
    let parts: Vec<(Entity, &str, &Path, Result<bool, String>)> = job
        .parts
        .par_iter()
        .map(|p| (p.entity, p.name.as_str(), p.toml_path.as_path(), save_part(p, job.stamp.as_ref(), &job.now)))
        .collect();
    for (entity, name, path, result) in parts {
        match result {
            Ok(true) => {
                out.written += 1;
                out.paths.push(path.to_path_buf());
                debug!("💾 Saved '{}'", name);
            }
            Ok(false) => out.unchanged += 1,
            Err(e) => {
                out.errors += 1;
                error!("❌ Failed to save '{}': {}", name, e);
                out.failed_parts.push(entity);
            }
        }
    }
    let attrs: Vec<(Entity, &Path, Result<bool, String>)> = job
        .attrs
        .par_iter()
        .map(|a| (a.entity, a.toml_path.as_path(), save_attributes(a)))
        .collect();
    for (entity, path, result) in attrs {
        match result {
            Ok(true) => {
                out.written += 1;
                out.paths.push(path.to_path_buf());
            }
            Ok(false) => out.unchanged += 1,
            Err(e) => {
                out.errors += 1;
                error!("❌ Failed to save tags and attributes of {}: {}", path.display(), e);
                out.failed_attrs.push(entity);
            }
        }
    }
    out
}

/// Patch a file's tags and attributes. A file that is gone (trashed, moved,
/// renamed) is left gone: `Ok(false)`.
fn save_attributes(a: &AttrSave) -> Result<bool, String> {
    if !a.toml_path.is_file() {
        return Ok(false);
    }
    crate::space::instance_loader::patch_tags_attributes_toml(&a.toml_path, a.tags.clone(), a.attrs.clone())?;
    Ok(true)
}

/// A file-backed entity's tags and attributes, ready to write.
fn attr_save_for(
    entity: Entity,
    file: &crate::space::instance_loader::InstanceFile,
    tags: Option<&eustress_common::attributes::Tags>,
    attributes: Option<&eustress_common::attributes::Attributes>,
) -> Option<AttrSave> {
    if is_synthetic_core_path(&file.toml_path) {
        return None;
    }
    Some(AttrSave {
        entity,
        toml_path: file.toml_path.clone(),
        tags: tags.map(|t| t.0.clone()),
        attrs: attributes.map(|a| {
            a.values
                .iter()
                // A runtime reference (an Object) never persists.
                .filter_map(|(k, v)| eustress_common::datamodel::record::attribute_to_toml(v).map(|value| (k.clone(), value)))
                .collect()
        }),
    })
}

impl PartSave {
    /// A hash of every value `save_part` writes: two equal fingerprints mean
    /// nothing to write. Physics re-assigns unmoved transforms every frame,
    /// which marks parts changed; this is what filters those out.
    fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.toml_path.hash(&mut h);
        self.has_file.hash(&mut h);
        let floats = self
            .translation
            .to_array()
            .into_iter()
            .chain(self.rotation.to_array())
            .chain(self.size.to_array())
            .chain(self.color)
            .chain([self.transparency, self.reflectance]);
        for v in floats {
            v.to_bits().hash(&mut h);
        }
        (self.anchored, self.can_collide, self.cast_shadow, self.locked, self.destructible, self.archivable).hash(&mut h);
        self.material.hash(&mut h);
        self.name.hash(&mut h);
        self.class_name.hash(&mut h);
        self.mesh.hash(&mut h);
        self.name_override.hash(&mut h);
        h.finish()
    }
}

/// Hand what changed since the last batch to the writer thread: at most every
/// `PERSIST_INTERVAL_SECS`, never during a drag (the tool writes one step on
/// release), a load, or Play (the registration's run condition). Parts whose
/// values match what was last written are skipped before any disk work.
#[allow(clippy::type_complexity)]
pub fn persist_edits(
    mut dirty: ResMut<SaveDirty>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    load_in_progress: Option<Res<crate::space::file_loader::LoadInProgress>>,
    tools: (
        Option<Res<crate::select_tool::SelectToolState>>,
        Option<Res<crate::move_tool::MoveToolState>>,
        Option<Res<crate::rotate_tool::RotateToolState>>,
        Option<Res<crate::scale_tool::ScaleToolState>>,
    ),
    parts: Query<(
        Entity,
        &eustress_common::classes::Instance,
        &eustress_common::classes::BasePart,
        &Transform,
        Option<&crate::space::instance_loader::InstanceFile>,
        Option<&eustress_common::classes::Part>,
    )>,
    attributes: Query<(
        Entity,
        &crate::space::instance_loader::InstanceFile,
        Option<&eustress_common::attributes::Tags>,
        Option<&eustress_common::attributes::Attributes>,
    )>,
    auth: Option<Res<crate::auth::AuthState>>,
    mut recently_written: Option<ResMut<crate::space::file_watcher::RecentlyWrittenFiles>>,
    time: Res<Time<Real>>,
    mut last_batch: Local<f64>,
) {
    let d = &mut *dirty;
    // Writes the writer thread could not make come back due.
    {
        let mut state = persist_state();
        for entity in state.failed_parts.drain(..) {
            d.fingerprints.remove(&entity);
            d.entities.insert(entity);
        }
        d.attrs.extend(state.failed_attrs.drain(..));
    }
    if d.entities.is_empty() && d.attrs.is_empty() {
        return;
    }
    if load_in_progress.is_some_and(|l| l.active) {
        return;
    }
    let now = time.elapsed_secs_f64();
    if now - *last_batch < PERSIST_INTERVAL_SECS {
        return;
    }
    let (select, move_tool, rotate, scale) = tools;
    let dragging = select.is_some_and(|s| s.dragging)
        || move_tool.is_some_and(|m| m.dragged_axis.is_some() || m.dragged_plane.is_some() || m.free_drag)
        || rotate.is_some_and(|r| r.dragged_axis.is_some())
        || scale.is_some_and(|s| s.dragged_axis.is_some());
    if dragging {
        return;
    }
    let Some(root) = space_root else { return };
    if restore_pending(&root.0) {
        d.entities.clear();
        d.attrs.clear();
        return;
    }
    *last_batch = now;
    let workspace_dir = root.0.join("Workspace");

    let mut job_parts = Vec::new();
    for entity in d.entities.drain() {
        let Ok((entity, instance, base_part, transform, file, part)) = parts.get(entity) else { continue };
        let Some(save) = part_save_for(entity, instance, base_part, transform, file, part, &workspace_dir) else {
            continue;
        };
        let fingerprint = save.fingerprint();
        if d.fingerprints.get(&entity) == Some(&fingerprint) {
            continue;
        }
        d.fingerprints.insert(entity, fingerprint);
        job_parts.push(save);
    }
    let mut job_attrs = Vec::new();
    for entity in d.attrs.drain() {
        let Ok((entity, file, tags, attrs)) = attributes.get(entity) else { continue };
        if let Some(save) = attr_save_for(entity, file, tags, attrs) {
            job_attrs.push(save);
        }
    }
    if job_parts.is_empty() && job_attrs.is_empty() {
        return;
    }
    // Marked now, before the writer runs, so the watcher never reloads our own write.
    if let Some(ref mut written) = recently_written {
        for path in job_parts.iter().map(|p| &p.toml_path).chain(job_attrs.iter().map(|a| &a.toml_path)) {
            written.mark_written(path.clone());
        }
    }
    let stamp = auth.as_deref().and_then(crate::space::instance_loader::current_stamp);
    enqueue_persist_job(PersistJob {
        space_root: root.0.clone(),
        parts: job_parts,
        attrs: job_attrs,
        stamp,
        now: Utc::now().to_rfc3339(),
    });
}

/// Write every edit not yet on disk, on the calling thread: the writer's
/// queue first, in order, then whatever is still marked changed. A save, a
/// Space switch and exit run this, so nothing an edit changed is left behind
/// and no queued job lands after it. Without the tracker (a test, a tool)
/// every part is written.
pub fn persist_pending_edits(world: &mut World) -> PersistOutcome {
    let mut total = flush_persist_queue();
    let (failed_parts, failed_attrs) = {
        let mut state = persist_state();
        (std::mem::take(&mut state.failed_parts), std::mem::take(&mut state.failed_attrs))
    };
    let Some(space_root) = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone()) else {
        return total;
    };
    if restore_pending(&space_root) {
        if let Some(mut d) = world.get_resource_mut::<SaveDirty>() {
            d.entities.clear();
            d.attrs.clear();
        }
        return total;
    }
    let workspace_dir = space_root.join("Workspace");
    let (dirty, dirty_attrs) = match world.get_resource_mut::<SaveDirty>() {
        Some(mut d) => {
            d.entities.extend(failed_parts);
            d.attrs.extend(failed_attrs);
            (Some(std::mem::take(&mut d.entities)), Some(std::mem::take(&mut d.attrs)))
        }
        None => (None, None),
    };

    let mut parts: Vec<PartSave> = Vec::new();
    {
        let mut query = world.query::<(
            Entity,
            &eustress_common::classes::Instance,
            &eustress_common::classes::BasePart,
            &Transform,
            Option<&crate::space::instance_loader::InstanceFile>,
            Option<&eustress_common::classes::Part>,
        )>();
        for (entity, instance, base_part, transform, file, part) in query.iter(world) {
            if dirty.as_ref().is_some_and(|d| !d.contains(&entity)) {
                continue;
            }
            if let Some(save) = part_save_for(entity, instance, base_part, transform, file, part, &workspace_dir) {
                parts.push(save);
            }
        }
    }
    let mut attrs: Vec<AttrSave> = Vec::new();
    if let Some(dirty_attrs) = dirty_attrs.as_ref().filter(|d| !d.is_empty()) {
        let mut query = world.query::<(
            Entity,
            &crate::space::instance_loader::InstanceFile,
            Option<&eustress_common::attributes::Tags>,
            Option<&eustress_common::attributes::Attributes>,
        )>();
        for (entity, file, tags, attributes) in query.iter(world) {
            if dirty_attrs.contains(&entity) {
                if let Some(save) = attr_save_for(entity, file, tags, attributes) {
                    attrs.push(save);
                }
            }
        }
    }

    let stamp = world
        .get_resource::<crate::auth::AuthState>()
        .and_then(crate::space::instance_loader::current_stamp);
    let fingerprints: Vec<(Entity, u64)> = parts.iter().map(|p| (p.entity, p.fingerprint())).collect();
    let outcome = run_persist_job(&PersistJob { space_root, parts, attrs, stamp, now: Utc::now().to_rfc3339() });
    if let Some(mut d) = world.get_resource_mut::<SaveDirty>() {
        d.fingerprints.extend(fingerprints);
        // A write that failed stays due, and is compared again next time.
        for entity in &outcome.failed_parts {
            d.fingerprints.remove(entity);
            d.entities.insert(*entity);
        }
        d.attrs.extend(outcome.failed_attrs.iter().copied());
    }
    total.absorb(outcome);
    total
}

/// On exit, write every edit not yet on disk before the app closes.
pub fn persist_edits_on_exit(world: &mut World) {
    let exiting = world.get_resource::<Messages<AppExit>>().is_some_and(|m| !m.is_empty());
    if exiting {
        let outcome = persist_pending_edits(world);
        if outcome.written > 0 || outcome.errors > 0 {
            info!("💾 Wrote {} pending edits before exit ({} errors)", outcome.written, outcome.errors);
        }
    }
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

    // Edits of the outgoing Space not yet on disk go out before it closes.
    persist_pending_edits(world);

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
        // Save unsaved script edits before they leave with the outgoing
        // Space: a snapshot is out of sight of the close and exit prompts.
        // This also names each tab's source file, which the snapshot keeps.
        crate::ui::center_tabs::save_dirty_code_tabs(world);
        if let Some(mut tab_mgr) = world.get_resource_mut::<crate::ui::center_tabs::CenterTabManager>() {
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
    // Release the outgoing Space's database at the same instant `SpaceRoot`
    // moves (below), not a frame later when the open decision runs. Anything
    // that pairs a DB reference with `SpaceRoot` in between would otherwise
    // key the incoming Space's paths into the outgoing Space's store. The
    // entities that belong to that store were despawned above, so nothing
    // legitimate still needs it; `finish_pending_open` installs the new
    // Space's handle, subscription, source and funnel together.
    #[cfg(feature = "world-db")]
    {
        crate::space::active_db::clear();
        if let Some(mut h) =
            world.get_resource_mut::<crate::space::world_db_plugin::WorldDbHandle>()
        {
            h.0 = None;
        }
        // The open decision runs once per Space path, so opening the Space
        // that is already open would clear its database above and never open
        // it again (and a waiting revert would never run). Every open decides
        // afresh.
        if let Some(mut decision) =
            world.get_resource_mut::<crate::space::world_db_plugin::WorldDbDecision>()
        {
            decision.0 = None;
        }
        if let Some(mut s) =
            world.get_resource_mut::<crate::space::world_db_plugin::WorldDbSubscription>()
        {
            s.0 = None;
        }
        if let Some(mut src) =
            world.get_resource_mut::<crate::space::space_source::ActiveSpaceSource>()
        {
            *src = crate::space::space_source::ActiveSpaceSource::disk(space_path.to_path_buf());
        }
    }
    // The outgoing Space's entities are gone: no selection, expansion or
    // Explorer row may still point at one.
    crate::ui::slint_ui::reset_explorer_for_space_reload(world);

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
        // The filesystem side too: the Terrain folder and the dynamic
        // services are read from the open Space, so rescan them for it.
        es.cached_dynamic_services.clear();
        es.explorer_fs_stale = true;
        es.dirty = true;
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
    // Gravity is live state that no Space saves: the new one starts at the
    // default, not at whatever the outgoing Space was set to.
    crate::plugins::physics_plugin::reset_gravity_for_new_space(world);
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
    // One tuple param: Bevy systems take at most 16.
    (pending_open, db_decision): (
        Res<crate::space::world_db_plugin::PendingWorldDbOpen>,
        Res<crate::space::world_db_plugin::WorldDbDecision>,
    ),
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
    // The Space's DB open now completes on a worker across frames. A rescan
    // that ran before it settled would read the previous (or disk) source
    // and load the wrong tree. Leave the request armed; it fires the first
    // frame the open is settled.
    //
    // "No open pending" alone does not mean settled: on the frame of a
    // switch, before `open_world_db_on_space_change` has run for the new
    // Space, nothing is pending YET while `ActiveSpaceSource` still serves
    // the previous Space's tree. A rescan in that gap loads the previous
    // Space's instances and registers them under the new Space's paths,
    // where the first edit persists them. So also require that the open
    // decision has been made for THIS Space.
    if db_decision.0.as_deref() != Some(space_root.0.as_path()) {
        return;
    }
    if pending_open.0.as_ref().map(|p| !p.installed()).unwrap_or(false) {
        return;
    }
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

        let lighting_template_dir = crate::resource_root()
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
# NOTE: the engine exports recordings to\n\
#   .eustress/knowledge/recordings/<space_name>/sim_<timestamp>.json\n\
# and appends live watchpoint telemetry to .eustress/telemetry.jsonl.\n\
# output_dir below is reserved for a future user-overridable target and\n\
# is NOT consulted today; do not rely on the old .eustress/local path.\n\
output_dir = \".eustress/knowledge/recordings\"\n\
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

/// The name the loader gives an instance file with no `[metadata] name`
/// (`spawn_instance`): its folder's for an `_instance.toml`, else the file
/// name up to its first dot.
fn loader_fallback_name(toml_path: &Path) -> Option<String> {
    let file = toml_path.file_name()?.to_str()?;
    if file == "_instance.toml" {
        toml_path.parent()?.file_name()?.to_str().map(str::to_string)
    } else {
        file.split('.').next().map(str::to_string)
    }
}

/// A binary core's synthetic path, `Workspace/__bin_{class}_{id}/_instance.toml`,
/// which no file stands behind.
fn is_synthetic_core_path(toml_path: &Path) -> bool {
    toml_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("__bin_"))
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

#[cfg(test)]
mod tests {
    // ── Rotations written short are not edits ─────────────────────────────

    #[test]
    fn rotations_compare_as_rotations() {
        let short = [0.0, -0.1693, 0.0, 0.9856];
        let live = Quat::from_xyzw(0.0, -0.1693, 0.0, 0.9856).normalize().to_array();
        assert!(same_rotation(short, live), "a normalised short quaternion is the same rotation");
        assert!(same_rotation(live, live.map(|c| -c)), "q and -q are one rotation");
        let turned = (Quat::from_array(live) * Quat::from_rotation_y(1f32.to_radians())).to_array();
        assert!(!same_rotation(live, turned), "a one-degree turn is a change");
        assert!(!same_rotation([0.0; 4], live));
    }

    /// The build 8 regression: a part whose file holds a four-digit
    /// quaternion, loaded (so its rotation is normalised) and touched with no
    /// edit, must not be rewritten or stamped.
    #[test]
    fn a_short_written_rotation_is_not_rewritten() {
        let root = std::env::temp_dir().join(format!("eustress_short_rotation_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("Workspace").join("Crate_1");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("_instance.toml");
        std::fs::write(
            &file,
            "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [4.0, 0.5, -2.0]\nrotation = [0.0, -0.1693, 0.0, 0.9856]\nscale = [2.0, 1.0, 2.0]\n",
        )
        .unwrap();
        let before = std::fs::read(&file).unwrap();

        // The part as the loader spawns it from this file.
        let def = crate::space::instance_loader::load_instance_definition_from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let mut part = persist_test_part(&file, 0.0);
        part.name = "Crate_1".to_string();
        part.stem = loader_fallback_name(&file);
        part.translation = Vec3::from_array(def.transform.position);
        part.rotation = Quat::from_array(def.transform.rotation).normalize();
        part.size = Vec3::from_array(def.transform.scale);
        part.color = def.properties.color;
        part.material = def.properties.material.clone();
        part.transparency = def.properties.transparency;
        part.reflectance = def.properties.reflectance;
        part.anchored = def.properties.anchored;
        part.can_collide = def.properties.can_collide;
        part.cast_shadow = def.properties.cast_shadow;
        part.locked = def.properties.locked;

        assert_eq!(save_part(&part, None, "2026-09-26T02:18:48Z"), Ok(false));
        assert_eq!(std::fs::read(&file).unwrap(), before, "the file was rewritten");
        let _ = std::fs::remove_dir_all(&root);
    }

    // ── Persisting edits as they happen ───────────────────────────────────

    /// A fresh `<tag>/Workspace/Brick/_instance.toml` holding a plain part
    /// at the origin.
    fn persist_test_file(tag: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("eustress_persist_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("Workspace").join("Brick");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("_instance.toml");
        std::fs::write(
            &file,
            "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [0.0, 0.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [1.0, 1.0, 1.0]\n",
        )
        .unwrap();
        (root, file)
    }

    /// The part in `file`, moved to `x`.
    fn persist_test_part(file: &Path, x: f32) -> PartSave {
        PartSave {
            entity: Entity::PLACEHOLDER,
            name: "Brick".to_string(),
            toml_path: file.to_path_buf(),
            has_file: true,
            translation: Vec3::new(x, 0.0, 0.0),
            rotation: Quat::IDENTITY,
            size: Vec3::ONE,
            color: [0.5, 0.5, 0.5, 1.0],
            material: "Plastic".to_string(),
            transparency: 0.0,
            anchored: false,
            can_collide: true,
            cast_shadow: true,
            reflectance: 0.0,
            locked: false,
            destructible: false,
            archivable: true,
            class_name: "Part".to_string(),
            mesh: "parts/block.glb",
            name_override: None,
            stem: loader_fallback_name(file),
        }
    }

    fn persist_test_x(file: &Path) -> f32 {
        let text = std::fs::read_to_string(file).unwrap();
        crate::space::instance_loader::load_instance_definition_from_str(&text).unwrap().transform.position[0]
    }

    /// A write queued before the part's folder was trashed must not bring the
    /// folder back.
    #[test]
    fn a_queued_write_never_recreates_a_trashed_folder() {
        let (root, file) = persist_test_file("trashed");
        let part = persist_test_part(&file, 4.0);
        let folder = file.parent().unwrap().to_path_buf();
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!(save_part(&part, None, "2026-09-25T00:00:00Z"), Ok(false));
        assert!(!folder.exists(), "the trashed folder came back");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A write replaces the file whole: no temporary file is left beside it,
    /// and what is there parses and holds the new value.
    #[test]
    fn a_write_leaves_no_temporary_file_and_parses() {
        let (root, file) = persist_test_file("atomic");
        assert_eq!(save_part(&persist_test_part(&file, 3.0), None, "2026-09-25T00:00:00Z"), Ok(true));
        let leftovers: Vec<_> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temporary files left: {leftovers:?}");
        assert!((persist_test_x(&file) - 3.0).abs() < 1e-4);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An attribute and a tag edit reach the file through the edit writer.
    #[test]
    fn an_attribute_edit_reaches_disk() {
        let (root, file) = persist_test_file("attributes");
        let save = AttrSave {
            entity: Entity::PLACEHOLDER,
            toml_path: file.clone(),
            tags: Some(vec!["Enemy".to_string()]),
            attrs: Some(std::collections::HashMap::from([("Speed".to_string(), toml::Value::Integer(5))])),
        };
        let out = run_persist_job(&PersistJob {
            space_root: root.clone(),
            parts: Vec::new(),
            attrs: vec![save],
            stamp: None,
            now: String::new(),
        });
        assert_eq!((out.written, out.errors), (1, 0));
        assert_eq!(out.paths, vec![file.clone()]);
        let text = std::fs::read_to_string(&file).unwrap();
        let doc: toml::Value = text.parse().unwrap();
        assert_eq!(doc["tags"][0].as_str(), Some("Enemy"));
        assert!(text.contains("Speed"), "attribute missing:\n{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Jobs land in the order they were queued, whether the writer thread or
    /// a flush writes them: the newest value is the one left on disk.
    #[test]
    fn queued_jobs_land_in_order() {
        let (root, file) = persist_test_file("order");
        for x in [1.0, 2.0, 3.0] {
            enqueue_persist_job(PersistJob {
                space_root: root.clone(),
                parts: vec![persist_test_part(&file, x)],
                attrs: Vec::new(),
                stamp: None,
                now: String::new(),
            });
        }
        let _ = flush_persist_queue();
        assert!((persist_test_x(&file) - 3.0).abs() < 1e-4);
        let _ = std::fs::remove_dir_all(&root);
    }

    use super::*;

    /// Save Space over a Space mixing a part imported in feet with one in
    /// metres: each file keeps its own unit, and each reads back where its
    /// part is.
    #[test]
    fn save_space_writes_each_file_in_its_own_unit() {
        use eustress_common::classes::{BasePart, ClassName, Instance};
        let root = std::env::temp_dir().join(format!("eustress_save_space_units_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let put = |name: &str, text: &str| {
            let dir = root.join("Workspace").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("_instance.toml"), text).unwrap();
            dir.join("_instance.toml")
        };
        let feet = put(
            "Feet",
            "[metadata]\nclass_name = \"Part\"\nunit = \"ft\"\n\n[transform]\nposition = [10.0, 0.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [4.0, 1.0, 2.0]\n",
        );
        let metres = put(
            "Metres",
            "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [1.0, 0.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [1.0, 1.0, 1.0]\n",
        );
        let mut world = World::new();
        world.insert_resource(crate::space::SpaceRoot(root.clone()));
        let spawn = |world: &mut World, name: &str, path: &std::path::Path, at: Vec3, size: Vec3| {
            world.spawn((
                Instance { name: name.to_string(), class_name: ClassName::Part, ..Default::default() },
                BasePart { size, ..Default::default() },
                Transform::from_translation(at),
                crate::space::instance_loader::InstanceFile {
                    toml_path: path.to_path_buf(),
                    mesh_path: std::path::PathBuf::new(),
                    name: name.to_string(),
                },
            ));
        };
        // Each moved 1 m along +X from where its file put it.
        spawn(&mut world, "Feet", &feet, Vec3::new(3.048 + 1.0, 0.0, 0.0), Vec3::new(1.2192, 0.3048, 0.6096));
        spawn(&mut world, "Metres", &metres, Vec3::new(2.0, 0.0, 0.0), Vec3::ONE);
        save_space(&mut world);

        let read = |path: &std::path::Path| -> toml::Value { std::fs::read_to_string(path).unwrap().parse().unwrap() };
        let nums = |doc: &toml::Value, k: &str| -> Vec<f64> {
            doc["transform"][k].as_array().unwrap().iter().map(|v| v.as_float().unwrap()).collect()
        };
        let f = read(&feet);
        assert_eq!(f["metadata"]["unit"].as_str(), Some("ft"));
        #[cfg(feature = "units_v1")]
        {
            assert!((nums(&f, "position")[0] - 4.048 / 0.3048).abs() < 1e-3, "{:?}", nums(&f, "position"));
            let s = nums(&f, "scale");
            assert!((s[0] - 4.0).abs() < 1e-3 && (s[1] - 1.0).abs() < 1e-3 && (s[2] - 2.0).abs() < 1e-3, "{s:?}");
        }
        let m = read(&metres);
        assert!((nums(&m, "position")[0] - 2.0).abs() < 1e-5, "{:?}", nums(&m, "position"));
        assert!((nums(&m, "scale")[0] - 1.0).abs() < 1e-5);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_loaders_fallback_name_and_the_synthetic_core_path() {
        assert_eq!(loader_fallback_name(Path::new("W/Brick/_instance.toml")).as_deref(), Some("Brick"));
        assert_eq!(loader_fallback_name(Path::new("W/Crate.part.toml")).as_deref(), Some("Crate"));
        assert!(is_synthetic_core_path(Path::new("W/__bin_Part_00000000000000ff/_instance.toml")));
        assert!(!is_synthetic_core_path(Path::new("W/Brick/_instance.toml")));
    }

    /// A save with nothing edited, as right after a Space opens (every part
    /// marked), leaves every file byte for byte: the merge base is not healed
    /// back to disk, a file's own `name` stays, a flat file is named by its
    /// stem, and a baked part's synthetic path gets no folder.
    #[test]
    fn a_save_with_no_edits_writes_nothing() {
        use eustress_common::classes::{BasePart, ClassName, Instance};
        let root = std::env::temp_dir().join(format!("eustress_save_no_edits_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = root.join("Workspace");
        std::fs::create_dir_all(workspace.join("Brick")).unwrap();
        // Keys out of canonical order and no template sections: a disk heal
        // rewrites this file.
        let brick = workspace.join("Brick").join("_instance.toml");
        std::fs::write(
            &brick,
            "[transform]\nscale = [2.0, 1.0, 4.0]\nposition = [1.0, 2.0, 3.0]\nrotation = [0.0, 0.0, 0.0, -1.0]\n\n[metadata]\nname = \"Brick\"\nclass_name = \"Part\"\n",
        )
        .unwrap();
        let crate_file = workspace.join("Crate.part.toml");
        std::fs::write(
            &crate_file,
            "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [5.0, 0.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [1.0, 1.0, 1.0]\n",
        )
        .unwrap();
        let before: Vec<Vec<u8>> = [&brick, &crate_file].iter().map(|p| std::fs::read(p).unwrap()).collect();

        let mut world = World::new();
        world.insert_resource(crate::space::SpaceRoot(root.clone()));
        // Each part exactly as its file describes it.
        let spawn = |world: &mut World, name: &str, path: &Path| {
            let text = std::fs::read_to_string(path).unwrap();
            let def = crate::space::instance_loader::load_instance_definition_from_str(&text).unwrap();
            let c = def.properties.color;
            world.spawn((
                Instance { name: name.to_string(), class_name: ClassName::Part, ..Default::default() },
                BasePart {
                    size: Vec3::from_array(def.transform.scale),
                    color: Color::srgba(c[0], c[1], c[2], c[3]),
                    material_name: def.properties.material.clone(),
                    transparency: def.properties.transparency,
                    reflectance: def.properties.reflectance,
                    anchored: def.properties.anchored,
                    can_collide: def.properties.can_collide,
                    cast_shadow: def.properties.cast_shadow,
                    locked: def.properties.locked,
                    ..Default::default()
                },
                Transform {
                    translation: Vec3::from_array(def.transform.position),
                    rotation: Quat::from_array(def.transform.rotation),
                    scale: Vec3::ONE,
                },
                crate::space::instance_loader::InstanceFile {
                    toml_path: path.to_path_buf(),
                    mesh_path: std::path::PathBuf::new(),
                    name: name.to_string(),
                },
            ));
        };
        spawn(&mut world, "Brick", &brick);
        spawn(&mut world, "Crate", &crate_file);
        let synthetic = workspace.join("__bin_Part_00000000000000ff").join("_instance.toml");
        world.spawn((
            Instance { name: "Baked".to_string(), class_name: ClassName::Part, ..Default::default() },
            BasePart::default(),
            Transform::default(),
            crate::space::instance_loader::InstanceFile {
                toml_path: synthetic.clone(),
                mesh_path: std::path::PathBuf::new(),
                name: "Baked".to_string(),
            },
        ));
        save_space(&mut world);

        let after: Vec<Vec<u8>> = [&brick, &crate_file].iter().map(|p| std::fs::read(p).unwrap()).collect();
        assert_eq!(String::from_utf8_lossy(&after[0]), String::from_utf8_lossy(&before[0]), "Brick was rewritten");
        assert_eq!(String::from_utf8_lossy(&after[1]), String::from_utf8_lossy(&before[1]), "Crate was rewritten");
        assert!(!synthetic.parent().unwrap().exists(), "a baked part got a folder on disk");
        let _ = std::fs::remove_dir_all(&root);
    }
}
