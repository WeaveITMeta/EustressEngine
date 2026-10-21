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
use eustress_echk::{is_content_hash, Record};
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
/// Returns the number of files written. The bytes are written by
/// [`eustress_networking::published::materialize_world`], which the mobile
/// Player calls too.
pub fn materialize(world: &DownloadedWorld, live: &LiveWorld) -> Result<usize, String> {
    eustress_networking::published::materialize_world(world, &live.universe_root, &live.space_root)
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

/// Where a published world's players should appear. The reader lives with
/// the download (`eustress_networking::published`) so the mobile Player uses
/// the same one.
pub fn find_spawn(world: &DownloadedWorld) -> Option<Vec3> {
    eustress_networking::published::find_spawn(world)
}

/// Turn an unpacked `.pak` into records. Same function the download uses.
pub fn world_from_universe_dir(universe: &Path) -> Result<DownloadedWorld, String> {
    eustress_networking::published::world_from_universe_dir(universe)
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
