//! Moving an instance to the Space trash: the one way Delete, Cut and the
//! CSG booleans all do it.
//!
//! A rename alone is not a delete in a database-backed Space. The stores
//! keep uuid-keyed records that a reopen rebuilds the instance from, so a
//! renamed-away part comes back. Every trash goes through [`trash_instance`]:
//! the file watcher is told to ignore the move, the rename is retried past
//! Windows' transient locks (a part folder stays locked for 50 to 200 ms
//! after Bevy drops a `.glb` handle), and on success the instance and
//! everything registered under it are despawned, purged from every store
//! and unregistered. A move that still fails changes nothing, and the caller
//! keeps the entity, so no file is ever left without its entity or the
//! reverse.
//!
//! The caller decides where the instance lands, despawns the instance's own
//! entity, records its undo step and words its toast; those differ between
//! Delete, Cut and a boolean.

use std::path::{Path, PathBuf};

use bevy::prelude::*;

use super::SpaceFileRegistry;

/// One instance to move to the trash.
pub struct TrashRequest<'a> {
    /// The instance's own entity. Its descendants are despawned here; the
    /// caller despawns this one.
    pub entity: Entity,
    /// What moves: the instance's folder (folder form) or its file (file
    /// form, such as a script loaded from a `.luau`).
    pub source: &'a Path,
    /// The path the registry and the stores know the instance by: the
    /// folder's `_instance.toml`, or the file itself.
    pub record: &'a Path,
    /// Where `source` lands. The caller picks it, so Delete keeps one entry
    /// per item and a boolean keeps its operands together.
    pub destination: &'a Path,
}

/// A completed move.
pub struct Trashed {
    /// Registered descendants removed with the instance.
    pub descendants: usize,
    /// Retries the rename needed (0 when the first try worked).
    pub retries: u32,
    /// Whether the instance was copied out of the database because it had
    /// no folder on disk. Always false: a missing source fails with
    /// `NotFound`.
    pub from_database: bool,
}

#[derive(Debug)]
pub enum TrashError {
    /// A core service, or a terrain's data folder: never trashed.
    Protected,
    /// The rename failed (after every try for a lock that never cleared,
    /// at once for anything else); nothing moved.
    Io(std::io::Error),
}

impl std::fmt::Display for TrashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrashError::Protected => write!(f, "it is a core service or terrain data, which is never trashed"),
            TrashError::Io(e) => write!(f, "{e}"),
        }
    }
}

const ATTEMPTS: u32 = 5;
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(60);

/// Whether a failed rename can succeed if tried again: a handle still open
/// on something in the folder, which Windows reports as a denied access,
/// a sharing violation or a lock violation until it closes, or a busy
/// resource. A missing source, an occupied destination and every other
/// error fail at once.
fn is_transient(e: &std::io::Error) -> bool {
    #[cfg(windows)]
    {
        const ERROR_ACCESS_DENIED: i32 = 5;
        const ERROR_SHARING_VIOLATION: i32 = 32;
        const ERROR_LOCK_VIOLATION: i32 = 33;
        if matches!(
            e.raw_os_error(),
            Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)
        ) {
            return true;
        }
    }
    e.kind() == std::io::ErrorKind::ResourceBusy
}

/// `std::fs::rename`, tried again only past a transient lock, at most
/// [`ATTEMPTS`] times. `Ok` holds the retries it needed; `Err` holds the
/// last error and the attempts made.
fn rename_retrying(from: &Path, to: &Path) -> Result<u32, (std::io::Error, u32)> {
    let mut attempts = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(attempts),
            Err(e) => {
                attempts += 1;
                if attempts == ATTEMPTS || !is_transient(&e) {
                    return Err((e, attempts));
                }
                std::thread::sleep(RETRY_DELAY);
            }
        }
    }
}

/// The Space root holding `path`: the nearest ancestor that looks like one.
///
/// Trash belongs at `<space>/.eustress/trash`. Anchoring it relative to the
/// deleted item's own folder was only right for a flat `Workspace/Thing/`
/// layout; anything nested deeper (a column, inside a rotunda, inside a
/// wing) scattered `.eustress/trash` folders through the authored tree at
/// any depth, where nobody sees them and a rewrite of a parent folder wipes
/// them.
pub fn space_root_for(path: &Path) -> Option<PathBuf> {
    let mut cur = path;
    while let Some(parent) = cur.parent() {
        if crate::space::looks_like_space_root(parent) {
            return Some(parent.to_path_buf());
        }
        cur = parent;
    }
    None
}

