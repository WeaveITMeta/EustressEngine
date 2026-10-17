//! `SpaceSource` — the content abstraction that lets the Space loader
//! read from either the filesystem (legacy) or the Fjall tree
//! partition (the 2026-05-15 ECS+DB-primary pivot) through one seam.
//!
//! ## Why this exists
//!
//! The loader (`file_loader`, `instance_loader`, `service_loader`,
//! `gui_loader`, `material_loader`) historically reached for
//! `std::fs::read*` / `read_dir` in ~18 places. For Fjall to be
//! primary "in all things" the loader must source *content* from the
//! DB without a disk round-trip. Rather than rewrite the loader's
//! service/hierarchy/deferred logic, this trait swaps only the
//! content provider. Relative forward-slash paths are the common
//! currency: `DiskSource` joins them onto the Space root; `FjallSource`
//! looks them up in the `tree` partition (keyed identically by the
//! faithful importer).
//!
//! ## Lifecycle
//!
//! On Space open the engine picks the source:
//! - Fjall tree non-empty → [`SpaceSource::Fjall`] (authoritative;
//!   disk never read).
//! - Fjall tree empty + disk Space present → run the faithful
//!   importer once (disk → Fjall tree), then use [`SpaceSource::Fjall`].
//! - No Fjall (feature off) → [`SpaceSource::Disk`] (legacy).
//!
//! The chosen source is stored as the [`ActiveSpaceSource`] resource;
//! loader systems read through it instead of `std::fs`.
//!
//! The trait, [`SourceEntry`] and [`DiskSource`] live in
//! `eustress_common::space_source`, where the Play runtime and the
//! browser Player read a Space through them too; this module adds the
//! world-DB source and the editor's resource.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::prelude::*;

pub use eustress_common::space_source::{DiskSource, FileStamp, SourceEntry, SpaceSource};

/// Fjall-backed source — reads through the `tree` partition the
/// faithful importer populated. Zero disk access; this is what makes
/// the engine genuinely ECS+DB-primary.
#[cfg(feature = "world-db")]
pub struct FjallSource {
    db: Arc<dyn eustress_worlddb::WorldDb>,
}

#[cfg(feature = "world-db")]
impl FjallSource {
    /// Wrap an open `WorldDb`. The caller has already decided the tree
    /// is populated (or just seeded it).
    pub fn new(db: Arc<dyn eustress_worlddb::WorldDb>) -> Self {
        Self { db }
    }
}

#[cfg(feature = "world-db")]
impl SpaceSource for FjallSource {
    fn read(&self, rel: &str) -> std::io::Result<Vec<u8>> {
        match self.db.get_file(rel) {
            Ok(Some(bytes)) => Ok(bytes),
            Ok(None) => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("worlddb tree miss: {rel}"),
            )),
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("worlddb read {rel}: {e}"),
            )),
        }
    }

    fn list(&self, rel_dir: &str) -> std::io::Result<Vec<SourceEntry>> {
        self.db
            .list_dir(rel_dir)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|e| SourceEntry {
                        name: e.name,
                        rel_path: e.rel_path,
                        is_dir: e.is_dir,
                    })
                    .collect()
            })
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("worlddb list {rel_dir}: {e}"),
                )
            })
    }

    fn exists(&self, rel: &str) -> bool {
        // A path exists if it's a stored file OR a prefix of one
        // (inferred directory). `list` on it being non-empty covers
        // the dir case; `get_file` covers the leaf case.
        matches!(self.db.get_file(rel), Ok(Some(_)))
            || self.db.list_dir(rel).map(|v| !v.is_empty()).unwrap_or(false)
    }

    fn is_fjall(&self) -> bool {
        true
    }
}

/// Bevy resource holding the active source for the current Space.
/// Loader systems read through `.0`. Defaults to a `DiskSource` at the
/// engine's resolved default Space root so the very first frame
/// (before the open/seed system runs) is still well-defined.
#[derive(Resource, Clone)]
pub struct ActiveSpaceSource(pub Arc<dyn SpaceSource>);

impl ActiveSpaceSource {
    /// Build a disk-backed source rooted at `space_root`.
    pub fn disk(space_root: impl Into<PathBuf>) -> Self {
        Self(Arc::new(DiskSource::new(space_root)))
    }
}

impl Default for ActiveSpaceSource {
    fn default() -> Self {
        Self::disk(super::default_space_root())
    }
}

/// Convenience: relative path from a Space root to an absolute path,
/// forward-slash normalised. Used by the loader while it still holds
/// absolute `Path`s during the staged thread-through.
pub fn rel_from_root(space_root: &Path, abs: &Path) -> Option<String> {
    abs.strip_prefix(space_root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}
