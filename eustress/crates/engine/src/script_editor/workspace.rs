//! # Workspace-wide symbol index
//!
//! Walks every `.rune` file under a Universe root at startup and
//! maintains a cross-file symbol map. Phase-C foundation for go-to-
//! definition, find-references, and rename operations that span
//! multiple script folders inside the same Universe.
//!
//! ## Design
//!
//! - One [`WorkspaceIndex`] per live LSP session, rooted at the
//!   resolved Universe.
//! - One [`FileEntry`] per indexed `.rune` source, storing the raw
//!   [`analyzer::SymbolIndex`] plus a file modification timestamp so
//!   `refresh_if_stale` can skip files that haven't changed.
//! - A flat `by_name: HashMap<String, Vec<WorkspaceSymbol>>` for O(1)
//!   lookups across the whole Universe.
//!
//! ## What's intentionally NOT here
//!
//! - No semantic scoping. `resolve_name()` returns every matching
//!   symbol; callers filter by `SymbolKind` or pick the first match
//!   for goto-def. Full scope resolution across `use` statements is
//!   out of scope for Phase C; Phase E + a deeper Rune upstream would
//!   be needed for that.
//! - No pretty-printed reports. `WorkspaceIndex` is consumed by the
//!   LSP adapter; humans see the LSP response, not the raw index.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::analyzer::{self, Range, Symbol, SymbolIndex, SymbolKind};

// ═══════════════════════════════════════════════════════════════════════════
// Public types
// ═══════════════════════════════════════════════════════════════════════════

/// A symbol declared in some `.rune` file within the workspace. Extends
/// [`analyzer::Symbol`] with the owning file path and a canonical
/// Workshop-style mention string for display.
#[derive(Debug, Clone)]
pub struct WorkspaceSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file: PathBuf,
    pub range: Range,
    pub byte_range: (u32, u32),
    /// `@script:<Space>/<rel_path>::<name>` — matches Workshop's mention
    /// scheme so cross-tool references stay consistent. Omits the
    /// `@script:` prefix when the file isn't under `Spaces/`.
    pub canonical: String,
}

/// Cached index + mtime for a single `.rune` file. Re-parsing is
/// skipped when mtime matches.
#[derive(Debug, Clone)]
struct FileEntry {
    mtime: SystemTime,
    symbols: SymbolIndex,
}

/// Workspace-wide symbol index. Cheap to clone by value (HashMaps use
/// Arcs internally if you wrap them); the LSP side stores one inside an
/// `Arc<RwLock<_>>` so request handlers can read without blocking saves.
#[derive(Debug, Default, Clone)]
pub struct WorkspaceIndex {
    /// Root of the walk — a Universe directory (contains `Spaces/`).
    /// `None` when the index has never been built (e.g. LSP opened on
    /// a scratch directory that isn't a Universe).
    pub root: Option<PathBuf>,
    by_file: HashMap<PathBuf, FileEntry>,
    by_name: HashMap<String, Vec<WorkspaceSymbol>>,
}

