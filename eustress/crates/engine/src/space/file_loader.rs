/// Dynamic file loader - scans Space folders and automatically loads supported file types
/// 
/// This system replaces hardcoded entity spawning with automatic discovery:
/// - Scans Workspace/, Lighting/, etc. folders for supported files
/// - Loads .glb files as meshes, .soul files as scripts, .png as textures, etc.
/// - Creates ECS entities dynamically based on file contents
/// - Watches for file changes and reloads automatically
/// - Properties panel edits actual files on disk, not just in-memory ECS

use bevy::prelude::*;
use std::path::{Path, PathBuf};
use std::collections::HashMap;

/// Supported file types and their loaders
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileType {
    // 3D Models
    Gltf,           // .gltf, .glb
    Obj,            // .obj
    Fbx,            // .fbx
    
    // Scripts
    Soul,           // .soul → .md (markdown scripts that compile to .rune in cache)
    Rune,           // .rune (compiled Rune bytecode in cache)
    Wasm,           // .wasm (compiled scripts)
    Lua,            // .lua
    
    // Textures
    Png,            // .png
    Jpg,            // .jpg, .jpeg
    Tga,            // .tga
    Dds,            // .dds
    Ktx2,           // .ktx2
    
    // Audio
    Ogg,            // .ogg
    Mp3,            // .mp3
    Wav,            // .wav
    Flac,           // .flac
    
    // Scenes
    Scene,          // .scene.toml
    
    // Materials
    Material,       // .mat.toml
    
    // Terrain
    Hgt,            // .hgt (SRTM elevation)
    GeoTiff,        // .tif, .tiff
    
    // UI
    Slint,          // .slint
    Html,           // .html
    
    // GUI Elements (StarterGui)
    GuiElement,     // .textlabel, .textbutton, .frame, .imagelabel, .imagebutton, .scrollingframe
    
    // Data
    Json,           // .json
    Toml,           // .toml
    Ron,            // .ron

    // Virtual — represents a filesystem subdirectory mapped to a Folder entity
    Directory,
}

impl FileType {
    /// Get file type from extension
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "gltf" | "glb" => Some(Self::Gltf),
            "obj" => Some(Self::Obj),
            "fbx" => Some(Self::Fbx),
            "soul" | "md" => Some(Self::Soul), // .soul or .md files compile to .rune
            "rune" => Some(Self::Rune), // Compiled bytecode in cache
            "wasm" => Some(Self::Wasm),
            "lua" | "luau" => Some(Self::Lua),
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpg),
            "tga" => Some(Self::Tga),
            "dds" => Some(Self::Dds),
            "ktx2" => Some(Self::Ktx2),
            "ogg" => Some(Self::Ogg),
            "mp3" => Some(Self::Mp3),
            "wav" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            "hgt" => Some(Self::Hgt),
            "tif" | "tiff" | "geotiff" => Some(Self::GeoTiff),
            "slint" => Some(Self::Slint),
            "html" => Some(Self::Html),
            // GUI elements
            "textlabel" | "textbutton" | "frame" | "imagelabel" | "imagebutton" | 
            "scrollingframe" | "textbox" | "viewportframe" => Some(Self::GuiElement),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "ron" => Some(Self::Ron),
            _ => None,
        }
    }
    
    /// Get file type from full path (handles compound extensions like .glb.toml, .part.toml)
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        let path_str = path.to_string_lossy();

        // Check for compound extensions first (order matters - check specific before generic)

        // `_service.toml` — service marker, never an entity.
        if path_str.ends_with("_service.toml") {
            return None;
        }

        // `_instance.toml` — folder-based entity marker. On INITIAL SCAN
        // the folder walker classifies the parent directory via
        // `scan_dir_entries` + `spawn_directory_entry`, so the marker
        // itself is skipped by filename inside that path
        // (scan_dir_entries line ~329). But on HOT-CREATE we need this
        // file to route through the file-watcher's FileType::Toml
        // branch — otherwise a brand-new `Workspace/Foo/_instance.toml`
        // written by a Workshop tool (create_entity, etc.) is dropped
        // by `process_event`'s `FileType::from_path(...)?` and the
        // entity never spawns until the next full rescan. Returning
        // `Some(Self::Toml)` makes `load_instance_definition_with_defaults`
        // read this file directly; its InstanceDefinition captures the
        // same data the folder-walker would have derived.
        if path_str.ends_with("_instance.toml") {
            return Some(Self::Toml);
        }
        
        // Instance files (spawn as entities)
        if path_str.ends_with(".glb.toml") 
            || path_str.ends_with(".part.toml") 
            || path_str.ends_with(".model.toml")
            || path_str.ends_with(".instance.toml") 
        {
            return Some(Self::Toml); // Instance file
        }
        // Scene files
        if path_str.ends_with(".scene.toml") {
            return Some(Self::Scene);
        }
        // Material files
        if path_str.ends_with(".mat.toml") {
            return Some(Self::Material);
        }
        // GUI element compound extensions (.textlabel.toml, .textbutton.toml, .frame.toml, etc.)
        // Must be checked BEFORE the plain .toml catch-all below
        if path_str.ends_with(".textlabel.toml") || path_str.ends_with(".textbutton.toml")
            || path_str.ends_with(".frame.toml") || path_str.ends_with(".imagelabel.toml")
            || path_str.ends_with(".imagebutton.toml") || path_str.ends_with(".scrollingframe.toml")
            || path_str.ends_with(".textbox.toml") || path_str.ends_with(".viewportframe.toml")
            || path_str.ends_with(".screengui.toml")
        {
            return Some(Self::GuiElement);
        }
        
        // Plain .toml files (config, settings, etc.) - don't spawn entities
        if path_str.ends_with(".toml") {
            return None; // Ignore plain .toml files - they're config, not instances
        }
        
        // Fall back to simple extension check for non-TOML files
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }
    
    /// Check if this file type should spawn an entity in the given service folder
    pub fn spawns_entity_in_service(&self, service: &str) -> bool {
        match (self, service) {
            // Workspace: Instance files (.glb.toml, .part.toml, .model.toml, .instance.toml, _instance.toml, _service.toml) and 3D models spawn as Parts
            (Self::Toml | Self::Gltf | Self::Obj | Self::Fbx, "Workspace") => true,
            
            // Lighting: TOML instance files (Sun, Moon, Sky, Atmosphere, Skybox)
            // spawn as non-visual Instance entities; hydrate_lighting_entities
            // attaches the real ECS components (DirectionalLight, markers, etc.)
            (Self::Toml | Self::Gltf | Self::Obj | Self::Fbx, "Lighting") => true,
            
            // SoulService: Scripts don't spawn visible entities, but need to be loaded
            (Self::Soul | Self::Rune | Self::Wasm | Self::Lua, "SoulService") => true,
            
            // SoundService: Audio files spawn as Sound entities
            (Self::Ogg | Self::Mp3 | Self::Wav | Self::Flac, "SoundService") => true,
            
            // StarterGui: GUI elements spawn as UI entities
            (Self::GuiElement | Self::Toml, "StarterGui") => true,

            // Workspace: GUI elements spawn here too — a BillboardGui /
            // SurfaceGui parented to a Part (e.g. SimpleBlock/Label/) can
            // have child UI elements (TextLabel, Frame, ImageLabel, …)
            // serialised as `.textlabel.toml` / etc. inside the same
            // folder. Without this arm, the file walker recurses into the
            // BillboardGui's directory, finds the child file, but
            // `spawns_entity_in_service` returns false → the child is
            // silently skipped → the Explorer reloads the BillboardGui
            // empty and the rendered billboard has no content. Mirror the
            // StarterGui rule so file-system-first parity holds wherever
            // the user attaches a GUI tree.
            (Self::GuiElement, "Workspace") => true,
            
            // MaterialService: Material definitions + texture images
            (Self::Material | Self::Png | Self::Jpg, "MaterialService") => true,

            // AdornmentService: Adornment definition TOMLs spawn as adornment entities
            (Self::Toml, "AdornmentService") => true,

            // Scripts in any service folder — Rune, Soul (markdown), and Luau
            // all spawn as SoulScript entities wherever the user drops them.
            (Self::Soul | Self::Rune | Self::Lua, _) => true,
            
            // Default: don't spawn
            _ => false,
        }
    }
}

/// Process-wide cache of file *content* (Space-relative key → UTF-8
/// text), populated by [`prewarm_read_cache`] on a rayon thread-pool
/// pass BEFORE the main-thread spawn walk. `src_read_string` consults
/// it first; a hit returns the already-read text and skips the disk /
/// Fjall round-trip entirely.
///
/// Why a `static` + `Mutex`: the read sites are reached through
/// `spawn_directory_entry`/`spawn_file_entry`, which are plain
/// functions (not Bevy systems) called from deep inside the recursion —
/// threading a `&Cache` parameter through every one of their ~18 read
/// call sites and recursive hops would be a large, error-prone diff and
/// would risk perturbing spawn order. A module-private `static` lets the
/// seam consult the cache with a one-line change and keeps every spawn
/// call site byte-for-byte identical. The lock is only ever held for a
/// hash lookup + `String` clone (no I/O under the lock), so it is not a
/// contention point; the parallel work happens in `prewarm_read_cache`
/// where the closures own their results and only the final inserts touch
/// the map.
///
/// Lifecycle: `load_space_files_system` calls `prewarm_read_cache` right
/// after the scan, then clears the cache once the synchronous priority
/// spawn returns (see `clear_read_cache`). Deferred-service frames and
/// hot-reloads run with an empty cache and therefore read live — the
/// cache only ever accelerates the one cold bulk-load it was filled for,
/// and a miss is always a correct live read.
static READ_CACHE: std::sync::Mutex<Option<HashMap<String, String>>> =
    std::sync::Mutex::new(None);

/// Look up `rel` in the pre-warmed [`READ_CACHE`]. Returns the cached
/// text on a hit, `None` on a miss (cache empty/cleared or path not
/// pre-read — both fall through to a live read in `src_read_string`).
fn read_cache_get(rel: &str) -> Option<String> {
    let guard = READ_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().and_then(|m| m.get(rel).cloned())
}

/// Read a file's text through the active [`SpaceSource`], deriving the
/// Space-relative key from the absolute `abs_path` the loader still
/// carries for identity. Falls back to a direct `std::fs` read if the
/// path can't be made relative (defensive — should not happen for
/// in-Space content). This is the single seam every loader read site
/// goes through so Disk vs Fjall is one decision, not 18.
///
/// Before touching the source it consults [`READ_CACHE`]: on a cold
/// bulk-load the rayon pre-warm pass has already read+decoded every
/// `_instance.toml` (and other text) across all cores, so this returns
/// the cached `String` and the spawn walk never blocks on serial
/// small-file I/O. A miss (cache cleared, or this path wasn't
/// pre-warmed) reads live exactly as before — the cache is a pure
/// accelerator and changes nothing about *what* text a call site parses.
fn src_read_string(
    source: &dyn super::space_source::SpaceSource,
    space_root: &Path,
    abs_path: &Path,
) -> std::io::Result<String> {
    match super::space_source::rel_from_root(space_root, abs_path) {
        Some(rel) => {
            if let Some(cached) = read_cache_get(&rel) {
                return Ok(cached);
            }
            source.read_to_string(&rel)
        }
        None => std::fs::read_to_string(abs_path),
    }
}

/// Collect every text-readable node's Space-relative path from an
/// already-scanned [`FileMetadata`] forest into `out`.
///
/// This mirrors exactly which paths the spawn walk will hand to
/// `src_read_string`:
/// - For each FILE node, the file itself (scripts, `.mat.toml`,
///   `.glb.toml`, GUI element TOMLs, flat instance TOMLs …).
/// - For each DIRECTORY node, its `_instance.toml` marker — the
///   directory branches all read `dir_meta.path.join("_instance.toml")`.
///
/// Binary assets (`.glb`, images, audio) are *not* pre-read here: they
/// are never fetched through `src_read_string` (GLTF goes via the
/// `space://` asset source, etc.), so reading them would waste I/O and
/// memory. Over-collecting is harmless for correctness (an unused cache
/// entry is just ignored) but pointless, so we collect precisely the set
/// the text read seam will ask for.
fn collect_readable_rel_paths(
    space_root: &Path,
    nodes: &[FileMetadata],
    out: &mut Vec<String>,
) {
    for node in nodes {
        match node.file_type {
            FileType::Directory => {
                let marker = node.path.join("_instance.toml");
                if let Some(rel) = super::space_source::rel_from_root(space_root, &marker) {
                    out.push(rel);
                }
                collect_readable_rel_paths(space_root, &node.children, out);
            }
            _ => {
                if let Some(rel) = super::space_source::rel_from_root(space_root, &node.path) {
                    out.push(rel);
                }
            }
        }
    }
}

/// Parallel pre-read + UTF-8 decode of every text node in the scanned
/// `entries` forest, filling [`READ_CACHE`] so the subsequent
/// main-thread spawn walk reads from memory instead of hitting disk /
/// Fjall serially.
///
/// This is the single optimisation of this change: the file read + text
/// decode (the dominant cost when opening a 161K-file Space — thousands
/// of tiny `_instance.toml` reads done one at a time) is moved EARLIER
/// and spread across all CPU cores via `rayon`. Spawning, parenting, the
/// `file_to_entity` map, component composition and ordering are all
/// downstream of this and completely untouched — they just find the text
/// already in hand.
///
/// Safety w.r.t. Bevy + rayon: the parallel closure does **no** Bevy
/// `World` / `Commands` / resource access whatsoever. It borrows only
/// `source` (a `&dyn SpaceSource`, which is `Send + Sync`) and a `&str`
/// key, and returns owned `(String, String)` data. All ECS mutation
/// stays on the main thread after this returns. Reads are independent
/// per path, so there is no ordering dependence inside the pool.
///
/// Misses are benign: a path that fails to read or isn't valid UTF-8 is
/// simply omitted from the cache, and `src_read_string` falls through to
/// the original live read (and original error handling) for it. So this
/// can only ever speed loading up, never change its result.
/// Everything the folder-form spawner needs from one `_instance.toml`,
/// computed once: on the worker pool when the pre-parse is ahead of the
/// drain, inline otherwise (same function either way). The raw text is not
/// kept. The class name, UUID and custom-mesh mention are extracted, and the
/// document is held in the one form its spawn branch consumes: the healed
/// typed definition for a Part, the GUI definition for a GUI class, and the
/// plain value for every other class (their branches read raw sections).
pub(crate) struct ParsedInstance {
    pub class_name: eustress_common::classes::ClassName,
    pub uuid: String,
    pub mentions_custom_mesh: bool,
    pub value: Option<toml::Value>,
    pub def: Option<Result<super::instance_loader::InstanceDefinition, String>>,
    pub gui: Option<Result<super::gui_loader::GuiTomlFile, String>>,
}

pub(crate) fn parse_instance_text(text: &str) -> ParsedInstance {
    use eustress_common::classes::ClassName;
    let value = toml::from_str::<toml::Value>(text).ok();
    let meta = value
        .as_ref()
        .and_then(|v| v.get("metadata").or_else(|| v.get("Metadata")));
    let class_name = meta
        .and_then(|m| m.get("class_name").or_else(|| m.get("ClassName")))
        .and_then(|cn| cn.as_str())
        .map(|cn| {
            // Legacy shim: "Script" used to mean the Rune script class
            // before the Soul/Luau split.
            let cn_resolved = if cn == "Script" { "SoulScript" } else { cn };
            ClassName::from_str(cn_resolved).unwrap_or(ClassName::Folder)
        })
        .unwrap_or(ClassName::Folder);
    let uuid = meta
        .and_then(|m| m.get("uuid").or_else(|| m.get("Uuid")))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let mentions_custom_mesh = super::representation::toml_mentions_custom_mesh(text);

    let is_gui = matches!(
        class_name,
        ClassName::ScreenGui
            | ClassName::Frame
            | ClassName::ScrollingFrame
            | ClassName::BillboardGui
            | ClassName::TextLabel
            | ClassName::TextButton
            | ClassName::TextBox
            | ClassName::ImageLabel
            | ClassName::ImageButton
            | ClassName::ViewportFrame
    );
    let (value, def, gui) = if matches!(class_name, ClassName::Part) {
        let def = match value {
            Some(v) => super::instance_loader::load_instance_definition_from_value(v),
            None => Err("unparsable _instance.toml".to_string()),
        };
        (None, Some(def), None)
    } else if is_gui {
        (value, None, Some(super::gui_loader::load_gui_definition_from_str(text)))
    } else {
        (value, None, None)
    };
    ParsedInstance { class_name, uuid, mentions_custom_mesh, value, def, gui }
}

/// `_instance.toml` rel path → parsed record, filled by the pre-parse thread.
/// Entries are TAKEN by the spawner (each instance spawns once), so the
/// cache shrinks as the drain proceeds instead of holding the whole Space
/// until settle.
static PARSED_CACHE: std::sync::Mutex<Option<HashMap<String, ParsedInstance>>> =
    std::sync::Mutex::new(None);

/// Bumped whenever the caches are reset (a new load, a settle); a pre-parse
/// thread that outlives its load stops inserting.
static PREWARM_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn parsed_cache_take(rel: &str) -> Option<ParsedInstance> {
    let mut guard = PARSED_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_mut().and_then(|m| m.remove(rel))
}

fn is_instance_marker(rel: &str) -> bool {
    rel == "_instance.toml" || rel.ends_with("/_instance.toml")
}

/// Read and parse the Space's text files on the rayon pool, in the
/// background, while the main thread spawns. `_instance.toml` files land in
/// [`PARSED_CACHE`] fully parsed (the spawner then does no TOML work at
/// all); every other text file lands in [`READ_CACHE`]. The drain takes
/// whatever is ready and reads plus parses the rest inline, so nothing
/// waits on this thread. The blocking pre-read this replaces held the main
/// thread for 1.2 to 2.8 s on Super Station, and the parse it now also does
/// was ~250 µs per entity on the main thread.
fn prewarm_in_background(
    source: std::sync::Arc<dyn super::space_source::SpaceSource>,
    space_root: &Path,
    entries: &[FileMetadata],
) {
    let mut rel_paths: Vec<String> = Vec::new();
    collect_readable_rel_paths(space_root, entries, &mut rel_paths);
    if rel_paths.is_empty() {
        return;
    }
    let total = rel_paths.len();
    let gen = PREWARM_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    {
        let mut guard = READ_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(HashMap::new());
    }
    {
        let mut guard = PARSED_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(HashMap::new());
    }

    let spawned = std::thread::Builder::new()
        .name("eustress-pre-parse".into())
        .spawn(move || {
            use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
            use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
            let t0 = std::time::Instant::now();
            let parsed_n = AtomicUsize::new(0);
            let text_n = AtomicUsize::new(0);
            // Batches keep the lock traffic to one acquisition per ~512
            // files, and let the drain see files in flights of hundreds
            // rather than all at the end.
            let chunks: Vec<&[String]> = rel_paths.chunks(512).collect();
            chunks.par_iter().for_each(|chunk| {
                if PREWARM_GEN.load(Relaxed) != gen {
                    return;
                }
                let mut parsed_batch: Vec<(String, ParsedInstance)> = Vec::new();
                let mut text_batch: Vec<(String, String)> = Vec::new();
                for rel in chunk.iter() {
                    let Ok(text) = source.read_to_string(rel) else { continue };
                    if is_instance_marker(rel) {
                        parsed_batch.push((rel.clone(), parse_instance_text(&text)));
                    } else {
                        text_batch.push((rel.clone(), text));
                    }
                }
                if PREWARM_GEN.load(Relaxed) != gen {
                    return;
                }
                parsed_n.fetch_add(parsed_batch.len(), Relaxed);
                text_n.fetch_add(text_batch.len(), Relaxed);
                {
                    let mut guard = PARSED_CACHE.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(m) = guard.as_mut() {
                        m.extend(parsed_batch);
                    }
                }
                {
                    let mut guard = READ_CACHE.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(m) = guard.as_mut() {
                        m.extend(text_batch);
                    }
                }
            });
            info!(
                target: "eustress_engine::world_db",
                "⚡ Pre-parse: {} instance files parsed + {} text files read of {} on the rayon pool in {:?} (background; the drain takes what is ready and parses the rest inline)",
                parsed_n.load(Relaxed), text_n.load(Relaxed), total, t0.elapsed()
            );
        });
    if let Err(e) = spawned {
        warn!(
            target: "eustress_engine::world_db",
            error = %e,
            "pre-parse thread failed to spawn — the drain reads and parses inline"
        );
    }
}

/// Drop both load caches and retire any pre-parse thread still filling
/// them. Called at settle; from then on a cache miss is a live read.
fn clear_read_cache() {
    PREWARM_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    {
        let mut guard = READ_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }
    let mut guard = PARSED_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

/// The cached GUI definition for this instance, or the read plus parse the
/// branch did before (a cache miss: the priority spawn ahead of the
/// pre-parse, or a hot reload after settle).
fn take_gui_def(
    parsed: &mut Option<ParsedInstance>,
    source: &dyn super::space_source::SpaceSource,
    space_root: &Path,
    instance_toml: &Path,
) -> Result<super::gui_loader::GuiTomlFile, String> {
    if let Some(g) = parsed.as_mut().and_then(|p| p.gui.take()) {
        return g;
    }
    src_read_string(source, space_root, instance_toml)
        .map_err(|e| e.to_string())
        .and_then(|c| super::gui_loader::load_gui_definition_from_str(&c))
}

/// The cached typed definition (Parts), or the read plus heal plus parse.
fn take_instance_def(
    parsed: &mut Option<ParsedInstance>,
    source: &dyn super::space_source::SpaceSource,
    space_root: &Path,
    instance_toml: &Path,
) -> Result<super::instance_loader::InstanceDefinition, String> {
    if let Some(d) = parsed.as_mut().and_then(|p| p.def.take()) {
        return d;
    }
    src_read_string(source, space_root, instance_toml)
        .map_err(|e| e.to_string())
        .and_then(|c| super::instance_loader::load_instance_definition_from_str(&c))
}

/// The cached raw value (classes whose branch reads sections directly), or
/// the read plus parse.
fn take_instance_value(
    parsed: &Option<ParsedInstance>,
    source: &dyn super::space_source::SpaceSource,
    space_root: &Path,
    instance_toml: &Path,
) -> Option<toml::Value> {
    if let Some(v) = parsed.as_ref().and_then(|p| p.value.clone()) {
        return Some(v);
    }
    src_read_string(source, space_root, instance_toml)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
}

/// Metadata extracted from a file or directory
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub path: PathBuf,
    pub file_type: FileType,
    pub service: String,
    pub name: String,
    pub size: u64,
    pub modified: std::time::SystemTime,
    /// For Directory entries: the child file entries inside this directory
    pub children: Vec<FileMetadata>,
}

