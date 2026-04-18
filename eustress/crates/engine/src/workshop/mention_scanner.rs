//! # Universe-wide mention scanner
//!
//! Populates [`MentionIndex`] with every referenceable item in the Universe,
//! regardless of which Space is currently loaded. Operates entirely on
//! filesystem data — no ECS dependency — so users can `@mention` a battery
//! from Space2 while editing Space1.
//!
//! ## Pass structure
//!
//! The scanner runs in one pass over `{Universe}/Spaces/*/`:
//!
//! 1. **Entities + services + scripts** — walk each Space for
//!    `_instance.toml` and `_service.toml`, parse the minimal `[metadata]`
//!    section, yield a [`MentionEntry`] with `source = Toml`.
//! 2. **Generic files** — walk each Space's filesystem, yield an entry per
//!    media / document file with `source = Filesystem`. Common formats
//!    (`.png`, `.jpg`, `.webp`, `.pdf`, `.md`, `.txt`, `.rune`, `.luau`) get
//!    dedicated icon hints; everything else falls back to a generic file icon.
//!
//! ## Invocation
//!
//! Runs once at Universe load (startup or on Space switch changing Universe),
//! and incrementally from the file watcher whenever `_instance.toml` or a
//! scanned media file is created/deleted.
//!
//! The full scan is parallelisable via `rayon` but kept single-threaded in
//! this first pass — profile before parallelising. For a 50k-entity
//! Universe on an NVMe drive expect ~2-4 seconds cold, well under 100 ms
//! with warm filesystem caches. Triggered on the Bevy startup task pool to
//! avoid blocking the main thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::Deserialize;
use bevy::prelude::*;

use super::mention::{
    MentionEntry, MentionId, MentionKind, MentionSource, MentionIndex,
};

// ═══════════════════════════════════════════════════════════════════════════
// 1. Minimal TOML shape for parsing
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct InstanceMetaShallow {
    #[serde(default)]
    metadata: MetadataShallow,
}

