//! Runtime window / taskbar icon.
//!
//! `build.rs` embeds `assets/icon.ico` into the executable's resource table,
//! which is what Explorer, the desktop and the Start menu read. That does
//! **not** cover the running window: winit registers its window class with
//! `hIcon: 0` and `hIconSm: 0` (winit-0.30.13,
//! `src/platform_impl/windows/window.rs:1417`), so the title bar, the Alt-Tab
//! switcher and the taskbar button all fall back to the generic Windows
//! application icon no matter what the binary contains.
//!
//! Only `Window::set_window_icon` fixes those, and it can only be called once
//! the window exists — hence a retrying `Update` system rather than `Startup`.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::WINIT_WINDOWS;

/// Rendered from `assets/icon.svg` by `build.rs`, alongside the `.ico` that
/// goes into the PE resource table — so the window icon and the file icon are
/// the same artwork by construction rather than by discipline.
const ICON_PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon_256.png"));

pub struct WindowIconPlugin;

impl Plugin for WindowIconPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, apply_window_icon);
    }
}

fn apply_window_icon(
    // Forces this system onto the main thread. `WINIT_WINDOWS` is a
    // THREAD-LOCAL, not a resource — on any other thread it is simply empty,
    // so `get_window` returns `None` forever and the icon is never applied.
    // Nothing about that failure is visible without checking for it.
    _main_thread: NonSendMarker,
    mut done: Local<bool>,
    mut attempts: Local<u32>,
    primary: Query<Entity, With<PrimaryWindow>>,
) {
    if *done {
        return;
    }
    // The window can lag the first few frames on a cold start. Give up
    // eventually rather than re-decoding a PNG forever.
    *attempts += 1;
    if *attempts > 600 {
        *done = true;
        warn!(
            "window icon: gave up after 600 frames (primary window found: {})",
            primary.single().is_ok()
        );
        return;
    }

    let Ok(entity) = primary.single() else { return };

    let icon = match decode_icon() {
        Ok(i) => i,
        Err(e) => {
            *done = true;
            warn!("window icon: {e}");
            return;
        }
    };

    let applied = WINIT_WINDOWS.with_borrow(|windows| {
        let Some(window) = windows.get_window(entity) else {
            return false;
        };
        window.set_window_icon(Some(icon.clone()));
        // The taskbar button is a separate icon slot on Windows: setting only
        // the window icon leaves the button generic.
        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::WindowExtWindows;
            window.set_taskbar_icon(Some(icon));
        }
        true
    });

    if applied {
        *done = true;
        info!("window icon applied");
    }
}

fn decode_icon() -> Result<winit::window::Icon, String> {
    let img = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)
        .map_err(|e| format!("icon PNG failed to decode: {e}"))?
        .into_rgba8();
    let (w, h) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), w, h)
        .map_err(|e| format!("icon rejected by winit: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A silently-unreadable icon is exactly the failure the user already hit
    /// once, so the asset is checked rather than assumed.
    #[test]
    fn the_embedded_icon_decodes_to_a_square_rgba_image() {
        let icon = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)
            .expect("icon_256.png did not decode")
            .into_rgba8();
        assert_eq!(icon.dimensions(), (256, 256));
        // Fully transparent art would "work" and still show nothing.
        let opaque = icon.pixels().filter(|p| p.0[3] > 200).count();
        assert!(
            opaque > 256 * 256 / 8,
            "icon is almost entirely transparent ({opaque} opaque pixels)"
        );
    }
}