impl WorkspaceIndex {
    /// Total number of indexed symbols. Useful for progress reporting
    /// and test assertions.
    pub fn len(&self) -> usize {
        self.by_name.values().map(|v| v.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Iterate every file path the index knows about, in no particular
    /// order.
    pub fn files(&self) -> impl Iterator<Item = &Path> {
        self.by_file.keys().map(|p| p.as_path())
    }

    /// Resolve a bare identifier to every workspace-visible
    /// declaration. Used by goto-def (pick `.first()`), find-refs
    /// (return the full slice), and rename (walk every definition +
    /// every file that mentions the name).
    pub fn resolve_name(&self, name: &str) -> &[WorkspaceSymbol] {
        self.by_name
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    // ─────────────────────────────────────────────────────────────
    // Construction / refresh
    // ─────────────────────────────────────────────────────────────

    /// Walk `root` for every `.rune` file and build a fresh index.
    /// Silently skips unreadable files and non-UTF-8 content — the
    /// analyzer handles broken source itself via diagnostic output, but
    /// unreadable-on-disk is a transient OS condition we just ignore.
    pub fn build(root: &Path) -> Self {
        let mut idx = Self {
            root: Some(root.to_path_buf()),
            by_file: HashMap::new(),
            by_name: HashMap::new(),
        };
        idx.walk_and_index(root);
        idx
    }

    /// Re-index a single file. Called from `didSave` / `didChange`
    /// when the LSP learns the file's contents have changed. Cheaper
    /// than a full rebuild.
    pub fn update_file(&mut self, path: &Path) {
        let canonical = canonical_path(path);
        // Remove any previous contributions from this file before
        // re-indexing so `by_name` doesn't accumulate stale entries.
        self.remove_file_entries(&canonical);
        let Some(root) = self.root.clone() else { return };
        self.index_one_file(&root, &canonical);
    }

    /// Re-index every file whose mtime has changed since last walk.
    /// Useful after a wholesale filesystem event (git checkout, mass
    /// rename) where `didSave` won't be delivered.
    pub fn refresh_if_stale(&mut self) {
        let Some(root) = self.root.clone() else { return };
        let paths: Vec<PathBuf> = self.by_file.keys().cloned().collect();
        for path in paths {
            let current_mtime = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok();
            let cached_mtime = self.by_file.get(&path).map(|e| e.mtime);
            if current_mtime != cached_mtime {
                self.remove_file_entries(&path);
                if current_mtime.is_some() {
                    self.index_one_file(&root, &path);
                }
            }
        }
        // Also pick up new files added since the last walk.
        self.walk_and_index(&root);
    }

    // ─────────────────────────────────────────────────────────────
    // Internals
    // ─────────────────────────────────────────────────────────────

    fn walk_and_index(&mut self, root: &Path) {
        // Bounded recursion: Universes can have symlinks. 8 levels is
        // comfortably deeper than any real Space hierarchy.
        self.walk_dir(root, root, 0);
    }

    fn walk_dir(&mut self, root: &Path, dir: &Path, depth: usize) {
        if depth > 12 {
            return;
        }
        let read = match std::fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => return,
        };
        for entry in read.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Standard ignore list — matches the MCP server's walker so
            // tooling sees the same Universe view everywhere.
            if name_str.starts_with('.')
                || name_str == "target"
                || name_str == "node_modules"
            {
                continue;
            }
            let path = entry.path();
            if ft.is_dir() {
                self.walk_dir(root, &path, depth + 1);
            } else if ft.is_file()
                && path.extension().and_then(|e| e.to_str()) == Some("rune")
            {
                self.index_one_file(root, &path);
            }
        }
    }

    fn index_one_file(&mut self, root: &Path, path: &Path) {
        let canonical = canonical_path(path);
        let mtime = match std::fs::metadata(&canonical).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => return,
        };
        // Skip if unchanged since last index.
        if let Some(existing) = self.by_file.get(&canonical) {
            if existing.mtime == mtime {
                return;
            }
        }
        let source = match std::fs::read_to_string(&canonical) {
            Ok(s) => s,
            Err(_) => return,
        };
        // Reuse the analyzer so parsing + symbol extraction stays
        // consistent with single-file analysis. Diagnostics are
        // discarded here — they're a per-request concern.
        let result = analyzer::analyze(&source);
        self.install_entry(root, &canonical, mtime, result.symbols);
    }

    fn install_entry(
        &mut self,
        root: &Path,
        path: &Path,
        mtime: SystemTime,
        symbols: SymbolIndex,
    ) {
        // Fan-out into the global by-name map.
        for sym in symbols.iter() {
            let canonical = workspace_canonical(root, path, &sym.name);
            self.by_name
                .entry(sym.name.clone())
                .or_default()
                .push(WorkspaceSymbol {
                    name: sym.name.clone(),
                    kind: sym.kind,
                    file: path.to_path_buf(),
                    range: sym.range,
                    byte_range: sym.byte_range,
                    canonical,
                });
        }
        self.by_file.insert(path.to_path_buf(), FileEntry { mtime, symbols });
    }

