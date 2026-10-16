//! Runtime-swappable `space://` Bevy asset source.
//!
//! ## Why this exists
//!
//! Meshes, textures, and GLTF scenes inside a Space are loaded through the
//! `space://{relative}` asset scheme (see `instance_loader`, `material_loader`,
//! `file_loader`). Bevy resolves that scheme through whatever
//! [`AssetReader`] was registered for the `"space"` source at startup.
//!
//! The old registration used
//! `AssetSourceBuilder::platform_default(&launch_root, None)`, which bakes the
//! *launch* Space root into a [`FileAssetReader`] **once**. When the user
//! switches Space/Universe at runtime (Vehicle Simulator → Universe1/Space1),
//! the [`SpaceRoot`](crate::space::SpaceRoot) resource updates but the asset
//! reader keeps resolving against the original launch root — so every mesh URL
//! for the new Space points at the *old* Space's folder:
//!
//! ```text
//! bevy_asset: Path not found: ...\Vehicle Simulator\...\meshes\Foo.glb
//! ```
//!
//! No meshes load → black screen.
//!
//! ## The fix
//!
//! A global, runtime-swappable root ([`SPACE_ASSET_ROOT`]) plus a custom
//! [`DynamicSpaceReader`] that resolves each path under `space_asset_root()`
//! **at call time**, on every read, refusing one that would leave the Space
//! (`in_space`). Switching Space updates the global (see
//! [`set_space_asset_root`]) and the very next asset read resolves against the
//! new root — no reader re-registration needed (Bevy doesn't support swapping
//! a source after `AssetPlugin` init anyway).
//!
//! ## Why a zero-sized reader (no captured root)
//!
//! On Windows, [`FileAssetReader`]'s returned reader borrows the reader
//! instance (`GuardedFile<'a>` carries a `PhantomData<&'a ()>`). Delegating to
//! a *temporary* `FileAssetReader` built per call would therefore fail to
//! compile — the returned future/reader would borrow a value dropped at the
//! end of the call. We sidestep that entirely: [`DynamicSpaceReader`] is
//! zero-sized, holds no root, and returns **owned** readers
//! ([`VecReader`], wrapping bytes read via the async filesystem) that have no
//! lifetime tie to `&self`. This is also exactly what makes per-call root
//! swapping correct: there is nothing cached to go stale.
//!
//! ## IO approach
//!
//! Reads go through [`async_fs`] — the same non-blocking filesystem crate
//! Bevy's own [`FileAssetReader`] uses — so asset reads never block the IO
//! task pool's executor thread. `async-fs` is already in the dependency graph
//! transitively (pulled by `bevy_asset`); it is declared as a direct dep so we
//! can name it here, pinned to the version already in `Cargo.lock`.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use bevy::asset::io::{AssetReader, AssetReaderError, PathStream, Reader, VecReader};
use bevy::prelude::*;
// `futures_lite` isn't a direct dependency of this crate, but Bevy re-exports
// it (`bevy_tasks::futures_lite`, surfaced as `bevy::tasks::futures_lite`).
// `StreamExt` provides `.filter_map` on the async-fs `ReadDir` stream.
use bevy::tasks::futures_lite::StreamExt;

/// The folders `space://` paths may reach, for one Space: the Space, and
/// its Universe's shared asset folders
/// ([`eustress_common::space_path::shared_asset_roots`]: `<Universe>/assets`,
/// where new shared meshes and media go, and `<Universe>/.eustress/assets`,
/// the Assets panel's imports and the default parts). Never the rest of the
/// Universe: it holds the other Spaces, their databases and the engine's own
/// files.
#[derive(Clone, Debug)]
pub(crate) struct AssetRoots {
    /// The Space.
    pub space: PathBuf,
    /// Its Universe, and that folder's canonical form, when it has one.
    universe: Option<(PathBuf, PathBuf)>,
}

impl AssetRoots {
    /// The roots of the Space at `space`, its Universe found once.
    pub(crate) fn of(space: PathBuf) -> Self {
        let universe = crate::space::universe_root_for_path(&space)
            .and_then(|universe| std::fs::canonicalize(&universe).ok().map(|canonical| (universe, canonical)));
        Self { space, universe }
    }

