/// Space management system - file-system-first architecture
/// 
/// A Space is a self-contained simulation environment:
/// - One Space = One scene = One folder
/// - Player-named (e.g., "My RPG", "City Builder", "Space Station")
/// - Git-native with sparse checkout for packages
/// - Can teleport between Spaces (scene transitions)
/// - Can load remote Spaces from .pak files (Cloudflare R2)

use bevy::prelude::*;
use std::path::{Path, PathBuf};

pub mod active_db;
pub mod class_defaults;
pub mod launch;
pub mod file_loader;
pub mod file_watcher;
pub mod gui_loader;
pub mod instance_create;
pub mod instance_loader;
pub mod material_loader;
pub mod service_loader;
pub mod draco_decoder;
pub mod space_ops;
pub mod space_source;
pub mod universe_registry;
pub mod representation;
#[cfg(feature = "world-db")]
pub mod arch_instance;
#[cfg(feature = "world-db")]
pub mod auto_convert;
#[cfg(feature = "world-db")]
pub mod world_db_binary;
#[cfg(feature = "world-db")]
pub mod world_db_plugin;

/// Resource holding the current Space root path
#[derive(Resource, Debug, Clone)]
pub struct SpaceRoot(pub PathBuf);

impl Default for SpaceRoot {
    fn default() -> Self {
        Self(default_space_root())
    }
}

pub fn workspace_root() -> PathBuf {
    // Resolution order:
    //   1. `EUSTRESS_WORKSPACE` env var — explicit override for power users
    //      and CI. Wins regardless of platform.
    //   2. Platform default (see `default_documents_root`).
    //   3. Current working directory as last resort.
    // 1. `EUSTRESS_WORKSPACE` override (CI / power users) — wins.
    if let Ok(env_path) = std::env::var("EUSTRESS_WORKSPACE") {
        let root = PathBuf::from(env_path);
        let _ = std::fs::create_dir_all(&root);
        if is_dir_empty(&root) {
            scaffold_default_universe(&root);
        }
        return root;
    }

    // 2. Canonical store: the **Documents** `Eustress/` Universe
    //    hierarchy (DIRECTION CHANGE 2026-05-17). The human-editable
    //    TOML hierarchy lives here alongside each Space's `.eustress`
    //    binary container; TOML and binary coexist and the TOML is an
    //    honored import source at runtime. (The earlier %LOCALAPPDATA%
    //    relocation was reverted — the user wants the hierarchy in
    //    Documents.)
    if let Some(docs) = default_documents_root() {
        let root = docs.join("Eustress");
        let _ = std::fs::create_dir_all(&root);

        // First-launch: scaffold default Universe + Space if empty.
        if is_dir_empty(&root) {
            scaffold_default_universe(&root);
        }

        return root;
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Return the user's Documents folder, preferring the **local** one on Windows.
///
/// `dirs::document_dir()` returns whatever Windows' `FOLDERID_Documents` known
/// folder resolves to. With OneDrive's "Known Folder Move" enabled (the default
/// on most modern Windows installs), that points at `OneDrive\Documents` (or
/// the locale equivalent — e.g. `OneDrive\Documentos` on Spanish systems).
///
/// Syncing the whole Eustress workspace through OneDrive is almost never what
/// users want: OneDrive rewrites file metadata, fights the file watcher, and
/// can silently restore deleted TOMLs. We explicitly bypass that by using
/// `%USERPROFILE%\Documents` on Windows. Other platforms keep the XDG /
/// macOS-standard behaviour.
pub fn default_documents_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Some(home) = dirs::home_dir() {
            let local_docs = home.join("Documents");
            if local_docs.exists() || std::fs::create_dir_all(&local_docs).is_ok() {
                return Some(local_docs);
            }
        }
        // Fall through if %USERPROFILE% is somehow unavailable
        dirs::document_dir()
    }
    #[cfg(not(target_os = "windows"))]
    {
        dirs::document_dir()
    }
}

/// Check if a directory exists and has no subdirectories
fn is_dir_empty(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut rd| rd.next().is_none())
        .unwrap_or(true)
}