/// Resource tracking all loaded files in the current Space
#[derive(Resource, Default)]
pub struct SpaceFileRegistry {
    /// Map: file path → entity spawned from that file
    pub file_to_entity: HashMap<PathBuf, Entity>,

    /// Map: entity → file path it was loaded from
    pub entity_to_file: HashMap<Entity, PathBuf>,

    /// Map: file path → metadata
    pub file_metadata: HashMap<PathBuf, FileMetadata>,

    /// Files that failed to load (with error message)
    pub failed_files: HashMap<PathBuf, String>,

    /// Paths currently being renamed — file watcher ignores delete events for these.
    /// Cleared after the rename completes.
    pub rename_in_progress: std::collections::HashSet<PathBuf>,
}

impl SpaceFileRegistry {
    /// Register a file and its spawned entity
    pub fn register(&mut self, path: PathBuf, entity: Entity, metadata: FileMetadata) {
        self.file_to_entity.insert(path.clone(), entity);
        self.entity_to_file.insert(entity, path.clone());
        self.file_metadata.insert(path, metadata);
    }
    
    /// Clear all registry entries (used when switching spaces).
    pub fn clear(&mut self) {
        self.file_to_entity.clear();
        self.entity_to_file.clear();
        self.file_metadata.clear();
        self.failed_files.clear();
        self.rename_in_progress.clear();
    }

    /// Unregister a file (when deleted or entity despawned)
    pub fn unregister_file(&mut self, path: &Path) {
        if let Some(entity) = self.file_to_entity.remove(path) {
            self.entity_to_file.remove(&entity);
        }
        self.file_metadata.remove(path);
        self.failed_files.remove(path);
    }
    
    /// Unregister an entity
    pub fn unregister_entity(&mut self, entity: Entity) {
        if let Some(path) = self.entity_to_file.remove(&entity) {
            self.file_to_entity.remove(&path);
        }
    }
    
    /// Get entity for a file path
    pub fn get_entity(&self, path: &Path) -> Option<Entity> {
        self.file_to_entity.get(path).copied()
    }
    
    /// Get file path for an entity
    pub fn get_file(&self, entity: Entity) -> Option<&PathBuf> {
        self.entity_to_file.get(&entity)
    }
    
    /// Rename a file in the registry — updates all maps to point to the new path.
    /// Returns Ok(()) on success, Err with message on failure.
    pub fn rename_file(&mut self, old_path: &Path, new_path: PathBuf) -> Result<(), String> {
        let entity = self.file_to_entity.remove(old_path)
            .ok_or_else(|| format!("No entity registered for {:?}", old_path))?;

        // Update all maps
        self.file_to_entity.insert(new_path.clone(), entity);
        self.entity_to_file.insert(entity, new_path.clone());

        if let Some(mut meta) = self.file_metadata.remove(old_path) {
            meta.path = new_path.clone();
            self.file_metadata.insert(new_path, meta);
        }

        Ok(())
    }

    /// Check if a file is loaded
    pub fn is_loaded(&self, path: &Path) -> bool {
        self.file_to_entity.contains_key(path)
    }

    /// Collect every (file_path, entity) entry whose path is a
    /// descendant of `ancestor`. Used by the delete handler when a
    /// parent folder gets renamed to trash so all descendant entities
    /// can be despawned together AND added to `rename_in_progress` —
    /// otherwise a child's still-pending save-on-Changed flushes after
    /// the parent rename, fails (parent dir gone), and can leave the
    /// engine and disk state subtly inconsistent. Match is strict
    /// `starts_with` on path components; the ancestor itself is
    /// included.
    pub fn descendants_of(&self, ancestor: &Path) -> Vec<(PathBuf, Entity)> {
        self.file_to_entity.iter()
            .filter(|(p, _)| p.starts_with(ancestor))
            .map(|(p, e)| (p.clone(), *e))
            .collect()
    }
}

/// Component marking an entity as loaded from a file
#[derive(Component, Debug, Clone)]
pub struct LoadedFromFile {
    pub path: PathBuf,
    pub file_type: FileType,
    pub service: String,
}

/// Scan a single directory level, returning file entries and Directory entries
/// (each Directory entry carries its own children recursively).
/// Recursively scan a directory's entries via [`SpaceSource`] rather
/// than `std::fs`. `rel_dir` is the Space-relative forward-slash path
/// of the directory being scanned; `space_root` reconstitutes the
/// absolute `FileMetadata.path` so the registry / file-watcher / undo
/// (all keyed on absolute paths) keep working unchanged — only the
/// *content + listing* moves to the source (Disk or Fjall).
fn scan_dir_entries(
    source: &dyn super::space_source::SpaceSource,
    space_root: &Path,
    rel_dir: &str,
    service: &str,
) -> Vec<FileMetadata> {
    let mut entries: Vec<FileMetadata> = Vec::new();
    let Ok(listing) = source.list(rel_dir) else { return entries };

    for ent in listing {
        let name = ent.name.clone();
        let rel = ent.rel_path.clone();
        // Absolute path kept for registry/watcher/undo identity.
        let path = {
            let mut p = space_root.to_path_buf();
            for seg in rel.split('/') {
                if !seg.is_empty() {
                    p.push(seg);
                }
            }
            p
        };

        if ent.is_dir {
            // Skip hidden/system directories (.eustress, .git, node_modules, target, trash)
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == "trash" {
                continue;
            }
            // EEP reserved names — `_instance.toml` and `_service.toml`
            // are MARKER FILE names, never valid as directory names.
            if name == "_instance.toml" || name == "_service.toml" {
                continue;
            }
            let children = scan_dir_entries(source, space_root, &rel, service);
            entries.push(FileMetadata {
                path,
                file_type: FileType::Directory,
                service: service.to_string(),
                name,
                size: 0,
                modified: std::time::SystemTime::UNIX_EPOCH,
                children,
            });
        } else {
            // Skip EEP marker files — these define folder types, not entities
            if name == "_instance.toml" || name == "_service.toml" {
                continue;
            }

            let Some(file_type) = FileType::from_path(&path) else { continue };
            // `FileMetadata.size` has ZERO downstream readers (verified by
            // grep: every site constructs the struct, none reads `.size`;
            // spawn/load/save/registry all key on path/name/file_type/
            // service). The old code did `source.read(&rel)` here purely to
            // measure a byte length that was never consumed — a SECOND full
            // read of every eager-service file on top of the parallel
            // `prewarm_read_cache` read. On the 161K-file Vehicle Simulator
            // place that duplicate serial read cost ~2-3s of scan time for
            // nothing. Drop the probe; record 0. (The source trait exposes
            // no metadata-only stat, and the value is cosmetic anyway.)
            let size = 0u64;
            // `Path::file_stem()` only strips the LAST extension component,
            // so a compound-extension instance file like `Sky.instance.toml`
            // stems to `Sky.instance`, not `Sky` — leaving `Sky.instance` as
            // this entry's `name` while a sibling `Sky/` folder entry (see
            // the `is_dir` branch above) is named plain `Sky`. That mismatch
            // is exactly what let the folder-vs-flat de-dup pass below
            // silently no-op (`"Sky.instance" != "Sky"`, so it never saw
            // them as the same instance). Strip each known compound suffix
            // explicitly so `name` matches the folder-form convention.
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown");
            let name = ["instance.toml", "part.toml", "glb.toml", "model.toml"]
                .iter()
                .find_map(|suffix| file_name.strip_suffix(suffix)?.strip_suffix('.'))
                .unwrap_or_else(|| {
                    path.file_stem().and_then(|n| n.to_str()).unwrap_or("Unknown")
                })
                .to_string();
            entries.push(FileMetadata {
                path,
                file_type,
                service: service.to_string(),
                name,
                size,
                modified: std::time::SystemTime::UNIX_EPOCH,
                children: Vec::new(),
            });
        }
    }

    // De-dup folder-form vs. flat-form: a `<Name>/` directory (holding
    // `_instance.toml`) and a sibling flat `<Name>.instance.toml` file are
    // two on-disk conventions for the SAME logical instance, not two
    // instances — an incomplete migration from the old flat form to the
    // newer folder form can leave both on disk (observed: Lighting/Sky/
    // + Lighting/Sky.instance.toml both present), and without this pass
    // every such instance silently double-spawns: two live entities, two
    // Sky/Atmosphere/etc. components each contributing to rendering, and
    // roughly double the per-frame system cost. The folder form wins
    // (it's the current EEP-compliant convention); the flat duplicate is
    // dropped from THIS SCAN only — the stale file itself is left alone
    // (not deleted) since cleaning it up is a separate, deliberate step.
    // Owned `String`s, not `&str` borrows into `entries` — `retain` below
    // needs `&mut entries`, which can't coexist with a live immutable
    // borrow still referencing its contents.
    let dir_names: std::collections::HashSet<String> = entries.iter()
        .filter(|e| e.file_type == FileType::Directory)
        .map(|e| e.name.clone())
        .collect();
    if !dir_names.is_empty() {
        entries.retain(|e| {
            e.file_type == FileType::Directory || !dir_names.contains(e.name.as_str())
        });
    }

    entries
}

/// Scan a Space directory and discover all loadable files and subdirectories.
/// Returns service directories as Directory entries with their children inline.
/// One node of the in-memory index built by [`build_tree_index`]: children
/// keyed by leaf name in name order, which is the order `list_dir` returns.
#[derive(Default)]
struct TreeIndexNode {
    children: std::collections::BTreeMap<String, TreeIndexNode>,
}

/// Which marker files a directory holds, recorded during the index scan so
/// the spawner can answer "is this a service / terrain export / instance"
/// without probing the source. On a Fjall source each probe is a point
/// lookup plus a prefix seek; three per directory came to ~1 ms per entity
/// in a debug build, most of the un-timed part of every drain frame.
pub(crate) mod dir_markers {
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub const INSTANCE: u8 = 1;
    pub const SERVICE: u8 = 2;
    pub const TERRAIN: u8 = 4;

    /// Space-relative directory path → marker bits. Only directories that
    /// hold at least one marker are listed, so an absent entry means
    /// "unknown, probe" rather than "no markers": a directory created while
    /// the load is still streaming (hot-create) is never misread.
    static TABLE: Mutex<Option<HashMap<String, u8>>> = Mutex::new(None);

    pub fn install(table: HashMap<String, u8>) {
        *TABLE.lock().unwrap_or_else(|e| e.into_inner()) = Some(table);
    }

    /// Dropped at settle: it is a load-time accelerator, not state.
    pub fn clear() {
        *TABLE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn get(rel_dir: &str) -> Option<u8> {
        TABLE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|t| t.get(rel_dir).copied())
    }
}

/// The whole Space's directory index from ONE ordered pass over the tree
/// partition's keys. `FjallSource::list` answers each directory with a
/// prefix scan of that directory's ENTIRE subtree, so the recursive
/// [`scan_dir_entries`] walk re-visited every key once per ancestor level:
/// 27 s on Super Station's 113K keys. This pass is O(keys). The same pass
/// fills the [`dir_markers`] table.
fn build_tree_index(db: &dyn eustress_worlddb::WorldDb) -> Option<TreeIndexNode> {
    let t0 = std::time::Instant::now();
    let mut root = TreeIndexNode::default();
    let mut markers: std::collections::HashMap<String, u8> = std::collections::HashMap::new();
    let mut keys = 0usize;
    for key in db.iter_tree_keys().ok()? {
        let Ok(key) = key else { continue };
        keys += 1;
        let mut node = &mut root;
        for part in key.split('/').filter(|s| !s.is_empty()) {
            node = node.children.entry(part.to_string()).or_default();
        }
        if let Some((parent, leaf)) = key.rsplit_once('/') {
            let bit = match leaf {
                "_instance.toml" => dir_markers::INSTANCE,
                "_service.toml" => dir_markers::SERVICE,
                "_terrain.toml" => dir_markers::TERRAIN,
                _ => 0,
            };
            if bit != 0 {
                *markers.entry(parent.to_string()).or_insert(0) |= bit;
            }
        }
    }
    info!(
        target: "eustress_engine::world_db",
        "🗂 Tree index: {} keys in one pass ({:?}), {} marked directories",
        keys,
        t0.elapsed(),
        markers.len()
    );
    dir_markers::install(markers);
    Some(root)
}

/// [`scan_dir_entries`] over the in-memory index instead of the source:
/// the same filtering, naming and folder-vs-flat de-dup, with no I/O.
fn entries_from_index(
    node: &TreeIndexNode,
    space_root: &Path,
    rel_dir: &str,
    service: &str,
) -> Vec<FileMetadata> {
    let mut entries: Vec<FileMetadata> = Vec::new();
    for (name, child) in &node.children {
        let rel = if rel_dir.is_empty() {
            name.clone()
        } else {
            format!("{rel_dir}/{name}")
        };
        let path = {
            let mut p = space_root.to_path_buf();
            for seg in rel.split('/') {
                if !seg.is_empty() {
                    p.push(seg);
                }
            }
            p
        };
        if name == "_instance.toml" || name == "_service.toml" {
            continue;
        }
        // A key never names a directory on its own; a name with keys below
        // it is a directory, a leaf key is a file (as `list_dir` infers).
        if !child.children.is_empty() {
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == "trash" {
                continue;
            }
            let children = entries_from_index(child, space_root, &rel, service);
            entries.push(FileMetadata {
                path,
                file_type: FileType::Directory,
                service: service.to_string(),
                name: name.clone(),
                size: 0,
                modified: std::time::SystemTime::UNIX_EPOCH,
                children,
            });
        } else {
            let Some(file_type) = FileType::from_path(&path) else { continue };
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown");
            let name = ["instance.toml", "part.toml", "glb.toml", "model.toml"]
                .iter()
                .find_map(|suffix| file_name.strip_suffix(suffix)?.strip_suffix('.'))
                .unwrap_or_else(|| {
                    path.file_stem().and_then(|n| n.to_str()).unwrap_or("Unknown")
                })
                .to_string();
            entries.push(FileMetadata {
                path,
                file_type,
                service: service.to_string(),
                name,
                size: 0,
                modified: std::time::SystemTime::UNIX_EPOCH,
                children: Vec::new(),
            });
        }
    }

    let dir_names: std::collections::HashSet<String> = entries
        .iter()
        .filter(|e| e.file_type == FileType::Directory)
        .map(|e| e.name.clone())
        .collect();
    if !dir_names.is_empty() {
        entries.retain(|e| e.file_type == FileType::Directory || !dir_names.contains(e.name.as_str()));
    }

    entries
}

///
/// Services are discovered from the filesystem by looking for directories
/// containing `_service.toml` marker files (EEP-compliant, no hardcoding).
pub fn scan_space_directory(
    source: &dyn super::space_source::SpaceSource,
    space_root: &Path,
) -> Vec<FileMetadata> {
    let mut entries = Vec::new();

    // DB-backed Space: index the whole tree once, then answer every
    // directory from memory. Disk Space: the per-directory walk is already
    // O(entries) and stays as it was.
    let index = if source.is_fjall() {
        super::active_db::db_arc().and_then(|db| build_tree_index(db.as_ref()))
    } else {
        None
    };

    // EEP-compliant service discovery: from the index root when there is
    // one (its marker table answers `_service.toml`; the source-side probe
    // was a second full scan of every key), otherwise via the active source
    // (Disk or Fjall) — no `std::fs` either way, so a migrated world
    // reconstructs the full service tree straight from the DB.
    let services = match &index {
        Some(root) => services_from_index(root),
        None => discover_services(source),
    };

    // LAZY NON-WORKSPACE LOAD (large streaming Spaces only). The non-rendered
    // storage services hold the bulk of a big imported place — Vehicle Simulator
    // has ~161K instances under ReplicatedStorage (race courses, plane kits)
    // alone. Spawning them as live ECS bloats every O(N) system AND the render
    // extract/visibility pipeline even though they are never drawn. When the DB
    // Space is in streaming mode, emit each non-essential service's HEADER (so it
    // still appears in the Explorer) but do NOT scan/spawn its subtree.
    // Workspace (rendered geometry — bare parts are already streamed by the
    // residency manager), Lighting, and StarterGui stay eager.
    // NOTE: this is the load-skip stage; lazy materialize-on-expand /
    // play-mode-materialize is the follow-up so functionality is preserved.
    //
    // Big-space test computed INLINE here. We must NOT use `streaming_active()`:
    // that flag is seeded by the residency boot-load decision, which runs minutes
    // AFTER this service scan — so it reads false here and the lazy skip would
    // never fire (observed: 223K entities still loaded). Mirror the file-loader's
    // own STREAM_DB_PARTS condition instead — an active DB with more than the
    // big-space threshold of binary cores — which IS already true at scan time
    // (the DB is opened at boot, before this runs).
    let streaming = super::active_db::is_active()
        && super::active_db::count_instance_cores_capped(BIG_SPACE_THRESHOLD + 1) > BIG_SPACE_THRESHOLD;
    // Indexed (DB-backed) Space: the same walk the open worker does ahead of
    // time in `prescan_tree`; this is the path for a Space switch or a disk
    // fallback where no pre-scan was handed over.
    if let Some(root) = &index {
        return scan_from_index(root, space_root, streaming);
    }
    const EAGER_SERVICES: &[&str] = &["Workspace", "Lighting", "StarterGui"];

    for service_name in &services {
        if !source.exists(service_name) {
            continue;
        }
        let service_path = space_root.join(service_name);
        let lazy = streaming && !EAGER_SERVICES.contains(&service_name.as_str());
        let children = if lazy {
            Vec::new()
        } else {
            scan_dir_entries(source, space_root, service_name, service_name)
        };
        if lazy {
            info!(
                "⏬ Lazy service (streaming): '{}' header only — subtree not spawned (saves O(N) + render load)",
                service_name
            );
        }
        entries.push(FileMetadata {
            path: service_path,
            file_type: FileType::Directory,
            service: service_name.clone(),
            name: service_name.clone(),
            size: 0,
            modified: std::time::SystemTime::UNIX_EPOCH,
            children,
        });
    }

    // Synthesize header-only entries for canonical services the source does not
    // list yet. A Space migrated to the DB *before* a service existed (e.g.
    // DataService, added with the Data Platform) has no record for it, and
    // `ensure_space_integrity` deliberately skips migrated worlds — so without
    // this the service would never appear in the Explorer. Header-only: the
    // backing dir / DB record is created lazily when the user adds the first
    // child. Disk-based Spaces already list every service via discover_services,
    // so this only fires for the genuinely-missing case.
    for &canonical in KNOWN_SERVICE_NAMES {
        if entries.iter().any(|e| e.name == canonical) {
            continue;
        }
        entries.push(FileMetadata {
            path: space_root.join(canonical),
            file_type: FileType::Directory,
            service: canonical.to_string(),
            name: canonical.to_string(),
            size: 0,
            modified: std::time::SystemTime::UNIX_EPOCH,
            children: Vec::new(),
        });
    }

    entries
}

/// Discover services by scanning for directories containing `_service.toml` marker files.
/// This is EEP-compliant: services are defined by filesystem structure, not hardcoded.
/// Well-known service directory names — auto-discovered even without _service.toml marker.
const KNOWN_SERVICE_NAMES: &[&str] = &[
    "Workspace", "Lighting", "StarterGui", "SoulService", "MaterialService",
    "AdornmentService", "DataService",
    "Players", "StarterPack", "StarterPlayer", "ReplicatedStorage",
    "ServerStorage", "ServerScriptService", "SoundService", "Teams", "Chat",
];

/// The entry tree the loader spawns from, built ahead of time on the open
/// worker (see [`prescan_tree`]) so the main thread's first frame does not
/// pay the 2.3 to 2.6 s index build and conversion.
pub struct PreScan {
    /// The Space it was built for; a switch mid-open leaves it unused.
    pub root: PathBuf,
    pub entries: Vec<FileMetadata>,
}

/// Handed from the open worker to `load_space_files_system`.
#[derive(Resource, Default)]
pub struct PendingPreScan(pub Option<PreScan>);

/// Build the entry tree for a DB-backed Space off the main thread: the
/// tree index (which also fills the [`dir_markers`] table), the services,
/// and the per-service entries. `cores` is the capped instance-core count
/// the worker already took, which decides the streaming lazy-service rule
/// exactly as [`scan_space_directory`] does with the installed DB.
pub fn prescan_tree(
    db: &dyn eustress_worlddb::WorldDb,
    space_root: &Path,
    cores: usize,
) -> Option<PreScan> {
    let t0 = std::time::Instant::now();
    let root = build_tree_index(db)?;
    let streaming = cores > BIG_SPACE_THRESHOLD;
    let entries = scan_from_index(&root, space_root, streaming);
    info!(
        target: "eustress_engine::world_db",
        "🗂 Pre-scan on the open worker: {} top-level entries in {:?}",
        entries.len(),
        t0.elapsed()
    );
    Some(PreScan { root: space_root.to_path_buf(), entries })
}

/// Mirrors `ResidencyConfig::big_space_threshold`; the streaming decision.
const BIG_SPACE_THRESHOLD: usize = 100_000;

