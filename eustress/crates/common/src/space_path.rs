//! Paths a Space names, kept inside the Space.
//!
//! A Space's files carry paths that are read relative to the Space folder: a
//! Sound's `SoundId`, a mesh, an image, a texture. A Space can come from
//! anyone (a shared folder, the Gallery), so a path that leaves the folder is
//! refused before anything opens it: an absolute path, a drive (`C:/x`,
//! `C:x`), a UNC share (`//host/share/x`, which Windows opens over SMB and so
//! sends the user's credentials to that host), a verbatim path (`\\?\C:\x`),
//! a `..`, or a `:` inside a name (an NTFS alternate stream). A path that
//! passes is then checked where it lands: its canonical form, or its nearest
//! existing ancestor's when it does not exist yet, must lie under the Space
//! folder's, so a junction or a link inside the Space cannot lead out either.
//!
//! A name Windows reserves for a device (`CON`, `NUL`, `COM1`, ...) is refused
//! too, with or without an extension: opening one reads a console or a port,
//! and can block.
//!
//! The loaders name a file outside the Space by its absolute path, and two
//! folders outside it are a Space's to read, its Universe's shared assets
//! ([`shared_asset_roots`]): `<Universe>/assets`, where new shared meshes go
//! ([`shared_mesh_dir`]; Workshop writes its products' meshes there, and a
//! publish ships the folder), and `<Universe>/.eustress/assets`, which holds
//! the Assets panel's imports and the default parts. Never the Universe
//! itself: it holds the other Spaces, their databases and the engine's own
//! files. [`resolve_in_roots`] takes such a path when it is a plain local
//! drive path inside one of the folders it is given; a network or device
//! path is refused by its text, before anything is opened, since resolving a
//! UNC path is itself an SMB access.
//!
//! By design a Space's `mesh` or media string may name any file in those two
//! folders, so a Space someone hands you can learn whether a named file
//! exists there and roughly how big it is (a mesh shows, or a box does). It
//! cannot read the file back or send it anywhere through these paths. If the
//! shared folders ever hold private work, the answer is to treat a downloaded
//! Space as untrusted and give it only its own folder and the bundled assets.
//!
//! [`contained_url`] is the same rule for a string a Space hands the asset
//! server (a `SoundId`, a `MeshId`, an image): one function for Studio, the
//! Player and the Play runtime.

use std::path::{Component, Path, PathBuf};