/// Create the default Universe + Space structure for first-time users.
/// Uses the full scaffold_new_space from space_ops for complete setup.
fn scaffold_default_universe(root: &Path) {
    let universe = root.join("Universe1");
    // Auto-migrate: rename lowercase "spaces" to "Spaces" if it exists
    let legacy_spaces = universe.join("spaces");
    let spaces_dir = universe.join("Spaces");
    if legacy_spaces.is_dir() && !spaces_dir.exists() {
        let _ = std::fs::rename(&legacy_spaces, &spaces_dir);
        info!("📂 Migrated spaces/ → Spaces/");
    }
    let _ = std::fs::create_dir_all(&spaces_dir);

    // Create Universe-level asset directories
    let _ = std::fs::create_dir_all(universe.join(".eustress").join("assets").join("parts"));
    let _ = std::fs::create_dir_all(universe.join(".eustress").join("assets").join("meshes"));

    match space_ops::scaffold_new_space(&spaces_dir, "Space1", "Eustress User") {
        Ok(_) => info!("🌍 Created default Universe with Space1 at {:?}", universe),
        Err(e) => warn!("⚠ First-launch scaffold failed: {} — creating minimal structure", e),
    }
}

pub fn looks_like_space_root(path: &Path) -> bool {
    path.join(".eustress").join("project.toml").exists()
        || path.join("Workspace").exists()
        || path.join("space.toml").exists()
        // A fully-converted `.eustress` world has no loose `Workspace/`
        // or `space.toml` — `header.bin` + `world.fjalldb/` are the
        // canonical container markers.
        || path.join("header.bin").exists()
        || path.join("world.fjalldb").exists()
}

/// Returns true if `path` resolves to a core service (Workspace,
/// Lighting, ReplicatedStorage, SoulService, …) — a directory directly
/// under the Space root with a `_service.toml` inside, or the
/// `_service.toml` file itself.
///
/// Core services are first-class scaffolding: deleting one orphans
/// every child entity (parts under Workspace, scripts under
/// SoulService, …) and leaves the file watcher in a broken state
/// because the service root entity is gone but its folder remains on
/// disk. This check is the single source of truth used by every
/// destructive surface (Delete shortcut, Explorer Cut, MCP
/// `delete_entity`, future bulk-trash flows) to refuse the operation
/// up front with a clear toast rather than silently corrupting state.
pub fn is_protected_service_path(path: &Path) -> bool {
    // Direct `_service.toml` file — services are loaded with
    // `LoadedFromFile.path = .../<Service>/_service.toml` (see
    // `service_loader::spawn_service`), so this is the path Delete
    // sees in the `loaded_from_file_query` fallback branch.
    if path
        .file_name()
        .map(|n| n.eq_ignore_ascii_case("_service.toml"))
        .unwrap_or(false)
    {
        return true;
    }
    // Folder that contains `_service.toml` — e.g. the Cut path might
    // hold the service folder path directly. Catches the case where a
    // future surface passes the directory rather than the toml file.
    if path.is_dir() && path.join("_service.toml").is_file() {
        return true;
    }
    false
}

pub fn universe_root_for_path(path: &Path) -> Option<PathBuf> {
    let workspace = workspace_root();

    // Walk up from the given path (typically a Space root) looking for the
    // Universe directory — the child of the workspace root that is NOT a
    // Space itself. The standard layout is:
    //
    //   {workspace}/Universe1/Spaces/Space1   (3 levels)
    //
    // But legacy layouts may omit the `Spaces/` tier:
    //
    //   {workspace}/Universe1/Space1           (2 levels)
    //
    // We iterate ancestors instead of hard-coding a fixed depth so both
    // layouts (and any future nesting) resolve correctly.
    let mut current = Some(path);
    while let Some(p) = current {
        if let Some(parent) = p.parent() {
            if parent == workspace.as_path() && !looks_like_space_root(p) {
                return Some(p.to_path_buf());
            }
        }
        current = p.parent();
    }

    None
}

/// True if `path` is a real Universe directory: a non-hidden directory that
/// holds a `Spaces/` (or legacy `spaces/`) tier and is not itself a Space.
/// Excludes hidden config dirs (`.claude`, `.eustress`, `.git`, …) and
/// stray files, so a workspace scan never mistakes them for a Universe.
pub fn is_universe_dir(path: &Path) -> bool {
    if !path.is_dir() || looks_like_space_root(path) {
        return false;
    }
    let hidden = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.'))
        .unwrap_or(true);
    if hidden {
        return false;
    }
    path.join("Spaces").is_dir() || path.join("spaces").is_dir()
}