/// The scan over an in-memory index: services from the index root, each
/// eager service's subtree converted, lazy services as headers, and the
/// well-known services filled in as empty headers.
fn scan_from_index(root: &TreeIndexNode, space_root: &Path, streaming: bool) -> Vec<FileMetadata> {
    const EAGER_SERVICES: &[&str] = &["Workspace", "Lighting", "StarterGui"];
    let mut entries: Vec<FileMetadata> = Vec::new();
    for service_name in services_from_index(root) {
        let Some(node) = root.children.get(service_name.as_str()) else { continue };
        let lazy = streaming && !EAGER_SERVICES.contains(&service_name.as_str());
        let children = if lazy {
            info!(
                "⏬ Lazy service (streaming): '{}' header only — subtree not spawned (saves O(N) + render load)",
                service_name
            );
            Vec::new()
        } else {
            entries_from_index(node, space_root, &service_name, &service_name)
        };
        entries.push(FileMetadata {
            path: space_root.join(&service_name),
            file_type: FileType::Directory,
            service: service_name.clone(),
            name: service_name,
            size: 0,
            modified: std::time::SystemTime::UNIX_EPOCH,
            children,
        });
    }
    for &canonical in KNOWN_SERVICE_NAMES {
        if entries.iter().any(|e| e.name == canonical) {
            continue;
        }
        entries.push(FileMetadata {
            path: space_root.join(canonical),
            file_type: FileType::Directory,
            service: canonical.to_string(),
            name: canonical.to_string(),
            size: 0,
            modified: std::time::SystemTime::UNIX_EPOCH,
            children: Vec::new(),
        });
    }
    entries
}

/// [`discover_services`] answered from the tree index: a top-level directory
/// with a `_service.toml` marker, or a well-known service name.
fn services_from_index(root: &TreeIndexNode) -> Vec<String> {
    let mut services: Vec<String> = root
        .children
        .iter()
        .filter(|(_, node)| !node.children.is_empty())
        .filter(|(name, _)| {
            dir_markers::get(name).map_or(false, |m| m & dir_markers::SERVICE != 0)
                || KNOWN_SERVICE_NAMES.contains(&name.as_str())
        })
        .map(|(name, _)| name.clone())
        .collect();
    services.sort_by(|a, b| {
        if a == "Workspace" { std::cmp::Ordering::Less }
        else if b == "Workspace" { std::cmp::Ordering::Greater }
        else { a.cmp(b) }
    });
    info!("📁 Discovered {} services from the tree index: {:?}", services.len(), services);
    services
}

fn discover_services(source: &dyn super::space_source::SpaceSource) -> Vec<String> {
    let mut services = Vec::new();

    let entries = match source.list("") {
        Ok(entries) => entries,
        Err(e) => {
            warn!("Failed to list Space root via source: {}", e);
            return services;
        }
    };

    for ent in entries {
        if !ent.is_dir { continue; }
        let name = ent.name.as_str();

        // Discover via _service.toml marker OR well-known service name
        let service_marker = format!("{}/_service.toml", ent.rel_path);
        if source.exists(&service_marker) || KNOWN_SERVICE_NAMES.contains(&name) {
            services.push(name.to_string());
            debug!("Discovered service: {}", name);
        }
    }

    // Sort for deterministic order (Workspace first for consistency)
    services.sort_by(|a, b| {
        if a == "Workspace" { std::cmp::Ordering::Less }
        else if b == "Workspace" { std::cmp::Ordering::Greater }
        else { a.cmp(b) }
    });

    info!("📁 Discovered {} services from filesystem: {:?}", services.len(), services);
    services
}

/// Spawn a single file entry as an ECS entity, optionally parented to `parent_entity`.
/// Returns the spawned entity if one was created.
pub fn spawn_file_entry(
    commands: &mut Commands,
    asset_server: &Res<AssetServer>,
    _meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<SpaceFileRegistry>,
    material_registry: &mut ResMut<super::material_loader::MaterialRegistry>,
    mesh_cache: &mut ResMut<super::instance_loader::PrimitiveMeshCache>,
    decal_materials: &mut ResMut<Assets<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>>,
    space_path: &Path,
    file_meta: &FileMetadata,
    parent_entity: Option<Entity>,
    class_defaults: Option<&super::class_defaults::ClassDefaultsRegistry>,
    source: &dyn super::space_source::SpaceSource,
) -> Option<Entity> {
    // Skip if already loaded
    if registry.is_loaded(&file_meta.path) {
        return None;
    }

    // Skip files inside 'meshes' folders - these are raw assets, not parts to spawn
    // They are referenced by .glb.toml or .part.toml instance files instead
    if file_meta.path.components().any(|c| c.as_os_str() == "meshes") {
        debug!("Skipping {:?} (inside meshes folder - raw asset)", file_meta.path);
        return None;
    }

    // Check if this file type should spawn an entity in this service
    if !file_meta.file_type.spawns_entity_in_service(&file_meta.service) {
        debug!("Skipping {:?} in {} (doesn't spawn entity)", file_meta.path, file_meta.service);
        return None;
    }

    // Skip _instance.toml marker files — they define the parent folder type,
    // not entities to render in the Explorer tree
    let is_instance_marker = file_meta.path.file_name()
        .map(|n| n.to_string_lossy() == "_instance.toml")
        .unwrap_or(false);
    if is_instance_marker {
        debug!("Skipping {:?} (folder container marker, not an entity)", file_meta.path);
        return None;
    }

    let entity = match file_meta.file_type {
        FileType::Toml => {
            // Check if this is a _service.toml file (service marker)
            let is_service = file_meta.path.file_name()
                .map(|n| n.to_string_lossy().ends_with("_service.toml"))
                .unwrap_or(false);
            
            if is_service {
                // Load as service entity — sourced through SpaceSource
                // (Fjall when migrated) then parsed in memory.
                match src_read_string(source, space_path, &file_meta.path)
                    .map_err(|e| e.to_string())
                    .and_then(|c| super::service_loader::load_service_definition_from_str(&c))
                {
                    Ok(service_def) => {
                        let e = super::service_loader::spawn_service(
                            commands,
                            file_meta.path.clone(),
                            service_def,
                        );
                        registry.register(file_meta.path.clone(), e, file_meta.clone());
                        e
                    }
                    Err(err) => {
                        error!("Failed to load service file {:?}: {}", file_meta.path, err);
                        return None;
                    }
                }
            } else {
                // Load as instance entity — sourced through SpaceSource
                // (Fjall when migrated, Disk otherwise) then healed/
                // parsed in memory. This is the flat-file twin of the
                // Part-folder branch: no `std::fs`, and crucially no
                // schema self-heal write-back (the in-memory
                // `*_from_str` path never rewrites the file), so a
                // migrated/DB-authoritative world spawns instances with
                // zero disk reads and zero loose-file resurrection.
                let _ = class_defaults; // schema is the common-crate source of truth now
                // Stage timers: this is the ~4 ms/entity path that has been
                // undiagnosed since August. See `toml_spawn_cost`.
                let t_read = std::time::Instant::now();
                let parsed = src_read_string(source, space_path, &file_meta.path)
                    .map_err(|e| e.to_string())
                    .and_then(|c| super::instance_loader::load_instance_definition_from_str(&c));
                toml_spawn_cost::add(&toml_spawn_cost::READ_PARSE_NS, t_read.elapsed());
                match parsed {
                    Ok(instance) => {
                        let t_spawn = std::time::Instant::now();
                        let e = super::instance_loader::spawn_instance(
                            commands,
                            asset_server,
                            materials,
                            material_registry,
                            mesh_cache,
                            decal_materials,
                            file_meta.path.clone(),
                            instance,
                        );
                        toml_spawn_cost::add(&toml_spawn_cost::SPAWN_NS, t_spawn.elapsed());
                        let t_reg = std::time::Instant::now();
                        // Attach LoadedFromFile so the Explorer can classify this
                        // entity by service (Workspace, Lighting, etc.)
                        commands.entity(e).insert(LoadedFromFile {
                            path: file_meta.path.clone(),
                            file_type: file_meta.file_type,
                            service: file_meta.service.clone(),
                        });
                        registry.register(file_meta.path.clone(), e, file_meta.clone());
                        toml_spawn_cost::add(&toml_spawn_cost::REGISTER_NS, t_reg.elapsed());
                        toml_spawn_cost::COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        e
                    }
                    Err(err) => {
                        error!("Failed to load instance file {:?}: {}", file_meta.path, err);
                        return None;
                    }
                }
            }
        }

        FileType::Gltf => {
            // Check for Draco compression before loading
            if super::draco_decoder::is_draco_compressed(&file_meta.path) {
                super::draco_decoder::warn_draco_file(&file_meta.path);
                return None; // Skip loading Draco-compressed files
            }
            
            // Use space:// asset source for GLB files in the Space directory.
            // Live root (follows runtime space/universe switches) — must match
            // what the dynamic space:// reader joins, NOT the stale launch default.
            let space_root = super::space_asset_source::space_asset_root();
            let relative_path = file_meta.path
                .strip_prefix(&space_root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| file_meta.path.to_string_lossy().replace('\\', "/"));
            let asset_path = format!("space://{}#Scene0", relative_path);
            info!("🔧 Loading GLTF: {} (from {:?})", asset_path, file_meta.path);
            let scene_handle = asset_server.load(asset_path);
            let e = commands.spawn((
                WorldAssetRoot(scene_handle),
                Transform::default(),
                Visibility::default(),
                eustress_common::classes::Instance {
                    name: file_meta.name.clone(),
                    class_name: eustress_common::classes::ClassName::Part,
                    archivable: true,
                    id: 0,
                    ai: false,
                uuid: String::new(),
                },
                eustress_common::default_scene::PartEntityMarker {
                    part_id: file_meta.name.clone(),
                },
                LoadedFromFile {
                    path: file_meta.path.clone(),
                    file_type: file_meta.file_type,
                    service: file_meta.service.clone(),
                },
                Name::new(file_meta.name.clone()),
            )).id();
            registry.register(file_meta.path.clone(), e, file_meta.clone());
            info!("✅ Loaded {} from {:?}", file_meta.name, file_meta.path);
            e
        }

        FileType::Soul => {
            match src_read_string(source, space_path, &file_meta.path) {
                Ok(markdown_source) => {
                    let e = commands.spawn((
                        eustress_common::classes::Instance {
                            name: file_meta.name.clone(),
                            class_name: eustress_common::classes::ClassName::SoulScript,
                            archivable: true,
                            id: 0,
                            ai: false,
                uuid: String::new(),
                        },
                        crate::soul::SoulScriptData {
                            source: markdown_source,
                            dirty: false,
                            ast: None,
                            generated_code: None,
                            build_status: crate::soul::SoulBuildStatus::NotBuilt,
                            errors: Vec::new(),
                            run_context: Default::default(),
                        },
                        LoadedFromFile {
                            path: file_meta.path.clone(),
                            file_type: file_meta.file_type,
                            service: file_meta.service.clone(),
                        },
                        Name::new(file_meta.name.clone()),
                    )).id();
                    registry.register(file_meta.path.clone(), e, file_meta.clone());
                    info!("📜 Loaded Soul script {} from {:?}", file_meta.name, file_meta.path);
                    e
                }
                Err(err) => {
                    error!("❌ Failed to read Soul script {:?}: {}", file_meta.path, err);
                    return None;
                }
            }
        }

        FileType::Rune => {
            // Load .rune files as SoulScript entities (Rune is the scripting language)
            match src_read_string(source, space_path, &file_meta.path) {
                Ok(rune_source) => {
                    let e = commands.spawn((
                        eustress_common::classes::Instance {
                            name: file_meta.name.clone(),
                            class_name: eustress_common::classes::ClassName::SoulScript,
                            archivable: true,
                            id: 0,
                            ai: false,
                uuid: String::new(),
                        },
                        crate::soul::SoulScriptData {
                            source: rune_source,
                            dirty: false,
                            ast: None,
                            generated_code: None,
                            build_status: crate::soul::SoulBuildStatus::NotBuilt,
                            errors: Vec::new(),
                            run_context: Default::default(),
                        },
                        LoadedFromFile {
                            path: file_meta.path.clone(),
                            file_type: file_meta.file_type,
                            service: file_meta.service.clone(),
                        },
                        Name::new(file_meta.name.clone()),
                    )).id();
                    registry.register(file_meta.path.clone(), e, file_meta.clone());
                    info!("📜 Loaded Rune script {} from {:?}", file_meta.name, file_meta.path);
                    e
                }
                Err(err) => {
                    error!("❌ Failed to read Rune script {:?}: {}", file_meta.path, err);
                    return None;
                }
            }
        }

        FileType::Lua => {
            // Luau/Lua scripts go through the mlua runtime. Spawned with the
            // same SoulScriptData component as Rune so `compile_scripts_on_play`
            // picks them up alongside .rune files; `run_context = Luau` routes
            // execution through `execute_chunk` instead of the Rune VM.
            match src_read_string(source, space_path, &file_meta.path) {
                Ok(lua_source) => {
                    let e = commands.spawn((
                        eustress_common::classes::Instance {
                            name: file_meta.name.clone(),
                            class_name: eustress_common::classes::ClassName::SoulScript,
                            archivable: true,
                            id: 0,
                            ai: false,
                            uuid: String::new(),
                        },
                        crate::soul::SoulScriptData {
                            source: lua_source,
                            dirty: false,
                            ast: None,
                            generated_code: None,
                            build_status: crate::soul::SoulBuildStatus::NotBuilt,
                            errors: Vec::new(),
                            run_context: crate::soul::SoulRunContext::Luau,
                        },
                        LoadedFromFile {
                            path: file_meta.path.clone(),
                            file_type: file_meta.file_type,
                            service: file_meta.service.clone(),
                        },
                        Name::new(file_meta.name.clone()),
                    )).id();
                    registry.register(file_meta.path.clone(), e, file_meta.clone());
                    info!("🌙 Loaded Luau script {} from {:?}", file_meta.name, file_meta.path);
                    e
                }
                Err(err) => {
                    error!("❌ Failed to read Luau script {:?}: {}", file_meta.path, err);
                    return None;
                }
            }
        }

        FileType::GuiElement => {
            // Load GUI element files (.textlabel.toml, .frame.toml, etc.) as Bevy UI entities.
            // Parses the TOML file for visual properties (position, size, colors, text)
            // and spawns with proper Bevy UI components (Node, BackgroundColor, Text, etc.)
            // so they render visually in the viewport.
            //
            // Sourced through SpaceSource (Fjall when migrated, Disk
            // otherwise) then parsed in memory — the flat-file twin of
            // the directory GUI branches, so a DB-authoritative world
            // loads GUI elements with zero disk reads.
            match src_read_string(source, space_path, &file_meta.path)
                .map_err(|e| e.to_string())
                .and_then(|c| super::gui_loader::load_gui_definition_from_str(&c))
            {
                Ok(gui_def) => {
                    let display_name = super::gui_loader::gui_display_name(&file_meta.path);
                    let gui_type = super::gui_loader::gui_class_from_extension(&file_meta.path);
                    let e = super::gui_loader::spawn_gui_element(
                        commands,
                        &file_meta.path,
                        &gui_def,
                    );
                    registry.register(file_meta.path.clone(), e, file_meta.clone());
                    // DEBUG: per-GUI-element; bulk-load hot path at scale.
                    debug!("🖼️ Loaded GUI element {} ({}) from {:?}", display_name, gui_type, file_meta.path);
                    e
                }
                Err(err) => {
                    error!("Failed to load GUI element {:?}: {}", file_meta.path, err);
                    return None;
                }
            }
        }

        FileType::Material => {
            // Load .mat.toml via SpaceSource (Fjall when migrated) then
            // parse in memory — zero disk on an authoritative world.
            match src_read_string(source, space_path, &file_meta.path)
                .map_err(|e| e.to_string())
                .and_then(|c| super::material_loader::load_material_definition_from_str(&c))
            {
                Ok(definition) => {
                    let mat_name = if definition.material.name.is_empty() {
                        super::material_loader::material_name_from_path(&file_meta.path)
                    } else {
                        definition.material.name.clone()
                    };
                    let mat_toml_dir = file_meta.path.parent().unwrap_or(std::path::Path::new("."));
                    let standard_mat = super::material_loader::build_standard_material(
                        &definition,
                        asset_server,
                        mat_toml_dir,
                        space_path,
                    );
                    let handle = materials.add(standard_mat);
                    // Register material in the MaterialRegistry for Part resolution
                    material_registry.insert(
                        mat_name.clone(),
                        handle,
                        definition.clone(),
                        file_meta.path.clone(),
                    );
                    // Maps are attached the first time a Part uses this material.
                    material_registry.defer_textures(
                        &mat_name,
                        &definition.textures,
                        mat_toml_dir,
                        space_path,
                    );
                    let e = super::material_loader::spawn_material_entity(
                        commands,
                        file_meta.path.clone(),
                        &definition,
                    );
                    commands.entity(e).insert(LoadedFromFile {
                        path: file_meta.path.clone(),
                        file_type: file_meta.file_type,
                        service: file_meta.service.clone(),
                    });
                    registry.register(file_meta.path.clone(), e, file_meta.clone());
                    info!("🎨 Loaded material '{}' from {:?}", mat_name, file_meta.path);
                    e
                }
                Err(err) => {
                    error!("Failed to load material {:?}: {}", file_meta.path, err);
                    return None;
                }
            }
        }

        FileType::Png => {
            // Spawn Image class entity for PNG files (textures in MaterialService)
            let img_name = file_meta.path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Image".to_string());
            commands.spawn((
                eustress_common::classes::Instance {
                    name: img_name.clone(),
                    class_name: eustress_common::classes::ClassName::Image,
                    archivable: true,
                    id: 0,
                    ..Default::default()
                },
                LoadedFromFile {
                    path: file_meta.path.clone(),
                    file_type: file_meta.file_type,
                    service: file_meta.service.clone(),
                },
                Name::new(img_name),
            )).id()
        }

        FileType::Ogg | FileType::Mp3 | FileType::Wav | FileType::Flac => {
            info!("🔊 Audio file discovered: {:?} (loader not yet implemented)", file_meta.path);
            return None;
        }

        _ => {
            debug!("File type {:?} loader not yet implemented", file_meta.file_type);
            return None;
        }
    };

    // PERF: non-GUI files inside StarterGui used to get a hidden
    // `Node { display: None }` "so Bevy doesn't treat them as stray non-UI
    // leaves of the UI root". Verified against bevy_ui 0.18 source: non-Node
    // children of Node parents are silently skipped (ui_surface::update_children
    // filters via entity_to_taffy; no warning, no panic), while every entity
    // that DOES carry `Node` is walked by ui_layout_system/update_clipping/
    // ui_stack every frame even at Display::None. The defensive Node was pure
    // per-frame cost — removed.
    // Parent to Folder entity if provided
    if let Some(parent) = parent_entity {
        commands.entity(entity).insert(ChildOf(parent));
    }

    Some(entity)
}

