//! # A path a Space names stays inside the Space
//!
//! A Space's own strings name its files: a Sound's `SoundId`, a part's
//! `MeshId`, an ImageLabel's `Image`, a material's texture maps. A Space can
//! come from anyone (a published world, a joined host, an import), so such a
//! string is never trusted to stay put. `../` would read outside the Space,
//! `C:/...` anywhere on the disk, and `\\host\share\...` would open a network
//! share, which hands the host this machine's Windows credentials.
//!
//! Every path builder in the Play runtime goes through here:
//! - [`contained_url`] for anything handed to Bevy's asset server;
//! - [`contained_file`] for anything read with `std::fs`;
//! - [`is_within`] after resolving, so a junction inside the Space that
//!   points elsewhere is refused too.
//!
//! And the desktop Player reads `space://` through [`ContainedFileReader`], so
//! a path that reaches the asset server some other way is checked once more
//! where the file is opened. The rules are common's `space_path`, which
//! Studio's `space://` reader applies too. The browser Player has no file
//! reader: its `space://` source holds only the Space's own records, so there
//! is no outside to reach, and the reader is not built for wasm32.

use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
use bevy::asset::io::file::FileAssetReader;
#[cfg(not(target_arch = "wasm32"))]
use bevy::asset::io::{AssetReader, AssetReaderError, PathStream, Reader};

pub use eustress_common::space_path::ALLOWED_SOURCES;

/// The asset URL a Space string may load, or `None` when it names somewhere
/// outside: common's [`eustress_common::space_path::contained_url`]. A bare
/// path is the Space's own (`space://`), a leading `/` included;
/// `space://`, `bundled://` and `embedded://` keep their source; any other
/// scheme is refused. A `#label` rides along.
pub fn contained_url(raw: &str, space_root: &Path) -> Option<String> {
    eustress_common::space_path::contained_url(raw, space_root)
}

/// The file in `space_root` a Space string names (bare, `space://`, or the
/// author's absolute path inside the Space), or `None` when it names
/// somewhere outside, or another kind of source.
pub fn contained_file(raw: &str, space_root: &Path) -> Option<PathBuf> {
    let url = contained_url(raw, space_root)?;
    let inside = url.strip_prefix("space://")?;
    let path = inside.split('#').next().unwrap_or(inside);
    Some(space_root.join(path))
}

/// Whether `path`, a path under `root`, is still inside `root` once every
/// link and junction is followed (Studio's `space://` reader asks the same,
/// through the same `space_path::resolve_in_space`). `false` for a path not
/// under `root` at all, or a root that does not exist.
pub fn is_within(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .and_then(|rel| eustress_common::space_path::resolve_in_space(root, rel))
        .is_some()
}

/// A Player asset source: Bevy's file reader over one folder, opening only
/// what lies inside it. The Player reads `space://`, `bundled://` and its
/// default source through it, so a raw string that reaches the asset server
/// unchecked still stays put. A relative path is checked by
/// [`eustress_common::space_path::resolve_in_space`] (no `..`, drive, share,
/// device name or `:`; a link or junction out is caught by its canonical
/// form). An absolute path is refused unless it lies in one of the reader's
/// absolute roots ([`eustress_common::space_path::resolve_in_roots`]: a
/// local drive path only, judged by its text before anything is opened). A
/// refused path reads as absent and names only the path asked for, so
/// nothing tells a caller whether a file outside exists.
#[cfg(not(target_arch = "wasm32"))]
pub struct ContainedFileReader {
    root: PathBuf,
    absolute_roots: Vec<PathBuf>,
    inner: FileAssetReader,
}

#[cfg(not(target_arch = "wasm32"))]
impl ContainedFileReader {
    /// A reader over `root`, which need not exist yet (a joined world is
    /// written there once it arrives; until then every read is absent). A
    /// relative `root` is the exe's folder's, as Bevy's own reader takes it.
    pub fn new(root: PathBuf) -> Self {
        Self::with_absolute_roots(root, Vec::new())
    }