/// Where `source` goes in the trash: `<space root>/.eustress/trash/<name>`,
/// or `<name>-<unix ms in hex>` when that is taken, so two deletes of
/// same-named items keep both. With no Space root above it, the trash sits
/// under the source's grandparent. Nothing is created here;
/// [`trash_instance`] creates the folder.
pub fn trash_destination(source: &Path) -> PathBuf {
    let trash_dir = space_root_for(source)
        .or_else(|| source.parent().and_then(|p| p.parent()).map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".eustress")
        .join("trash");
    let name = source.file_name().and_then(|n| n.to_str()).unwrap_or("entity");
    let base = trash_dir.join(name);
    if !base.exists() {
        return base;
    }
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    trash_dir.join(format!("{name}-{ms:x}"))
}

/// Move `req.source` to `req.destination` and clear the instance out of
/// every store. See the module documentation.
pub fn trash_instance(
    req: &TrashRequest<'_>,
    commands: &mut Commands,
    mut registry: Option<&mut SpaceFileRegistry>,
) -> Result<Trashed, TrashError> {
    if super::is_protected_service_path(req.source) || super::is_protected_service_path(req.record) {
        return Err(TrashError::Protected);
    }
    if let Some(parent) = req.destination.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Everything registered under the folder moves with it on disk; its
    // entities go too, or their queued saves would recreate files in a
    // folder that no longer exists. The folder's own record is in the list.
    let descendants: Vec<(PathBuf, Entity)> = if req.source.is_dir() {
        registry.as_deref().map(|r| r.descendants_of(req.source)).unwrap_or_default()
    } else {
        Vec::new()
    };
    if let Some(reg) = registry.as_deref_mut() {
        reg.rename_in_progress.insert(req.record.to_path_buf());
        reg.rename_in_progress.insert(req.source.to_path_buf());
        for (path, _) in &descendants {
            reg.rename_in_progress.insert(path.clone());
        }
    }

    let retries = match rename_retrying(req.source, req.destination) {
        Ok(retries) => retries,
        Err((e, _attempts)) => {
            if let Some(reg) = registry.as_deref_mut() {
                reg.rename_in_progress.remove(req.record);
                reg.rename_in_progress.remove(req.source);
                for (path, _) in &descendants {
                    reg.rename_in_progress.remove(path);
                }
            }
            return Err(TrashError::Io(e));
        }
    };

    let mut removed = 0usize;
    for (path, desc) in &descendants {
        if *desc == req.entity {
            continue;
        }
        commands.entity(*desc).try_despawn();
        if let Some(reg) = registry.as_deref_mut() {
            reg.unregister_file(path);
        }
        removed += 1;
    }
    if let Some(reg) = registry.as_deref_mut() {
        reg.unregister_file(req.record);
    }
    // One pass over the stores by the source's ORIGINAL path: the folder
    // takes its own record and every key under it (descendant records,
    // script sources, binaries, the uuid cores and indices), and a file
    // takes itself. It reads only the database, so it runs after the move.
    super::active_db::purge_subtree(req.source);
    Ok(Trashed { descendants: removed, retries, from_database: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::{FileMetadata, FileType};

    fn meta(path: &Path) -> FileMetadata {
        FileMetadata {
            path: path.to_path_buf(),
            file_type: FileType::Toml,
            service: "Workspace".into(),
            name: "x".into(),
            size: 0,
            modified: std::time::SystemTime::now(),
            children: Vec::new(),
        }
    }

    /// A folder-form part with two registered children, in a temp folder of
    /// its own, spawned into a plain World.
    struct Fixture {
        root: PathBuf,
        folder: PathBuf,
        record: PathBuf,
        kid_paths: [PathBuf; 2],
        world: World,
        part: Entity,
        kids: [Entity; 2],
        registry: SpaceFileRegistry,
    }

    fn fixture(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("eustress_trash_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = root.join("Workspace").join("PartA");
        let kid_paths = [folder.join("Child1").join("_instance.toml"), folder.join("Child2").join("_instance.toml")];
        for k in &kid_paths {
            std::fs::create_dir_all(k.parent().unwrap()).unwrap();
            std::fs::write(k, "").unwrap();
        }
        let record = folder.join("_instance.toml");
        std::fs::write(&record, "").unwrap();

        let mut world = World::new();
        let part = world.spawn_empty().id();
        let kids = [world.spawn_empty().id(), world.spawn_empty().id()];
        let mut registry = SpaceFileRegistry::default();
        registry.register(record.clone(), part, meta(&record));
        for (p, e) in kid_paths.iter().zip(kids) {
            registry.register(p.clone(), e, meta(p));
        }
        Fixture { root, folder, record, kid_paths, world, part, kids, registry }
    }

    fn run(f: &mut Fixture, destination: &Path) -> Result<Trashed, TrashError> {
        let req = TrashRequest { entity: f.part, source: &f.folder, record: &f.record, destination };
        let result = {
            let mut commands = f.world.commands();
            trash_instance(&req, &mut commands, Some(&mut f.registry))
        };
        f.world.flush();
        result
    }

    #[test]
    fn a_move_takes_the_instance_and_its_descendants_out() {
        let mut f = fixture("ok");
        let dest = f.root.join(".eustress").join("trash").join("PartA");
        let trashed = run(&mut f, &dest).expect("the move succeeds");
        assert_eq!(trashed.descendants, 2);
        assert!(!trashed.from_database);
        assert!(dest.join("_instance.toml").is_file() && !f.folder.exists());
        assert!(f.world.get_entity(f.part).is_ok(), "the caller despawns the instance itself");
        for k in f.kids {
            assert!(f.world.get_entity(k).is_err(), "every descendant is despawned");
        }
        assert!(!f.registry.is_loaded(&f.record));
        assert!(f.kid_paths.iter().all(|k| !f.registry.is_loaded(k)));
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn a_failed_move_changes_nothing() {
        let mut f = fixture("fail");
        // A non-empty folder already at the destination: the rename fails
        // on every platform, every try.
        let dest = f.root.join("Occupied");
        std::fs::create_dir_all(dest.join("inside")).unwrap();
        let result = run(&mut f, &dest);
        assert!(matches!(result, Err(TrashError::Io(_))), "the move must fail");
        assert!(f.record.is_file() && f.kid_paths.iter().all(|k| k.is_file()), "nothing moved");
        assert!(f.world.get_entity(f.part).is_ok() && f.kids.iter().all(|k| f.world.get_entity(*k).is_ok()));
        assert!(f.registry.is_loaded(&f.record) && f.kid_paths.iter().all(|k| f.registry.is_loaded(k)));
        assert!(f.registry.rename_in_progress.is_empty(), "the watcher guard is released");
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn the_trash_is_at_the_space_root_whatever_the_depth() {
        let root = std::env::temp_dir().join(format!("eustress_trash_dest_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let deep = root.join("Workspace").join("Wing").join("Rotunda").join("Column");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(space_root_for(&deep).as_deref(), Some(root.as_path()));
        let first = trash_destination(&deep);
        assert_eq!(first, root.join(".eustress").join("trash").join("Column"));
        // Taken: the next one gets its own name beside it.
        std::fs::create_dir_all(&first).unwrap();
        let second = trash_destination(&deep);
        assert_ne!(second, first);
        assert_eq!(second.parent(), first.parent());
        assert!(second.file_name().unwrap().to_string_lossy().starts_with("Column-"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_source_fails_and_changes_nothing() {
        let mut f = fixture("missing");
        // A source that never existed: a folder deleted just before could
        // still be pending deletion on Windows, which reports a denied
        // access (a lock) rather than a missing path.
        f.folder = f.root.join("Workspace").join("NeverThere");
        f.record = f.folder.join("_instance.toml");
        let dest = f.root.join("trash").join("NeverThere");
        let result = run(&mut f, &dest);
        let Err(TrashError::Io(e)) = result else { panic!("a missing source must fail") };
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
        assert!(!dest.exists(), "nothing lands in the trash");
        assert!(f.world.get_entity(f.part).is_ok() && f.kids.iter().all(|k| f.world.get_entity(*k).is_ok()));
        assert!(f.registry.rename_in_progress.is_empty(), "the watcher guard is released");
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn a_missing_source_is_tried_once() {
        let root = std::env::temp_dir().join(format!("eustress_trash_once_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let Err((e, attempts)) = rename_retrying(&root.join("NeverThere"), &root.join("Dest")) else {
            panic!("a missing source must fail")
        };
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(attempts, 1, "a missing source is never retried");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_a_lock_is_worth_another_try() {
        use std::io::{Error, ErrorKind};
        assert!(!is_transient(&Error::from(ErrorKind::NotFound)));
        assert!(!is_transient(&Error::from(ErrorKind::AlreadyExists)));
        assert!(!is_transient(&Error::from(ErrorKind::DirectoryNotEmpty)));
        assert!(is_transient(&Error::from(ErrorKind::ResourceBusy)));
        #[cfg(windows)]
        {
            for (code, transient) in [(5, true), (32, true), (33, true), (2, false), (3, false), (145, false), (183, false)] {
                assert_eq!(is_transient(&Error::from_raw_os_error(code)), transient, "Windows error {code}");
            }
        }
    }
}
