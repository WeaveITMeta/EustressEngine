//! `.eustress` launch-manifest — the tiny binary pointer file that
//! lives in a Universe directory. Double-clicking it (OS file
//! association → `eustress.exe <file>`) or `--open <file>` opens the
//! Space it points at.
//!
//! A LIVE world is a Fjall *directory* (write-ahead log + sorted-string
//! tables + manifest), not a single sealed file, so the `.eustress`
//! file is a POINTER to the container directory, never the container
//! itself.
//!
//! Format: 8-byte magic `EUSLNCH1`, then the absolute path of the
//! Space container directory as UTF-8 (the path is the entire
//! remainder — no length prefix needed). Tiny, inspectable, and
//! forward-stable (bump the trailing magic digit to version).

use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"EUSLNCH1";

/// Write / refresh the launch pointer at `eustress_file` so it
/// resolves to `space_dir` (the Space container directory that holds
/// `world.fjalldb/` + `header.bin`). Creates the parent directory.
pub fn write_launch_manifest(eustress_file: &Path, space_dir: &Path) -> std::io::Result<()> {
    let mut bytes = Vec::with_capacity(8 + 260);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(space_dir.to_string_lossy().as_bytes());
    if let Some(parent) = eustress_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(eustress_file, &bytes)
}

/// Resolve a `.eustress` launch pointer to the Space container
/// directory it references. `None` if it isn't a valid pointer or the
/// target directory no longer exists.
pub fn read_launch_manifest(eustress_file: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(eustress_file).ok()?;
    if bytes.len() < 8 || &bytes[..8] != MAGIC {
        return None;
    }
    let path = std::str::from_utf8(&bytes[8..]).ok()?.trim();
    if path.is_empty() {
        return None;
    }
    let dir = PathBuf::from(path);
    if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

/// Resolve a double-clicked / `--open` argument to a Space container
/// directory: a directory is taken as the Space itself; a `.eustress`
/// file is read as a launch pointer; anything else → `None`.
pub fn resolve_open_target(arg: &str) -> Option<PathBuf> {
    let p = PathBuf::from(arg);
    if p.is_dir() {
        return Some(p);
    }
    let is_eustress = p
        .extension()
        .map(|e| e.to_string_lossy().eq_ignore_ascii_case("eustress"))
        .unwrap_or(false);
    if is_eustress {
        if let Some(dir) = read_launch_manifest(&p) {
            return Some(dir);
        }
    }
    None
}