/// `rel` as a path relative to a Space folder, or `None` when it could name
/// anything outside it. Only names and `.` pass; the empty path is the folder
/// itself. Lexical: nothing is read.
pub fn relative_in_space(rel: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in rel.components() {
        match component {
            Component::Normal(name) => {
                let name = name.to_str()?;
                // `\` is a separator on Windows and a name character
                // elsewhere: refused, so a path means the same everywhere.
                if name.contains(':') || name.contains('\\') || is_device_name(name) {
                    return None;
                }
                out.push(name);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// `rel` joined under `root`, or `None` when it would leave it: the rules of
/// [`relative_in_space`], then the canonical check. `None` too when `root`
/// does not exist.
pub fn resolve_in_space(root: &Path, rel: &Path) -> Option<PathBuf> {
    let rel = relative_in_space(rel)?;
    let canonical_root = std::fs::canonicalize(root).ok()?;
    let full = root.join(rel);
    lies_under(&full, &[canonical_root]).then_some(full)
}

/// False for a path naming the network or a device: on Windows a UNC,
/// verbatim or device-namespace prefix, or an NT `\??\` path (which Rust reads
/// as a root and a name). Judged by its text alone. A relative path and a
/// drive path are local; elsewhere every path is.
pub fn is_local(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::Prefix;
        let mut parts = path.components();
        match parts.next() {
            Some(Component::Prefix(p)) => matches!(p.kind(), Prefix::Disk(_)),
            // `\??\UNC\host\share` reaches the network through the NT
            // namespace although Rust sees no prefix in it.
            Some(Component::RootDir) => !matches!(parts.next(), Some(Component::Normal(n)) if n == "??"),
            _ => true,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        true
    }
}

/// `path` inside one of `roots`, or `None`. A relative path is
/// [`resolve_in_space`]'s, under the FIRST root (the Space). An absolute path
/// (how the loaders name a file outside the Space) must be [`is_local`],
/// checked before anything is opened, hold no `..` and no `:` past its drive,
/// and land, by its canonical form or its nearest existing ancestor's, under
/// one root's canonical form. A path with a drive or a root but not both
/// (`C:x`, `\x`) is refused.
pub fn resolve_in_roots(roots: &[&Path], path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return resolve_in_space(roots.first()?, path);
    }
    if !is_local(path) {
        return None;
    }
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => return None,
            Component::Normal(name) => {
                let name = name.to_str()?;
                if name.contains(':') || is_device_name(name) {
                    return None;
                }
            }
        }
    }
    let canonical_roots: Vec<PathBuf> =
        roots.iter().filter(|root| is_local(root)).filter_map(|root| std::fs::canonicalize(root).ok()).collect();
    lies_under(path, &canonical_roots).then(|| path.to_path_buf())
}

/// Whether a file name is one Windows reserves for a device: `CON`, `PRN`,
/// `AUX`, `NUL`, `COM0` to `COM9`, `LPT0` to `LPT9` (and their superscript
/// digits), `CONIN$` or `CONOUT$`, in any case, with any extension, and with
/// the trailing dots and spaces Windows ignores.
fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches([' ', '.']);
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") {
        return true;
    }
    let mut chars = stem.chars();
    let head: String = chars.by_ref().take(3).collect::<String>().to_ascii_uppercase();
    let tail: Vec<char> = chars.collect();
    (head == "COM" || head == "LPT")
        && tail.len() == 1
        && (tail[0].is_ascii_digit() || matches!(tail[0], '\u{b9}' | '\u{b2}' | '\u{b3}'))
}

/// A Universe's shared asset folders, the only folders outside a Space its
/// paths may reach: `<Universe>/assets` (new shared meshes, media, and what a
/// publish ships) and `<Universe>/.eustress/assets` (the Assets panel's
/// imports, the default parts). Named folders only, never the Universe.
pub fn shared_asset_roots(universe_root: &Path) -> [PathBuf; 2] {
    [universe_root.join("assets"), universe_root.join(".eustress").join("assets")]
}

/// Where a new shared mesh goes: `<Universe>/assets/meshes`. Every writer of
/// one (Workshop, an import, the Assets panel) uses this, so the reader and a
/// publish find it.
pub fn shared_mesh_dir(universe_root: &Path) -> PathBuf {
    universe_root.join("assets").join("meshes")
}

/// The asset sources a Space string may name: its own folder, the engine's
/// bundled library, and assets compiled into the program.
pub const ALLOWED_SOURCES: [&str; 3] = ["space", "bundled", "embedded"];

/// An absolute path the author wrote inside their own Space, as the
/// Space-relative path it is; anything else unchanged.
fn relative_to_root<'a>(path: &'a str, root: &Path) -> &'a str {
    let root = root.to_string_lossy().replace('\\', "/");
    let root = root.trim_end_matches('/');
    if root.is_empty() || path.len() <= root.len() + 1 || path.as_bytes()[root.len()] != b'/' {
        return path;
    }
    let (head, rest) = path.split_at(root.len());
    // Windows paths compare without case.
    let same = if cfg!(windows) { head.eq_ignore_ascii_case(root) } else { head == root };
    if same {
        &rest[1..]
    } else {
        path
    }
}

/// `path` split before its `#label` (a glTF sub-asset), the label kept whole.
fn split_label(path: &str) -> (&str, &str) {
    match path.find('#') {
        Some(i) => path.split_at(i),
        None => (path, ""),
    }
}

