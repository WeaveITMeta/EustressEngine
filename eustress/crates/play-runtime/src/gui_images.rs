//! # Images the GUI draws
//!
//! An ImageLabel's or ImageButton's `Image`, decoded once and kept for the
//! billboard and HUD rasterizer. A path is absolute, `space://` inside the open
//! Space, or relative to the Space's folder ([`crate::apply::PlayAssetRoot`]).
//! A content id this machine holds no file for (`rbxassetid://...`) draws the
//! rasterizer's placeholder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use tiny_skia::{IntSize, Pixmap};

/// Decoded GUI images by file. A file that failed to load is remembered as
/// `None`, so it is not read again on every paint.
#[derive(Resource, Default)]
pub struct GuiImages {
    decoded: HashMap<PathBuf, Option<Pixmap>>,
}

impl GuiImages {
    /// The image an element names, decoded the first time it is asked for.
    pub fn get(&mut self, image: &str, space_root: &Path) -> Option<&Pixmap> {
        let path = resolve(image, space_root)?;
        self.decoded.entry(path).or_insert_with_key(|p| decode(p)).as_ref()
    }
}

/// The file an `Image` names, if it names one.
fn resolve(image: &str, space_root: &Path) -> Option<PathBuf> {
    let image = image.trim();
    if image.is_empty() {
        return None;
    }
    if let Some(inside) = image.strip_prefix("space://") {
        return Some(space_root.join(inside.trim_start_matches('/')));
    }
    if image.contains("://") {
        return None; // a content id, not a file
    }
    let path = Path::new(image);
    Some(if path.is_absolute() { path.to_path_buf() } else { space_root.join(path) })
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

    #[test]
    fn an_image_resolves_inside_the_space_unless_it_is_a_content_id() {
        let root = Path::new("/spaces/Box Head");
        assert_eq!(resolve("Assets/ammo.png", root), Some(root.join("Assets/ammo.png")));
        assert_eq!(resolve("space://Assets/ammo.png", root), Some(root.join("Assets/ammo.png")));
        assert_eq!(resolve("rbxassetid://123", root), None);
        assert_eq!(resolve("  ", root), None);
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