    /// A reader over `root` that also opens an absolute path inside one of
    /// `absolute_roots` (the Player's enhancement cache, which it names by
    /// absolute path).
    pub fn with_absolute_roots(root: PathBuf, absolute_roots: Vec<PathBuf>) -> Self {
        let root = FileAssetReader::get_base_path().join(root);
        Self { inner: FileAssetReader::new(&root), root, absolute_roots }
    }

    /// Whether this reader would open `path` (the check every read makes).
    pub fn opens(&self, path: &Path) -> bool {
        self.check(path).is_ok()
    }

    fn check(&self, path: &Path) -> Result<(), AssetReaderError> {
        let inside = if path.is_absolute() || !eustress_common::space_path::is_local(path) {
            let roots: Vec<&Path> = self.absolute_roots.iter().map(PathBuf::as_path).collect();
            !roots.is_empty() && path.is_absolute() && eustress_common::space_path::resolve_in_roots(&roots, path).is_some()
        } else {
            eustress_common::space_path::resolve_in_space(&self.root, path).is_some()
        };
        if inside {
            Ok(())
        } else {
            Err(AssetReaderError::NotFound(path.to_path_buf()))
        }
    }
}

/// Bevy's sidecar for `path`: `Foo.glb` is `Foo.glb.meta`, `Foo` is
/// `Foo.meta` (`bevy_asset::io::get_meta_path`, which is not public).
#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
impl AssetReader for ContainedFileReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.check(path)?;
        self.inner.read(path).await
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        // The sidecar is the file opened, so it is checked too: a `.meta`
        // that is a link out reads as absent, as the asset would.
        self.check(path)?;
        self.check(&meta_path(path))?;
        self.inner.read_meta(path).await
    }

    async fn read_directory<'a>(&'a self, path: &'a Path) -> Result<Box<PathStream>, AssetReaderError> {
        self.check(path)?;
        self.inner.read_directory(path).await
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        self.check(path)?;
        self.inner.is_directory(path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/spaces/Box Head";

    #[test]
    fn a_space_path_becomes_a_space_url() {
        let root = Path::new(ROOT);
        assert_eq!(contained_url("Assets/shot.ogg", root).as_deref(), Some("space://Assets/shot.ogg"));
        assert_eq!(contained_url("/SoundService/a.wav", root).as_deref(), Some("space://SoundService/a.wav"));
        assert_eq!(contained_url(r"SoundService\a.wav", root).as_deref(), Some("space://SoundService/a.wav"));
        assert_eq!(contained_url("space://Meshes/car.glb#Mesh0/Primitive0", root).as_deref(), Some("space://Meshes/car.glb#Mesh0/Primitive0"));
        assert_eq!(contained_url("bundled://sounds/click.ogg", root).as_deref(), Some("bundled://sounds/click.ogg"));
        assert_eq!(contained_url("/spaces/Box Head/Meshes/car.glb", root).as_deref(), Some("space://Meshes/car.glb"), "the author's own absolute path");
    }

    /// Every way out of the Space is refused, the network share above all.
    #[test]
    fn a_path_out_of_the_space_is_refused() {
        let root = Path::new(ROOT);
        for bad in [
            "../../../Windows/win.ini",
            "Assets/../../secret.txt",
            "space://../x.ogg",
            "space://Assets/../../x.ogg",
            "C:/Windows/win.ini",
            r"C:\Windows\win.ini",
            "bundled://../../x",
            "file:///C:/x.png",
            "https://example.com/x.ogg",
            "rbxassetid://123",
            "Assets/shot.ogg:hidden",
            "/spaces/Box Head2/x.ogg:z",
            "space://Assets/NUL",
            "",
        ] {
            assert_eq!(contained_url(bad, root), None, "{bad:?}");
        }
        // A share, written any way, never leaves the Space's own folder.
        for share in [r"\\host\share\x.ogg", "//host/share/x.ogg", "space:////host/share/x.ogg"] {
            assert_eq!(contained_url(share, root).as_deref(), Some("space://host/share/x.ogg"), "{share:?}");
        }
    }

    #[test]
    fn a_file_resolves_inside_the_space() {
        let root = Path::new(ROOT);
        assert_eq!(contained_file("Assets/ammo.png", root), Some(root.join("Assets/ammo.png")));
        assert_eq!(contained_file("space://Assets/ammo.png#x", root), Some(root.join("Assets/ammo.png")));
        assert_eq!(contained_file("bundled://ui/x.png", root), None, "a file read here is the Space's");
        assert_eq!(contained_file(r"\\host\share\x.png", root), Some(root.join("host/share/x.png")));
        assert_eq!(contained_file("C:/x.png", root), None);
    }

    /// A file reached through a link that points outside the root is outside.
    #[test]
    fn is_within_follows_the_file_system() {
        let base = std::env::temp_dir().join(format!("eustress_space_paths_{}", std::process::id()));
        let root = base.join("space");
        let outside = base.join("outside");
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("Assets/in.png"), b"x").unwrap();
        std::fs::write(outside.join("out.png"), b"x").unwrap();
        assert!(is_within(&root.join("Assets/in.png"), &root));
        assert!(!is_within(&outside.join("out.png"), &root));
        assert!(!is_within(&root.join("Assets/in.png"), &base.join("missing_root")), "no root, nothing inside it");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The Player's reader opens what is in its Space and nothing else, an
    /// absolute path to a file that exists included.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_players_reader_opens_only_its_space() {
        let base = std::env::temp_dir().join(format!("eustress_contained_reader_{}", std::process::id()));
        let root = base.join("space");
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        std::fs::write(root.join("Assets/in.ogg"), b"in").unwrap();
        std::fs::write(base.join("out.ogg"), b"out").unwrap();
        let reader = ContainedFileReader::new(root.clone());
        assert!(reader.check(Path::new("Assets/in.ogg")).is_ok());
        for bad in ["../out.ogg", "Assets/../../out.ogg", "Assets/NUL", "Assets/in.ogg:stream"] {
            assert!(reader.check(Path::new(bad)).is_err(), "{bad:?}");
        }
        assert!(reader.check(&base.join("out.ogg")).is_err(), "an absolute path, though the file exists");
        assert!(reader.check(&root.join("Assets/in.ogg")).is_err(), "absolute even inside: the source is relative");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The default source opens its own folder and, by absolute path, only
    /// the roots it names (the enhancement cache); a share never.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn an_absolute_path_opens_only_inside_a_named_root() {
        let base = std::env::temp_dir().join(format!("eustress_absolute_roots_{}", std::process::id()));
        let assets = base.join("assets");
        let cache = base.join("cache");
        std::fs::create_dir_all(assets.join("parts")).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(assets.join("parts/block.glb"), b"x").unwrap();
        std::fs::write(cache.join("tree.glb"), b"x").unwrap();
        std::fs::write(base.join("secret.glb"), b"x").unwrap();
        let reader = ContainedFileReader::with_absolute_roots(assets.clone(), vec![cache.clone()]);
        assert!(reader.check(Path::new("parts/block.glb")).is_ok());
        assert!(reader.check(&cache.join("tree.glb")).is_ok(), "the named root, by absolute path");
        assert!(reader.check(&base.join("secret.glb")).is_err(), "absolute, outside every root");
        assert!(reader.check(&cache.join("../secret.glb")).is_err(), "a climb out of the named root");
        assert!(reader.check(Path::new(r"\\host\share\x.glb")).is_err());
        assert!(reader.check(Path::new("//host/share/x.glb")).is_err());
        assert!(reader.check(Path::new("../secret.glb")).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_sidecar_is_named_as_bevy_names_it() {
        assert_eq!(meta_path(Path::new("Meshes/car.glb")), PathBuf::from("Meshes/car.glb.meta"));
        assert_eq!(meta_path(Path::new("Meshes/car")), PathBuf::from("Meshes/car.meta"));
    }
}