/// The asset URL a Space string may load, or `None` when it names somewhere
/// outside. A bare path is the Space's own (`space://`), a leading `/`
/// included, and the author's absolute path inside `space_root` becomes the
/// relative path it is; `space://`, `bundled://` and `embedded://` keep their
/// source; any other scheme is refused. A `#label` rides along. The path
/// itself follows `animation::content::safe_relative`: relative segments
/// only, no `..`, no drive, and no `:` inside a segment.
pub fn contained_url(raw: &str, space_root: &Path) -> Option<String> {
    let raw = raw.trim().replace('\\', "/");
    let (source, rest) = match raw.split_once("://") {
        Some((source, rest)) => {
            let source = source.to_ascii_lowercase();
            if !ALLOWED_SOURCES.contains(&source.as_str()) {
                return None;
            }
            (source, rest.to_string())
        }
        None => ("space".to_string(), relative_to_root(&raw, space_root).to_string()),
    };
    let (path, label) = split_label(&rest);
    let path = crate::animation::content::safe_relative(path.trim_start_matches('/'))?;
    if path.split('/').any(is_device_name) {
        return None;
    }
    Some(format!("{source}://{path}{label}"))
}

/// Whether `path`, by its canonical form or its nearest existing ancestor's,
/// lies under one of `canonical_roots`.
fn lies_under(path: &Path, canonical_roots: &[PathBuf]) -> bool {
    let mut probe = path;
    loop {
        match std::fs::canonicalize(probe) {
            Ok(canonical) => return canonical_roots.iter().any(|root| canonical.starts_with(root)),
            Err(_) => match probe.parent() {
                Some(parent) => probe = parent,
                None => return false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("eustress_space_path_{tag}_{}_{nanos}", std::process::id()));
        std::fs::create_dir_all(root.join("SoundService")).unwrap();
        std::fs::write(root.join("SoundService").join("flush.ogg"), b"ogg").unwrap();
        std::fs::write(root.join("top.ogg"), b"ogg").unwrap();
        root
    }

    /// A Universe holding the Space `Garage` and `Other`, its shared assets
    /// with a Workshop mesh, and its engine files.
    fn universe(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let universe = std::env::temp_dir().join(format!("eustress_universe_{tag}_{}_{nanos}", std::process::id()));
        let space = universe.join("Spaces").join("Garage");
        let shared = universe.join(".eustress").join("assets");
        std::fs::create_dir_all(space.join("Workspace")).unwrap();
        std::fs::write(space.join("Workspace").join("own.glb"), b"glb").unwrap();
        std::fs::create_dir_all(universe.join("Spaces").join("Other")).unwrap();
        std::fs::write(universe.join("Spaces").join("Other").join("secret.glb"), b"glb").unwrap();
        std::fs::create_dir_all(shared.join("meshes").join("Drone")).unwrap();
        std::fs::write(shared.join("meshes").join("Drone").join("body.glb"), b"glb").unwrap();
        std::fs::write(universe.join(".eustress").join("engine.port"), b"1234").unwrap();
        (universe, space, shared)
    }

    /// Every way a path can name something outside the folder is refused,
    /// before anything is read.
    #[test]
    fn hostile_paths_are_refused() {
        let root = space("hostile");
        let hostile = [
            "..",
            "../x.ogg",
            "../../x",
            "a/../../x",
            "SoundService/../../x.ogg",
            "//host/share/x.ogg",
            r"\\host\share\x.ogg",
            r"\\?\C:\x",
            r"\\.\pipe\x",
            "C:/x",
            r"C:\Windows\win.ini",
            "C:x",
            "/Windows/x",
            r"\Windows\x",
            r"a\..\..\x",
            r"a/..\../x",
            "top.ogg:stream",
            "SoundService/flush.ogg:$DATA",
            "NUL",
            "con.ogg",
            "SoundService/COM1",
            "SoundService/lpt9.txt",
            "aux. ",
            "CONOUT$",
        ];
        for path in hostile {
            assert_eq!(relative_in_space(Path::new(path)), None, "{path:?} passed the lexical rules");
            assert_eq!(resolve_in_space(&root, Path::new(path)), None, "{path:?} resolved");
            assert_eq!(resolve_in_roots(&[&root], Path::new(path)), None, "{path:?} resolved among roots");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A plain path and a nested one resolve under the folder, a `.` is
    /// dropped, and a file that does not exist yet still resolves (its read
    /// then finds nothing).
    #[test]
    fn paths_in_the_space_resolve() {
        let root = space("inside");
        assert_eq!(resolve_in_space(&root, Path::new("top.ogg")), Some(root.join("top.ogg")));
        assert_eq!(
            resolve_in_space(&root, Path::new("SoundService/flush.ogg")),
            Some(root.join("SoundService").join("flush.ogg"))
        );
        assert_eq!(
            resolve_in_space(&root, Path::new("./SoundService/./flush.ogg")),
            Some(root.join("SoundService").join("flush.ogg"))
        );
        assert_eq!(resolve_in_space(&root, Path::new("Missing/new.ogg")), Some(root.join("Missing").join("new.ogg")));
        assert_eq!(resolve_in_space(&root, Path::new("")), Some(root.clone()), "the folder itself");
        assert_eq!(resolve_in_space(&root.join("absent"), Path::new("top.ogg")), None, "no folder, no path");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The loaders' absolute paths: one in the Space and one in the
    /// Universe's shared assets (a Workshop mesh) resolve; a sibling Space,
    /// the engine's files and a `..` back out are refused.
    #[test]
    fn absolute_paths_resolve_only_in_the_space_and_the_shared_assets() {
        let (universe, space, shared) = universe("roots");
        let roots: [&Path; 2] = [&space, &shared];
        let own = space.join("Workspace").join("own.glb");
        let workshop = shared.join("meshes").join("Drone").join("body.glb");
        assert_eq!(resolve_in_roots(&roots, &own), Some(own.clone()), "a drive path in the Space");
        assert_eq!(resolve_in_roots(&roots, &workshop), Some(workshop.clone()), "a Workshop mesh");
        assert_eq!(resolve_in_roots(&roots, &shared.join("meshes").join("New.glb")), Some(shared.join("meshes").join("New.glb")));
        assert_eq!(resolve_in_roots(&roots, Path::new("Workspace/own.glb")), Some(space.join("Workspace").join("own.glb")), "relative is the Space's");
        for refused in [
            universe.join("Spaces").join("Other").join("secret.glb"),
            universe.join(".eustress").join("engine.port"),
            universe.clone(),
            space.join("..").join("Other").join("secret.glb"),
            shared.join("..").join("engine.port"),
        ] {
            assert_eq!(resolve_in_roots(&roots, &refused), None, "{refused:?}");
        }
        assert_eq!(resolve_in_roots(&[&space], &workshop), None, "no shared root given, none reached");
        let _ = std::fs::remove_dir_all(&universe);
    }

    /// A network or device path is refused by its text, before anything
    /// touches the network: a host that cannot exist answers at once.
    #[cfg(windows)]
    #[test]
    fn network_paths_are_refused_without_a_lookup() {
        let (universe, space, shared) = universe("network");
        let roots: [&Path; 2] = [&space, &shared];
        for path in [
            r"\\no-such-host.invalid\share\x.glb",
            "//no-such-host.invalid/share/x.glb",
            r"\\?\UNC\no-such-host.invalid\share\x.glb",
            r"\\.\UNC\no-such-host.invalid\share\x.glb",
            r"\??\UNC\no-such-host.invalid\share\x.glb",
            r"\\?\C:\Windows\win.ini",
        ] {
            let started = std::time::Instant::now();
            assert!(!is_local(Path::new(path)) || !Path::new(path).is_absolute(), "{path:?} read as local");
            assert_eq!(resolve_in_roots(&roots, Path::new(path)), None, "{path:?}");
            assert!(started.elapsed() < std::time::Duration::from_millis(50), "{path:?} took {:?}: it was looked up", started.elapsed());
        }
        assert!(is_local(Path::new(r"C:\Windows")) && is_local(Path::new("Workspace/own.glb")));
        let _ = std::fs::remove_dir_all(&universe);
    }

    /// Only the reserved device names are refused: a name that merely starts
    /// like one is an ordinary file.
    #[test]
    fn device_names_are_refused_and_lookalikes_are_not() {
        for device in ["CON", "nul", "Aux.txt", "COM1", "com9.ogg", "LPT0", "lpt\u{b9}", "PRN ", "nul.", "CONIN$"] {
            assert!(is_device_name(device), "{device:?}");
        }
        for ordinary in ["console.ogg", "CONE.glb", "COM10", "LPTX", "nullable", "auxiliary", "com", "prn2", "Sounds"] {
            assert!(!is_device_name(ordinary), "{ordinary:?}");
        }
        let root = space("devices");
        assert_eq!(resolve_in_roots(&[&root], &root.join("NUL")), None, "an absolute device name");
        assert_eq!(resolve_in_roots(&[&root], &root.join("SoundService").join("com1.ogg")), None);
        assert_eq!(resolve_in_roots(&[&root], &root.join("top.ogg")), Some(root.join("top.ogg")));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The two shared folders are named folders under the Universe, and new
    /// shared meshes go to the visible one.
    #[test]
    fn the_shared_asset_folders_are_named() {
        let universe = Path::new("/u/Games");
        assert_eq!(shared_asset_roots(universe), [universe.join("assets"), universe.join(".eustress").join("assets")]);
        assert!(shared_asset_roots(universe).iter().all(|root| root != universe), "never the Universe itself");
        assert_eq!(shared_mesh_dir(universe), universe.join("assets").join("meshes"));
        assert!(shared_mesh_dir(universe).starts_with(&shared_asset_roots(universe)[0]));
    }

    const ROOT: &str = "/spaces/Box Head";

    #[test]
    fn a_space_string_becomes_a_space_url() {
        let root = Path::new(ROOT);
        assert_eq!(contained_url("Assets/shot.ogg", root).as_deref(), Some("space://Assets/shot.ogg"));
        assert_eq!(contained_url("/SoundService/a.wav", root).as_deref(), Some("space://SoundService/a.wav"));
        assert_eq!(contained_url(r"SoundService\a.wav", root).as_deref(), Some("space://SoundService/a.wav"));
        assert_eq!(contained_url("space://Meshes/car.glb#Mesh0/Primitive0", root).as_deref(), Some("space://Meshes/car.glb#Mesh0/Primitive0"));
        assert_eq!(contained_url("bundled://sounds/click.ogg", root).as_deref(), Some("bundled://sounds/click.ogg"));
        assert_eq!(contained_url("SPACE://Meshes/car.glb", root).as_deref(), Some("space://Meshes/car.glb"), "the source in any case");
        assert_eq!(contained_url("/spaces/Box Head/Meshes/car.glb", root).as_deref(), Some("space://Meshes/car.glb"), "the author's own absolute path");
    }

    /// Every way out of the Space is refused, the network share above all.
    #[test]
    fn a_space_string_out_of_the_space_is_refused() {
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
            "Sounds/con.ogg",
            "",
        ] {
            assert_eq!(contained_url(bad, root), None, "{bad:?}");
        }
        // A share, written any way, never leaves the Space's own folder.
        for share in [r"\\host\share\x.ogg", "//host/share/x.ogg", "space:////host/share/x.ogg"] {
            assert_eq!(contained_url(share, root).as_deref(), Some("space://host/share/x.ogg"), "{share:?}");
        }
    }

    /// A junction inside the Space that points outside it leads nowhere.
    #[cfg(windows)]
    #[test]
    fn a_junction_out_of_the_space_is_refused() {
        let root = space("junction");
        let outside = root.with_extension("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.ogg"), b"x").unwrap();
        let link = root.join("Link");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output();
        match made {
            Ok(out) if out.status.success() => {
                assert_eq!(resolve_in_space(&root, Path::new("Link/secret.ogg")), None, "read through the junction");
                assert_eq!(resolve_in_space(&root, Path::new("Link/missing.ogg")), None, "a new file through it");
                assert_eq!(resolve_in_space(&root, Path::new("Link")), None, "the junction itself");
                assert_eq!(resolve_in_roots(&[&root], &link.join("secret.ogg")), None, "its absolute path");
            }
            Ok(out) => panic!("mklink /J refused: {}", String::from_utf8_lossy(&out.stderr)),
            Err(e) => panic!("could not run mklink: {e}"),
        }
        let _ = std::fs::remove_dir(&link);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