    /// The roots of a Space in a Universe named outright.
    #[cfg(test)]
    pub(crate) fn with_universe(space: PathBuf, universe: &Path) -> Self {
        let universe = std::fs::canonicalize(universe).ok().map(|canonical| (universe.to_path_buf(), canonical));
        Self { space, universe }
    }

    /// The Space's Universe folder, when it has one.
    pub(crate) fn universe(&self) -> Option<&Path> {
        self.universe.as_ref().map(|(universe, _)| universe.as_path())
    }

    /// The folders a `space://` path may reach now: the Space first, then
    /// each shared asset folder that exists and lies inside the Universe (a
    /// folder that is a link out of it is never a root). Checked on every
    /// call, so a folder made while the Space is open (Workshop's first
    /// product, an import) serves at once.
    pub(crate) fn allowed(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.space.clone()];
        if let Some((universe, canonical)) = &self.universe {
            roots.extend(
                eustress_common::space_path::shared_asset_roots(universe)
                    .into_iter()
                    .filter(|root| std::fs::canonicalize(root).is_ok_and(|resolved| resolved.starts_with(canonical))),
            );
        }
        roots
    }
}

/// The live [`AssetRoots`]. Updated on every Space/Universe switch via
/// [`set_space_asset_root`]. Read on every asset read.
///
/// Initialised lazily to [`crate::space::default_space_root`] on first access
/// so the value is correct even if an asset read somehow races ahead of the
/// explicit startup stamp in `main`.
static SPACE_ASSET_ROOT: RwLock<Option<AssetRoots>> = RwLock::new(None);

/// The live Space's [`AssetRoots`]. Unstamped: the default Space's, not
/// cached, so a later explicit `set_space_asset_root` still wins.
pub(crate) fn space_asset_roots() -> AssetRoots {
    if let Ok(guard) = SPACE_ASSET_ROOT.read() {
        if let Some(roots) = guard.as_ref() {
            return roots.clone();
        }
    }
    AssetRoots::of(crate::space::default_space_root())
}

/// Returns the current `space://` resolution root (the Space).
///
/// If the global hasn't been stamped yet, falls back to
/// [`crate::space::default_space_root`] (and does not cache it, so a later
/// explicit `set_space_asset_root` still wins).
pub fn space_asset_root() -> PathBuf {
    // Fast path: already set.
    if let Ok(guard) = SPACE_ASSET_ROOT.read() {
        if let Some(roots) = guard.as_ref() {
            return roots.space.clone();
        }
    }
    // Not set yet — derive the same default the rest of the engine boots with.
    crate::space::default_space_root()
}

/// Point the `space://` asset source at `root` and its Universe's shared
/// asset folders. Call this anywhere the live
/// [`SpaceRoot`](crate::space::SpaceRoot) changes so subsequent mesh/texture
/// loads resolve against the new Space. Cheap and idempotent.
pub fn set_space_asset_root(root: PathBuf) {
    // A panic in another thread poisons the lock; the roots are plain paths,
    // so recover and overwrite rather than propagate it.
    let mut guard = SPACE_ASSET_ROOT.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.as_ref().map(|roots| roots.space.as_path()) != Some(root.as_path()) {
        info!("📁 space:// asset root → {:?}", root);
        *guard = Some(AssetRoots::of(root));
    }
}

/// Bevy `AssetReader` for the `space://` scheme that resolves the Space root
/// fresh on every call. Zero-sized: holds no state, so there is nothing to go
/// stale across a Space switch.
#[derive(Default)]
pub struct DynamicSpaceReader;

/// Bevy's meta-path convention: `Foo.glb` → `Foo.glb.meta`, `Foo` → `Foo.meta`.
/// Mirrors `bevy_asset::io::get_meta_path` (which is `pub(crate)`, so we
/// replicate it). Most Space assets (`.glb`, `.png`) have no `.meta` sidecar,
/// so `read_meta` returns `NotFound` — the expected signal that tells Bevy to
/// use default loader settings.
fn meta_path(path: &Path) -> PathBuf {
    let mut meta = path.to_path_buf();
    let mut ext = path.extension().unwrap_or_default().to_os_string();
    if !ext.is_empty() {
        ext.push(".");
    }
    ext.push("meta");
    meta.set_extension(ext);
    meta
}