/// Spawn a Directory entry as a Folder entity (or Service entity if it contains _service.toml),
/// then spawn all its children parented to that entity. Recurses for nested subdirectories.
pub fn spawn_directory_entry(
    commands: &mut Commands,
    asset_server: &Res<AssetServer>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<SpaceFileRegistry>,
    material_registry: &mut ResMut<super::material_loader::MaterialRegistry>,
    mesh_cache: &mut ResMut<super::instance_loader::PrimitiveMeshCache>,
    decal_materials: &mut ResMut<Assets<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>>,
    space_path: &Path,
    dir_meta: &FileMetadata,
    parent_entity: Option<Entity>,
    class_defaults: Option<&super::class_defaults::ClassDefaultsRegistry>,
    source: &dyn super::space_source::SpaceSource,
) {
    // Skip if this directory path already has an entity registered
    if registry.is_loaded(&dir_meta.path) {
        return;
    }

    // Skip 'meshes' directories - these are asset storage, not part of the scene hierarchy
    if dir_meta.name == "meshes" || dir_meta.path.components().any(|c| c.as_os_str() == "meshes") {
        debug!("Skipping meshes directory {:?} (asset storage)", dir_meta.path);
        return;
    }

    // Skip the worldgen Terrain EXPORT directory (chunks/*.r16, splatmap/*.png,
    // _terrain.toml, materials/*.mat.toml) — it has no `_instance.toml`, so
    // without this check it falls through to the generic Folder-class default
    // below and gets spawned as a real ECS entity named "Terrain", DUPLICATING
    // the actual terrain mesh entity (which the disk-load / worldgen hydration
    // path spawns separately, see terrain_disk_load.rs). This is asset/export
    // storage exactly like `meshes/`, not a scene-hierarchy instance.
    // One marker-table lookup answers the three "is there a _terrain /
    // _service / _instance marker" questions below; the source is probed
    // only for a directory the scan did not index (see `dir_markers`).
    let dir_rel = super::space_source::rel_from_root(space_path, &dir_meta.path).unwrap_or_default();
    let markers = dir_markers::get(&dir_rel);
    let terrain_toml_rel =
        super::space_source::rel_from_root(space_path, &dir_meta.path.join("_terrain.toml"))
            .unwrap_or_default();
    let has_terrain = match markers {
        Some(m) => m & dir_markers::TERRAIN != 0,
        None => source.exists(&terrain_toml_rel),
    };
    if has_terrain {
        debug!("Skipping terrain export directory {:?} (asset storage, not an instance)", dir_meta.path);
        return;
    }

    // Check for service directory — either has _service.toml or is a well-known service name
    let service_toml_path = dir_meta.path.join("_service.toml");
    let service_toml_rel =
        super::space_source::rel_from_root(space_path, &service_toml_path).unwrap_or_default();
    let has_service_toml = match markers {
        Some(m) => m & dir_markers::SERVICE != 0,
        None => source.exists(&service_toml_rel),
    };
    let is_known_service = KNOWN_SERVICE_NAMES.contains(&dir_meta.name.as_str());
    if has_service_toml || is_known_service {
        // Load service definition from _service.toml, or create a default for known services
        let service_def = if has_service_toml {
            match src_read_string(source, space_path, &service_toml_path)
                .map_err(|e| e.to_string())
                .and_then(|c| super::service_loader::load_service_definition_from_str(&c))
            {
                Ok(def) => def,
                Err(err) => {
                    error!("Failed to load service {:?}: {}", service_toml_path, err);
                    return;
                }
            }
        } else {
            // Create a default definition for well-known services without _service.toml
            super::service_loader::ServiceDefinition {
                service: super::service_loader::ServiceProperties {
                    class_name: dir_meta.name.clone(),
                    icon: None,
                    description: None,
                    can_have_children: true,
                    properties: std::collections::HashMap::new(),
                },
                metadata: super::service_loader::ServiceMetadata::default(),
                properties: std::collections::HashMap::new(),
            }
        };

        let is_gui_service = dir_meta.name == "StarterGui";
        let service_entity = if is_gui_service {
            // StarterGui must be a Bevy UI root so child ScreenGui/Frame entities render.
            // A Transform+Visibility parent breaks the Bevy UI hierarchy chain.
            super::service_loader::spawn_service_as_ui_root(commands, service_toml_path.clone(), service_def)
        } else {
            super::service_loader::spawn_service(commands, service_toml_path.clone(), service_def)
        };
        registry.register(dir_meta.path.clone(), service_entity, dir_meta.clone());
        info!("🏢 Spawned Service '{}' with {} children", dir_meta.name, dir_meta.children.len());

        // Spawn all children parented to this service, under the same
        // count + time budget as the folder loop below. A service's DIRECT
        // children are the widest fan-out in the tree (Super Station:
        // 10,035 under Workspace). Without this check every one of them
        // spawned in a single frame: the folder-level budget only tripped
        // one level down, so each child still paid its own folder spawn
        // (~4 ms) inside one 42 s frame.
        for (idx, child) in dir_meta.children.iter().enumerate() {
            if SPAWN_BUDGET.fetch_sub(1, std::sync::atomic::Ordering::Relaxed) <= 0
                || spawn_deadline_passed()
            {
                super::material_loader::set_dense_material_mode(true);
                let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
                for rest in &dir_meta.children[idx..] {
                    q.push((rest.clone(), Some(service_entity)));
                }
                break;
            }
            match child.file_type {
                FileType::Directory => {
                    spawn_directory_entry(
                        commands, asset_server, meshes, materials, registry,
                        material_registry, mesh_cache, decal_materials, space_path, child, Some(service_entity),
                        class_defaults, source,
                    );
                }
                _ => {
                    spawn_file_entry(
                        commands, asset_server, meshes, materials, registry,
                        material_registry, mesh_cache, decal_materials, space_path, child, Some(service_entity),
                        class_defaults, source,
                    );
                }
            }
        }
        return;
    }

    // Check for _instance.toml — it may declare a richer class
    // (ScreenGui, Frame, BillboardGui, TextLabel, …). Historic on-disk
    // files are `[metadata] class_name` snake_case; files left over
    // from the aborted PascalCase migration are `[Metadata] ClassName`.
    // Try both so either layout keeps loading. Class-name dispatch
    // delegates to `ClassName::from_str` — the canonical string→enum
    // map lives on the enum itself, so adding a new class means one
    // variant + one from_str arm in `common/classes.rs`, no changes
    // here. Legacy `"Script"` alias routes to `SoulScript`.
    let t_pre = std::time::Instant::now();
    let instance_toml_path = dir_meta.path.join("_instance.toml");
    let instance_toml_rel = super::space_source::rel_from_root(space_path, &instance_toml_path)
        .unwrap_or_default();
    // ONE probe, ONE (cached) read and ONE parse of `_instance.toml` for
    // everything below. The class name, the custom-mesh mention and the
    // UUID each used to probe, read and parse it again: three TOML parses
    // per entity on the bulk-load hot path.
    let has_instance = match markers {
        Some(m) => m & dir_markers::INSTANCE != 0,
        None => source.exists(&instance_toml_rel),
    };
    // One parsed record per `_instance.toml`. The pre-parse thread fills the
    // cache ahead of the drain; a miss (the priority spawn, a hot reload
    // after settle) reads and parses inline with the same function. Each
    // branch below takes the one form it needs from the record.
    let mut parsed: Option<ParsedInstance> = if has_instance {
        parsed_cache_take(&instance_toml_rel).or_else(|| {
            src_read_string(source, space_path, &instance_toml_path)
                .ok()
                .map(|text| parse_instance_text(&text))
        })
    } else {
        None
    };
    let class_name = parsed
        .as_ref()
        .map(|p| p.class_name)
        .unwrap_or(eustress_common::classes::ClassName::Folder);

    // STREAMING-PRIMARY: on the initial load of a large DB-backed Space, the
    // residency manager streams bare parts from the `entities` partition by
    // camera — so do NOT also bulk-spawn them from the tree here (that double-
    // load is what pinned huge imports at ~2 FPS). Skip CONSERVATIVELY: only a
    //   • childless dir (a part with child dirs would orphan them), that is
    //   • a `BinaryEcs`-representation class (bare Part/WedgePart/Model …), with
    //   • no custom-mesh reference (custom meshes are FileSystem-stored, not in
    //     the partition residency streams — never drop them).
    // `STREAM_DB_PARTS` is only set during the initial load of a qualifying
    // Space and reset on settle, so paste / hot-reload always spawn normally.
    if STREAM_DB_PARTS.load(std::sync::atomic::Ordering::Relaxed) {
        let has_children = dir_meta
            .children
            .iter()
            .any(|c| c.file_type == FileType::Directory);
        let has_custom_mesh = parsed
            .as_ref()
            .map(|p| p.mentions_custom_mesh)
            .unwrap_or(false);
        // Shared with `bake_cores`: this skip and that conversion are two
        // halves of one invariant (exactly one loader owns each entity), so
        // they must consult the SAME function. Kept as separate predicates
        // they drift, and the drift is silent — a mismatch either spawns the
        // entity twice or loses it entirely.
        if super::representation::streams_from_db(
            &format!("{:?}", class_name),
            has_children,
            has_custom_mesh,
        ) {
            return; // residency streams this bare part from the DB
        }
    }

    // Stable UUID from `[metadata].uuid` — carried onto the spawned
    // `Instance` so cross-references resolve by identity (constraints bind
    // their `Part0`/`Attachment0` joint bodies by UUID; Attachment +
    // Folder/Model entities spawned via the fall-through arm need it too).
    let instance_uuid: String = parsed
        .as_ref()
        .map(|p| p.uuid.clone())
        .unwrap_or_default();

    toml_spawn_cost::add(&toml_spawn_cost::READ_PARSE_NS, t_pre.elapsed());
    let t_spawn = std::time::Instant::now();

    // Spawn the Folder / ScreenGui / Frame / Model entity
    let is_screen_gui = matches!(class_name, eustress_common::classes::ClassName::ScreenGui);
    let is_gui_container = matches!(class_name,
        eustress_common::classes::ClassName::Frame
        | eustress_common::classes::ClassName::ScrollingFrame
    );

    let folder_entity = if is_screen_gui {
        // ScreenGui: fullscreen UI root — read enabled/visible from _instance.toml
        let instance_toml = dir_meta.path.join("_instance.toml");
        let screen_gui_visible = if let Ok(gui_def) = take_gui_def(&mut parsed, source, space_path, &instance_toml) {
            gui_def.gui.visible
        } else {
            true // default: visible
        };

        let entity_cmds = commands.spawn((
            eustress_common::classes::Instance {
                name: dir_meta.name.clone(),
                class_name,
                archivable: true,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            LoadedFromFile {
                path: dir_meta.path.clone(),
                file_type: FileType::Directory,
                service: dir_meta.service.clone(),
            },
            Name::new(dir_meta.name.clone()),
            // No bevy_ui Node — ScreenGui renders via GuiElementDisplay/Slint
            // overlay; a Node here only adds per-frame ui_layout cost (see
            // gui_loader::spawn_frame_element PERF note).
            // GuiElementDisplay with visible flag — children inherit this for rendering
            eustress_common::gui::billboard_renderer::GuiElementDisplay {
                x: 0.0, y: 0.0, width: 0.0, height: 0.0,
                position_udim2: [0.0; 4], size_udim2: [0.0; 4],
                anchor_point: [0.0, 0.0],
                z_order: 0, visible: screen_gui_visible, clip_children: false,
                scroll_x: 0.0, scroll_y: 0.0,
                bg_color: [0.0; 4], border_size: 0.0, border_color: [0.0; 4],
                corner_radius: 0.0,
                text: String::new(), text_color: [1.0; 4],
                font: String::new(),
                font_size: 14.0, font_weight: 400,
                text_align: "Center".to_string(), text_y_align: "Center".to_string(),
                text_stroke_color: [0.0, 0.0, 0.0, 0.0],
                text_scaled: false,
                image_path: String::new(),
                class_type: "screengui".to_string(),
                mouse_filter: "ignore".to_string(),
            },
        ));
        if !screen_gui_visible {
            info!("📋 ScreenGui '{}' loaded as hidden (enabled=false)", dir_meta.name);
        }
        entity_cmds.id()
    } else if is_gui_container {
        // Frame/ScrollingFrame directory — load GUI properties from _instance.toml
        // and attach GuiElementDisplay so it renders through Slint overlay
        let instance_toml = dir_meta.path.join("_instance.toml");
        let gui_display = if let Ok(gui_def) = take_gui_def(&mut parsed, source, space_path, &instance_toml) {
            let class_str = format!("{:?}", class_name).to_lowercase();
            super::gui_loader::gui_display_from_props(&gui_def.gui, gui_def.text.as_ref(), &class_str)
        } else {
            // Fallback: invisible container
            eustress_common::gui::billboard_renderer::GuiElementDisplay {
                x: 0.0, y: 0.0, width: 0.0, height: 0.0,
                position_udim2: [0.0; 4], size_udim2: [0.0; 4],
                anchor_point: [0.0, 0.0],
                z_order: 1, visible: true, clip_children: false,
                scroll_x: 0.0, scroll_y: 0.0,
                bg_color: [0.0, 0.0, 0.0, 0.0],
                border_size: 0.0, border_color: [0.0; 4],
                corner_radius: 0.0,
                text: String::new(), text_color: [1.0; 4],
                font: String::new(),
                font_size: 14.0, font_weight: 400,
                text_align: "Center".to_string(), text_y_align: "Center".to_string(),
                text_stroke_color: [0.0, 0.0, 0.0, 0.0],
                text_scaled: false,
                image_path: String::new(),
                class_type: "Frame".to_string(),
                mouse_filter: "stop".to_string(),
            }
        };
        commands.spawn((
            eustress_common::classes::Instance {
                name: dir_meta.name.clone(),
                class_name,
                archivable: true,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            LoadedFromFile {
                path: dir_meta.path.clone(),
                file_type: FileType::Directory,
                service: dir_meta.service.clone(),
            },
            Name::new(dir_meta.name.clone()),
            // No bevy_ui Node — rendered via GuiElementDisplay (PERF, see above).
            gui_display,
        )).id()
    } else if matches!(class_name, eustress_common::classes::ClassName::BillboardGui) {
        // BillboardGui — 3D billboard entity (quad facing camera).
        //
        // Roblox parity: every `BillboardGui` instance property is
        // round-tripped through `_instance.toml`. Optional fields default
        // to the class's `Default::default()` so older TOML files (which
        // pre-date a property) load cleanly without overwriting existing
        // defaults.
        let instance_toml = dir_meta.path.join("_instance.toml");
        let mut bb_class = eustress_common::classes::BillboardGui::default();
        let mut bb_offset = Vec3::new(0.0, 2.0, 0.0);
        // Collected from `gui_def.tags` so we can attach the ECS Tags
        // component after spawn (lives outside the `if let Ok` scope).
        let mut bb_tags: Vec<String> = Vec::new();

        if let Ok(gui_def) = take_gui_def(&mut parsed, source, space_path, &instance_toml) {
            bb_tags = gui_def.tags.clone();
            let g = &gui_def.gui;

            // Geometry
            bb_class.size = g.resolved_size();
            if let Some(v) = g.size_offset { bb_class.size_offset = v; }
            if let Some(v) = g.extents_offset { bb_class.extents_offset = v; }
            if let Some(v) = g.extents_offset_world_space { bb_class.extents_offset_world_space = v; }
            if let Some(v) = g.units_offset { bb_class.units_offset = v; }
            if let Some(v) = g.units_offset_world_space { bb_class.units_offset_world_space = v; }

            // BillboardGui's 3D placement still comes from `units_offset`
            // (Vec3 stud offset) — separate from the 2D-canvas
            // `position: UDim2`. Mirror it into `bb_offset` for the
            // entity's Transform.
            bb_offset = Vec3::new(
                bb_class.units_offset[0],
                bb_class.units_offset[1],
                bb_class.units_offset[2],
            );

            // Distance
            if let Some(v) = g.max_distance { bb_class.max_distance = v; }
            if let Some(v) = g.distance_lower_limit { bb_class.distance_lower_limit = v; }
            if let Some(v) = g.distance_upper_limit { bb_class.distance_upper_limit = v; }
            if let Some(v) = g.distance_step { bb_class.distance_step = v; }

            // Behaviour flags
            if let Some(v) = g.active { bb_class.active = v; }
            if let Some(v) = g.enabled { bb_class.enabled = v; }
            if let Some(v) = g.always_on_top { bb_class.always_on_top = v; }
            if let Some(v) = g.clips_descendants { bb_class.clips_descendants = v; }
            if let Some(v) = g.reset_on_spawn { bb_class.reset_on_spawn = v; }
            if let Some(v) = g.stiffness_by_distance { bb_class.stiffness_by_distance = v; }

            // Appearance
            if let Some(v) = g.brightness { bb_class.brightness = v; }
            if let Some(v) = g.light_influence { bb_class.light_influence = v; }

            // Sorting
            if let Some(ref s) = g.z_index_behavior {
                bb_class.z_index_behavior = match s.as_str() {
                    "Global" => eustress_common::classes::ZIndexBehavior::Global,
                    _ => eustress_common::classes::ZIndexBehavior::Sibling,
                };
            }
            // ZIndex depth-bias (per-billboard integer). MindSpace's add-label
            // derives it from the adornee part's longest scale axis; read it
            // back so the value persists across reload instead of resetting to 0.
            bb_class.z_index = g.z_index;
            // NOTE: `g.adornee` carries the instance name; resolution to
            // an Entity reference happens in a post-load pass once all
            // instances are spawned (see resolve_billboard_adornees, TODO).
        }

        // Build the runtime marker by copying the parity-relevant subset
        // of the class. `sync_billboard_class_to_marker` will keep these
        // in sync on subsequent edits, but we initialise them here so
        // the first frame shows correct visibility / depth / size.
        // Marker mirrors the renderer's resolved-pixel inputs. Class
        // carries the source-of-truth UDim2; we resolve to pixels here
        // (Scale × PIXELS_PER_METER + Offset) so the renderer doesn't
        // have to revisit UDim2 math each frame.
        let [size_w_px, size_h_px] = bb_class.size.to_pixels(50.0, 50.0);
        // `size_offset` is Roblox-parity `Vector2` — already in pixels.
        let [size_off_x, size_off_y] = bb_class.size_offset;
        let marker = eustress_common::gui::billboard_renderer::BillboardGuiMarker {
            size: [size_w_px.max(1.0), size_h_px.max(1.0)],
            size_offset: [size_off_x, size_off_y],
            extents_offset: bb_class.extents_offset,
            extents_offset_world_space: bb_class.extents_offset_world_space,
            units_offset_world_space: bb_class.units_offset_world_space,
            max_distance: if bb_class.max_distance > 0.0 && bb_class.distance_upper_limit > 0.0 {
                bb_class.max_distance.min(bb_class.distance_upper_limit)
            } else {
                bb_class.max_distance.max(bb_class.distance_upper_limit)
            },
            distance_lower_limit: bb_class.distance_lower_limit.max(0.0),
            distance_step: bb_class.distance_step.max(0.0),
            always_on_top: bb_class.always_on_top,
            clips_descendants: bb_class.clips_descendants,
            brightness: bb_class.brightness.clamp(0.0, 8.0),
            light_influence: bb_class.light_influence.clamp(0.0, 1.0),
            face_camera: true,
            visible: bb_class.enabled,
            z_index: bb_class.z_index,
        };

        // The Properties panel keys its rich-class display branch off
        // `InstanceFile` (it's the marker that says "this entity has a
        // canonical `_instance.toml` on disk you can edit"). Without it,
        // a freshly-reloaded BillboardGui falls through to the basic
        // Data + Transform fallback and the user can't edit AlwaysOnTop /
        // MaxDistance / Brightness / etc. through the panel.
        let instance_toml_path = dir_meta.path.join("_instance.toml");
        let entity = commands.spawn((
            eustress_common::classes::Instance {
                name: dir_meta.name.clone(),
                class_name,
                archivable: true,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            bb_class,
            marker,
            LoadedFromFile {
                path: dir_meta.path.clone(),
                file_type: FileType::Directory,
                service: dir_meta.service.clone(),
            },
            super::instance_loader::InstanceFile {
                toml_path: instance_toml_path,
                mesh_path: std::path::PathBuf::new(),
                name: dir_meta.name.clone(),
            },
            Name::new(dir_meta.name.clone()),
            Transform::from_translation(bb_offset),
            Visibility::default(),
        )).id();
        // Tag hydration matches `instance_loader::spawn_instance`'s path
        // for Parts so MCP / ECS queries see GUI tags identically.
        if !bb_tags.is_empty() {
            commands.entity(entity).insert(eustress_common::attributes::Tags(bb_tags));
        }
        entity
    } else if matches!(class_name, eustress_common::classes::ClassName::Image | eustress_common::classes::ClassName::Video) {
        // Image / Video — imported media class. Loads the asset_path
        // referenced by `[asset].path` in _instance.toml, which lives
        // under the Universe-level `assets/` folder. The entity is a
        // 3D quad: for Image, the texture is the loaded image; for
        // Video, a mid-grey placeholder material until the decoder
        // integration lands.
        let instance_toml = dir_meta.path.join("_instance.toml");
        let toml_value: Option<toml::Value> =
            take_instance_value(&parsed, source, space_path, &instance_toml);

        // Universe-relative asset path stored in `[asset].path`. The
        // engine-runtime path is `<Universe>/<asset_path>`.
        let asset_rel = toml_value
            .as_ref()
            .and_then(|v| v.get("asset"))
            .and_then(|a| a.get("path"))
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string();

        // Per-class section: [image] for Image, [video] for Video.
        let section_name = match class_name {
            eustress_common::classes::ClassName::Image => "image",
            _                                          => "video",
        };
        let section = toml_value.as_ref().and_then(|v| v.get(section_name));
        // Quad extent, in priority order:
        //   1. `[transform].scale` — what the Scale gizmo maintains. The
        //      quad's mesh is a unit square, so x/y ARE the world size and
        //      z is unused. The class template already ships this key
        //      (4, 4, 1 for Image), matching the size default below.
        //   2. `[image].size` / `[video].size` — the authored property.
        //   3. The per-class default.
        //
        // Reading `size` first would make the object un-resizable: the
        // transform writer persists gizmo edits into `[transform].scale`,
        // so a size-first loader would discard every resize on reload and
        // snap the quad back to its authored value.
        let scale_xy = toml_value
            .as_ref()
            .and_then(|v| v.get("transform"))
            .and_then(|t| t.get("scale"))
            .and_then(|s| s.as_array())
            .and_then(|arr| {
                let x = arr.first()?.as_float().map(|f| f as f32)?;
                let y = arr.get(1)?.as_float().map(|f| f as f32)?;
                // A degenerate axis would collapse the quad to nothing and
                // leave no way to grab it again, so treat it as unset.
                (x.abs() > 1.0e-4 && y.abs() > 1.0e-4).then_some([x, y])
            });
        let size_xy = scale_xy
            .or_else(|| {
                section
                    .and_then(|s| s.get("size"))
                    .and_then(|s| s.as_array())
                    .and_then(|arr| {
                        let x = arr.first()?.as_float().map(|f| f as f32)?;
                        let y = arr.get(1)?.as_float().map(|f| f as f32)?;
                        Some([x, y])
                    })
            })
            .unwrap_or(match class_name {
                eustress_common::classes::ClassName::Image => [4.0, 4.0],
                _                                          => [6.0, 3.375],
            });
        let color_rgba = section
            .and_then(|s| s.get("color"))
            .and_then(|c| c.as_array())
            .and_then(|arr| {
                Some([
                    arr.first()?.as_float()? as f32,
                    arr.get(1)?.as_float()? as f32,
                    arr.get(2)?.as_float()? as f32,
                    arr.get(3).and_then(|v| v.as_float()).unwrap_or(1.0) as f32,
                ])
            })
            .unwrap_or([1.0, 1.0, 1.0, 1.0]);
        let transparency = section
            .and_then(|s| s.get("transparency"))
            .and_then(|t| t.as_float())
            .map(|f| f as f32)
            .unwrap_or(0.0);
        let position = toml_value
            .as_ref()
            .and_then(|v| v.get("transform"))
            .and_then(|t| t.get("position"))
            .and_then(|p| p.as_array())
            .and_then(|arr| {
                Some(Vec3::new(
                    arr.first()?.as_float()? as f32,
                    arr.get(1)?.as_float()? as f32,
                    arr.get(2)?.as_float()? as f32,
                ))
            })
            .unwrap_or(Vec3::new(0.0, 2.0, 0.0));

        // Resolve the asset on disk. Universe root = grandparent of
        // SpaceRoot (`<Universe>/Spaces/<SpaceN>`); asset path is
        // joined from there.
        let universe_root = space_path
            .ancestors()
            .nth(2)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| space_path.to_path_buf());
        let abs_asset_path = universe_root.join(&asset_rel);

        // Build a unit quad mesh — same shape as `BillboardCard`'s
        // quad, scaled to size via Transform.scale.
        let mesh_handle = meshes.add(build_imported_media_quad());

        // Material: Image gets the loaded texture as albedo; Video
        // gets a mid-grey placeholder material. Both honour the part's
        // colour tint + transparency.
        let alpha = 1.0 - transparency.clamp(0.0, 1.0);
        let mut mat = StandardMaterial {
            base_color: bevy::color::Color::srgba(color_rgba[0], color_rgba[1], color_rgba[2], color_rgba[3] * alpha),
            unlit: true,
            alpha_mode: if alpha < 1.0 || matches!(class_name, eustress_common::classes::ClassName::Image) {
                bevy::prelude::AlphaMode::Blend
            } else {
                bevy::prelude::AlphaMode::Opaque
            },
            cull_mode: None,
            ..default()
        };
        if matches!(class_name, eustress_common::classes::ClassName::Image) {
            // Bevy's AssetServer wants paths relative to its asset root.
            // The simplest cross-platform approach: pass the absolute
            // path as a string. AssetServer accepts absolute filesystem
            // paths on the filesystem source.
            if !asset_rel.is_empty() && abs_asset_path.exists() {
                let texture: Handle<bevy::image::Image> = asset_server.load(abs_asset_path.clone());
                mat.base_color_texture = Some(texture);
            } else {
                warn!(
                    "🖼️ Image entity '{}' has missing asset at {:?} — rendering as flat colour quad",
                    dir_meta.name, abs_asset_path
                );
            }
        } else {
            // Video — initial material is placeholder (replaced next
            // frame by `attach_pending_video_players`, which binds the
            // VideoPlayer's GPU image as base_color_texture). The
            // placeholder colour shows for ~1 frame between spawn and
            // pump kickoff; imperceptible.
            mat.base_color = bevy::color::Color::srgba(0.18, 0.20, 0.22, alpha);
            mat.emissive = bevy::color::LinearRgba::new(0.05, 0.06, 0.08, 1.0);
        }
        let material_handle = materials.add(mat);

        // Build the entity. For Video, also attach `PendingVideoSetup`
        // so the video plugin's deferred-attach system mints a real
        // VideoPlayer next frame and rewrites the material's base
        // color texture to point at the freshly-decoded frames.
        let is_video = matches!(class_name, eustress_common::classes::ClassName::Video);
        let entity = commands.spawn((
            eustress_common::classes::Instance {
                name: dir_meta.name.clone(),
                class_name,
                archivable: true,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            Mesh3d(mesh_handle),
            MeshMaterial3d(material_handle),
            LoadedFromFile {
                path: dir_meta.path.clone(),
                file_type: FileType::Directory,
                service: dir_meta.service.clone(),
            },
            Name::new(dir_meta.name.clone()),
            Transform::from_translation(position).with_scale(Vec3::new(size_xy[0], size_xy[1], 1.0)),
            Visibility::default(),
            bevy::light::NotShadowCaster,
            // Imported media is an ordinary editable object, so it needs the
            // same disk identity every other instance carries.
            // `write_instance_changes_system` requires `&InstanceFile` in its
            // query, so an entity without one is invisible to the transform
            // writer: the gizmos move, rotate and scale the quad on screen and
            // nothing is ever written back, which reads as "the tools do not
            // work on Images". For a media quad the Transform IS the authored
            // state (scale carries `[image].size`), so persisting it is the
            // whole contract.
            super::instance_loader::InstanceFile {
                toml_path: dir_meta.path.join("_instance.toml"),
                mesh_path: std::path::PathBuf::new(),
                name: dir_meta.name.clone(),
            },
        )).id();

        if is_video {
            // Texture-buffer dimensions for the video. 1280×720 gives a
            // sensible default that handles most imported clips without
            // being wasteful for thumbnails. Real decoder integration
            // will set width/height from the file's video stream
            // metadata when the asset opens.
            let video_w: u32 = 1280;
            let video_h: u32 = 720;
            commands.entity(entity).insert(crate::video::PendingVideoSetup {
                width: video_w,
                height: video_h,
                asset_path: abs_asset_path.clone(),
            });
        }

        entity
    } else if matches!(class_name, eustress_common::classes::ClassName::SoulScript) {
        // Script folder — find the .rune/.luau/.soul source file inside and load it.
        let instance_toml = dir_meta.path.join("_instance.toml");
        // Read the "source" field from _instance.toml to find the script filename,
        // or scan the folder for the first .rune/.luau/.soul file.
        let source_file = take_instance_value(&parsed, source, space_path, &instance_toml)
            .and_then(|v| {
                use eustress_common::class_schema::get_section_insensitive as get_ci;
                get_ci(&v, "script")
                    .and_then(|s| get_ci(s, "source"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string())
            })
            .map(|rel| dir_meta.path.join(rel))
            .or_else(|| {
                // Fallback: scan the folder via the source for the first
                // script file. `dir_meta.path` is absolute; derive the
                // Space-relative dir so Fjall-sourced worlds resolve it.
                let dir_rel = super::space_source::rel_from_root(space_path, &dir_meta.path)
                    .unwrap_or_default();
                source.list(&dir_rel).ok().and_then(|entries| {
                    entries
                        .into_iter()
                        .find(|e| {
                            !e.is_dir
                                && (e.name.ends_with(".rune")
                                    || e.name.ends_with(".luau")
                                    || e.name.ends_with(".soul")
                                    || e.name.ends_with(".lua"))
                        })
                        .map(|e| {
                            let mut p = space_path.to_path_buf();
                            for seg in e.rel_path.split('/') {
                                if !seg.is_empty() {
                                    p.push(seg);
                                }
                            }
                            p
                        })
                })
            });

        if let Some(ref src_path) = source_file {
            if let Ok(script_src) = src_read_string(source, space_path, src_path) {
                let script_name = dir_meta.name.clone();
                commands.spawn((
                    eustress_common::classes::Instance {
                        name: script_name.clone(),
                        class_name: eustress_common::classes::ClassName::SoulScript,
                        archivable: true, id: 0, ai: false, uuid: String::new(),
                    },
                    crate::soul::SoulScriptData {
                        source: script_src,
                        dirty: false,
                        ast: None,
                        generated_code: None,
                        build_status: crate::soul::SoulBuildStatus::NotBuilt,
                        errors: Vec::new(),
                        run_context: Default::default(),
                    },
                    LoadedFromFile {
                        // The SOURCE FILE, not the folder that holds it.
                        //
                        // This recorded `dir_meta.path`, the folder, which has no
                        // extension. `compile_scripts_on_play` selects what to
                        // compile with `loaded.path.extension()` matched against
                        // "rune" | "soul", so every script packaged as a folder
                        // presented an empty extension, failed that match, and was
                        // silently skipped: the source was read, the entity was
                        // spawned, the Explorer showed it, and it never ran. Bare
                        // `.rune` files in a scripts/ directory were unaffected,
                        // which is why the failure looked like it belonged to the
                        // scripts rather than to the loader.
                        path: src_path.clone(),
                        file_type: FileType::Rune,
                        service: dir_meta.service.clone(),
                    },
                    Name::new(script_name),
                )).id()
            } else {
                warn!("Failed to read script source {:?}", src_path);
                commands.spawn((
                    eustress_common::classes::Instance {
                        name: dir_meta.name.clone(),
                        class_name: eustress_common::classes::ClassName::Folder,
                        archivable: true, id: 0, ai: false, uuid: String::new(),
                    },
                    LoadedFromFile {
                        path: dir_meta.path.clone(),
                        file_type: FileType::Directory,
                        service: dir_meta.service.clone(),
                    },
                    Name::new(dir_meta.name.clone()),
                    Transform::default(),
                    Visibility::default(),
                )).id()
            }
        } else {
            warn!("Script folder {:?} has no source file", dir_meta.path);
            commands.spawn((
                eustress_common::classes::Instance {
                    name: dir_meta.name.clone(),
                    class_name: eustress_common::classes::ClassName::Folder,
                    archivable: true, id: 0, ai: false, uuid: String::new(),
                },
                LoadedFromFile {
                    path: dir_meta.path.clone(),
                    file_type: FileType::Directory,
                    service: dir_meta.service.clone(),
                },
                Name::new(dir_meta.name.clone()),
                Transform::default(),
                Visibility::default(),
            )).id()
        }
    } else if matches!(class_name, eustress_common::classes::ClassName::Part) {
        // Part folder — load via spawn_instance (same path as flat .glb.toml files).
        // The _instance.toml inside contains the full InstanceDefinition with mesh, transform, etc.
        // Realism sections (`[material]` / `[thermodynamic]` / `[electrochemical]`)
        // are dynamic on every Part — `InstanceDefinition` consumes them as
        // typed fields and `spawn_instance` attaches the matching ECS
        // components when present, with no subclass required.
        let instance_toml = dir_meta.path.join("_instance.toml");
        // Source the TOML through the active SpaceSource (Fjall when
        // migrated, Disk otherwise) then heal/parse in memory — no
        // `std::fs` so a Fjall-authoritative world spawns Parts with
        // zero disk reads. `instance_toml` (absolute) is still handed
        // to `spawn_instance` for InstanceFile identity / write-back.
        let def = take_instance_def(&mut parsed, source, space_path, &instance_toml);
        match def {
            Ok(instance_def) => {
                // spawn_instance attaches InstanceFile internally with the toml_path
                super::instance_loader::spawn_instance(
                    commands,
                    asset_server,
                    materials,
                    material_registry,
                    mesh_cache,
                    decal_materials,
                    instance_toml,
                    instance_def,
                )
            }
            Err(e) => {
                warn!("Failed to load Part folder {:?}: {}", dir_meta.path, e);
                // Fall back to empty folder entity
                commands.spawn((
                    eustress_common::classes::Instance {
                        name: dir_meta.name.clone(),
                        class_name: eustress_common::classes::ClassName::Folder,
                        archivable: true, id: 0, ai: false, uuid: String::new(),
                    },
                    LoadedFromFile {
                        path: dir_meta.path.clone(),
                        file_type: FileType::Directory,
                        service: dir_meta.service.clone(),
                    },
                    Name::new(dir_meta.name.clone()),
                    Transform::default(),
                    Visibility::default(),
                )).id()
            }
        }
    } else if matches!(class_name,
        eustress_common::classes::ClassName::TextLabel
        | eustress_common::classes::ClassName::TextButton
        | eustress_common::classes::ClassName::TextBox
        | eustress_common::classes::ClassName::ImageLabel
        | eustress_common::classes::ClassName::ImageButton
        | eustress_common::classes::ClassName::ViewportFrame,
    ) {
        // Leaf UI class in folder form — new Insert-menu convention
        // writes `Name/_instance.toml` with `class_name = "TextLabel"`
        // (etc.) instead of the legacy flat `Name.textlabel.toml`.
        // Route through the same `spawn_gui_element` helper the flat
        // path uses so Bevy UI components, click handlers, and text
        // rendering all stay in one implementation. The loader at
        // `gui_class_from_extension` now peeks the `_instance.toml`
        // metadata when the file ends in `_instance.toml`, so
        // `spawn_gui_element` resolves the right class without any
        // extra parameter threading here.
        let instance_toml = dir_meta.path.join("_instance.toml");
        match take_gui_def(&mut parsed, source, space_path, &instance_toml) {
            Ok(gui_def) => {
                super::gui_loader::spawn_gui_element(commands, &instance_toml, &gui_def)
            }
            Err(e) => {
                warn!("Failed to load leaf UI folder {:?}: {}", dir_meta.path, e);
                commands.spawn((
                    eustress_common::classes::Instance {
                        name: dir_meta.name.clone(),
                        class_name: eustress_common::classes::ClassName::Folder,
                        archivable: true, id: 0, ai: false, uuid: String::new(),
                    },
                    LoadedFromFile {
                        path: dir_meta.path.clone(),
                        file_type: FileType::Directory,
                        service: dir_meta.service.clone(),
                    },
                    Name::new(dir_meta.name.clone()),
                )).id()
            }
        }
    } else if matches!(class_name,
        eustress_common::classes::ClassName::Atmosphere
        | eustress_common::classes::ClassName::Sky
        | eustress_common::classes::ClassName::Clouds
        | eustress_common::classes::ClassName::DirectionalLight,
    ) {
        // Environment folder — hydrate the AUTHORED Atmosphere / Sky / Clouds /
        // DirectionalLight component from the importer-written [section] so the
        // scene renders with the imported values (not clear_day defaults). We
        // insert the typed component in the SAME spawn, so `hydrate_lighting_
        // entities`' `Without<Sky>`/`Without<EustressAtmosphere>` filters skip
        // these entities (no double-hydrate). `sync_atmosphere_to_rendering`
        // and `manage_cloud_particles` fire on the Changed<> insert.
        use eustress_common::classes::{
            Atmosphere, Sky, SkyboxTextures, Clouds, EustressDirectionalLight,
        };
        use crate::plugins::lighting_plugin::LightingServiceOwner;
        use eustress_common::services::lighting::{AtmosphereRenderingMode, EustressAtmosphere};

        let instance_toml = dir_meta.path.join("_instance.toml");
        let toml_value: Option<toml::Value> =
            take_instance_value(&parsed, source, space_path, &instance_toml);

        // u8 0-255 RGB array (÷255) → [f32;4] alpha 1.0; try int THEN float.
        let rgba4 = |sec: Option<&toml::Value>, key: &str, fallback: [f32; 4]| -> [f32; 4] {
            let Some(arr) = sec.and_then(|s| s.get(key)).and_then(|v| v.as_array()) else {
                return fallback;
            };
            if arr.len() != 3 && arr.len() != 4 {
                return fallback;
            }
            let ch = |i: usize, def: f32| -> f32 {
                arr.get(i)
                    .and_then(|v| v.as_integer().map(|n| n as f32 / 255.0).or_else(|| v.as_float().map(|f| f as f32)))
                    .unwrap_or(def)
            };
            [ch(0, fallback[0]), ch(1, fallback[1]), ch(2, fallback[2]),
             if arr.len() == 4 { ch(3, fallback[3]) } else { fallback[3] }]
        };
        let f32_at = |sec: Option<&toml::Value>, key: &str| -> Option<f32> {
            sec.and_then(|s| s.get(key))
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
                .map(|f| f as f32)
        };
        let bool_at = |sec: Option<&toml::Value>, key: &str| -> Option<bool> {
            sec.and_then(|s| s.get(key)).and_then(|v| v.as_bool())
        };
        let str_at = |sec: Option<&toml::Value>, key: &str| -> Option<String> {
            sec.and_then(|s| s.get(key)).and_then(|v| v.as_str()).map(|s| s.to_string())
        };
        let vec3_at = |sec: Option<&toml::Value>, key: &str| -> Option<[f32; 3]> {
            let arr = sec.and_then(|s| s.get(key)).and_then(|v| v.as_array())?;
            let ch = |i: usize| -> Option<f32> {
                arr.get(i)
                    .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
                    .map(|f| f as f32)
            };
            Some([ch(0)?, ch(1)?, ch(2)?])
        };

        // Position (lights/sky/atmosphere are positionless, but keep transform).
        let position = toml_value
            .as_ref()
            .and_then(|v| v.get("transform"))
            .and_then(|t| t.get("position"))
            .and_then(|p| p.as_array())
            .and_then(|arr| Some(Vec3::new(
                arr.first()?.as_float()? as f32,
                arr.get(1)?.as_float()? as f32,
                arr.get(2)?.as_float()? as f32,
            )))
            .unwrap_or(Vec3::ZERO);

        let spawned = commands.spawn((
            eustress_common::classes::Instance {
                name: dir_meta.name.clone(),
                class_name,
                archivable: true,
                id: 0,
                ai: false,
                uuid: instance_uuid.clone(),
            },
            LoadedFromFile {
                path: dir_meta.path.clone(),
                file_type: FileType::Directory,
                service: dir_meta.service.clone(),
            },
            super::instance_loader::InstanceFile {
                toml_path: instance_toml,
                mesh_path: std::path::PathBuf::new(),
                name: dir_meta.name.clone(),
            },
            Name::new(dir_meta.name.clone()),
            Transform::from_translation(position),
            Visibility::default(),
        )).id();

        match class_name {
            eustress_common::classes::ClassName::Atmosphere => {
                let sec = toml_value.as_ref().and_then(|v| v.get("atmosphere"));
                let d = Atmosphere::default();
                let atmo = Atmosphere {
                    density: f32_at(sec, "density").unwrap_or(d.density),
                    offset: f32_at(sec, "offset").unwrap_or(d.offset),
                    color: rgba4(sec, "color", d.color),
                    // Importer writes `decay_color`; older files may use `decay`.
                    decay: if sec.and_then(|s| s.get("decay_color")).is_some() {
                        rgba4(sec, "decay_color", d.decay)
                    } else {
                        rgba4(sec, "decay", d.decay)
                    },
                    glare: f32_at(sec, "glare").unwrap_or(d.glare),
                    haze: f32_at(sec, "haze").unwrap_or(d.haze),
                };

                // The scattering model. Previously this was always
                // `EustressAtmosphere::default()`, so the authored Rayleigh/Mie
                // coefficients, planet radius, atmosphere thickness and
                // rendering mode never left the file. Nothing noticed, because
                // nothing downstream read them either.
                let ed = EustressAtmosphere::default();
                let eustress_atmo = EustressAtmosphere {
                    // Mirror the artistic properties so a direct
                    // EustressAtmosphere read sees the same values as Atmosphere.
                    density: atmo.density,
                    offset: atmo.offset,
                    color: atmo.color,
                    decay: atmo.decay,
                    glare: atmo.glare,
                    haze: atmo.haze,
                    rendering_mode: match str_at(sec, "rendering_mode")
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "raymarched" | "raymarch" => AtmosphereRenderingMode::Raymarched,
                        _ => ed.rendering_mode,
                    },
                    sky_max_samples: sec
                        .and_then(|s| s.get("sky_max_samples"))
                        .and_then(|v| v.as_integer())
                        .map(|n| n.clamp(8, 128) as u32)
                        .unwrap_or(ed.sky_max_samples),
                    planet_radius: f32_at(sec, "planet_radius").unwrap_or(ed.planet_radius),
                    atmosphere_height: f32_at(sec, "atmosphere_height")
                        .unwrap_or(ed.atmosphere_height),
                    rayleigh_coefficient: vec3_at(sec, "rayleigh_coefficient")
                        .unwrap_or(ed.rayleigh_coefficient),
                    mie_coefficient: f32_at(sec, "mie_coefficient")
                        .unwrap_or(ed.mie_coefficient),
                    mie_direction: f32_at(sec, "mie_direction")
                        .or_else(|| f32_at(sec, "mie_directional_factor"))
                        .unwrap_or(ed.mie_direction),
                    environment_map_enabled: bool_at(sec, "environment_map_enabled")
                        .unwrap_or(ed.environment_map_enabled),
                    environment_intensity: f32_at(sec, "environment_intensity")
                        .unwrap_or(ed.environment_intensity),
                    atmosphere_environment_light: bool_at(sec, "atmosphere_environment_light")
                        .unwrap_or(ed.atmosphere_environment_light),
                };

                commands.entity(spawned).insert((
                    atmo,
                    eustress_atmo,
                    LightingServiceOwner,
                ));
            }
            eustress_common::classes::ClassName::Sky => {
                let sec = toml_value.as_ref().and_then(|v| v.get("sky"));
                let d = Sky::default();
                let sky = Sky {
                    skybox_textures: SkyboxTextures {
                        back: str_at(sec, "skybox_back").unwrap_or_default(),
                        front: str_at(sec, "skybox_front").unwrap_or_default(),
                        left: str_at(sec, "skybox_left").unwrap_or_default(),
                        right: str_at(sec, "skybox_right").unwrap_or_default(),
                        up: str_at(sec, "skybox_top").unwrap_or_default(),
                        down: str_at(sec, "skybox_bottom").unwrap_or_default(),
                    },
                    star_count: sec
                        .and_then(|s| s.get("star_count"))
                        .and_then(|v| v.as_integer())
                        .map(|n| n.max(0) as u32)
                        .unwrap_or(d.star_count),
                    celestial_bodies_shown: bool_at(sec, "celestial_bodies_shown")
                        .unwrap_or(d.celestial_bodies_shown),
                };
                commands.entity(spawned).insert((sky, LightingServiceOwner));
            }
            eustress_common::classes::ClassName::Clouds => {
                let sec = toml_value.as_ref().and_then(|v| v.get("clouds"));
                let d = Clouds::default();
                let clouds = Clouds {
                    enabled: bool_at(sec, "enabled").unwrap_or(d.enabled),
                    density: f32_at(sec, "density").unwrap_or(d.density),
                    // Importer writes `cover` → engine `coverage`.
                    coverage: f32_at(sec, "cover").unwrap_or(d.coverage),
                    color: rgba4(sec, "color", d.color),
                    ..d
                };
                commands.entity(spawned).insert((clouds, LightingServiceOwner));
            }
            _ => {
                // DirectionalLight
                let sec = toml_value.as_ref().and_then(|v| v.get("light"));
                let color = rgba4(sec, "color", [1.0, 1.0, 1.0, 1.0]);
                let brightness = f32_at(sec, "brightness").unwrap_or(1.0);
                let shadows = bool_at(sec, "shadows").unwrap_or(true);
                let mut edl = EustressDirectionalLight::default();
                edl.brightness = brightness;
                edl.color = Color::srgb(color[0], color[1], color[2]);
                edl.shadows = shadows;
                commands.entity(spawned).insert((
                    DirectionalLight {
                        color: Color::srgb(color[0], color[1], color[2]),
                        illuminance: brightness * 10_000.0,
                        shadow_maps_enabled: shadows,
                        shadow_depth_bias: edl.shadow_depth_bias,
                        shadow_normal_bias: edl.shadow_normal_bias,
                        ..default()
                    },
                    edl,
                    LightingServiceOwner,
                ));
            }
        }
        spawned
    } else if matches!(class_name,
        eustress_common::classes::ClassName::PointLight
        | eustress_common::classes::ClassName::SpotLight
        | eustress_common::classes::ClassName::SurfaceLight,
    ) {
        // Light folder — make imported / template lights actually emit.
        //
        // The `_instance.toml` carries a `[Light]` section (from the class
        // template, possibly hand-edited) plus, for Roblox imports,
        // `[properties.extras]` `light_*` keys written by
        // `roblox-import::property_map`. We build the Eustress light
        // component from the template/section defaults, then let the
        // importer's `light_*` extras override (the extras are the *actual*
        // imported values; the template only seeds sane defaults).
        //
        // Brightness is already in lumens on disk — `property_map` applied
        // the ×800 Roblox→lumens scale before writing `light_brightness`,
        // and the `[Light]` template section is authored in lumens too.
        let instance_toml = dir_meta.path.join("_instance.toml");
        let toml_value: Option<toml::Value> =
            take_instance_value(&parsed, source, space_path, &instance_toml);

        // `[light]` / `[Light]` section (case-insensitive).
        let light_section = toml_value
            .as_ref()
            .and_then(|v| v.get("light").or_else(|| v.get("Light")));
        // `[properties.extras]` — importer's `light_*` overrides.
        let extras = toml_value
            .as_ref()
            .and_then(|v| v.get("properties"))
            .and_then(|p| p.get("extras"));

        // Field readers: prefer the extras `light_*` value (real import
        // data), then the `[Light]` section (PascalCase template key),
        // else `None` (component default stands).
        let read_f32 = |sec_key: &str, extra_key: &str| -> Option<f32> {
            extras
                .and_then(|e| e.get(extra_key))
                .and_then(|v| v.as_float().map(|f| f as f32))
                .or_else(|| {
                    light_section
                        .and_then(|l| l.get(sec_key))
                        .and_then(|v| v.as_float().map(|f| f as f32))
                })
        };
        let read_bool = |sec_key: &str, extra_key: &str| -> Option<bool> {
            extras
                .and_then(|e| e.get(extra_key))
                .and_then(|v| v.as_bool())
                .or_else(|| {
                    light_section
                        .and_then(|l| l.get(sec_key))
                        .and_then(|v| v.as_bool())
                })
        };
        // Color: extras `light_color` = [r,g,b] floats (0..1); section
        // `Color` = same shape.
        let read_color = || -> Option<Color> {
            let arr = extras
                .and_then(|e| e.get("light_color"))
                .and_then(|v| v.as_array())
                .or_else(|| {
                    light_section
                        .and_then(|l| l.get("Color"))
                        .and_then(|v| v.as_array())
                })?;
            let r = arr.first()?.as_float()? as f32;
            let g = arr.get(1)?.as_float()? as f32;
            let b = arr.get(2)?.as_float()? as f32;
            Some(Color::srgb(r, g, b))
        };

        // Transform position from `[transform]` (lights are point sources;
        // rotation only matters for SpotLight, handled by the spawner's
        // default-forward emission — Roblox spotlights orient via the part
        // they parent, which our ChildOf link preserves).
        let position = toml_value
            .as_ref()
            .and_then(|v| v.get("transform"))
            .and_then(|t| t.get("position"))
            .and_then(|p| p.as_array())
            .and_then(|arr| {
                Some(Vec3::new(
                    arr.first()?.as_float()? as f32,
                    arr.get(1)?.as_float()? as f32,
                    arr.get(2)?.as_float()? as f32,
                ))
            })
            .unwrap_or(Vec3::ZERO);
        let transform = Transform::from_translation(position);

        let instance = eustress_common::classes::Instance {
            name: dir_meta.name.clone(),
            class_name,
            archivable: true,
            id: 0,
            ai: false,
            uuid: String::new(),
        };

        let spawned = match class_name {
            eustress_common::classes::ClassName::PointLight => {
                let mut light = eustress_common::classes::EustressPointLight::default();
                if let Some(b) = read_f32("Brightness", "light_brightness") { light.brightness = b; }
                if let Some(r) = read_f32("Range", "light_range") { light.range = r; }
                if let Some(rad) = read_f32("Radius", "light_radius") { light.radius = rad; }
                if let Some(c) = read_color() { light.color = c; }
                if let Some(s) = read_bool("Shadows", "light_shadows") { light.shadows = s; }
                crate::spawn::spawn_point_light(commands, instance, light, transform)
            }
            eustress_common::classes::ClassName::SpotLight => {
                let mut light = eustress_common::classes::EustressSpotLight::default();
                if let Some(b) = read_f32("Brightness", "light_brightness") { light.brightness = b; }
                if let Some(r) = read_f32("Range", "light_range") { light.range = r; }
                if let Some(a) = read_f32("Angle", "light_angle") { light.angle = a; }
                if let Some(c) = read_color() { light.color = c; }
                if let Some(s) = read_bool("Shadows", "light_shadows") { light.shadows = s; }
                crate::spawn::spawn_spot_light(commands, instance, light, transform)
            }
            _ => {
                // SurfaceLight
                let mut light = eustress_common::classes::SurfaceLight::default();
                if let Some(b) = read_f32("Brightness", "light_brightness") { light.brightness = b; }
                if let Some(r) = read_f32("Range", "light_range") { light.range = r; }
                if let Some(c) = read_color() { light.color = c; }
                if let Some(s) = read_bool("Shadows", "light_shadows") { light.shadows = s; }
                // Pass the AUTHORED transform so the surface lights from where it
                // was placed (not the origin); light_sync keeps its PointLight
                // intensity/color synced from `brightness`.
                crate::spawn::spawn_surface_light(commands, instance, light, transform)
            }
        };

        // Make the light Properties-panel + Explorer manageable, matching
        // the sibling arms: `InstanceFile` (canonical on-disk marker the
        // panel keys its rich-class editor off) + `LoadedFromFile`.
        commands.entity(spawned).insert((
            super::instance_loader::InstanceFile {
                toml_path: instance_toml,
                mesh_path: std::path::PathBuf::new(),
                name: dir_meta.name.clone(),
            },
            LoadedFromFile {
                path: dir_meta.path.clone(),
                file_type: FileType::Directory,
                service: dir_meta.service.clone(),
            },
        ));
        spawned
    } else {
        // Regular Folder / Model — 3D entity
        let display_name = {
            let mut chars = dir_meta.name.chars();
            match chars.next() {
                None => dir_meta.name.clone(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        };
        commands.spawn((
            eustress_common::classes::Instance {
                name: display_name.clone(),
                class_name,
                archivable: true,
                id: 0,
                ai: false,
                // Carry the UUID so a constraint loaded here can be resolved
                // by identity, and so attachments loaded here are findable
                // as constraint joint endpoints.
                uuid: instance_uuid.clone(),
            },
            LoadedFromFile {
                path: dir_meta.path.clone(),
                file_type: FileType::Directory,
                service: dir_meta.service.clone(),
            },
            Name::new(display_name),
            Transform::default(),
            Visibility::default(),
        )).id()
    };

    // PERF: non-GUI directories inside StarterGui used to be swapped from
    // Transform+Visibility to a hidden `Node` so they wouldn't be "stray
    // non-UI leaves of the UI root". bevy_ui 0.18 silently skips non-Node
    // children (verified in ui_surface::update_children), so the swap only
    // added per-frame ui_layout cost — removed; they keep the normal
    // Transform+Visibility folder shape.
    if let Some(parent) = parent_entity {
        commands.entity(folder_entity).insert(ChildOf(parent));
    }
    toml_spawn_cost::add(&toml_spawn_cost::SPAWN_NS, t_spawn.elapsed());
    let t_reg = std::time::Instant::now();

    registry.register(dir_meta.path.clone(), folder_entity, dir_meta.clone());
    // Also index the entity under its `_instance.toml` marker when one
    // exists. The file watcher delivers Modify/Remove events against
    // the FILE (the marker), not the enclosing folder; without this
    // secondary entry, UPDATE / DELETE hot-reloads on an initially-
    // scanned Part folder couldn't resolve the entity and silently
    // no-op'd. Hot-created entities already register under the
    // `_instance.toml` path (see `handle_file_created` Toml branch),
    // so this just brings initial-scan registration into parity.
    // `instance_src` already answered "is there a marker" through the
    // source; a `Path::is_file` here was one more disk stat per entity.
    let instance_marker = dir_meta.path.join("_instance.toml");
    if has_instance {
        registry.register(instance_marker, folder_entity, dir_meta.clone());
    }
    toml_spawn_cost::add(&toml_spawn_cost::REGISTER_NS, t_reg.elapsed());
    toml_spawn_cost::COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // DEBUG, not INFO: this fires once per directory ENTITY. At 50k
    // (the benchmark) an INFO here is 50k synchronous stderr writes
    // under a lock — ~3-4ms each ≈ minutes of pure logging that
    // throttles the entire load (a log-I/O stall, the same class of
    // bug as the old 53s write storm). Keep it for single-entity
    // debugging only; never on the bulk-load hot path.
    debug!("📁 Spawned Folder '{}' ({} items)", dir_meta.name, dir_meta.children.len());

    // SoulScript folders are full leaves — their children are source +
    // build artefacts (`.rune`, `Summary.md`), never scene entities.
    let is_full_leaf = matches!(class_name, eustress_common::classes::ClassName::SoulScript);
    if is_full_leaf {
        return;
    }

    // Part folders are *partial* leaves: their FILE children are internal
    // assets (`.glb`, `.toml` mesh metadata) that we don't want to surface
    // as scene entities, but their DIRECTORY children CAN be UI hangers-on
    // (BillboardGui labels, SurfaceGui overlays) that the user attached
    // via MindSpace and absolutely must reload across sessions. Without
    // this descent, MindSpace-created billboards survived only one
    // session — the TOML on disk was correct but the loader never saw it.
    let part_subfolders_only = matches!(class_name, eustress_common::classes::ClassName::Part);

    // Spawn all children parented to this folder, frame-budgeted.
    for (idx, child) in dir_meta.children.iter().enumerate() {
        // Once the per-frame budget is spent, spill the REMAINING
        // children (each carries this just-spawned folder as its
        // parent, so linkage is preserved) to the streaming queue and
        // stop. `drain_pending_spawns` continues them over subsequent
        // frames — a 50k subtree never blocks a single frame again.
        // `fetch_sub` returns the value BEFORE decrement, so exactly
        // `budget` children are processed before the first spill; with
        // the `i64::MAX` default (unbudgeted paths) this never trips.
        // The COUNT budget alone did not bound the frame: at the measured
        // ~4 ms per TOML entity, the 8192-entity burst was a 36 s freeze on
        // Super Station. The TIME budget (`spawn_deadline_passed`) is what
        // actually keeps the window alive — see `load_frame_ms`.
        if SPAWN_BUDGET.fetch_sub(1, std::sync::atomic::Ordering::Relaxed) <= 0
            || spawn_deadline_passed()
        {
            // This subtree is bigger than a whole frame's budget — by
            // definition a dense scene. Engage adaptive material-color
            // quantization so its (potentially all-unique) colors
            // collapse into a few batched GPU materials instead of one
            // draw call per part. No-op for any scene that never spills.
            super::material_loader::set_dense_material_mode(true);
            let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
            for rest in &dir_meta.children[idx..] {
                q.push((rest.clone(), Some(folder_entity)));
            }
            break;
        }
        match child.file_type {
            FileType::Directory => {
                spawn_directory_entry(
                    commands, asset_server, meshes, materials, registry,
                    material_registry, mesh_cache, decal_materials, space_path, child, Some(folder_entity),
                    class_defaults, source,
                );
            }
            _ => {
                if part_subfolders_only {
                    // Skip raw files inside a Part folder (`.glb`, mesh
                    // TOML metadata) — those aren't scene entities.
                    continue;
                }
                spawn_file_entry(
                    commands, asset_server, meshes, materials, registry,
                    material_registry, mesh_cache, decal_materials, space_path, child, Some(folder_entity),
                    class_defaults, source,
                );
            }
        }
    }
}

/// Per-frame cap on synchronous entity spawns during Space load.
///
/// A service subtree with this many nodes or fewer loads FULLY in one
/// frame, byte-for-byte as before (normal scenes / the MegaTower city
/// are unchanged — zero regression). Only a larger subtree (the 50k
/// benchmark) spills its overflow to [`SPILL`] and streams over later
/// frames via [`drain_pending_spawns`], eliminating the multi-second
/// single-frame freeze that was the verified 4-FPS root cause
/// (`Workspace ∈ PRIORITY_SERVICES` → entire subtree recursed
/// synchronously in one frame).
const SPAWN_BUDGET_PER_FRAME: i64 = 4096;

/// Default per-frame spawn budget DURING the initial load only. The eager
/// Workspace set (~161K nodes on Vehicle Simulator) drains at
/// [`SPAWN_BUDGET_PER_FRAME`] per frame, which at 4096 is ~40 budget-frames
/// — the ~37s eager-spawn tail. While the user is WAITING on the cold load
/// nothing is interactive, so long frames are fine: bursting the budget
/// ~16× collapses that tail into a handful of long frames. Steady-state
/// editing (after load) keeps the conservative [`SPAWN_BUDGET_PER_FRAME`] so
/// a paste / hot-create / rescan never janks an interactive frame. Override
/// with `EUSTRESS_LOAD_SPAWN_BUDGET` for no-rebuild tuning (e.g. `=65536` to
/// drain effectively the whole queue per frame, or `=4096` to disable the
/// burst entirely).
const LOAD_SPAWN_BUDGET_DEFAULT: i64 = 8192;

/// True ONLY while the initial / rescan / switch load is draining (mirrors
/// `LoadInProgress.active`'s lifecycle exactly). Set in
/// [`begin_budgeted_load`]; cleared in `tick_load_in_progress` the frame
/// `LoadInProgress.active` flips false. Read by [`spawn_budget_per_frame`]
/// from the plain spawn functions (which have no Bevy resource access), so
/// the burst applies to every budget site — priority spawn, the spill
/// threshold, and the per-frame drain — but ONLY during load.
static LOAD_BURST_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// The per-frame spawn budget to arm RIGHT NOW: the burst value while a load
/// is in flight, the conservative steady-state value otherwise.
///
/// During load the burst value comes from `EUSTRESS_LOAD_SPAWN_BUDGET` (parsed
/// once and cached), defaulting to [`LOAD_SPAWN_BUDGET_DEFAULT`]. A malformed
/// or `<= 0` env value falls back to the default so a typo can't stall the
/// load. After load every caller gets [`SPAWN_BUDGET_PER_FRAME`] again, so
/// interactive spawns stay smooth.
fn spawn_budget_per_frame() -> i64 {
    if !LOAD_BURST_ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        return SPAWN_BUDGET_PER_FRAME;
    }
    use std::sync::atomic::{AtomicI64, Ordering};
    // -1 sentinel == "not yet parsed". Cached so we don't re-read the env on
    // every budget arm (multiple per frame).
    static CACHED: AtomicI64 = AtomicI64::new(-1);
    let cached = CACHED.load(Ordering::Relaxed);
    if cached >= 0 {
        return cached;
    }
    let resolved = std::env::var("EUSTRESS_LOAD_SPAWN_BUDGET")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(LOAD_SPAWN_BUDGET_DEFAULT);
    CACHED.store(resolved, Ordering::Relaxed);
    resolved
}

/// Remaining spawn budget for the current frame. `i64::MAX` ==
/// "unbudgeted": any spawn path NOT driven by the budgeted loader
/// (hot-reload, single-entity create, MCP) is completely unaffected.
/// The budgeted systems store a finite value before recursing.
static SPAWN_BUDGET: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(i64::MAX);

/// Overflow queue of `(node, parent)` pairs deferred past the frame
/// budget. `Vec` (not `VecDeque`) so the `static` initializer uses the
/// long-stable const `Vec::new()`; drained as a front batch per frame.
static SPILL: std::sync::Mutex<Vec<(FileMetadata, Option<Entity>)>> =
    std::sync::Mutex::new(Vec::new());

/// `SpaceLoadGeneration` the current [`SPILL`] belongs to. A genuine
/// Space switch bumps the generation and the drain discards stale
/// spill instead of spawning it into the wrong Space.
static SPILL_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Arm the per-frame spawn budget for a fresh FULL load — both the
/// initial `load_space_files_system` AND `apply_space_rescan` (the
/// rescan was a second un-budgeted full-50k path: it re-ran the whole
/// scan+spawn synchronously every time `SpaceRescanNeeded` fired, which
/// is the periodic multi-second stutter). Discards prior spill, tags
/// the generation, gives this frame a full budget.
pub(crate) fn begin_budgeted_load(generation: u64) {
    {
        let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
        q.clear();
    }
    SPILL_GEN.store(generation, std::sync::atomic::Ordering::Relaxed);
    // Arm the LOAD BURST: from here until `tick_load_in_progress` declares the
    // load settled, every budget site uses the bursted per-frame budget so the
    // eager set drains in a handful of long frames instead of ~40. Set BEFORE
    // the store so `spawn_budget_per_frame()` already returns the burst value.
    LOAD_BURST_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    SPAWN_BUDGET.store(spawn_budget_per_frame(), std::sync::atomic::Ordering::Relaxed);
    arm_spawn_deadline();
    // Every fresh load starts assuming a normal-sized scene: lossless
    // material keys (zero visual change). The first frame-budget spill
    // below re-engages dense color quantization for huge scenes only.
    super::material_loader::set_dense_material_mode(false);
}

/// Drop every queued spilled spawn. Called by the Space teardown
/// BEFORE it despawns the outgoing world's entities, so
/// `drain_pending_spawns` can't spawn an old world's children parented
/// to entities that are about to be destroyed (the dead-`ChildOf`
/// orphan storm). The next load's `begin_budgeted_load` re-arms a
/// fresh generation.
pub(crate) fn discard_pending_spawns() {
    let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
    let n = q.len();
    q.clear();
    if n > 0 {
        warn!(
            target: "eustress_engine::world_db",
            "🧹 Discarded {} queued spilled spawns on Space teardown (prevents dead-ChildOf orphan storm)",
            n
        );
    }
}

/// Number of spawns still queued in the frame-budget spill. Feeds the
/// Slint Space-load progress pill so the user sees live progress instead
/// of a silent half-populated scene during large loads.
pub(crate) fn pending_spill_len() -> usize {
    SPILL.lock().unwrap_or_else(|e| e.into_inner()).len()
}

/// Re-arm the budget before each priority service so a small one
/// (`Lighting`) still loads instantly even after a huge one spilled.
pub(crate) fn rearm_priority_budget() {
    SPAWN_BUDGET.store(spawn_budget_per_frame(), std::sync::atomic::Ordering::Relaxed);
    arm_spawn_deadline();
}

// ── Time-based spawn budget ──────────────────────────────────────────────
//
// The count budget (`SPAWN_BUDGET`) bounds how many entities a frame may
// spawn; it does not bound how LONG that takes. Measured on Super Station
// (2026-09-20): the file-loader path costs ~4 ms per TOML entity, so the
// 8192-entity load burst was a single 36 s frame, the spill drain another
// 18 s per 4096-batch frame, and the window read "Not Responding" for most
// of a three-minute open. A per-frame DEADLINE bounds the frame instead:
// once it passes, the remaining children spill exactly as they do when the
// count runs out, and the drain re-queues what it did not reach.
//
// Total load time is CPU-bound either way; what changes is that the editor
// keeps rendering, input keeps flowing, the bridge keeps answering and the
// progress is visible. The slice defaults to 100 ms (≈10 FPS during a heavy
// load — overhead of roughly one frame's render per slice), tunable without
// a rebuild via `EUSTRESS_LOAD_FRAME_MS` (`0` = no time budget, count only).

/// Per-frame spawn time slice in milliseconds during a load.
fn load_frame_ms() -> u64 {
    static V: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var("EUSTRESS_LOAD_FRAME_MS")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(100)
    })
}

/// Monotonic epoch for the deadline (`Instant` cannot live in an atomic).
fn spawn_epoch() -> std::time::Instant {
    static E: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    *E.get_or_init(std::time::Instant::now)
}

/// Deadline as nanoseconds since [`spawn_epoch`]; `u64::MAX` = no deadline.
static SPAWN_DEADLINE_NS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(u64::MAX);

/// Start this frame's spawn slice. Call wherever `SPAWN_BUDGET` is (re)armed.
/// Spawn slice when the user is focused but has not touched anything for
/// the quiet threshold (`EUSTRESS_LOAD_FRAME_MS_IDLE`, default 400 ms).
fn load_frame_ms_idle() -> u64 {
    static V: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var("EUSTRESS_LOAD_FRAME_MS_IDLE")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(400)
    })
}

