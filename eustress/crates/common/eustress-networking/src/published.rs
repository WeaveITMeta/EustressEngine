//! Download a published simulation.
//!
//! The same requests the desktop Player makes:
//!
//! ```text
//! GET {API}/api/simulations/{id}/world/manifest
//!     each chunk not already cached:
//!         GET {API}/api/simulations/{id}/world/chunks/{blake3}
//!     (409: an older publish, one .pak)
//! GET {API}/api/simulations/{id}/download
//! ```
//!
//! Chunks land in `{data_root}/echk/{hash}.echk`. A legacy `.pak` is unpacked
//! under `{data_root}/pak/{id}`.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use bevy::prelude::Vec3;
use eustress_echk::{content_hash, is_content_hash, is_safe_record_path, Record, WorldManifest};

use crate::session::{assemble_world, ChunkCache, DownloadedWorld};

/// Where a published world's players stand when it names no SpawnLocation.
const DEFAULT_SPAWN: [f32; 3] = [0.0, 2.0, 8.0];

/// Download `sim_id` from `api_base` (no trailing path). `data_root` is the
/// Player's cache directory (`<local data>/eustress` on the desktop).
pub fn fetch_published(api_base: &str, sim_id: &str, token: Option<&str>, data_root: &Path) -> Result<DownloadedWorld, String> {
    let base = api_base.trim().trim_end_matches('/');
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let get = |url: &str| {
        let mut req = client.get(url);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        req.send().map_err(|e| format!("GET {url}: {e}"))
    };

    let manifest_url = format!("{base}/api/simulations/{sim_id}/world/manifest");
    let resp = get(&manifest_url)?;
    let status = resp.status();
    if status.as_u16() == 409 {
        return fetch_legacy_pak(base, sim_id, &get, data_root);
    }
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("{status} from {manifest_url}: {}", body.chars().take(200).collect::<String>()));
    }
    let manifest: WorldManifest = resp.json().map_err(|e| format!("read the world manifest: {e}"))?;
    manifest.validate().map_err(|e| format!("the world manifest was refused: {e}"))?;

    let cache = DirChunkCache { dir: data_root.join("echk") };
    let mut chunks: HashMap<String, Vec<u8>> = HashMap::new();
    for hash in manifest.hashes() {
        if !is_content_hash(&hash) {
            return Err(format!("bad chunk name {hash:?}"));
        }
        if let Some(bytes) = cache.get(&hash).filter(|b| content_hash(b) == hash) {
            chunks.insert(hash, bytes);
            continue;
        }
        let url = format!("{base}/api/simulations/{sim_id}/world/chunks/{hash}");
        let resp = get(&url)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("{status} from {url}"));
        }
        let bytes = resp.bytes().map_err(|e| format!("read chunk {hash}: {e}"))?.to_vec();
        if content_hash(&bytes) != hash {
            return Err(format!("chunk {hash} does not match its content hash"));
        }
        cache.put(&hash, &bytes);
        chunks.insert(hash, bytes);
    }

    let mut world = assemble_world(&manifest, |h| chunks.get(h).map(|b| b.as_slice()), DEFAULT_SPAWN)?;
    if let Some(at) = find_spawn(&world) {
        world.spawn = at.to_array();
    }
    Ok(world)
}

fn fetch_legacy_pak(
    base: &str,
    sim_id: &str,
    get: &dyn Fn(&str) -> Result<reqwest::blocking::Response, String>,
    data_root: &Path,
) -> Result<DownloadedWorld, String> {
    let url = format!("{base}/api/simulations/{sim_id}/download");
    let resp = get(&url)?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("{status} from {url}: {}", body.chars().take(200).collect::<String>()));
    }
    let bytes = resp.bytes().map_err(|e| format!("read body: {e}"))?;
    if bytes.is_empty() {
        return Err("empty .pak".into());
    }
    let dest = data_root.join("pak").join(sim_id);
    unpack_pak(&bytes, &dest)?;
    world_from_universe_dir(&dest)
}

/// `{dir}/{blake3}.echk`. The desktop Player and the mobile Player share this
/// layout, so a chunk downloaded by either is a hit for the other on the
/// same machine. On Android `dir` is the app's files directory.
pub struct DirChunkCache {
    dir: PathBuf,
}

impl DirChunkCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
}

impl ChunkCache for DirChunkCache {
    fn get(&self, hash: &str) -> Option<Vec<u8>> {
        if !is_content_hash(hash) {
            return None;
        }
        std::fs::read(self.dir.join(format!("{hash}.echk"))).ok()
    }

    fn put(&self, hash: &str, bytes: &[u8]) {
        if !is_content_hash(hash) || std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let tmp = self.dir.join(format!("{hash}.{}.tmp", std::process::id()));
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, self.dir.join(format!("{hash}.echk")));
        }
        let _ = std::fs::remove_file(&tmp);
    }
}