/// Read a file's full bytes into an owned [`VecReader`] (non-blocking). Missing
/// files map to [`AssetReaderError::NotFound`] with the absolute path, matching
/// [`FileAssetReader`] behaviour so log lines stay diagnosable.
async fn read_owned(full_path: PathBuf) -> Result<VecReader, AssetReaderError> {
    match async_fs::read(&full_path).await {
        Ok(bytes) => Ok(VecReader::new(bytes)),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                Err(AssetReaderError::NotFound(full_path))
            } else {
                Err(e.into())
            }
        }
    }
}

/// `path` in one of the `allowed` folders ([`AssetRoots::allowed`]: the
/// Space, then its Universe's shared asset folders), or the `NotFound` a
/// missing file gets. A relative path stays in the Space; an absolute one
/// (how the loaders name a mesh outside the Space) must be a local drive path
/// in one of them ([`eustress_common::space_path::resolve_in_roots`]: a UNC
/// share, a `..`, a device name, a junction out or a sibling Space is
/// refused, a network path by its text before anything is opened). A refused
/// path reads as absent and names only the path asked for, so nothing tells
/// a caller whether a file outside exists. Studio runs with
/// `UnapprovedPathMode::Allow`, so this is the only check a `space://` path
/// meets, and every method goes through it.
fn in_space(allowed: &[PathBuf], path: &Path) -> Result<PathBuf, AssetReaderError> {
    let allowed: Vec<&Path> = allowed.iter().map(PathBuf::as_path).collect();
    eustress_common::space_path::resolve_in_roots(&allowed, path).ok_or_else(|| AssetReaderError::NotFound(path.to_path_buf()))
}

impl AssetReader for DynamicSpaceReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        // Resolve against the LIVE root on every call — this is the whole point.
        let full_path = in_space(&space_asset_roots().allowed(), path)?;
        read_owned(full_path).await
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let full_path = in_space(&space_asset_roots().allowed(), &meta_path(path))?;
        read_owned(full_path).await
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        // Entries are listed relative to the Space. A directory in a shared
        // asset folder (reached only by its absolute path) lists nothing,
        // which is fine: nothing lists one.
        let roots = space_asset_roots();
        let root = roots.space.clone();
        let full_path = in_space(&roots.allowed(), path)?;
        match async_fs::read_dir(&full_path).await {
            Ok(read_dir) => {
                // Map each entry to a path RELATIVE to the live root, matching
                // FileAssetReader. Filter out `.meta` sidecars and hidden
                // files (they are directly targetable but not listed).
                let mapped = read_dir.filter_map(move |entry| {
                    entry.ok().and_then(|dir_entry| {
                        let p = dir_entry.path();
                        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                            if ext.eq_ignore_ascii_case("meta") {
                                return None;
                            }
                        }
                        if p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n.starts_with('.'))
                            .unwrap_or(false)
                        {
                            return None;
                        }
                        // `full_path` is `root.join(path)`, so stripping `root`
                        // yields a path relative to the source root (what Bevy
                        // expects), preserving the `path/` prefix.
                        p.strip_prefix(&root).ok().map(|rel| rel.to_owned())
                    })
                });
                let stream: Box<PathStream> = Box::new(mapped);
                Ok(stream)
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Err(AssetReaderError::NotFound(full_path))
                } else {
                    Err(e.into())
                }
            }
        }
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        let full_path = in_space(&space_asset_roots().allowed(), path)?;
        match async_fs::metadata(&full_path).await {
            Ok(md) => Ok(md.file_type().is_dir()),
            Err(_) => Err(AssetReaderError::NotFound(full_path)),
        }
    }
}

/// A reader over several folders, tried in order: the first holding a path
/// answers for it. Studio's default asset source reads its own `assets/`,
/// then common's shipped assets, so a file that moves into `common/assets`
/// (the one folder both apps ship) still resolves by the path Studio and
/// saved Spaces use.
pub struct LayeredAssetReader {
    roots: Vec<PathBuf>,
}

