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
//! [`DynamicSpaceReader`] that resolves `space_asset_root().join(path)` **at
//! call time**, on every read. Switching Space updates the global (see
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

/// The live root that `space://` paths resolve against. Updated on every
/// Space/Universe switch via [`set_space_asset_root`]. Read on every asset
/// read via [`space_asset_root`].
///
/// Initialised lazily to [`crate::space::default_space_root`] on first access
/// so the value is correct even if an asset read somehow races ahead of the
/// explicit startup stamp in `main`.
static SPACE_ASSET_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Returns the current `space://` resolution root.
///
/// If the global hasn't been stamped yet, falls back to
/// [`crate::space::default_space_root`] (and does not cache it, so a later
/// explicit `set_space_asset_root` still wins).
pub fn space_asset_root() -> PathBuf {
    // Fast path: already set.
    if let Ok(guard) = SPACE_ASSET_ROOT.read() {
        if let Some(root) = guard.as_ref() {
            return root.clone();
        }
    }
    // Not set yet — derive the same default the rest of the engine boots with.
    crate::space::default_space_root()
}

/// Point the `space://` asset source at `root`. Call this anywhere the live
/// [`SpaceRoot`](crate::space::SpaceRoot) changes so subsequent mesh/texture
/// loads resolve against the new Space. Cheap and idempotent.
pub fn set_space_asset_root(root: PathBuf) {
    match SPACE_ASSET_ROOT.write() {
        Ok(mut guard) => {
            if guard.as_deref() != Some(root.as_path()) {
                info!("📁 space:// asset root → {:?}", root);
                *guard = Some(root);
            }
        }
        Err(poisoned) => {
            // A panic in another thread poisoned the lock. The root is just a
            // PathBuf; recover and overwrite rather than propagate the panic.
            let mut guard = poisoned.into_inner();
            *guard = Some(root);
        }
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

impl AssetReader for DynamicSpaceReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        // Resolve against the LIVE root on every call — this is the whole point.
        let full_path = space_asset_root().join(path);
        read_owned(full_path).await
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let full_path = space_asset_root().join(meta_path(path));
        read_owned(full_path).await
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        let root = space_asset_root();
        let full_path = root.join(path);
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
        let full_path = space_asset_root().join(path);
        match async_fs::metadata(&full_path).await {
            Ok(md) => Ok(md.file_type().is_dir()),
            Err(_) => Err(AssetReaderError::NotFound(full_path)),
        }
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