/// Spawn slice when the window is not focused
/// (`EUSTRESS_LOAD_FRAME_MS_UNFOCUSED`, default 1000 ms).
fn load_frame_ms_unfocused() -> u64 {
    static V: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var("EUSTRESS_LOAD_FRAME_MS_UNFOCUSED")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(1000)
    })
}

/// The slice for this frame, by what the user is doing. The 100 ms default
/// keeps the editor responsive while someone is working in it; once nobody
/// has touched it for a couple of seconds, or it is in the background,
/// responsiveness is worth less than finishing the load, so the slice grows
/// (the per-frame cost of a 120K-entity scene is ~300-400 ms in a debug
/// build, so a 100 ms slice was a 20 % duty cycle on Super Station).
/// `EUSTRESS_LOAD_FRAME_MS=0` disables the time budget in every state.
fn effective_load_frame_ms() -> u64 {
    let base = load_frame_ms();
    if base == 0 {
        return 0;
    }
    match crate::window_focus::user_attention() {
        // Equal time: while the user works, spend at least as long spawning
        // as the rest of the frame costs, up to the idle slice. A fixed
        // 100 ms slice against the 300 ms a 100K-entity scene already costs
        // per frame was a 25 % duty cycle; this holds it at 50 % or better,
        // and on a small scene (tens of ms of overhead) stays at the base.
        crate::window_focus::Attention::Interacting => {
            let overhead_ms = last_frame_overhead_ms();
            overhead_ms.clamp(base, load_frame_ms_idle().max(base))
        }
        crate::window_focus::Attention::IdleFocused => load_frame_ms_idle().max(base),
        crate::window_focus::Attention::Unfocused => load_frame_ms_unfocused().max(base),
    }
}

