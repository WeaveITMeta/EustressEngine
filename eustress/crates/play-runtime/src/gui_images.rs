//! # Images the GUI draws
//!
//! An ImageLabel's or ImageButton's `Image`, decoded once and kept for the
//! billboard and HUD rasterizer. A path is `space://` inside the open Space, or
//! relative to the Space's folder ([`crate::apply::PlayAssetRoot`]); one that
//! leaves the Space is never read ([`crate::space_paths`]).
//! A content id this machine holds no file for (`rbxassetid://...`) draws the
//! rasterizer's placeholder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use tiny_skia::{IntSize, Pixmap};

/// Decoded GUI images by file. A file that failed to load is remembered as
/// `None`, so it is not read again on every paint; [`GuiImages::refresh`]
/// lets go of any image whose file has changed since, so a replaced or fixed
/// file shows without a restart.
#[derive(Resource, Default)]
pub struct GuiImages {
    decoded: HashMap<PathBuf, Cached>,
    generation: u64,
}

/// One file's image, and the file as it was when it was read.
struct Cached {
    stamp: Option<FileStamp>,
    image: Option<Pixmap>,
}

/// A file's size and modification time: when either moves, it was rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
}

fn stamp(path: &Path) -> Option<FileStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(FileStamp { len: meta.len(), modified: meta.modified().ok() })
}

impl GuiImages {
    /// The image an element names, decoded the first time it is asked for.
    pub fn get(&mut self, image: &str, space_root: &Path) -> Option<&Pixmap> {
        let path = resolve(image, space_root)?;
        self.decoded
            .entry(path)
            .or_insert_with_key(|p| {
                // Through a link or junction, the file may still be elsewhere.
                if !crate::space_paths::is_within(p, space_root) {
                    return Cached { stamp: stamp(p), image: None };
                }
                Cached { stamp: stamp(p), image: decode(p) }
            })
            .image
            .as_ref()
    }

    /// Let go of every image whose file changed, appeared or went since it was
    /// read, so it is read again when next drawn. Returns whether any did.
    pub fn refresh(&mut self) -> bool {
        let before = self.decoded.len();
        self.decoded.retain(|path, cached| stamp(path) == cached.stamp);
        let changed = self.decoded.len() != before;
        if changed {
            self.generation = self.generation.wrapping_add(1);
        }
        changed
    }

    /// Counts up whenever [`GuiImages::refresh`] let go of an image, so a
    /// painted panel can tell its pixels may be stale.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// How often the GUI image files are looked at again, seconds.
const REFRESH_EVERY_S: f32 = 1.0;

/// Look at the GUI image files about once a second (a `stat` per cached file).
pub fn refresh_gui_images(time: Res<Time>, mut since: Local<f32>, mut images: ResMut<GuiImages>) {
    *since += time.delta_secs();
    if *since < REFRESH_EVERY_S {
        return;
    }
    *since = 0.0;
    images.refresh();
}

/// The file in the Space an `Image` names, if it names one there. A content
/// id is not a file, and a path out of the Space is refused.
fn resolve(image: &str, space_root: &Path) -> Option<PathBuf> {
    crate::space_paths::contained_file(image, space_root)
}

/// A file decoded to a premultiplied pixmap, the form tiny-skia draws.
fn decode(path: &Path) -> Option<Pixmap> {
    let bytes = std::fs::read(path).ok()?;
    let rgba = match image::load_from_memory(&bytes) {
        Ok(img) => img.to_rgba8(),
        Err(e) => {
            warn!("GUI image {} could not be decoded: {e}", path.display());
            return None;
        }
    };
    let (w, h) = rgba.dimensions();
    let mut data = rgba.into_raw();
    premultiply(&mut data);
    Pixmap::from_vec(data, IntSize::from_wh(w, h)?)
}

fn premultiply(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u16;
        for c in &mut px[..3] {
            *c = ((*c as u16 * a + 127) / 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file rewritten after it was read is read again; one unchanged is kept.
    #[test]
    fn a_rewritten_image_file_is_read_again() {
        let dir = std::env::temp_dir().join(format!("eustress_gui_images_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("logo.png");
        std::fs::write(&file, b"not a png").unwrap();
        let mut images = GuiImages::default();
        assert!(images.get("logo.png", &dir).is_none(), "a broken file draws the placeholder");
        assert!(!images.refresh(), "nothing changed");
        let png = {
            let mut bytes = Vec::new();
            image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
                .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
                .unwrap();
            bytes
        };
        std::fs::write(&file, &png).unwrap();
        assert!(images.refresh(), "the fixed file is noticed");
        assert_eq!(images.generation(), 1);
        assert!(images.get("logo.png", &dir).is_some(), "and drawn");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_image_resolves_inside_the_space_unless_it_is_a_content_id() {
        let root = Path::new("/spaces/Box Head");
        assert_eq!(resolve("Assets/ammo.png", root), Some(root.join("Assets/ammo.png")));
        assert_eq!(resolve("space://Assets/ammo.png", root), Some(root.join("Assets/ammo.png")));
        assert_eq!(resolve("rbxassetid://123", root), None);
        assert_eq!(resolve("  ", root), None);
    }

    /// An Image never reads outside the Space, the disk or a network share.
    #[test]
    fn an_image_outside_the_space_is_refused() {
        let root = Path::new("/spaces/Box Head");
        for bad in ["../../x.png", "C:/Users/x.png", "space://../x.png", "file:///C:/x.png"] {
            assert_eq!(resolve(bad, root), None, "{bad:?}");
        }
        assert_eq!(resolve(r"\\host\share\x.png", root), Some(root.join("host/share/x.png")));
    }

    #[test]
    fn premultiplying_scales_colour_by_alpha() {
        let mut px = [255, 128, 0, 128, 10, 20, 30, 255, 200, 200, 200, 0];
        premultiply(&mut px);
        assert_eq!(&px[..4], &[128, 64, 0, 128]);
        assert_eq!(&px[4..8], &[10, 20, 30, 255], "opaque stays");
        assert_eq!(&px[8..], &[0, 0, 0, 0], "transparent is black");
    }
}