#[derive(Debug, Default, Deserialize)]
struct MetadataShallow {
    #[serde(default)]
    class_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ServiceMetaShallow {
    #[serde(default)]
    service: ServicePropsShallow,
}

#[derive(Debug, Default, Deserialize)]
struct ServicePropsShallow {
    #[serde(default)]
    class_name: Option<String>,
    #[serde(default)]
    icon: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Scan entry point
// ═══════════════════════════════════════════════════════════════════════════

/// Scan the entire Universe tree under `{universe_root}/Spaces/`. Returns
/// a map keyed by [`MentionId`] so the caller can merge it atomically into
/// the live index.
///
/// This is a pure function — no Bevy / resource access. Spawn it on the
/// IoTaskPool for large Universes so the main thread stays responsive.
pub fn scan_universe(universe_root: &Path) -> HashMap<MentionId, MentionEntry> {
    let mut entries = HashMap::new();
    let spaces_dir = universe_root.join("Spaces");
    if !spaces_dir.is_dir() {
        warn!("mention-scan: no Spaces/ folder at {:?}", universe_root);
        return entries;
    }

    let space_dirs = match std::fs::read_dir(&spaces_dir) {
        Ok(rd) => rd.filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .collect::<Vec<_>>(),
        Err(err) => {
            warn!("mention-scan: cannot read Spaces/: {}", err);
            return entries;
        }
    };

    let mut total_files = 0usize;
    for space_entry in space_dirs {
        let space_path = space_entry.path();
        let space_name = space_path.file_name()
            .and_then(|n| n.to_str()).unwrap_or("").to_string();
        if space_name.is_empty() { continue; }

        scan_space_into(&space_path, &space_name, &mut entries, &mut total_files);
    }

    info!("mention-scan: indexed {} entries across {} spaces ({} files visited)",
        entries.len(),
        std::fs::read_dir(&spaces_dir).map(|rd| rd.count()).unwrap_or(0),
        total_files);
    entries
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Per-Space walker
// ═══════════════════════════════════════════════════════════════════════════

fn scan_space_into(
    space_path: &Path,
    space_name: &str,
    out: &mut HashMap<MentionId, MentionEntry>,
    total_files: &mut usize,
) {
    // Recurse depth-first. Skip `.eustress/` and other hidden / auxiliary
    // dirs — those aren't user-referenceable content.
    walk_dir(space_path, space_path, space_name, out, total_files);
}

fn walk_dir(
    space_root: &Path,
    current: &Path,
    space_name: &str,
    out: &mut HashMap<MentionId, MentionEntry>,
    total_files: &mut usize,
) {
    let rd = match std::fs::read_dir(current) {
        Ok(rd) => rd,
        Err(_) => return,
    };

    for entry in rd.filter_map(|e| e.ok()) {
        let path = entry.path();
        let ty = match entry.file_type() { Ok(t) => t, Err(_) => continue };
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Skip hidden/auxiliary folders and files.
        if file_name.starts_with('.') { continue; }

        if ty.is_dir() {
            // Process `_instance.toml` / `_service.toml` scaffold inside this
            // folder BEFORE descending, so the folder's own entry gets emitted
            // regardless of whether children succeed.
            let inst = path.join("_instance.toml");
            if inst.is_file() {
                if let Some(e) = parse_instance_toml(space_root, space_name, &inst) {
                    out.insert(e.id, e);
                }
            }
            let svc = path.join("_service.toml");
            if svc.is_file() {
                if let Some(e) = parse_service_toml(space_root, space_name, &svc) {
                    out.insert(e.id, e);
                }
            }
            walk_dir(space_root, &path, space_name, out, total_files);
        } else if ty.is_file() {
            *total_files += 1;
            // Skip marker files — they're captured via their parent folder.
            if file_name == "_instance.toml" || file_name == "_service.toml" {
                continue;
            }

            // Flat-file entity formats (.glb.toml, .part.toml, .script.toml,
            // .textlabel.toml, etc.) get handled as instance-style entries.
            if let Some(flat_kind) = classify_flat_entity(file_name) {
                if let Some(e) = parse_flat_entity(space_root, space_name, &path, flat_kind) {
                    out.insert(e.id, e);
                }
                continue;
            }

            // Generic file entry (image / doc / script source / data file).
            if let Some(e) = entry_for_file(space_root, space_name, &path) {
                out.insert(e.id, e);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Per-file classifiers + parsers
// ═══════════════════════════════════════════════════════════════════════════

/// Return `(MentionKind, class_name_hint)` for flat-file entity TOMLs.
/// Folder-based parts use `_instance.toml`; legacy flat files use the
/// extensions below.
fn classify_flat_entity(file_name: &str) -> Option<(MentionKind, &'static str)> {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".part.toml") { Some((MentionKind::Entity, "Part")) }
    else if lower.ends_with(".glb.toml") { Some((MentionKind::Entity, "Part")) }
    else if lower.ends_with(".model.toml") { Some((MentionKind::Entity, "Model")) }
    else if lower.ends_with(".instance.toml") { Some((MentionKind::Entity, "Instance")) }
    else if lower.ends_with(".textlabel.toml") { Some((MentionKind::Entity, "TextLabel")) }
    else if lower.ends_with(".textbutton.toml") { Some((MentionKind::Entity, "TextButton")) }
    else if lower.ends_with(".imagelabel.toml") { Some((MentionKind::Entity, "ImageLabel")) }
    else if lower.ends_with(".screengui.toml") { Some((MentionKind::Entity, "ScreenGui")) }
    else if lower.ends_with(".frame.toml") { Some((MentionKind::Entity, "Frame")) }
    else { None }
}

fn parse_instance_toml(space_root: &Path, space_name: &str, path: &Path) -> Option<MentionEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let shallow: InstanceMetaShallow = toml::from_str(&text).ok()?;
    let class_raw = shallow.metadata.class_name.unwrap_or_else(|| "Instance".to_string());

    // Folder name is the canonical display name unless metadata overrides.
    let folder_name = path.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("Instance")
        .to_string();
    let display = shallow.metadata.name.unwrap_or(folder_name.clone());

    let kind = classify_class_name(&class_raw);
    let rel = path.parent()?
        .strip_prefix(space_root).ok()?
        .to_string_lossy().replace('\\', "/");

    build_entry(
        kind, space_name, &rel,
        display,
        format!("{} · {}", class_raw, space_rel_qualifier(space_name, &rel)),
        icon_hint_for_class(&class_raw),
        MentionSource::Toml,
    )
}

fn parse_service_toml(space_root: &Path, space_name: &str, path: &Path) -> Option<MentionEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let shallow: ServiceMetaShallow = toml::from_str(&text).ok()?;
    let class_name = shallow.service.class_name.unwrap_or_else(|| "Service".to_string());
    let folder_name = path.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())?
        .to_string();
    let rel = path.parent()?
        .strip_prefix(space_root).ok()?
        .to_string_lossy().replace('\\', "/");

    build_entry(
        MentionKind::Service, space_name, &rel,
        folder_name,
        format!("Service · {} · {}", class_name, space_name),
        shallow.service.icon.unwrap_or_else(|| class_name.to_lowercase()),
        MentionSource::Toml,
    )
}

fn parse_flat_entity(
    space_root: &Path,
    space_name: &str,
    path: &Path,
    kind_hint: (MentionKind, &'static str),
) -> Option<MentionEntry> {
    // Best-effort: read the metadata section if present, else derive
    // everything from the filename.
    let text = std::fs::read_to_string(path).ok()?;
    let shallow: InstanceMetaShallow = toml::from_str(&text).unwrap_or(InstanceMetaShallow {
        metadata: MetadataShallow::default(),
    });
    let file_stem = path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.split('.').next())
        .unwrap_or("entity")
        .to_string();
    let display = shallow.metadata.name.unwrap_or(file_stem);
    let class_name = shallow.metadata.class_name.unwrap_or_else(|| kind_hint.1.to_string());
    let rel = path.strip_prefix(space_root).ok()?
        .to_string_lossy().replace('\\', "/");

    build_entry(
        kind_hint.0, space_name, &rel,
        display,
        format!("{} · {} (flat)", class_name, space_rel_qualifier(space_name, &rel)),
        icon_hint_for_class(&class_name),
        MentionSource::Toml,
    )
}

fn entry_for_file(space_root: &Path, space_name: &str, path: &Path) -> Option<MentionEntry> {
    let rel = path.strip_prefix(space_root).ok()?
        .to_string_lossy().replace('\\', "/");
    let name = path.file_name().and_then(|n| n.to_str())?.to_string();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();

    let (kind, icon_hint, qualifier_prefix) = match ext.as_str() {
        // Script sources
        "rune" => (MentionKind::Script, "script".to_string(), "Rune script"),
        "luau" | "lua" => (MentionKind::Script, "script".to_string(), "Luau script"),
        // Media
        "png" | "jpg" | "jpeg" | "webp" | "gif" => (MentionKind::File, "image".to_string(), "Image"),
        "pdf" => (MentionKind::File, "document".to_string(), "PDF"),
        "md" | "markdown" => (MentionKind::File, "markdown".to_string(), "Markdown"),
        "txt" => (MentionKind::File, "text".to_string(), "Text"),
        "json" => (MentionKind::File, "json".to_string(), "JSON"),
        "toml" => (MentionKind::File, "toml".to_string(), "TOML"),
        "ron" => (MentionKind::File, "ron".to_string(), "RON"),
        "docx" | "doc" => (MentionKind::File, "document".to_string(), "Document"),
        "mp4" | "mov" | "webm" => (MentionKind::File, "video".to_string(), "Video"),
        "wav" | "mp3" | "ogg" => (MentionKind::File, "audio".to_string(), "Audio"),
        "glb" | "gltf" | "fbx" | "obj" | "stl" => (MentionKind::File, "mesh".to_string(), "Mesh asset"),
        // Skip very large / binary-only extensions that aren't useful to @-ref
        // `obj` is claimed by the mesh arm above (Wavefront .obj); here we
        // skip only the remaining binary-artifact extensions.
        "exe" | "dll" | "so" | "dylib" | "a" | "lib" | "o" => return None,
        _ => (MentionKind::File, "file".to_string(), "File"),
    };

    build_entry(
        kind, space_name, &rel,
        name.clone(),
        format!("{} · {}", qualifier_prefix, space_rel_qualifier(space_name, &rel)),
        icon_hint,
        MentionSource::Filesystem,
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn build_entry(
    kind: MentionKind,
    space: &str,
    rel_path: &str,
    name: String,
    qualifier: String,
    icon_hint: String,
    source: MentionSource,
) -> Option<MentionEntry> {
    let canonical = MentionEntry::canonical_for(kind, space, rel_path);
    let id = MentionId::from_canonical(kind, &canonical);
    Some(MentionEntry {
        id, kind, name, qualifier,
        canonical_path: canonical,
        space: space.to_string(),
        rel_path: rel_path.to_string(),
        icon_hint,
        source,
        entity: None,
    })
}

fn space_rel_qualifier(space: &str, rel: &str) -> String {
    let dir = rel.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    if dir.is_empty() {
        space.to_string()
    } else {
        format!("{}/{}", space, dir)
    }
}

fn classify_class_name(class_name: &str) -> MentionKind {
    match class_name {
        "SoulScript" | "Script" | "LocalScript" | "ModuleScript" | "LuauScript" => MentionKind::Script,
        _ => MentionKind::Entity,
    }
}

fn icon_hint_for_class(class: &str) -> String {
    match class {
        "Part" | "BasePart" | "MeshPart" | "UnionOperation" => "part",
        "Model" | "PVInstance" => "model",
        "Folder" => "folder",
        "Camera" => "camera",
        "SoulScript" | "Script" | "LocalScript" | "ModuleScript" | "LuauScript" => "soulservice",
        "BillboardGui" => "billboardgui",
        "ScreenGui" => "screengui",
        "SurfaceGui" => "surfacegui",
        "TextLabel" => "textlabel",
        "TextButton" => "textbutton",
        "Frame" => "frame",
        _ => "instance",
    }
    .to_string()
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Bevy systems — trigger rescans on Universe switch / file events
// ═══════════════════════════════════════════════════════════════════════════

/// Task-pool handle for in-flight Universe scans. `None` when no scan is
/// running. The result is polled by [`poll_universe_scan`] and merged into
/// `MentionIndex` when ready.
#[derive(Resource, Default)]
pub struct UniverseScanTask {
    pub task: Option<bevy::tasks::Task<HashMap<MentionId, MentionEntry>>>,
    /// Universe root that was scanned — used to detect mid-flight Universe
    /// switches and discard stale results.
    pub scanning: Option<PathBuf>,
}

/// Triggers a fresh scan whenever [`crate::space::SpaceRoot`] changes to
/// point at a new Universe. Runs the walk on `IoTaskPool` so the main
/// thread stays responsive during large scans.
pub fn trigger_rescan_on_universe_change(
    mut last_universe: Local<Option<PathBuf>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut scan_task: ResMut<UniverseScanTask>,
    mut index: ResMut<MentionIndex>,
) {
    let Some(sr) = space_root else { return };
    let universe = match crate::space::universe_root_for_path(&sr.0) {
        Some(u) => u,
        None => return,
    };

    let changed = last_universe.as_ref() != Some(&universe);
    if !changed { return; }
    *last_universe = Some(universe.clone());

    // Point the index at the new Universe's knowledge dir.
    index.set_universe_root(Some(universe.clone()));

    // Drop any previously-loaded static entries — a fresh scan replaces them.
    // Live ECS entries stay; they'll be synced in over the next frames.
    index.rebuild(HashMap::new());

    // Spawn the scan on the IoTaskPool.
    let task = bevy::tasks::IoTaskPool::get().spawn({
        let universe = universe.clone();
        async move {
            scan_universe(&universe)
        }
    });
    scan_task.task = Some(task);
    scan_task.scanning = Some(universe);
    info!("mention-scan: started rescan");
}

/// Polls the in-flight scan task and merges results when complete.
pub fn poll_universe_scan(
    mut scan_task: ResMut<UniverseScanTask>,
    mut index: ResMut<MentionIndex>,
) {
    let Some(task) = scan_task.task.as_mut() else { return };
    if !task.is_finished() { return; }

    // `Task::is_finished` only reports true after the future has completed,
    // so `block_on` here is effectively non-blocking.
    let task = scan_task.task.take().unwrap();
    let scanning = scan_task.scanning.take();
    let result = bevy::tasks::block_on(task);

    // Sanity: if the user switched Universes during the scan, drop the
    // result — the trigger system has already queued a new scan.
    if scanning != index.universe_root().map(PathBuf::from) {
        info!("mention-scan: discarded stale result (universe changed mid-scan)");
        return;
    }

    let count = result.len();
    index.merge_batch(result.into_values().collect());
    info!("mention-scan: merged {} static entries", count);
}