/// zstd → tar → directory. A local `.pak` can be opened without a round trip.
pub fn unpack_pak(pak: &[u8], dest: &Path) -> Result<(), String> {
    let mut tar_bytes = Vec::new();
    zstd::stream::Decoder::new(std::io::Cursor::new(pak))
        .map_err(|e| format!("zstd init: {e}"))?
        .read_to_end(&mut tar_bytes)
        .map_err(|e| format!("zstd decode: {e}"))?;

    if dest.exists() {
        std::fs::remove_dir_all(dest).map_err(|e| format!("clear {dest:?}: {e}"))?;
    }
    std::fs::create_dir_all(dest).map_err(|e| format!("create {dest:?}: {e}"))?;

    let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
    for entry in archive.entries().map_err(|e| format!("tar entries: {e}"))? {
        let mut entry = entry.map_err(|e| format!("tar entry: {e}"))?;
        let path = entry.path().map_err(|e| format!("tar path: {e}"))?.into_owned();
        if is_unsafe_archive_path(&path) {
            return Err(format!("refusing unsafe archive path {path:?}"));
        }
        let out = dest.join(&path);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
        }
        entry.unpack(&out).map_err(|e| format!("unpack {path:?}: {e}"))?;
    }
    Ok(())
}

/// Would extracting this entry escape the destination directory?
pub fn is_unsafe_archive_path(path: &Path) -> bool {
    path.is_absolute()
        || path.has_root()
        || path.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::Prefix(_)))
}

/// The feet of a top-level `SpawnLocation`, or `None` when the Space names none.
pub fn find_spawn(world: &DownloadedWorld) -> Option<Vec3> {
    let (_, records) = world.spaces.iter().find(|(name, _)| *name == world.start_space)?;
    for (path, bytes) in records {
        let is_top_level = path.strip_prefix("Workspace/").is_some_and(|rest| rest.matches('/').count() == 1);
        if !is_top_level || !path.ends_with("/_instance.toml") {
            continue;
        }
        let Ok(text) = std::str::from_utf8(bytes) else { continue };
        if !text.contains("SpawnLocation") {
            continue;
        }
        let Ok(doc) = text.parse::<toml::Value>() else { continue };
        if doc.get("metadata").and_then(|m| m.get("class_name")).and_then(|c| c.as_str()) != Some("SpawnLocation") {
            continue;
        }
        let transform = doc.get("transform");
        let array = |key: &str, i: usize, default: f64| -> f32 {
            transform
                .and_then(|t| t.get(key))
                .and_then(|a| a.as_array())
                .and_then(|a| a.get(i))
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
                .unwrap_or(default) as f32
        };
        let position = Vec3::new(array("position", 0, 0.0), array("position", 1, 0.0), array("position", 2, 0.0));
        let half_height = array("scale", 1, 1.0) * 0.5;
        return Some(position + Vec3::Y * (half_height + 0.1));
    }
    None
}

/// Turn an unpacked `.pak` (a Universe folder) into the same record form a
/// `.echk` download produces.
pub fn world_from_universe_dir(universe: &Path) -> Result<DownloadedWorld, String> {
    let spaces_dir = universe.join("Spaces");
    let mut names: Vec<String> = std::fs::read_dir(&spaces_dir)
        .map_err(|e| format!("read {}: {e}", spaces_dir.display()))?
        .flatten()
        .filter(|e| e.path().join("Workspace").is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    let start_space = names.first().cloned().ok_or("the package holds no Space with a Workspace")?;

    let mut spaces = Vec::new();
    for name in &names {
        spaces.push((name.clone(), records_from_dir(&spaces_dir.join(name), "")?));
    }
    let assets_dir = universe.join("assets");
    let assets = if assets_dir.is_dir() { records_from_dir(&assets_dir, "assets/")? } else { Vec::new() };

    let mut world = DownloadedWorld {
        universe: universe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        start_space,
        spaces,
        assets,
        spawn: DEFAULT_SPAWN,
    };
    if let Some(at) = find_spawn(&world) {
        world.spawn = at.to_array();
    }
    Ok(world)
}

fn records_from_dir(root: &Path, prefix: &str) -> Result<Vec<Record>, String> {
    fn walk(base: &Path, dir: &Path, prefix: &str, out: &mut Vec<Record>) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name.starts_with("world.fjalldb") || name.starts_with("header.bin") {
                continue;
            }
            if path.is_dir() {
                walk(base, &path, prefix, out)?;
            } else {
                let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                let rel = format!("{prefix}{rel}");
                if is_safe_record_path(&rel) {
                    let bytes = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
                    out.push((rel, bytes));
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, prefix, &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Write a downloaded world onto disk. The start Space goes in `space_root`.
/// Every other Space goes in `{universe_root}/Spaces/{name}`. Shared assets
/// go in `universe_root`. Returns how many files were written.
///
/// The desktop Player (`client` `live_world::materialize`) and the mobile
/// Player both call this. A path that would leave the folder is refused.
pub fn materialize_world(world: &DownloadedWorld, universe_root: &Path, space_root: &Path) -> Result<usize, String> {
    if universe_root.exists() {
        std::fs::remove_dir_all(universe_root).map_err(|e| format!("clear {}: {e}", universe_root.display()))?;
    }
    std::fs::create_dir_all(space_root).map_err(|e| format!("create {}: {e}", space_root.display()))?;

    let mut written = 0;
    for (name, records) in &world.spaces {
        let root = if *name == world.start_space {
            space_root.to_path_buf()
        } else {
            universe_root.join("Spaces").join(name)
        };
        written += write_records(&root, records)?;
    }
    written += write_records(universe_root, &world.assets)?;
    Ok(written)
}

fn write_records(root: &Path, records: &[Record]) -> Result<usize, String> {
    for (path, bytes) in records {
        if !is_safe_record_path(path) {
            return Err(format!("refusing to write {path:?} outside the world folder"));
        }
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, bytes).map_err(|e| format!("write {}: {e}", target.display()))?;
    }
    Ok(records.len())
}
