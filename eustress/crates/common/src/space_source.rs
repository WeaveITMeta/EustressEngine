//! # A Space's files, wherever the Space is kept
//!
//! The one seam every reader of a Space's own files goes through:
//! Space-relative, `/`-separated paths in, bytes out. Studio's loader reads a
//! Space's folder ([`DiskSource`]) or its world DB (the engine's
//! `FjallSource`); the desktop Player reads the folder it opened; the
//! browser Player reads the records it downloaded, with no disk at all
//! ([`RecordsSource`]). The Play runtime reads through whichever its shell
//! hands it, so a Space's materials and GUI images read the same in all three.
//!
//! A path's segments are taken as [`DiskSource`] has always taken them: split
//! on `/` and `\`, with empty, `.` and `..` segments dropped, so no path
//! climbs out of the Space. [`RecordsSource`] keys its files by the same
//! rule, so both answer the same path with the same file.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::path::PathBuf;
use std::sync::Arc;

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

/// A file's size and modification time: when either moves, it was rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    pub modified: Option<std::time::SystemTime>,
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

    /// The file's size and modification time, for a reader that reloads a
    /// file once it is rewritten. `None` when the source cannot tell, and
    /// always for a source whose files never change.
    fn stamp(&self, _rel: &str) -> Option<FileStamp> {
        None
    }
}

/// The segments of a Space-relative path, as every source takes them.
fn segments(rel: &str) -> impl Iterator<Item = &str> {
    rel.split(['/', '\\']).filter(|seg| !seg.is_empty() && *seg != "." && *seg != "..")
}

/// A Space-relative path in its one spelling: [`segments`] joined by `/`.
fn normalized(rel: &str) -> String {
    segments(rel).collect::<Vec<_>>().join("/")
}

