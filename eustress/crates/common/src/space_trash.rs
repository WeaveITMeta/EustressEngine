//! Moving a file or folder into a Space's trash, on disk only: the part of a
//! delete every surface shares, for callers that hold no live scene (the
//! Workshop and MCP tools in `eustress-tools`).
//!
//! The layout is the engine's (`engine/src/space/trash.rs`): the trash sits
//! at `<space>/.eustress/trash`, never beside the deleted item, and a name
//! already there gets a `-<unix ms in hex>` suffix, so two deletes of
//! same-named items keep both. The rename is retried past Windows' transient
//! locks (a part folder stays locked for 50 to 200 ms after a `.glb` handle
//! is dropped). There is deliberately no remove fallback: a failed move
//! changes nothing, so nothing is ever lost without a trash entry.
//!
//! The loader and the file watcher skip `.eustress`, so a running engine sees
//! the move as the item leaving the Space.

use std::path::{Path, PathBuf};

const ATTEMPTS: u32 = 5;
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(60);

/// The trash folder of the Space at `space_root`.
pub fn trash_dir(space_root: &Path) -> PathBuf {
    space_root.join(".eustress").join("trash")
}

/// Where `source` goes in the trash of the Space at `space_root`:
/// `<space>/.eustress/trash/<name>`, or `<name>-<unix ms in hex>` when that
/// is taken. Nothing is created here.
pub fn trash_destination_in(space_root: &Path, source: &Path) -> PathBuf {
    let dir = trash_dir(space_root);
    let name = source.file_name().and_then(|n| n.to_str()).unwrap_or("entity");
    let base = dir.join(name);
    if !base.exists() {
        return base;
    }
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    dir.join(format!("{name}-{ms:x}"))
}

/// Whether a failed rename can succeed if tried again: a handle still open
/// on something in the folder, which Windows reports as a denied access, a
/// sharing violation or a lock violation until it closes, or a busy
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

/// `dir` is a real folder (no symbolic link or junction) whose canonical form
/// lies under `root`, the Space root's canonical form. A Space copied from
/// elsewhere can carry a link at `.eustress` or `.eustress/trash`, and a
/// rename through it would carry the user's part out of the Space.
fn real_folder_inside(root: &Path, dir: &Path) -> std::io::Result<()> {
    let refuse = |why: &str| {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} {why}, so nothing was moved", dir.display()),
        ))
    };
    let meta = std::fs::symlink_metadata(dir)?;
    if meta.file_type().is_symlink() {
        return refuse("is a link, not a folder of the Space");
    }
    if !meta.is_dir() {
        return refuse("is not a folder");
    }
    if !dir.canonicalize()?.starts_with(root) {
        return refuse("leads outside the Space");
    }
    Ok(())
}

/// Move `source` (a file or a folder) into the trash of the Space at
/// `space_root`, and return where it went. `.eustress` and its `trash` folder
/// must be real folders inside the Space, checked before and after the trash
/// folder is made. The rename is retried only past a transient lock, at most
/// [`ATTEMPTS`] times; on any failure `source` is where it was. The caller
/// has already confined `source` to the Space.
pub fn move_to_trash(space_root: &Path, source: &Path) -> std::io::Result<PathBuf> {
    if !source.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} does not exist", source.display()),
        ));
    }
    let root = space_root.canonicalize()?;
    let folders = [space_root.join(".eustress"), trash_dir(space_root)];
    for dir in &folders {
        if std::fs::symlink_metadata(dir).is_ok() {
            real_folder_inside(&root, dir)?;
        }
    }
    std::fs::create_dir_all(trash_dir(space_root))?;
    for dir in &folders {
        real_folder_inside(&root, dir)?;
    }
    let destination = trash_destination_in(space_root, source);
    let mut attempts = 0;
    loop {
        match std::fs::rename(source, &destination) {
            Ok(()) => return Ok(destination),
            Err(e) => {
                attempts += 1;
                if attempts == ATTEMPTS || !is_transient(&e) {
                    return Err(e);
                }
                std::thread::sleep(RETRY_DELAY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("eus_space_trash_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Workspace")).unwrap();
        root
    }

    #[test]
    fn a_folder_moves_whole_into_the_space_trash() {
        let root = space("folder");
        let part = root.join("Workspace").join("Tower").join("Floor 3");
        std::fs::create_dir_all(&part).unwrap();
        std::fs::write(part.join("_instance.toml"), "[metadata]\nclass_name = \"Part\"\n").unwrap();
        let went = move_to_trash(&root, &part).unwrap();
        assert_eq!(went, root.join(".eustress").join("trash").join("Floor 3"), "at the Space root, not beside it");
        assert!(!part.exists());
        assert_eq!(
            std::fs::read_to_string(went.join("_instance.toml")).unwrap(),
            "[metadata]\nclass_name = \"Part\"\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_delete_of_the_same_name_keeps_both() {
        let root = space("twice");
        let mut kept = Vec::new();
        for body in ["first", "second"] {
            let file = root.join("Workspace").join("Lamp.part.toml");
            std::fs::write(&file, body).unwrap();
            kept.push(move_to_trash(&root, &file).unwrap());
        }
        assert_ne!(kept[0], kept[1]);
        assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), "first");
        assert_eq!(std::fs::read_to_string(&kept[1]).unwrap(), "second");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A trash folder that is a junction to somewhere else is refused, and
    /// the part stays exactly where it was; so is a junction at `.eustress`.
    #[cfg(windows)]
    #[test]
    fn a_trash_that_leads_out_of_the_space_is_refused() {
        for linked in [".eustress/trash", ".eustress"] {
            let root = space(&format!("junction_{}", linked.len()));
            let elsewhere = root.with_extension("elsewhere");
            let _ = std::fs::remove_dir_all(&elsewhere);
            std::fs::create_dir_all(&elsewhere).unwrap();
            // Built from components: mklink takes no forward slashes.
            let link = linked.split('/').fold(root.clone(), |p, part| p.join(part));
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            let made = std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&link)
                .arg(&elsewhere)
                .output()
                .unwrap();
            assert!(
                made.status.success(),
                "mklink: {} {}",
                String::from_utf8_lossy(&made.stdout),
                String::from_utf8_lossy(&made.stderr)
            );
            let part = root.join("Workspace").join("Crate");
            std::fs::create_dir_all(&part).unwrap();
            std::fs::write(part.join("_instance.toml"), "kept").unwrap();

            let err = move_to_trash(&root, &part).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "{linked}: {err}");
            assert_eq!(std::fs::read_to_string(part.join("_instance.toml")).unwrap(), "kept", "{linked}: left in place");
            assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0, "{linked}: nothing reached the other folder");

            std::fs::remove_dir(&link).unwrap();
            let _ = std::fs::remove_dir_all(&root);
            let _ = std::fs::remove_dir_all(&elsewhere);
        }
    }

    #[test]
    fn a_missing_source_fails_and_moves_nothing() {
        let root = space("missing");
        let err = move_to_trash(&root, &root.join("Workspace").join("Gone")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(!trash_dir(&root).exists(), "no trash folder made for nothing");
        let _ = std::fs::remove_dir_all(&root);
    }
}