/// The previous frame's duration and the spawn slice inside it, recorded by
/// `drain_pending_spawns`, so the next slice can be sized to the frame's
/// other work.
static LAST_FRAME_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_SLICE_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn last_frame_overhead_ms() -> u64 {
    use std::sync::atomic::Ordering::Relaxed;
    LAST_FRAME_NS
        .load(Relaxed)
        .saturating_sub(LAST_SLICE_NS.load(Relaxed))
        / 1_000_000
}

pub(crate) fn arm_spawn_deadline() {
    arm_spawn_deadline_with(effective_load_frame_ms());
}

/// Arm a slice of exactly `ms` (0 = no time budget), ignoring attention.
/// The priority spawn uses the base budget: it runs before the first frame
/// is on screen, where a longer idle slice only delays the first render.
pub(crate) fn arm_spawn_deadline_with(ms: u64) {
    let ns = if ms == 0 {
        u64::MAX
    } else {
        spawn_epoch().elapsed().as_nanos() as u64 + ms * 1_000_000
    };
    SPAWN_DEADLINE_NS.store(ns, std::sync::atomic::Ordering::Relaxed);
}

/// True once the current frame's spawn slice is used up.
#[inline]
pub(crate) fn spawn_deadline_passed() -> bool {
    let d = SPAWN_DEADLINE_NS.load(std::sync::atomic::Ordering::Relaxed);
    d != u64::MAX && spawn_epoch().elapsed().as_nanos() as u64 >= d
}