    fn remove_file_entries(&mut self, path: &Path) {
        if self.by_file.remove(path).is_none() {
            return;
        }
        // Prune per-name buckets. Small N — workspaces with 10k symbols
        // still walk this in microseconds.
        for bucket in self.by_name.values_mut() {
            bucket.retain(|s| s.file != path);
        }
        self.by_name.retain(|_, v| !v.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn canonical_path(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Produce a Workshop-style canonical mention for a symbol. Given the
/// Universe root, a file path, and a symbol name, emit:
///
///   `@script:<Space>/<rel_without_ext>::<name>` when the file sits
///   under `<root>/Spaces/<Space>/` AND the file name matches its
///   folder (the canonical Rune script layout).
///
///   `<relative_path>::<name>` otherwise — still unique, just not
///   tied to the @mention scheme.
fn workspace_canonical(root: &Path, file: &Path, symbol: &str) -> String {
    let rel = match file.strip_prefix(root) {
        Ok(r) => r,
        Err(_) => return format!("{}::{}", file.display(), symbol),
    };
    let rel_str = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    // Spaces/<Space>/<rest...>
    let mut parts = rel_str.split('/');
    if parts.next() == Some("Spaces") {
        if let Some(space) = parts.next() {
            let remainder = parts.collect::<Vec<_>>().join("/");
            // Strip the trailing `/<folder>.rune` to land on the script
            // folder path (which is what @script: mentions expect).
            let script_folder = match remainder.rsplit_once('/') {
                Some((folder, _file)) => folder.to_string(),
                None => remainder.trim_end_matches(".rune").to_string(),
            };
            return format!("@script:{}/{}::{}", space, script_folder, symbol);
        }
    }
    format!("{}::{}", rel_str, symbol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_universe(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress-ws-test-{}", name));
        let _ = fs::remove_dir_all(&dir);
        let space = dir.join("Spaces").join("S1").join("Scripts").join("foo");
        fs::create_dir_all(&space).unwrap();
        fs::write(space.join("foo.rune"), "pub fn greet() { }\n").unwrap();
        fs::write(space.join("_instance.toml"), "").unwrap();
        let other = dir.join("Spaces").join("S1").join("Scripts").join("bar");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("bar.rune"), "pub fn farewell() { }\n").unwrap();
        dir
    }

    #[test]
    fn indexes_both_scripts() {
        let root = make_universe("indexes");
        let idx = WorkspaceIndex::build(&root);
        assert!(idx.len() >= 2);
        assert!(!idx.resolve_name("greet").is_empty());
        assert!(!idx.resolve_name("farewell").is_empty());
    }

    #[test]
    fn canonical_emits_mention_form() {
        let root = make_universe("canonical");
        let idx = WorkspaceIndex::build(&root);
        let hit = &idx.resolve_name("greet")[0];
        assert!(
            hit.canonical.starts_with("@script:S1/"),
            "canonical was {}",
            hit.canonical,
        );
        assert!(hit.canonical.ends_with("::greet"));
    }

    #[test]
    fn update_file_replaces_symbols() {
        let root = make_universe("update");
        let mut idx = WorkspaceIndex::build(&root);
        assert!(!idx.resolve_name("greet").is_empty());

        // Rewrite foo.rune with a different symbol. Sleep briefly so
        // mtime bumps on filesystems with coarse timestamp resolution
        // (ext4 sometimes floors to 1s).
        std::thread::sleep(std::time::Duration::from_millis(10));
        let foo = root.join("Spaces/S1/Scripts/foo/foo.rune");
        fs::write(&foo, "pub fn renamed() { }\n").unwrap();

        idx.update_file(&foo);
        assert!(idx.resolve_name("greet").is_empty());
        assert!(!idx.resolve_name("renamed").is_empty());
    }
}
