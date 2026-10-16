//! The Publish dialog's optional cover (`docs/networking/LISTING_COVER.md`):
//! portrait box art or 16:9, shown in the Gallery once it is approved (the
//! capture until then).
//!
//! The author picks an image; it is checked here before anything is sent,
//! with the Worker's rules: PNG, JPEG or WebP (sniffed from the bytes, never
//! SVG), at most 2 MB, the short side at least 320 px, width:height between
//! 1:2 and 2:1. The checked bytes travel in [`CoverChoice::Set`], so what is
//! sent is what was checked; Multiplayer's publish uploads them before it
//! submits the listing for review. A Space-only publish never touches the
//! cover. The dialog offers removing a cover only when the listing the
//! publish goes to has one, or while that is not known yet.

use std::sync::Mutex;

use bevy::prelude::*;
use eustress_common::project_manifest::CoverChoice;

use super::slint_ui::{SlintUiState, StudioWindow};
use crate::multiplayer::{ListingSummary, MyListings, MyListingsState};

/// The largest cover, in bytes.
pub const COVER_MAX_BYTES: usize = 2 * 1024 * 1024;
/// The shortest a cover's short side is, in pixels.
pub const COVER_MIN_SIDE: u32 = 320;

/// The rules, as the dialog states them.
pub const COVER_RULES: &str =
    "Portrait box art or 16:9: PNG, JPEG or WebP, up to 2 MB, at least 320 px on the short side.";

/// A cover's size in pixels, or why it cannot be one.
pub fn check_cover(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.is_empty() {
        return Err("That file is empty.".into());
    }
    if bytes.len() > COVER_MAX_BYTES {
        return Err(format!(
            "That image is {:.1} MB; a cover is at most 2 MB.",
            bytes.len() as f64 / (1024.0 * 1024.0)
        ));
    }
    let format = image::guess_format(bytes).ok();
    if !matches!(format, Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)) {
        return Err("A cover is a PNG, JPEG or WebP image.".into());
    }
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .ok_or_else(|| "That image cannot be read.".to_string())?;
    if w.min(h) < COVER_MIN_SIDE {
        return Err(format!("That image is {w} × {h}; a cover is at least {COVER_MIN_SIDE} px on its short side."));
    }
    // Between 1:2 (tall) and 2:1 (wide), in whole numbers.
    if u64::from(w) > 2 * u64::from(h) || u64::from(h) > 2 * u64::from(w) {
        return Err(format!(
            "That image is {w} × {h}; a cover is no more than twice as wide as it is tall, or as tall as it is wide."
        ));
    }
    Ok((w, h))
}

static CHOICE: Mutex<Option<CoverChoice>> = Mutex::new(None);

fn set_choice(choice: CoverChoice) {
    *CHOICE.lock().unwrap_or_else(|e| e.into_inner()) = Some(choice);
}

/// The cover as the next publish takes it.
pub fn current() -> CoverChoice {
    CHOICE.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

/// A small preview of the picked image.
fn preview(bytes: &[u8]) -> Option<slint::Image> {
    let thumb = image::load_from_memory(bytes).ok()?.thumbnail(144, 144).to_rgba8();
    let (w, h) = thumb.dimensions();
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(thumb.as_raw(), w, h);
    Some(slint::Image::from_rgba8(buffer))
}

fn show(ui: &StudioWindow, state: &str, info: &str, error: &str) {
    ui.set_publish_cover_state(state.into());
    ui.set_publish_cover_info(info.into());
    ui.set_publish_cover_error(error.into());
}

/// The dialog opened: the listing's current cover stays unless the author
/// picks or removes one.
pub fn reset(ui: &StudioWindow) {
    set_choice(CoverChoice::Keep);
    ui.set_publish_cover_image(slint::Image::default());
    show(ui, "keep", "", "");
}

/// Choose...: pick an image, check it, and hold it for the publish.
pub fn choose(ui: &StudioWindow) {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Choose a cover")
        .add_filter("Cover image", &["png", "jpg", "jpeg", "webp"])
        .pick_file()
    else {
        return;
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            ui.set_publish_cover_error(format!("That file cannot be read: {e}").into());
            return;
        }
    };
    match check_cover(&bytes) {
        Ok((w, h)) => {
            let kind = match image::guess_format(&bytes) {
                Ok(image::ImageFormat::Png) => "PNG",
                Ok(image::ImageFormat::Jpeg) => "JPEG",
                _ => "WebP",
            };
            let info = format!("{w} × {h} {kind}, {} KB", bytes.len().div_ceil(1024));
            ui.set_publish_cover_image(preview(&bytes).unwrap_or_default());
            set_choice(CoverChoice::Set(bytes));
            show(ui, "set", &info, "");
        }
        // The current choice stands; the dialog says why this one cannot.
        Err(why) => ui.set_publish_cover_error(why.into()),
    }
}