/// True from `begin_budgeted_load` until the eager spawn settles: the
/// window in which spilled entities stream in over frames.
pub fn bulk_load_active() -> bool {
    LOAD_BURST_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
        || !SPILL.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
}

/// Run condition for per-frame UI syncs (Explorer, GUI elements, tags,
/// Soul panel, lighting hydration). Every frame normally. During a bulk
/// load, once per `UI_SYNC_LOAD_INTERVAL_MS`, and every system that asks in
/// the same frame gets the same answer. Each of those syncs is change-
/// driven (tick-based `Added`/`Changed` filters or its own content hash),
/// so skipped frames batch the work rather than lose it. Measured on Super
/// Station's drain: the syncs cost ~30 ms per frame themselves, and the
/// Slint model churn they caused made the overlay repaint every frame at
/// 59 ms, which together starved the 100 ms spawn slice to a 39 % duty.
pub fn ui_sync_tick(frames: Res<bevy::diagnostic::FrameCount>) -> bool {
    ui_sync_tick_for_frame(frames.0 as u64)
}

/// The same tick for use inside a system body (pass its `FrameCount`).
pub fn ui_sync_tick_for_frame(frame: u64) -> bool {
    UI_TICK.fire(frame, 250, 4)
}

/// A slower tick (2 s, 8 frames) for the syncs whose push replaces a whole
/// Slint model, the Explorer above all: during a bulk load its tree changes
/// every frame, and each push re-instantiates every row on the next paint.
pub fn ui_sync_tick_slow(frames: Res<bevy::diagnostic::FrameCount>) -> bool {
    UI_TICK_SLOW.fire(frames.0 as u64, 2000, 8)
}

struct LoadTick {
    last_fire_ns: std::sync::atomic::AtomicU64,
    fire_frame: std::sync::atomic::AtomicU64,
}

static UI_TICK: LoadTick = LoadTick {
    last_fire_ns: std::sync::atomic::AtomicU64::new(0),
    fire_frame: std::sync::atomic::AtomicU64::new(u64::MAX),
};
static UI_TICK_SLOW: LoadTick = LoadTick {
    last_fire_ns: std::sync::atomic::AtomicU64::new(0),
    fire_frame: std::sync::atomic::AtomicU64::new(u64::MAX),
};

impl LoadTick {
    /// True every frame outside a bulk load. During one, true once both
    /// `interval_ms` and `min_frames` have passed since it last fired, and
    /// the same answer for every caller in a frame. The frame floor is what
    /// does the work on a large Space: once load frames exceed the interval
    /// (they were 1 to 2 s on Super Station) a wall-clock tick alone fires
    /// every frame and throttles nothing.
    fn fire(&self, frame: u64, interval_ms: u64, min_frames: u64) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        if !bulk_load_active() {
            return true;
        }
        let last_frame = self.fire_frame.load(Relaxed);
        if last_frame == frame {
            return true;
        }
        let frames_since = if last_frame == u64::MAX {
            u64::MAX
        } else {
            frame.saturating_sub(last_frame)
        };
        let now = spawn_epoch().elapsed().as_nanos() as u64;
        if now.saturating_sub(self.last_fire_ns.load(Relaxed)) >= interval_ms * 1_000_000
            && frames_since >= min_frames
        {
            self.last_fire_ns.store(now, Relaxed);
            self.fire_frame.store(frame, Relaxed);
            return true;
        }
        false
    }
}

/// While a bulk load runs, cap how much virtual time one frame may advance.
/// After a 700 ms load frame the fixed schedule otherwise replays ~15 fixed
/// steps to catch up (~13 ms per frame measured on Super Station's drain),
/// in edit mode where physics is paused and nothing needs them. The
/// previous cap is restored at settle.
fn bound_fixed_catchup_during_load(
    mut virt: ResMut<Time<Virtual>>,
    mut saved: Local<Option<std::time::Duration>>,
) {
    if bulk_load_active() {
        if saved.is_none() {
            *saved = Some(virt.max_delta());
            virt.set_max_delta(std::time::Duration::from_millis(50));
        }
    } else if let Some(d) = saved.take() {
        virt.set_max_delta(d);
    }
}

/// Per-stage cost accumulators for the TOML / file-loader spawn path.
///
/// The binary-ECS path has had `world_db_binary::spawn_cost` since M0; this
/// path — the one every loose-TOML Space and every non-Part entity of a
/// DB Space goes through — had nothing, which is why "~4.4 ms per entity"
/// stayed a number without a culprit. Three stages, one atomic add each,
/// summarised once per load at the settle point. Always on: the cost is
/// three `Instant::now()` pairs per entity.
pub(crate) mod toml_spawn_cost {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    pub static READ_PARSE_NS: AtomicU64 = AtomicU64::new(0);
    pub static SPAWN_NS: AtomicU64 = AtomicU64::new(0);
    pub static REGISTER_NS: AtomicU64 = AtomicU64::new(0);
    pub static COUNT: AtomicU64 = AtomicU64::new(0);

    #[inline]
    pub fn add(slot: &AtomicU64, d: std::time::Duration) {
        slot.fetch_add(d.as_nanos() as u64, Relaxed);
    }

    pub fn log_summary_and_reset() {
        let n = COUNT.swap(0, Relaxed);
        let rp = READ_PARSE_NS.swap(0, Relaxed);
        let sp = SPAWN_NS.swap(0, Relaxed);
        let rg = REGISTER_NS.swap(0, Relaxed);
        if n == 0 {
            return;
        }
        let ms = |ns: u64| ns as f64 / 1e6;
        let per = |ns: u64| ns as f64 / 1e3 / n as f64;
        bevy::log::warn!(
            target: "eustress_engine::load_phase",
            "TOML-SPAWN-COST: {} entities — read+parse {:.0} ms ({:.0} µs/ea), spawn_instance {:.0} ms ({:.0} µs/ea), register {:.0} ms ({:.0} µs/ea)",
            n, ms(rp), per(rp), ms(sp), per(sp), ms(rg), per(rg)
        );
    }
}

/// Priority services loaded immediately at startup (the 3D scene).
/// Everything else is deferred to avoid blocking the first frame.
pub const PRIORITY_SERVICES: &[&str] = &["Workspace", "Lighting"];

/// Monotonically-incrementing counter bumped on every `open_space` call.
/// DeferredServiceLoader stamps the generation it was built for; if the
/// live counter moves on (rename, switch, or rapid re-open) the stale
/// queue is discarded before it can load entries into the wrong Space.
#[derive(Resource, Default)]
pub struct SpaceLoadGeneration(pub u64);

/// Resource tracking deferred service loading state
#[derive(Resource, Default)]
pub struct DeferredServiceLoader {
    /// Services waiting to be loaded (one per frame)
    pub pending: Vec<FileMetadata>,
    /// Whether the initial priority load has completed
    pub priority_done: bool,
    /// Generation counter stamped when this queue was built.
    /// Compared against `SpaceLoadGeneration` each frame; mismatch
    /// means a Space switch happened mid-load and the queue is stale.
    pub generation: u64,
}

/// Gate that suppresses `write_instance_changes_system` while the
/// loader is materialising entities from disk. Without it, downstream
/// systems that fire on first-load (mesh-handle resolve →
/// `update_base_part_size_from_mesh`, class-default backfill, material
/// registry resolve) mark `BasePart` as `Changed`, which the writer
/// then persists straight back to disk — at 50k entities that costs
/// ~53 s of background TOML writes for zero useful work.
///
/// Lifecycle:
/// - `load_space_files_system` (Startup) sets `active = true`.
/// - `apply_space_rescan` (Update) sets `active = true` on every rescan.
/// - `open_space` (in `space_ops`) sets `active = true` on Space switch.
/// - `tick_load_in_progress` (Update, runs after `load_deferred_services`)
///   increments `frames_since_quiescent` each frame the deferred queue is
///   empty and `priority_done`. Once the count reaches
///   `QUIESCENT_THRESHOLD`, `active` flips to false and live writes
///   resume.
#[derive(Resource, Debug, Default)]
pub struct LoadInProgress {
    pub active: bool,
    pub frames_since_quiescent: u32,
    /// When the deferred queue was last seen empty with priority done.
    pub quiescent_since: Option<std::time::Instant>,
}

impl LoadInProgress {
    /// The deferred queue must stay empty for BOTH of these before the load
    /// is declared settled: enough frames to absorb the async mesh-handle
    /// resolution + BasePart-size sync that runs after the last spawn, and
    /// enough wall time that the frame count means the same at any frame
    /// rate. The old 60-frame rule was sized for 16 ms frames; at the 300 ms
    /// frames of a large load it was an 18 s wait with write-back gated.
    pub const QUIESCENT_FRAMES: u32 = 3;
    pub const QUIESCENT_TIME: std::time::Duration = std::time::Duration::from_secs(1);

    /// Mark loading as active. Called by the load entry-points so the
    /// quiescent counter restarts whenever a fresh load begins.
    pub fn begin(&mut self) {
        self.active = true;
        self.frames_since_quiescent = 0;
        self.quiescent_since = None;
    }
}

/// Update system: advances the quiescent counter when the deferred
/// queue is empty + priority_done, and flips `active` off once the
/// settle threshold passes. Any frame with pending work resets the
/// counter so a Space switch (or partial rescan) keeps writes gated
/// until the new load also settles.
pub fn tick_load_in_progress(
    deferred: Res<DeferredServiceLoader>,
    mut load: ResMut<LoadInProgress>,
) {
    if !load.active {
        return;
    }
    if deferred.priority_done && deferred.pending.is_empty() {
        load.frames_since_quiescent = load.frames_since_quiescent.saturating_add(1);
        let since = *load
            .quiescent_since
            .get_or_insert_with(std::time::Instant::now);
        if load.frames_since_quiescent >= LoadInProgress::QUIESCENT_FRAMES
            && since.elapsed() >= LoadInProgress::QUIESCENT_TIME
        {
            load.active = false;
            // Disarm the LOAD BURST: steady-state spawns (paste, hot-create,
            // rescan spill) revert to the conservative per-frame budget so an
            // interactive frame never janks. Mirrors LoadInProgress.active.
            LOAD_BURST_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
            // Initial load done — let paste / hot-reload spawn parts normally.
            STREAM_DB_PARTS.store(false, std::sync::atomic::Ordering::Relaxed);
            // LOAD-PHASE milestone 5: eager spawn settled — the priority
            // services finished spawning + the deferred queue drained + the
            // async mesh/material backfill quiesced (LoadInProgress.active
            // flips false here). For a streaming Space this is also when the
            // residency manager is allowed to start filling the camera box.
            super::load_phase::mark("eager-spawn-complete");
            // M0 (diagnostics): emit the SPAWN-COST breakdown for the cores
            // spawned during this load — decode vs arch_to_instance vs
            // spawn_instance totals — at the same settle point the LOAD-PHASE
            // mark fires. Env-gated on EUSTRESS_PROFILE; silent otherwise.
            #[cfg(feature = "world-db")]
            super::world_db_binary::spawn_cost::log_summary();
            // Same settle point, TOML/file-loader path (always on — it is
            // one log line per load).
            toml_spawn_cost::log_summary_and_reset();
            dir_markers::clear();
            clear_read_cache();
            info!(
                "🟢 Load settled — TOML write-back enabled after {} quiescent frames",
                load.frames_since_quiescent
            );
        }
    } else {
        load.frames_since_quiescent = 0;
        load.quiescent_since = None;
    }
}

/// Startup system: load only Workspace + Lighting immediately.
/// Other services are queued for deferred loading (one service per frame).
/// Walk `root` recursively and repair the `_instance.toml/_instance.toml`
/// corruption pattern: a directory named `_instance.toml` containing a
/// single file also named `_instance.toml` inside it. This shape arises
/// when a buggy spawner calls `create_dir_all(parent.join("_instance.toml"))`
/// + `fs::write(parent.join("_instance.toml/_instance.toml"), ...)`,
/// which is exactly what happened on 2026-04-25 to `Part-7ed7/`.
///
/// Repair: move the inner file up one level (`<parent>/_instance.toml`)
/// and remove the now-empty wrapper directory. If any unexpected
/// content lives alongside the inner file, leaves it alone and logs a
/// warning rather than risk data loss.
///
/// Returns the number of folders healed. Caller logs the total so a
/// silent repair pass on a clean tree stays quiet.
fn repair_reserved_name_corruption(root: &Path) -> u32 {
    let mut healed = 0u32;
    let mut stack: Vec<std::path::PathBuf> = vec![root.to_path_buf()];

    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else { continue };
            if !file_type.is_dir() { continue; }

            // Recurse first so nested corruption gets healed bottom-up.
            stack.push(path.clone());

            // Is this directory the corrupt-`_instance.toml` shape?
            // (a directory whose name is the reserved marker)
            let dir_name_is_reserved = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.eq_ignore_ascii_case("_instance.toml"))
                .unwrap_or(false);
            if !dir_name_is_reserved { continue; }

            let parent = match path.parent() {
                Some(p) => p,
                None => continue,
            };
            let target = parent.join("_instance.toml");

            // Don't overwrite a real sibling _instance.toml file.
            if target.is_file() {
                tracing::warn!(
                    "🛠 Skipping repair of {:?} — sibling {:?} already exists as a file",
                    path, target
                );
                continue;
            }

            // The wrapper dir should contain exactly one `_instance.toml` file.
            let inner_file = path.join("_instance.toml");
            if !inner_file.is_file() {
                tracing::warn!(
                    "🛠 Skipping repair of {:?} — inner {:?} not present as a file",
                    path, inner_file
                );
                continue;
            }

            // Bail if the wrapper has unexpected siblings (we don't
            // want to silently lose data).
            let extra_count = std::fs::read_dir(&path)
                .map(|d| d.flatten()
                    .filter(|e| e.file_name() != "_instance.toml")
                    .count())
                .unwrap_or(0);
            if extra_count > 0 {
                tracing::warn!(
                    "🛠 Skipping repair of {:?} — has {} unexpected sibling entries",
                    path, extra_count
                );
                continue;
            }

            // Two-step move so we don't ever try to rename a file
            // into the path of its own parent directory (the wrapper).
            let temp = parent.join(".__instance_toml_repair.tmp");
            if let Err(e) = std::fs::rename(&inner_file, &temp) {
                tracing::warn!("🛠 repair: move-out of {:?} failed: {}", inner_file, e);
                continue;
            }
            if let Err(e) = std::fs::remove_dir(&path) {
                tracing::warn!("🛠 repair: rmdir of wrapper {:?} failed: {}", path, e);
                // Try to put the file back so we don't leave a tmp orphan.
                let _ = std::fs::rename(&temp, &inner_file);
                continue;
            }
            if let Err(e) = std::fs::rename(&temp, &target) {
                tracing::warn!("🛠 repair: rename to final {:?} failed: {}", target, e);
                continue;
            }
            healed += 1;
            tracing::info!("🛠 repaired _instance.toml corruption at {:?}", parent);
        }
    }

    healed
}

/// Set true at the start of loading a large, DB-backed (migrated) Space. While
/// set, the file-loader SKIPS bulk-spawning bare (`BinaryEcs`-representation)
/// parts because the residency manager streams them from the `entities`
/// partition by camera locality — avoiding the ~200K-live double-load that
/// pinned huge imports at ~2 FPS. Reset to false when the load settles
/// (`tick_load_in_progress`) so paste / hot-reload create parts normally.
/// Custom-mesh / file-natured parts and parts with child dirs always spawn.
static STREAM_DB_PARTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn load_space_files_system(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<SpaceFileRegistry>,
    mut material_registry: ResMut<super::material_loader::MaterialRegistry>,
    mut mesh_cache: ResMut<super::instance_loader::PrimitiveMeshCache>,
    mut decal_materials: ResMut<Assets<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>>,
    space_root: Res<super::SpaceRoot>,
    class_defaults: Option<Res<super::class_defaults::ClassDefaultsRegistry>>,
    mut deferred: ResMut<DeferredServiceLoader>,
    gen: Res<SpaceLoadGeneration>,
    mut load_in_progress: ResMut<LoadInProgress>,
    active_source: Res<super::space_source::ActiveSpaceSource>,
    mut prescan: ResMut<PendingPreScan>,
) {
    // First statement, so the milestone log shows whether a silent gap
    // before `scan-begin` is inside this system or in the frame before it.
    super::load_phase::mark("loader-begin");
    let space_path = &space_root.0;
    let source = active_source.0.clone();
    let source = source.as_ref();

    if !space_path.exists() {
        warn!("Space path does not exist: {:?}", space_path);
        return;
    }

    // Gate TOML write-back until the load settles (see LoadInProgress
    // docstring for the failure mode this prevents).
    load_in_progress.begin();

    // Ensure the Space has all required service folders and lighting TOMLs.
    // This covers the initial-startup path where SpaceRoot is inserted
    // directly (e.g. --space flag or auto-resume) without going through
    // open_space(), which normally calls ensure_space_integrity().
    // Idempotent — never overwrites existing files.
    super::space_ops::ensure_space_integrity(space_path);

    // One-pass repair: walk the Space tree and fix any
    // `<entity>/_instance.toml/_instance.toml` corruption produced by
    // older builds (or external tools) that mistook the marker
    // filename for a folder name. Idempotent — does nothing on a
    // clean tree. Runs before `scan_space_directory` so the loader
    // sees the healed structure.
    //
    // Skipped for a migrated `.eustress` world: there are no loose
    // instance folders on disk to repair (they live in
    // `world.fjalldb/`), so this would only be a wasted full-tree
    // `std::fs` walk on every load.
    // The reserved-name repair walks EVERY directory of the Space on disk
    // (7.8 s on Super Station's 114K files, measured with `loader-begin`).
    // It fixes folder names the disk loader would misread; with the DB
    // installed as the source the loader spawns from the tree and never
    // reads those names, so the walk is skipped there. It still runs for a
    // disk-backed Space, and the header's `migrated_at` gate stays as it is
    // (this Space's header has none, yet its DB is authoritative).
    if !super::space_ops::space_is_migrated(space_path) && !super::active_db::is_active() {
        let healed = repair_reserved_name_corruption(space_path);
        if healed > 0 {
            warn!(
                "🛠 Repaired {} corrupt _instance.toml folder(s) under {:?}",
                healed, space_path
            );
        }
    }

    // LOAD-PHASE milestone 3a: service scan begins. The scan discovers
    // services + walks the eager (Workspace/Lighting/StarterGui) subtrees;
    // lazy storage services emit header-only (file_loader's existing gate).
    super::load_phase::mark("scan-begin");
    let scan_t0 = std::time::Instant::now();
    // The open worker pre-scans a DB-backed Space while this thread does its
    // first-frame work; take that when it is for this Space, else scan here.
    let entries = match prescan.0.take() {
        Some(p) if p.root == *space_path => {
            info!(
                target: "eustress_engine::world_db",
                "🗂 Entry tree taken from the open worker's pre-scan ({} top-level entries)",
                p.entries.len()
            );
            p.entries
        }
        _ => scan_space_directory(source, space_path),
    };
    info!(
        target: "eustress_engine::world_db",
        "🔍 Discovered {} top-level entries in Space (scan took {:?})",
        entries.len(),
        scan_t0.elapsed()
    );
    // LOAD-PHASE milestone 3b: service scan complete.
    super::load_phase::mark("scan-complete");

    // Parallel pre-read+parse pass: read + UTF-8-decode every text node
    // the spawn walk will consume, across the whole rayon pool, into
    // READ_CACHE BEFORE the (still sequential, still main-thread) spawn
    // begins. This collapses the serial small-file I/O that dominated
    // opening huge Spaces (e.g. the 161K-file Vehicle Simulator place)
    // onto all cores. Spawn/parenting/registry/ordering logic below is
    // unchanged — it now just finds the text already in memory. Cleared
    // after the priority spawn returns (`clear_read_cache`).
    prewarm_in_background(active_source.0.clone(), space_path, &entries);
    // LOAD-PHASE milestone 4: parallel pre-read of eager-service text done
    // (READ_CACHE populated; the spawn walk below reads from memory).
    super::load_phase::mark("prewarm-started");

    let cd_ref = class_defaults.as_deref();
    let mut deferred_entries = Vec::new();

    // New Space load → discard any spill left from a previous Space and
    // tag the spill generation so a later switch can discard ours.
    {
        let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
        q.clear();
    }
    SPILL_GEN.store(gen.0, std::sync::atomic::Ordering::Relaxed);
    // This IS a bulk load: everything keyed on `bulk_load_active()` (the
    // UI sync tick, the billboard raster cap, the power-mode hold, the
    // spawn burst budget) must see it from here until `tick_load_in_progress`
    // settles. Only the Space-switch path used to raise the flag, so on the
    // initial open the drain ran with every per-frame throttle disengaged.
    LOAD_BURST_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);

    // STREAMING-PRIMARY gate: only skip file-spawning parts when the residency
    // manager will actually stream them — match ITS enable condition (a
    // DB-backed Space with more binary cores than the big-Space threshold). On
    // a small or non-DB Space this stays false and the loader spawns everything
    // as before.
    {
        const BIG_SPACE_THRESHOLD: usize = 100_000; // mirrors ResidencyConfig::big_space_threshold
        let stream = super::active_db::is_active()
            && super::active_db::count_instance_cores_capped(BIG_SPACE_THRESHOLD + 1)
                > BIG_SPACE_THRESHOLD;
        STREAM_DB_PARTS.store(stream, std::sync::atomic::Ordering::Relaxed);
        if stream {
            warn!(
                target: "eustress_engine::world_db",
                "STREAMING-PRIMARY: large DB Space — file-loader will skip bare parts (residency streams them by camera)"
            );
        }
    }

    for entry in entries {
        let is_priority = PRIORITY_SERVICES.iter().any(|s| entry.name == *s);

        if is_priority {
            // Load immediately — this is the 3D scene. Frame-budgeted:
            // each priority service gets a fresh budget so a huge one
            // (`Workspace` with the 50k grid) spawns up to
            // SPAWN_BUDGET_PER_FRAME this frame and SPILLS the rest to
            // stream over later frames — no multi-second freeze — while
            // a small one (`Lighting`) still loads fully and instantly.
            // `prio_t0` elapsed should now be small even at 50k.
            let prio_t0 = std::time::Instant::now();
            // Watchdog: this spawn is synchronous and unbounded below the
            // budget threshold — a service subtree smaller than the budget
            // never spills, so it all lands in one frame however long that
            // takes. Guarded so the popup names the service that is stuck.
            let _phase = super::load_phase::scope(
                "priority-spawn",
                format!("spawning service '{}'", entry.name),
            );
            SPAWN_BUDGET.store(spawn_budget_per_frame(), std::sync::atomic::Ordering::Relaxed);
            // Arm the TIME budget here too. It used to be armed only by
            // `drain_pending_spawns` in earlier frames, which held while the
            // loader waited on the reconcile; now that the loader starts in
            // the first frames, no drain has run yet, the deadline is unset,
            // and the priority spawn ran to the 8,192 count budget (5.1 s
            // measured) instead of one slice.
            arm_spawn_deadline_with(load_frame_ms());
            match entry.file_type {
                FileType::Directory => {
                    spawn_directory_entry(
                        &mut commands, &asset_server, &mut meshes, &mut materials,
                        &mut registry, &mut material_registry, &mut mesh_cache, &mut decal_materials, space_path, &entry, None,
                        cd_ref, source,
                    );
                }
                _ => {
                    spawn_file_entry(
                        &mut commands, &asset_server, &mut meshes, &mut materials,
                        &mut registry, &mut material_registry, &mut mesh_cache, &mut decal_materials, space_path, &entry, None,
                        cd_ref, source,
                    );
                }
            }
            let prio_elapsed = prio_t0.elapsed();
            // Always WARN-level so it is visible without RUST_LOG. With
            // the frame budget this should read SMALL even at 50k (only
            // SPAWN_BUDGET_PER_FRAME spawned this frame, rest spilled).
            // A multi-second value here means the budget is NOT being
            // applied on this path.
            warn!(
                target: "eustress_engine::world_db",
                service = %entry.name,
                "⚡ Priority '{}' spawned in {:?} (frame-budgeted — small == fix working; multi-second == budget not applied)",
                entry.name, prio_elapsed
            );
        } else {
            deferred_entries.push(entry);
        }
    }

    info!("📋 Deferred {} services for background loading", deferred_entries.len());
    deferred.pending = deferred_entries;
    deferred.priority_done = true;
    // Stamp the generation so load_deferred_services can detect a
    // mid-load Space switch and discard this queue.
    deferred.generation = gen.0;

    // The pre-read cache stays alive through the drain: every spilled entity
    // reads its `_instance.toml` from it instead of from the source (one
    // Fjall point read each, ~100 µs in a debug build, ~11 s over Super
    // Station's 111K entities). `tick_load_in_progress` drops it at settle;
    // from then on a cache miss is a correct live read.
}

