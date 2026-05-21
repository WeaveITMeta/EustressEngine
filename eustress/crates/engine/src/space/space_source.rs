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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::prelude::*;

/// One entry in a [`SpaceSource`] directory listing — mirrors the
/// shape of `std::fs::DirEntry` the loader needs (name + is_dir),
/// plus the Space-relative path so callers don't re-derive it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    /// Leaf name (no separators).
    pub name: String,
    /// Space-relative forward-slash path.
    pub rel_path: String,
    /// Directory vs file.
    pub is_dir: bool,
}

/// Read-side content provider for the Space loader. Object-safe so it
/// can live behind `Arc<dyn SpaceSource>` in a Bevy resource.
pub trait SpaceSource: Send + Sync + 'static {
    /// Read a file's bytes by Space-relative path.
    fn read(&self, rel: &str) -> std::io::Result<Vec<u8>>;

    /// Read a file's bytes as UTF-8 text — convenience for the many
    /// TOML/script call sites.
    fn read_to_string(&self, rel: &str) -> std::io::Result<String> {
        let bytes = self.read(rel)?;
        String::from_utf8(bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// List the immediate children of a Space-relative directory.
    /// `""` lists the Space root.
    fn list(&self, rel_dir: &str) -> std::io::Result<Vec<SourceEntry>>;

    /// True when a file or directory exists at the relative path.
    fn exists(&self, rel: &str) -> bool;

    /// True when this source serves from Fjall (diagnostics + the
    /// "should I also write back to disk?" decision in the writer).
    fn is_fjall(&self) -> bool;
}

/// Legacy filesystem source — joins relative paths onto the Space
/// root and uses `std::fs`. Behaviour identical to the pre-pivot
/// loader, so `--features toml` (or a non-migrated world) is a
/// byte-for-byte no-op change.
pub struct DiskSource {
    root: PathBuf,
}

impl DiskSource {
    /// `root` is the on-disk Space directory
    /// (`<universe>/Spaces/<space>/`).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn abs(&self, rel: &str) -> PathBuf {
        let mut p = self.root.clone();
        for seg in rel.split(['/', '\\']) {
            if !seg.is_empty() && seg != "." && seg != ".." {
                p.push(seg);
            }
        }
        p
    }
}

impl SpaceSource for DiskSource {
    fn read(&self, rel: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.abs(rel))
    }

    fn list(&self, rel_dir: &str) -> std::io::Result<Vec<SourceEntry>> {
        let abs = self.abs(rel_dir);
        let mut out = Vec::new();
        for entry in std::fs::read_dir(abs)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let rel_path = if rel_dir.is_empty() {
                name.clone()
            } else {
                format!("{rel_dir}/{name}")
            };
            out.push(SourceEntry {
                name,
                rel_path,
                is_dir,
            });
        }
        Ok(out)
    }

    fn exists(&self, rel: &str) -> bool {
        self.abs(rel).exists()
    }

    fn is_fjall(&self) -> bool {
        false
    }
}

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