impl LayeredAssetReader {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }

    /// The first root holding `path` reads it; a missing file names the
    /// first root's path, as a single-folder reader would.
    async fn read_first(&self, path: &Path) -> Result<VecReader, AssetReaderError> {
        // A network or device path is never opened: Studio runs with
        // `UnapprovedPathMode::Allow`, so nothing else stops one, and
        // resolving a UNC path is itself an SMB access. A local absolute path
        // still loads, as the importers need.
        if !eustress_common::space_path::is_local(path) {
            return Err(AssetReaderError::NotFound(path.to_path_buf()));
        }
        let mut first_missing: Option<PathBuf> = None;
        for root in &self.roots {
            match read_owned(root.join(path)).await {
                Err(AssetReaderError::NotFound(missing)) => {
                    first_missing.get_or_insert(missing);
                }
                other => return other,
            }
        }
        Err(AssetReaderError::NotFound(first_missing.unwrap_or_else(|| path.to_path_buf())))
    }
}

impl AssetReader for LayeredAssetReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.read_first(path).await
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.read_first(&meta_path(path)).await
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        // The first root holding the directory lists it, relative to that
        // root, without `.meta` sidecars or hidden files. Never a network or
        // device path (`read_first`).
        if !eustress_common::space_path::is_local(path) {
            return Err(AssetReaderError::NotFound(path.to_path_buf()));
        }
        for root in &self.roots {
            let Ok(read_dir) = async_fs::read_dir(root.join(path)).await else { continue };
            let root = root.clone();
            let mapped = read_dir.filter_map(move |entry| {
                entry.ok().and_then(|dir_entry| {
                    let p = dir_entry.path();
                    if p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("meta")) {
                        return None;
                    }
                    if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')) {
                        return None;
                    }
                    p.strip_prefix(&root).ok().map(|rel| rel.to_owned())
                })
            });
            let stream: Box<PathStream> = Box::new(mapped);
            return Ok(stream);
        }
        Err(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        if !eustress_common::space_path::is_local(path) {
            return Err(AssetReaderError::NotFound(path.to_path_buf()));
        }
        for root in &self.roots {
            if let Ok(md) = async_fs::metadata(root.join(path)).await {
                return Ok(md.file_type().is_dir());
            }
        }
        Err(AssetReaderError::NotFound(path.to_path_buf()))
    }
}