/// Queue of freshly-pasted folder ROOTS (absolute paths, normally under
/// Workspace) that must be spawned DETERMINISTICALLY instead of via the
/// (unreliable) file watcher. `clipboard::spawn_pasted_entity` pushes here
/// right after it copies the folder tree to disk; `drain_paste_spawn_queue`
/// then scans + spawns the WHOLE subtree parent-first by reusing the cold-load
/// `spawn_directory_entry`, so a pasted Part brings its BillboardGui/TextLabel
/// children correctly attached — no dependency on notify event ordering/timing
/// (which dropped/orphaned children at low FPS and under churn).
#[derive(Resource, Default)]
pub struct PasteSpawnQueue {
    pub folders: Vec<std::path::PathBuf>,
}

/// Deterministically spawn pasted folder subtrees queued in [`PasteSpawnQueue`].
/// Scans the pasted folder through a `DiskSource` (the files are on disk; the
/// active source may be Fjall, which wouldn't see them yet) and spawns the tree
/// with `spawn_directory_entry`, parented to the Workspace service. Entities are
/// registered, so the file watcher's later `is_loaded` check skips the same
/// paths — no double-spawn.
pub fn drain_paste_spawn_queue(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<SpaceFileRegistry>,
    mut material_registry: ResMut<super::material_loader::MaterialRegistry>,
    mut mesh_cache: ResMut<super::instance_loader::PrimitiveMeshCache>,
    mut decal_materials: ResMut<Assets<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>>,
    space_root: Res<super::SpaceRoot>,
    class_defaults: Option<Res<super::class_defaults::ClassDefaultsRegistry>>,
    mut queue: ResMut<PasteSpawnQueue>,
    // Paste selects what it created. `handle_paste_event` can only do that
    // for entities it spawns inline; folder-form instances (every Part, and
    // anything with children) land HERE a frame later, so its `created_ids`
    // is empty and the selection stays on the ORIGINAL. That is what made
    // Duplicate feel broken: the copy lands exactly under the source, the
    // source keeps the move gizmo, and the gizmo's hit-test claims the click
    // before selection ever runs — so the copy could not be clicked at all
    // until the user deselected first.
    selection_manager: Option<Res<crate::rendering::BevySelectionManager>>,
) {
    if queue.folders.is_empty() {
        return;
    }
    let space_path = space_root.0.clone();
    let disk = super::space_source::DiskSource::new(space_path.clone());
    let cd_ref = class_defaults.as_deref();
    let workspace_dir = space_path.join("Workspace");
    let folders: Vec<std::path::PathBuf> = queue.folders.drain(..).collect();
    // Roots spawned by THIS drain, in queue order, so the selection ends up
    // matching what the user just pasted.
    let mut spawned_roots: Vec<std::path::PathBuf> = Vec::new();
    for folder in folders {
        if registry.is_loaded(&folder) {
            continue; // already spawned (watcher beat us) — don't duplicate
        }
        if !folder.exists() {
            warn!("📋 paste-spawn: folder vanished before load: {:?}", folder);
            continue;
        }
        let rel = super::space_source::rel_from_root(&space_path, &folder).unwrap_or_default();
        let children = scan_dir_entries(&disk, &space_path, &rel, "Workspace");
        let name = folder
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Pasted")
            .to_string();
        let dir_meta = FileMetadata {
            path: folder.clone(),
            file_type: FileType::Directory,
            service: "Workspace".to_string(),
            name,
            size: 0,
            modified: std::time::SystemTime::now(),
            children,
        };
        // Parent the pasted top-level entity to the Workspace service so it
        // sits in the scene tree exactly like a cold-loaded part.
        let parent_entity = registry.get_entity(&workspace_dir);
        spawn_directory_entry(
            &mut commands,
            &asset_server,
            &mut meshes,
            &mut materials,
            &mut registry,
            &mut material_registry,
            &mut mesh_cache,
            &mut decal_materials,
            &space_path,
            &dir_meta,
            parent_entity,
            cd_ref,
            &disk,
        );
        info!("📋 paste: deterministically spawned pasted subtree {:?}", folder);
        spawned_roots.push(folder);
    }

    // Select the pasted roots. `spawn_directory_entry` registers rather than
    // returning an entity, so resolve through the registry the same way the
    // template-insert path does — folder-form instances are keyed under BOTH
    // the folder and its `_instance.toml`, and the marker is what the spawner
    // writes, so probe that first.
    if spawned_roots.is_empty() {
        return;
    }
    let Some(selection_manager) = selection_manager else { return };
    let ids: Vec<String> = spawned_roots
        .iter()
        .filter_map(|folder| {
            registry
                .get_entity(&folder.join("_instance.toml"))
                .or_else(|| registry.get_entity(folder))
        })
        .map(crate::entity_utils::entity_to_id_string)
        .collect();
    if ids.is_empty() {
        // Spawned but not resolvable — leave the previous selection alone
        // rather than clearing it, so a registry miss can't silently drop
        // whatever the user had selected.
        warn!("📋 paste: spawned {} subtree(s) but none resolved to an entity — selection unchanged",
              spawned_roots.len());
        return;
    }
    let count = ids.len();
    selection_manager.0.write().set_selected(ids);
    info!("📋 paste: selected {} pasted object(s)", count);
}

/// Update system: loads one deferred service per frame to keep the viewport responsive.
pub fn load_deferred_services(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<SpaceFileRegistry>,
    mut material_registry: ResMut<super::material_loader::MaterialRegistry>,
    mut mesh_cache: ResMut<super::instance_loader::PrimitiveMeshCache>,
    mut decal_materials: ResMut<Assets<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>>,
    space_root: Res<super::SpaceRoot>,
    class_defaults: Option<Res<super::class_defaults::ClassDefaultsRegistry>>,
    mut deferred: ResMut<DeferredServiceLoader>,
    gen: Res<SpaceLoadGeneration>,
    active_source: Res<super::space_source::ActiveSpaceSource>,
) {
    if deferred.pending.is_empty() { return; }

    // Generation mismatch — a Space switch (or rename) happened while this
    // queue was draining. Discard the stale entries; open_space already
    // set SpaceRescanNeeded so the new Space will build a fresh queue.
    if deferred.generation != gen.0 {
        let discarded = deferred.pending.len();
        deferred.pending.clear();
        info!("🗑️ Discarded {} stale deferred entries (generation {} → {})",
              discarded, deferred.generation, gen.0);
        return;
    }

    // Load one service per frame
    let entry = deferred.pending.remove(0);
    let space_path = &space_root.0;
    let cd_ref = class_defaults.as_deref();
    let remaining = deferred.pending.len();
    let source = active_source.0.clone();
    let source = source.as_ref();

    match entry.file_type {
        FileType::Directory => {
            spawn_directory_entry(
                &mut commands, &asset_server, &mut meshes, &mut materials,
                &mut registry, &mut material_registry, &mut mesh_cache, &mut decal_materials, space_path, &entry, None,
                cd_ref, source,
            );
        }
        _ => {
            spawn_file_entry(
                &mut commands, &asset_server, &mut meshes, &mut materials,
                &mut registry, &mut material_registry, &mut mesh_cache, &mut decal_materials, space_path, &entry, None,
                cd_ref, source,
            );
        }
    }

    info!("📦 Loaded service: {} ({} remaining)", entry.name, remaining);
}

/// Update system: streams the frame-budget overflow. Each frame it
/// spawns up to [`SPAWN_BUDGET_PER_FRAME`] of the `(node, parent)`
/// pairs that [`spawn_directory_entry`] spilled, so a huge subtree (the
/// 50k benchmark) loads progressively over ~a dozen frames instead of
/// freezing one frame for seconds. Params mirror
/// [`load_deferred_services`] plus `LoadInProgress` (kept armed while
/// streaming so persistence/quiescence don't fire mid-load).
pub fn drain_pending_spawns(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<SpaceFileRegistry>,
    mut material_registry: ResMut<super::material_loader::MaterialRegistry>,
    mut mesh_cache: ResMut<super::instance_loader::PrimitiveMeshCache>,
    mut decal_materials: ResMut<Assets<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>>,
    space_root: Res<super::SpaceRoot>,
    class_defaults: Option<Res<super::class_defaults::ClassDefaultsRegistry>>,
    gen: Res<SpaceLoadGeneration>,
    active_source: Res<super::space_source::ActiveSpaceSource>,
    mut load_in_progress: ResMut<LoadInProgress>,
    time: Res<Time>,
) {
    use std::sync::atomic::Ordering::Relaxed;

    // Space switch while spill is pending → discard it (it belongs to
    // the previous Space; spawning it now would corrupt the new one).
    if SPILL_GEN.load(Relaxed) != gen.0 {
        let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
        if !q.is_empty() {
            info!("🗑️ Discarded {} stale spilled spawns (Space switch)", q.len());
            q.clear();
        }
        return;
    }

    // Fresh per-frame budget for any recursion the drained nodes do. While the
    // load is in flight this is the BURST budget (env-tunable), so the spill
    // queue drains in a handful of long frames; after load it reverts to the
    // conservative steady-state value for any interactive spill.
    let per_frame = spawn_budget_per_frame();
    SPAWN_BUDGET.store(per_frame, Relaxed);
    // The previous frame's length feeds the equal-time slice rule.
    LAST_FRAME_NS.store(time.delta().as_nanos() as u64, Relaxed);
    arm_spawn_deadline();
    let t_slice = std::time::Instant::now();

    let batch: Vec<(FileMetadata, Option<Entity>)> = {
        let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
        if q.is_empty() {
            return;
        }
        let n = q.len().min(per_frame as usize);
        q.drain(..n).collect()
    };

    // Still loading — keep persistence + the LoadInProgress quiescence
    // window armed until the spill is fully drained.
    load_in_progress.begin();

    let space_path = &space_root.0;
    let cd_ref = class_defaults.as_deref();
    let source = active_source.0.clone();
    let source = source.as_ref();
    let mut count = 0usize;
    let mut batch = batch.into_iter();
    while let Some((meta, parent)) = batch.next() {
        // Time slice spent: hand everything not yet reached BACK to the
        // FRONT of the queue (this item first, so order is preserved) and
        // stop. The count budget alone let one 4096-entity batch hold the
        // frame for ~18 s on Super Station.
        if spawn_deadline_passed() {
            let rest: Vec<(FileMetadata, Option<Entity>)> =
                std::iter::once((meta, parent)).chain(batch).collect();
            let n = rest.len();
            let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
            q.splice(0..0, rest);
            debug!("🧩 spawn slice ended — re-queued {n} spilled spawns at the front");
            break;
        }
        count += 1;
        match meta.file_type {
            FileType::Directory => {
                spawn_directory_entry(
                    &mut commands, &asset_server, &mut meshes, &mut materials,
                    &mut registry, &mut material_registry, &mut mesh_cache, &mut decal_materials, space_path, &meta, parent,
                    cd_ref, source,
                );
            }
            _ => {
                spawn_file_entry(
                    &mut commands, &asset_server, &mut meshes, &mut materials,
                    &mut registry, &mut material_registry, &mut mesh_cache, &mut decal_materials, space_path, &meta, parent,
                    cd_ref, source,
                );
            }
        }
    }

    LAST_SLICE_NS.store(t_slice.elapsed().as_nanos() as u64, Relaxed);
    let remaining = SPILL.lock().unwrap_or_else(|e| e.into_inner()).len();
    warn!(
        target: "eustress_engine::world_db",
        unique_materials = material_registry.dedup_cache_len(),
        dense_quant = super::material_loader::dense_material_mode(),
        "🧩 Streamed {} spilled spawns ({} remaining). unique_materials is the draw-call batch count — if it's ~50k the render ceiling stands; with dense_quant=true it should be only a few thousand (≈12× fewer draws).",
        count, remaining
    );
}

/// Run condition: fires exactly once, on its first evaluation, then latches.
/// Combined with `world_db_open_settled` via a short-circuiting `.and()`, so
/// that first evaluation is the first frame the Space's DB is installed (or
/// the disk fallback decided). A `Local` (not a resource) because run
/// conditions may only take read-only params; `Local` is exempt.
fn initial_load_once(mut done: Local<bool>) -> bool {
    if *done {
        return false;
    }
    *done = true;
    true
}

/// Plugin for dynamic file loading
pub struct SpaceFileLoaderPlugin;

impl Plugin for SpaceFileLoaderPlugin {
    fn build(&self, app: &mut App) {
        // Note: The "space://" asset source is registered in main.rs BEFORE DefaultPlugins
        // This must happen before AssetPlugin is initialized, so we can't do it here.
        
        app.init_resource::<super::SpaceRoot>()
            .init_resource::<SpaceFileRegistry>()
            .init_resource::<SpaceLoadGeneration>()
            .init_resource::<PendingPreScan>()
            .init_resource::<super::material_loader::MaterialRegistry>()
            .init_resource::<super::instance_loader::PrimitiveMeshCache>()
            .init_resource::<super::file_watcher::RecentlyWrittenFiles>()
            // Latch shared by the Startup + Update copies of
            // `setup_file_watcher` so the watcher is built once per Space.
            .init_resource::<super::file_watcher::WatchedSpace>()
            .init_resource::<super::space_ops::SpaceRescanNeeded>()
            .init_resource::<DeferredServiceLoader>()
            .init_resource::<LoadInProgress>()
            // Deterministic spawn queue for copy-paste/duplicate folder trees
            // (drained by `drain_paste_spawn_queue` — reliable child parenting).
            .init_resource::<PasteSpawnQueue>()
            // Content source for the loader — Disk by default; the
            // world-db plugin swaps it to Fjall on Space open once the
            // tree is seeded. Registered here (not behind the feature)
            // so loader systems can always read through it.
            .init_resource::<super::space_source::ActiveSpaceSource>()
            // Class schema — common-crate source of truth for every
            // `_instance.toml`. Embedded templates normalise to PascalCase,
            // `load_and_heal_instance` fills missing fields + self-heals
            // the file on disk. Plugins extend the schema by registering
            // `ExtraSectionClaim` impls on `ExtraSectionRegistry`.
            .init_resource::<eustress_common::class_schema::ClassSchemaResource>()
            .init_resource::<eustress_common::class_schema::ExtraSectionRegistry>()
            // Single canonical filesystem-change broadcast. Sourced by
            // `SpaceFileWatcher`'s notify thread, consumed by any
            // subsystem that needs to react to disk changes (streaming
            // spatial grid, plugins, future tooling). See
            // `common::file_events` for the design rationale.
            .add_message::<eustress_common::file_events::FileChanged>()
            .add_systems(Startup, (
                // Register every first-party ExtraSectionClaim that
                // ships with common (currently just ThermodynamicClaim).
                // Plugins that want to add more claimants do the same
                // dance in their own `build` after this runs.
                |mut registry: ResMut<eustress_common::class_schema::ExtraSectionRegistry>| {
                    registry.register_builtins();
                },
                super::class_defaults::startup_load_class_defaults,
                // Template / enum / filename-stem drift check. Runs
                // once at boot, logs warnings for any class template
                // whose `[metadata] class_name` doesn't match its
                // file stem, or whose stem isn't in the ClassName
                // enum. Catches "added a template but forgot the
                // enum variant" bugs at startup instead of at
                // load-a-Part time.
                eustress_common::class_schema::log_schema_validation,
                super::file_watcher::setup_file_watcher,
            ))
            // The initial Space load used to be a Startup system, ordered after
            // the DB open. The open's disk work now runs on a worker across
            // frames (`world_db_plugin::PendingWorldDbOpen`), so the load waits
            // for it in Update instead: it runs on the first frame the open is
            // settled and latches itself off (`initial_load_once`). Same
            // ordering as before, one frame boundary later.
            .add_systems(Update,
                load_space_files_system
                    // `.and` short-circuits: the once-latch is only consumed
                    // on a frame where the open has actually settled.
                    .run_if(
                        bevy::ecs::schedule::SystemCondition::and_then(
                            super::world_db_plugin::world_db_open_settled,
                            initial_load_once,
                        ),
                    )
                    .after(super::world_db_plugin::open_world_db_on_space_change),
            )
            .add_systems(Update, (
                load_deferred_services,
                drain_pending_spawns.after(load_deferred_services),
                tick_load_in_progress.after(drain_pending_spawns),
                bound_fixed_catchup_during_load.after(tick_load_in_progress),
                // Deterministic paste spawn BEFORE the watcher so pasted paths
                // are registered first → the watcher's `is_loaded` check skips
                // the same Create events (no double-spawn).
                drain_paste_spawn_queue.before(super::file_watcher::process_file_changes),
                // Re-point the watcher when the Space changes. It is latched
                // per path, so this is a no-op on every frame that is not a
                // Space switch. WITHOUT this the Startup copy above is the
                // only one that ever runs, and the watcher keeps watching the
                // Space the engine LAUNCHED into for the rest of the session —
                // so that Space's `_instance.toml` writes hot-spawn into
                // whichever Space is open now (a splat cloud from Universe A
                // appearing in the Explorer of Universe B, unrenderable).
                // Ordered before the consumer so a switch takes effect in the
                // same frame it happens.
                super::file_watcher::setup_file_watcher
                    .before(super::file_watcher::process_file_changes),
                super::file_watcher::process_file_changes,
                super::instance_loader::ensure_tags_and_attributes_components,
                super::instance_loader::ensure_measure_unit,
                // Live: edited Workspace `render_distance` service
                // property → part VisibilityRange. Changed-gated.
                super::instance_loader::sync_workspace_render_distance,
                super::space_ops::apply_space_rescan,
                super::instance_loader::update_base_part_size_from_mesh,
                // Per-frame safety net: clamp NaN/Inf and sky-distance
                // overflow on every Avian-tracked Transform so any
                // drag-tool / plugin that writes a degenerate value
                // can't reach Avian's `assert_components_finite` and
                // panic the engine. Tools should clamp at the source
                // via `safe_translation`, but this catches misses.
                super::instance_loader::sanitize_part_transforms_safety_net,
                // Hand each freshly-loaded instance's extra
                // `[section]`s to any plugin that registered for them
                // via `ExtraSectionRegistry`. Runs once per entity on
                // the frame `PendingExtraSections` is inserted, then
                // removes the component.
                eustress_common::class_schema::dispatch_pending_extras,
            ));

        // Legacy TOML write-back — only when the `toml` feature is on.
        // Default build (ECS+DB authoritative, 2026-05-15 pivot) does
        // NOT register these: runtime edits persist to `world.fjalldb/`
        // via `world_db_plugin`, not to `_instance.toml`. The TOML
        // *read* path above stays compiled regardless — it's the
        // first-run Fjall seed + recovery loader.
        #[cfg(feature = "toml")]
        {
            app.add_systems(
                Update,
                (
                    super::instance_loader::write_instance_changes_system,
                    super::instance_loader::save_tags_and_attributes_changes,
                ),
            );
        }
    }
}

/// Unit quad mesh used by Image / Video classes. UVs are origin-top-left
/// to match the convention the rest of the engine uses (BillboardCard,
/// Decal). Scaled by `Transform::from_scale(Vec3::new(width, height, 1))`
/// so the same mesh handle is shared across every imported media entity.
fn build_imported_media_quad() -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5, -0.5, 0.0],
            [ 0.5, -0.5, 0.0],
            [ 0.5,  0.5, 0.0],
            [-0.5,  0.5, 0.0],
        ],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4]);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![
            [0.0, 1.0],   // bottom-left
            [1.0, 1.0],   // bottom-right
            [1.0, 0.0],   // top-right
            [0.0, 0.0],   // top-left
        ],
    );
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}