/// Remove the listing's current cover on publish.
pub fn remove(ui: &StudioWindow) {
    set_choice(CoverChoice::Remove);
    ui.set_publish_cover_image(slint::Image::default());
    show(ui, "remove", "", "");
}

/// Keep the listing's current cover (undo a pick or a removal).
pub fn keep(ui: &StudioWindow) {
    reset(ui);
}

/// Whether the listing a publish goes to has a cover now: "yes", "no", or ""
/// while that is not known (the listings are loading, or did not load). A
/// new listing, and a Universe's first publish, have none.
pub fn cover_now(target_id: &str, state: &MyListingsState, items: &[ListingSummary]) -> &'static str {
    if target_id.is_empty() {
        return "no";
    }
    if !matches!(state, MyListingsState::Ready) {
        return "";
    }
    match items.iter().find(|i| i.id == target_id) {
        Some(listing) if listing.cover_url.is_some() => "yes",
        Some(_) => "no",
        None => "",
    }
}

/// Tell the Publish dialog whether its target has a cover, as the listings
/// arrive and as the author picks a target. A removal stands only while the
/// target may have a cover to remove.
pub fn sync_publish_cover_now(
    slint_context: Option<NonSend<SlintUiState>>,
    listings: Option<Res<MyListings>>,
    mut shown: Local<Option<(String, &'static str)>>,
) {
    let Some(ctx) = slint_context.as_ref() else { return };
    let ui = &ctx.window;
    if !ui.get_show_publish_dialog() {
        *shown = None;
        return;
    }
    let target = ui.get_publish_target_id().to_string();
    let now = listings.as_deref().map_or("", |l| cover_now(&target, &l.state, &l.items));
    if shown.as_ref().is_some_and(|(t, n)| *t == target && *n == now) {
        return;
    }
    ui.set_publish_cover_now(now.into());
    if now == "no" && current() == CoverChoice::Remove {
        reset(ui);
    }
    *shown = Some((target, now));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::new(w, h);
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_good_cover_passes() {
        assert_eq!(check_cover(&png(640, 360)).unwrap(), (640, 360), "16:9");
        assert_eq!(check_cover(&png(600, 900)).unwrap(), (600, 900), "box art");
        assert_eq!(check_cover(&png(320, 640)).unwrap(), (320, 640), "exactly 1:2 and 320 px");
    }

    #[test]
    fn a_bad_cover_says_why() {
        assert!(check_cover(&[]).unwrap_err().contains("empty"));
        assert!(check_cover(&vec![0u8; COVER_MAX_BYTES + 1]).unwrap_err().contains("2 MB"));
        assert!(check_cover(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap_err().contains("PNG, JPEG or WebP"));
        assert!(check_cover(&png(300, 600)).unwrap_err().contains("320 px"));
        assert!(check_cover(&png(2000, 600)).unwrap_err().contains("twice as wide"));
        assert!(check_cover(&png(400, 900)).unwrap_err().contains("twice as wide"), "taller than 1:2");
    }

    fn listing(id: &str, cover_url: Option<&str>) -> ListingSummary {
        ListingSummary {
            id: id.into(),
            title: "Box Head".into(),
            status: "published".into(),
            updated: "2026-09-30T10:00:00Z".into(),
            thumbnail_url: None,
            tags: Vec::new(),
            tags_suggested: false,
            cover_url: cover_url.map(Into::into),
        }
    }

    #[test]
    fn removing_is_offered_only_where_a_cover_may_be() {
        let items = [listing("a1", Some("https://x/c?v=1")), listing("b2", None)];
        let ready = MyListingsState::Ready;
        assert_eq!(cover_now("a1", &ready, &items), "yes");
        assert_eq!(cover_now("b2", &ready, &items), "no");
        assert_eq!(cover_now("", &ready, &items), "no", "a new listing has none");
        assert_eq!(cover_now("c3", &ready, &items), "", "not in the list: not known");
        assert_eq!(cover_now("a1", &MyListingsState::Loading, &items), "", "still loading");
    }
}