/// A listing's `rel_path` for `name` in `rel_dir`, spelled as the caller
/// spelled the directory.
fn entry_path(rel_dir: &str, name: &str) -> String {
    if rel_dir.is_empty() {
        name.to_string()
    } else {
        format!("{rel_dir}/{name}")
    }
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
        for seg in segments(rel) {
            p.push(seg);
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
            let rel_path = entry_path(rel_dir, &name);
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

    fn stamp(&self, rel: &str) -> Option<FileStamp> {
        let meta = std::fs::metadata(self.abs(rel)).ok()?;
        Some(FileStamp { len: meta.len(), modified: meta.modified().ok() })
    }
}

/// A Space held in memory as its records, `(Space-relative path, bytes)`
/// pairs: the browser Player's, which downloaded its world and has no disk.
/// Its directories are the folders its files sit in. Its files never
/// change, so it gives no [`SpaceSource::stamp`].
pub struct RecordsSource {
    files: BTreeMap<String, Arc<Vec<u8>>>,
    /// Every folder a file sits in, the Space root (`""`) included.
    dirs: BTreeSet<String>,
}

impl RecordsSource {
    /// The Space whose records these are. A path is keyed by the rule every
    /// source reads paths by; a record with no segment left (`""`, `.`) is
    /// no file, and of two records with one key, the later wins.
    pub fn new(records: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        let mut files = BTreeMap::new();
        let mut dirs = BTreeSet::from([String::new()]);
        for (path, bytes) in records {
            let key = normalized(&path);
            if key.is_empty() {
                continue;
            }
            let mut at = key.as_str();
            while let Some((parent, _)) = at.rsplit_once('/') {
                dirs.insert(parent.to_string());
                at = parent;
            }
            files.insert(key, Arc::new(bytes));
        }
        Self { files, dirs }
    }

    /// Every file, by its key, sharing its bytes: the browser hands these to
    /// Bevy's in-memory asset source without a second copy.
    pub fn files(&self) -> impl Iterator<Item = (&str, &Arc<Vec<u8>>)> {
        self.files.iter().map(|(key, bytes)| (key.as_str(), bytes))
    }
}

impl SpaceSource for RecordsSource {
    fn read(&self, rel: &str) -> std::io::Result<Vec<u8>> {
        let key = normalized(rel);
        match self.files.get(&key) {
            Some(bytes) => Ok(bytes.to_vec()),
            None if self.dirs.contains(&key) => {
                Err(std::io::Error::new(std::io::ErrorKind::IsADirectory, format!("{key} is a folder")))
            }
            None => Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("no file {key}"))),
        }
    }

    fn list(&self, rel_dir: &str) -> std::io::Result<Vec<SourceEntry>> {
        let dir = normalized(rel_dir);
        if !self.dirs.contains(&dir) {
            let kind = if self.files.contains_key(&dir) {
                std::io::ErrorKind::NotADirectory
            } else {
                std::io::ErrorKind::NotFound
            };
            return Err(std::io::Error::new(kind, format!("no folder {dir}")));
        }
        // Every key under the folder sorts in one run from "<dir>/", so only
        // that run is walked.
        let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        let child = |key: &str| -> Option<String> {
            let rest = key.strip_prefix(prefix.as_str())?;
            (!rest.is_empty() && !rest.contains('/')).then(|| rest.to_string())
        };
        let under = (Bound::Included(prefix.as_str()), Bound::Unbounded);
        let dirs = self.dirs.range::<str, _>(under).take_while(|d| d.starts_with(prefix.as_str()));
        let files = self.files.range::<str, _>(under).take_while(|(f, _)| f.starts_with(prefix.as_str()));
        let mut out = Vec::new();
        for (name, is_dir) in dirs
            .filter_map(|d| child(d).map(|name| (name, true)))
            .chain(files.filter_map(|(f, _)| child(f).map(|name| (name, false))))
        {
            let rel_path = entry_path(rel_dir, &name);
            out.push(SourceEntry { name, rel_path, is_dir });
        }
        Ok(out)
    }

    fn exists(&self, rel: &str) -> bool {
        let key = normalized(rel);
        self.files.contains_key(&key) || self.dirs.contains(&key)
    }

    fn is_fjall(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space() -> Vec<(String, Vec<u8>)> {
        vec![
            ("space.toml".into(), b"[space]\n".to_vec()),
            ("Workspace/_service.toml".into(), b"[service]\nclass_name = \"Workspace\"\n".to_vec()),
            ("Workspace/Ball/_instance.toml".into(), b"[metadata]\nclass_name = \"Part\"\n".to_vec()),
            ("Workspace/Ball/ball.png".into(), vec![0x89, b'P', b'N', b'G', 0, 1, 2, 3]),
            ("MaterialService/Brick.mat.toml".into(), b"[material]\nname = \"Brick\"\n".to_vec()),
            ("MaterialService/Textures/Brick/color.png".into(), vec![9, 8, 7]),
        ]
    }

    fn sorted(mut entries: Vec<SourceEntry>) -> Vec<SourceEntry> {
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    /// The same Space, read from its folder and from its records, answers
    /// every listing, read and existence question the same way.
    #[test]
    fn a_space_reads_alike_from_its_folder_and_its_records() {
        let root = std::env::temp_dir().join(format!("eustress_space_source_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, bytes) in space() {
            let file = root.join(&path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, bytes).unwrap();
        }
        let disk = DiskSource::new(&root);
        let records = RecordsSource::new(space());

        // "Workspace/" too: a listing spells each path as the caller spelled
        // the folder, as the folder source always has.
        let dirs = [
            "",
            "Workspace",
            "Workspace/",
            "Workspace/Ball",
            "MaterialService",
            "MaterialService/Textures",
            "MaterialService/Textures/Brick",
        ];
        for dir in dirs {
            let a = sorted(disk.list(dir).expect("the folder lists"));
            let b = sorted(records.list(dir).expect("the records list"));
            assert_eq!(a, b, "list({dir:?})");
            assert!(disk.exists(dir) && records.exists(dir), "exists({dir:?})");
            assert!(records.read(dir).is_err(), "a folder is not a file: {dir:?}");
        }
        for (path, bytes) in space() {
            assert_eq!(disk.read(&path).expect("on disk"), bytes, "{path}");
            assert_eq!(records.read(&path).expect("in the records"), bytes, "{path}");
            assert!(disk.exists(&path) && records.exists(&path), "{path}");
        }
        assert_eq!(
            disk.read_to_string("MaterialService/Brick.mat.toml").unwrap(),
            records.read_to_string("MaterialService/Brick.mat.toml").unwrap()
        );
        for path in ["Workspace/Gone.toml", "Nowhere", "Workspace/Ball/_instance.toml/x"] {
            assert!(disk.read(path).is_err() && records.read(path).is_err(), "{path}");
            assert!(!disk.exists(path) && !records.exists(path), "{path}");
            assert!(disk.list(path).is_err() && records.list(path).is_err(), "{path}");
        }
        // Each spelling of one path names the same file in both.
        for spelling in ["./Workspace/Ball/_instance.toml", "Workspace\\Ball\\_instance.toml", "Workspace//Ball/./_instance.toml"] {
            let a = disk.read(spelling).expect("on disk");
            assert_eq!(records.read(spelling).expect("in the records"), a, "{spelling}");
        }
        assert!(disk.stamp("space.toml").is_some(), "a folder's file has a stamp");
        assert_eq!(records.stamp("space.toml"), None, "records never change");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A folder lists its own children only, beside folders whose names start
    /// the same way.
    #[test]
    fn a_folder_lists_only_its_own_children() {
        let records = RecordsSource::new(
            ["Work/a.toml", "Workspace/b.toml", "Workspace/Sub/d.toml", "Workspace-old/c.toml", "Workspace2/e.toml"]
                .into_iter()
                .map(|path| (path.to_string(), Vec::new())),
        );
        let names = |dir: &str| -> Vec<(String, bool)> {
            sorted(records.list(dir).unwrap()).into_iter().map(|e| (e.name, e.is_dir)).collect()
        };
        assert_eq!(names("Workspace"), [("Sub".to_string(), true), ("b.toml".to_string(), false)]);
        assert_eq!(names("Work"), [("a.toml".to_string(), false)]);
        assert_eq!(names("Workspace/Sub"), [("d.toml".to_string(), false)]);
        assert_eq!(names("").len(), 4, "the root lists its four folders");
    }

    /// Records spelled with `\`, `./`, `//` or `..` are filed under the one
    /// key (`..` never climbs, as on disk), a record that names no file is
    /// dropped, and the later of two records with one key wins.
    #[test]
    fn records_are_keyed_by_the_one_spelling() {
        let records = RecordsSource::new(vec![
            ("Workspace\\Ball.toml".to_string(), b"one".to_vec()),
            ("./Workspace//Ball.toml".to_string(), b"two".to_vec()),
            ("Lighting/../Sky.toml".to_string(), b"sky".to_vec()),
            ("..".to_string(), b"nothing".to_vec()),
        ]);
        let keys: Vec<&str> = records.files().map(|(k, _)| k).collect();
        assert_eq!(keys, ["Lighting/Sky.toml", "Workspace/Ball.toml"]);
        assert_eq!(records.read("Workspace/Ball.toml").unwrap(), b"two");
        assert!(records.exists("Lighting"), "a file's folder exists");
        assert!(!records.exists("Sky.toml"));
        assert!(records.list("Lighting").unwrap().iter().all(|e| !e.is_dir));
    }
}