/// Bevy system: stamp the global `space://` root whenever
/// [`SpaceRoot`](crate::space::SpaceRoot) changes.
///
/// This is the authoritative chokepoint. Every Space-switch site mutates the
/// `SpaceRoot` resource (runtime switch in `space_ops::open_space`, the
/// `--space`/`--universe` CLI overrides in `startup`, and the in-place
/// "Save As" mutation in `file_event_handler`). `Changed<SpaceRoot>` fires for
/// **all** of them — current and future — so a new switch path can never
/// silently leave the asset root stale. Runs cheaply: a no-op when the
/// resource didn't change, and `set_space_asset_root` early-outs when the value
/// is unchanged.
pub fn sync_space_asset_root_on_change(space_root: Res<crate::space::SpaceRoot>) {
    if space_root.is_changed() {
        set_space_asset_root(space_root.0.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Space in a Universe: its own file, a Workshop mesh in
    /// `<Universe>/assets`, an Assets panel mesh in `.eustress/assets`, and a
    /// sibling Space.
    fn universe(tag: &str) -> (PathBuf, PathBuf) {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let universe = std::env::temp_dir().join(format!("eustress_space_reader_{tag}_{}_{nanos}", std::process::id()));
        let space = universe.join("Spaces").join("Garage");
        std::fs::create_dir_all(space.join("SoundService")).unwrap();
        std::fs::write(space.join("SoundService").join("flush.ogg"), b"ogg").unwrap();
        std::fs::create_dir_all(universe.join("assets").join("meshes").join("Drone")).unwrap();
        std::fs::write(universe.join("assets").join("meshes").join("Drone").join("body.glb"), b"glb").unwrap();
        std::fs::create_dir_all(universe.join(".eustress").join("assets").join("meshes")).unwrap();
        std::fs::write(universe.join(".eustress").join("assets").join("meshes").join("crate.glb"), b"glb").unwrap();
        std::fs::write(universe.join(".eustress").join("engine.port"), b"1234").unwrap();
        std::fs::create_dir_all(universe.join("Spaces").join("Other")).unwrap();
        std::fs::write(universe.join("Spaces").join("Other").join("secret.glb"), b"glb").unwrap();
        (universe, space)
    }

    /// A `space://` path that would leave the Space and the shared asset
    /// folders reads as absent, naming the path asked for and never the file
    /// it would have reached; its sidecar is held to the same rule.
    #[test]
    fn a_path_out_of_the_space_reads_as_absent() {
        let (universe, space) = universe("out");
        let allowed = AssetRoots::with_universe(space.clone(), &universe).allowed();
        let mut hostile: Vec<PathBuf> = ["//attacker.example/share/x.ogg", r"\\host\share\x.ogg", "C:/Windows/win.ini", "/Windows/win.ini", "../x.ogg", "a/../../x.ogg", "SoundService/NUL"]
            .iter()
            .map(PathBuf::from)
            .collect();
        hostile.push(universe.join("Spaces").join("Other").join("secret.glb"));
        hostile.push(universe.join(".eustress").join("engine.port"));
        hostile.push(universe.clone());
        for asked in &hostile {
            match in_space(&allowed, asked) {
                Err(AssetReaderError::NotFound(named)) => assert_eq!(&named, asked, "{asked:?} named another path"),
                other => panic!("{asked:?} was not refused: {other:?}"),
            }
            assert!(in_space(&allowed, &meta_path(asked)).is_err(), "{asked:?}'s .meta");
        }
        let _ = std::fs::remove_dir_all(&universe);
    }

    /// The Space's own file reads, and so do a Workshop mesh in
    /// `<Universe>/assets` and an Assets panel mesh in `.eustress/assets`, by
    /// the absolute paths the loaders build.
    #[test]
    fn the_space_and_both_shared_folders_read() {
        let (universe, space) = universe("in");
        let allowed = AssetRoots::with_universe(space.clone(), &universe).allowed();
        assert_eq!(in_space(&allowed, Path::new("SoundService/flush.ogg")).unwrap(), space.join("SoundService").join("flush.ogg"));
        let workshop = universe.join("assets").join("meshes").join("Drone").join("body.glb");
        let panel = universe.join(".eustress").join("assets").join("meshes").join("crate.glb");
        assert_eq!(in_space(&allowed, &workshop).unwrap(), workshop, "a Workshop mesh");
        assert_eq!(in_space(&allowed, &panel).unwrap(), panel, "an Assets panel mesh");
        let loose = AssetRoots { space: space.clone(), universe: None }.allowed();
        assert!(in_space(&loose, &workshop).is_err(), "no Universe, no shared folders");
        let _ = std::fs::remove_dir_all(&universe);
    }

    /// A shared folder made while the Space is open serves at once; one that
    /// does not exist is not a root.
    #[test]
    fn a_shared_folder_made_later_serves_at_once() {
        let (universe, space) = universe("later");
        std::fs::remove_dir_all(universe.join("assets")).unwrap();
        let roots = AssetRoots::with_universe(space.clone(), &universe);
        assert_eq!(roots.allowed().len(), 2, "the Space and .eustress/assets");
        std::fs::create_dir_all(universe.join("assets").join("meshes")).unwrap();
        std::fs::write(universe.join("assets").join("meshes").join("new.glb"), b"glb").unwrap();
        assert_eq!(roots.allowed().len(), 3);
        let new = universe.join("assets").join("meshes").join("new.glb");
        assert_eq!(in_space(&roots.allowed(), &new).unwrap(), new);
        let _ = std::fs::remove_dir_all(&universe);
    }

    /// A shared folder that is a junction out of the Universe is never a
    /// root.
    #[cfg(windows)]
    #[test]
    fn a_shared_folder_linked_out_of_the_universe_is_not_a_root() {
        let (universe, space) = universe("junction");
        std::fs::remove_dir_all(universe.join("assets")).unwrap();
        let outside = universe.with_extension("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.glb"), b"x").unwrap();
        let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(universe.join("assets")).arg(&outside).output();
        match made {
            Ok(out) if out.status.success() => {
                let roots = AssetRoots::with_universe(space.clone(), &universe);
                assert!(!roots.allowed().contains(&universe.join("assets")), "the linked folder is a root");
                assert!(in_space(&roots.allowed(), &universe.join("assets").join("secret.glb")).is_err());
            }
            Ok(out) => panic!("mklink /J refused: {}", String::from_utf8_lossy(&out.stderr)),
            Err(e) => panic!("could not run mklink: {e}"),
        }
        let _ = std::fs::remove_dir(universe.join("assets"));
        let _ = std::fs::remove_dir_all(&universe);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
