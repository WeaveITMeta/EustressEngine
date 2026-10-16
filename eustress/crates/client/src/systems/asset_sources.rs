//! # The Player's asset sources
//!
//! The three file sources the Player reads (its default source, the engine's
//! assets; `bundled://`; and `space://`, the open Space) are each a
//! `ContainedFileReader` (play-runtime `space_paths`) that opens only what lies
//! inside its folder. The Player runs Bevy with `UnapprovedPathMode::Allow`, so
//! these readers, not that flag, are what keep a path a Space or a host
//! carries (`//host/share/x`, `C:/x`, `../x`) off the disk and the network.
//! `main` registers exactly these, and the test below proves each one.

use std::path::PathBuf;

use eustress_play_runtime::space_paths::ContainedFileReader;

/// The default source: the engine's assets, and by absolute path the
/// enhancement cache, the one absolute load the Player makes
/// (`asset_applicator`).
pub fn default_reader() -> ContainedFileReader {
    ContainedFileReader::with_absolute_roots(
        eustress_common::assets_dir(),
        vec![super::enhancement_scheduler::EnhancementCache::default().path],
    )
}

/// `bundled://`: the avatar's bodies and clips, and the bundled library.
pub fn bundled_reader() -> ContainedFileReader {
    ContainedFileReader::new(eustress_common::avatar::boot::bundled_root())
}

/// `space://`: the Space the Player opens (its world folder, for a joined or
/// published world).
pub fn space_reader(space_root: PathBuf) -> ContainedFileReader {
    ContainedFileReader::new(space_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Every source the Player reads refuses a network share, a drive path, a
    /// climb out and a device name, whatever `UnapprovedPathMode` says. On
    /// Windows a share is refused by its text (`space_path::is_local`); on
    /// other targets the same strings are plain relative names with a `\` or a
    /// leading `/`, and the relative rules refuse them, so the test holds on
    /// every target.
    #[test]
    fn every_player_asset_source_refuses_a_path_out() {
        let space = std::env::temp_dir().join(format!("eustress_player_sources_{}", std::process::id()));
        std::fs::create_dir_all(space.join("Assets")).unwrap();
        std::fs::write(space.join("Assets/in.ogg"), b"x").unwrap();
        let readers = [("default", default_reader()), ("bundled", bundled_reader()), ("space", space_reader(space.clone()))];
        for (name, reader) in &readers {
            for bad in [
                "//host/share/x.ogg",
                r"\\host\share\x.ogg",
                r"\\?\UNC\host\share\x.ogg",
                "C:/Windows/win.ini",
                "../x.ogg",
                "Assets/../../x.ogg",
                "NUL",
                "Assets/COM1.txt",
                "Assets/in.ogg:stream",
            ] {
                assert!(!reader.opens(Path::new(bad)), "the {name} source opened {bad:?}");
            }
        }
        assert!(readers[2].1.opens(Path::new("Assets/in.ogg")), "the Space's own file opens");
        let _ = std::fs::remove_dir_all(&space);
    }
}
