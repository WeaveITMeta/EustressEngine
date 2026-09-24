//! # Where a downloaded world lives
//!
//! A world the Player did not load from its own disk (a host's Space over
//! the network, or a published simulation from R2) arrives as records: a
//! Space-relative path and the file's bytes. The Space reader
//! (`eustress_common::space_read`) and the `space://` asset source both read
//! folders, so the records are written into a folder that belongs to this
//! process before the Space is opened.
//!
//! ```text
//! <data_local>/eustress/live/<pid>/Universe/
//!     Spaces/Current/      the Space the Player opens
//!     Spaces/<other>/      the Universe's other Spaces
//!     assets/              the Universe's shared assets
//! ```
//!
//! The Space the Player opens is always `Spaces/Current`, because Bevy
//! freezes asset sources when the app is built, before any world has arrived:
//! `space://` is registered at that fixed folder.
//!
//! This module is the one place the Player touches the filesystem for a
//! downloaded world. A browser build replaces it with an in-memory source;
//! the records are the seam.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::prelude::*;
use eustress_echk::{is_content_hash, is_safe_record_path, Record};
use eustress_networking::session::{ChunkCache, DownloadedWorld};

/// The folders a downloaded world is written to.
#[derive(Resource, Clone, Debug)]
pub struct LiveWorld {
    pub universe_root: PathBuf,
    pub space_root: PathBuf,
}

impl LiveWorld {
    /// This process's folders.
    pub fn for_this_process() -> Self {
        let universe_root = data_root().join("live").join(std::process::id().to_string()).join("Universe");
        let space_root = universe_root.join("Spaces").join("Current");
        Self { universe_root, space_root }
    }

    /// Create the folders (the asset source needs a directory that exists) and
    /// clear out folders left by Players that exited more than a day ago.
    pub fn prepare(&self) {
        let _ = std::fs::create_dir_all(&self.space_root);
        prune_stale(&data_root().join("live"), self.universe_root.parent());
    }
}

/// `<data_local>/eustress`.
pub fn data_root() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("eustress")
}

fn prune_stale(live_root: &Path, keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(live_root) else { return };
    let day = Duration::from_secs(24 * 60 * 60);
    for entry in entries.flatten() {
        let path = entry.path();
        if Some(path.as_path()) == keep {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age > day);
        if old {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// Write a downloaded world into `live`, replacing whatever was there.
/// Returns the number of files written.
pub fn materialize(world: &DownloadedWorld, live: &LiveWorld) -> Result<usize, String> {
    if live.universe_root.exists() {
        std::fs::remove_dir_all(&live.universe_root)
            .map_err(|e| format!("clear {}: {e}", live.universe_root.display()))?;
    }
    std::fs::create_dir_all(&live.space_root).map_err(|e| format!("create {}: {e}", live.space_root.display()))?;

    let mut written = 0;
    for (name, records) in &world.spaces {
        let root = if *name == world.start_space {
            live.space_root.clone()
        } else {
            live.universe_root.join("Spaces").join(name)
        };
        written += write_records(&root, records)?;
    }
    written += write_records(&live.universe_root, &world.assets)?;
    Ok(written)
}

fn write_records(root: &Path, records: &[Record]) -> Result<usize, String> {
    for (path, bytes) in records {
        // Checked again here, at the one place bytes reach the disk.
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

/// Chunks kept between sessions under `<data_local>/eustress/echk/`, named by
/// content hash, so rejoining an unchanged world downloads nothing.
pub struct DiskChunkCache {
    dir: PathBuf,
}

impl Default for DiskChunkCache {
    fn default() -> Self {
        Self { dir: data_root().join("echk") }
    }
}

impl ChunkCache for DiskChunkCache {
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
        // Temp then rename: two Players caching the same chunk at once each
        // write their own temp file, and the rename is atomic.
        let tmp = self.dir.join(format!("{hash}.{}.tmp", std::process::id()));
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, self.dir.join(format!("{hash}.echk")));
        }
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Where a published world's players should appear: the first top-level
/// SpawnLocation in the opening Space, standing on it. `None` when it has none.
pub fn find_spawn(world: &DownloadedWorld) -> Option<Vec3> {
    let (_, records) = world.spaces.iter().find(|(name, _)| *name == world.start_space)?;
    for (path, bytes) in records {
        // Top-level only: a nested spawn's transform is relative to its parent.
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
/// `.echk` download produces, so both kinds of published world open the same
/// way.
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
        spawn: [0.0, 2.0, 8.0],
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

#[cfg(test)]
mod tests {
    use super::*;

    fn world(records: Vec<Record>) -> DownloadedWorld {
        DownloadedWorld {
            universe: "U".into(),
            start_space: "S".into(),
            spaces: vec![("S".into(), records)],
            assets: vec![("assets/a.txt".into(), b"a".to_vec())],
            spawn: [0.0; 3],
        }
    }

    #[test]
    fn materializes_into_the_live_folder_and_refuses_escapes() {
        let base = std::env::temp_dir().join(format!("eustress_live_test_{}", std::process::id()));
        let live = LiveWorld { universe_root: base.join("Universe"), space_root: base.join("Universe/Spaces/Current") };
        let ok = world(vec![("Workspace/Floor/_instance.toml".into(), b"[metadata]\n".to_vec())]);
        assert_eq!(materialize(&ok, &live).unwrap(), 2);
        assert!(live.space_root.join("Workspace/Floor/_instance.toml").is_file());
        assert!(live.universe_root.join("assets/a.txt").is_file());

        let evil = world(vec![("../../escape.txt".into(), b"x".to_vec())]);
        assert!(materialize(&evil, &live).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn finds_a_top_level_spawn_location() {
        let spawn = b"[metadata]\nclass_name = \"SpawnLocation\"\n\n[transform]\nposition = [10.0, 1.0, -4.0]\nscale = [6.0, 1.0, 6.0]\n";
        let w = world(vec![
            ("Workspace/Floor/_instance.toml".into(), b"[metadata]\nclass_name = \"Part\"\n".to_vec()),
            ("Workspace/Spawn/_instance.toml".into(), spawn.to_vec()),
        ]);
        let at = find_spawn(&w).unwrap();
        assert!((at - Vec3::new(10.0, 1.6, -4.0)).length() < 1e-4, "{at}");
    }
}