pub fn first_universe_root() -> Option<PathBuf> {
    let workspace = workspace_root();
    let mut universes: Vec<PathBuf> = std::fs::read_dir(&workspace)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_universe_dir(path))
        .collect();
    universes.sort();
    universes.into_iter().next()
}

/// The Universe directory named by the `.default_universe` marker the
/// launcher writes under the workspace root, when it names an existing dir.
pub fn recorded_default_universe() -> Option<PathBuf> {
    let workspace = workspace_root();
    let raw = std::fs::read_to_string(workspace.join(".default_universe")).ok()?;
    let name = raw.trim();
    if name.is_empty() {
        return None;
    }
    let candidate = workspace.join(name);
    candidate.is_dir().then_some(candidate)
}

/// Best default Universe root when no specific Space is loaded: the
/// recorded `.default_universe`, else the first real Universe, else
/// `<workspace>/Universe1`. Always routes through `workspace_root`
/// (OneDrive-avoiding). Use this — not a raw `read_dir().next()` — anywhere
/// you need "the Universe the engine boots into" without a `SpaceRoot` in
/// hand (port files, stream advertisements, default-space resolution).
pub fn best_default_universe_root() -> PathBuf {
    recorded_default_universe()
        .or_else(first_universe_root)
        .unwrap_or_else(|| workspace_root().join("Universe1"))
}

pub fn first_space_root_in_universe(universe_root: &Path) -> Option<PathBuf> {
    // First check the "Spaces/" subdirectory (standard Universe structure)
    // Also check legacy lowercase "spaces/" for backward compatibility
    let spaces_dir = universe_root.join("Spaces");
    let spaces_dir = if spaces_dir.is_dir() { spaces_dir } else { universe_root.join("spaces") };
    if spaces_dir.is_dir() {
        let mut spaces: Vec<PathBuf> = std::fs::read_dir(&spaces_dir)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir() && looks_like_space_root(path))
            .collect();
        spaces.sort();
        if let Some(space) = spaces.into_iter().next() {
            return Some(space);
        }
    }
    
    // Fallback: check directly inside the universe root (legacy structure)
    let mut spaces: Vec<PathBuf> = std::fs::read_dir(universe_root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && looks_like_space_root(path))
        .collect();
    spaces.sort();
    spaces.into_iter().next()
}

pub fn default_space_root() -> PathBuf {
    // Try to restore last opened space from editor settings.
    // Prefer the current `.eustress_engine/` dir; fall back to the
    // legacy `.eustress_studio/` location so users whose settings
    // haven't been migrated yet still see their last-opened space.
    if let Some(home) = dirs::home_dir() {
        let current = home.join(".eustress_engine").join("settings.json");
        let legacy  = home.join(".eustress_studio").join("settings.json");
        let settings_path = if current.exists() { current } else { legacy };
        if let Ok(contents) = std::fs::read_to_string(&settings_path) {
            if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&contents) {
                if let Some(last) = settings.get("last_space_path").and_then(|v| v.as_str()) {
                    let path = PathBuf::from(last);
                    if path.exists() {
                        return path;
                    }
                }
            }
        }
    }

    // Fallback: first alphabetical space
    let workspace = workspace_root();

    if let Ok(read_dir) = std::fs::read_dir(&workspace) {
        let mut universes: Vec<PathBuf> = read_dir
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir() && !looks_like_space_root(path))
            .collect();
        universes.sort();

        for universe_root in universes {
            if let Some(space_root) = first_space_root_in_universe(&universe_root) {
                return space_root;
            }
        }
    }

    first_universe_root().unwrap_or(workspace)
}

pub use file_loader::{
    FileType, FileMetadata, SpaceFileRegistry, LoadedFromFile,
    SpaceFileLoaderPlugin, scan_space_directory,
};
pub use file_watcher::{
    SpaceFileWatcher, FileChangeEvent, FileChangeType,
};
pub use instance_loader::{
    InstanceDefinition, InstanceFile, AssetReference,
    TransformData, InstanceProperties, InstanceMetadata,
    load_instance_definition, load_instance_definition_with_defaults,
    write_instance_definition,
};
pub use class_defaults::ClassDefaultsRegistry;
pub use universe_registry::{UniverseInfo, UniverseRegistry, UniverseRegistryPlugin, SpaceInfo};

