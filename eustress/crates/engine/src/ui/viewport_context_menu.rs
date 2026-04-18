//! # Viewport (3D) right-click context menu
//!
//! Detects a right-click on the 3D viewport that wasn't a camera-orbit
//! drag, and asks the Slint side to show the entity context menu at the
//! release position.
//!
//! A right-click in the viewport is ambiguous — it can start a camera
//! orbit (hold + drag) or request a context menu (press + release in
//! place). We distinguish by tracking the press position and comparing
//! it against the release position; if the cursor moved less than
//! [`DRAG_THRESHOLD_PX`], we treat it as a click, otherwise a drag.
//!
//! The menu content reuses the default entity menu (empty
//! `context-menu-items` means the Slint `ContextMenu` component falls
//! through to its built-in entity layout), so Cut/Copy/Paste/Delete/
//! Copy Path/etc. already work via the shared [`SlintAction::ContextAction`]
//! drain handler.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use super::slint_bridge::SlintBridge;
use super::ViewportBounds;
use super::SlintUIFocus;

/// If the cursor moves farther than this (in pixels) between right-mouse
/// down and up, treat the gesture as a camera orbit rather than a click.
const DRAG_THRESHOLD_PX: f32 = 4.0;

/// Per-click state for the viewport right-click detector.
#[derive(Resource, Default)]
pub struct ViewportRightClickState {
    /// Cursor position at press time (window-local). `None` when not
    /// currently pressed.
    press_pos: Option<Vec2>,
}

/// Bevy system. Runs every frame; cheap when idle (no mouse events).
pub fn detect_viewport_right_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    viewport: Option<Res<ViewportBounds>>,
    ui_focus: Option<Res<SlintUIFocus>>,
    bridge: Option<Res<SlintBridge>>,
    mut state: ResMut<ViewportRightClickState>,
) {
    let Some(bridge) = bridge else { return };
    let Some(viewport) = viewport else { return };
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        // Cursor left the window — discard any pending press so a release
        // outside doesn't fire the menu.
        state.press_pos = None;
        return;
    };

    // Skip if Slint owns pointer focus (user is inside a panel/UI).
    let ui_has_focus = ui_focus.map(|f| f.has_focus).unwrap_or(false);
    if ui_has_focus {
        state.press_pos = None;
        return;
    }

    // Only consider clicks inside the viewport rectangle.
    let in_viewport = cursor.x >= viewport.x
        && cursor.x <= viewport.x + viewport.width
        && cursor.y >= viewport.y
        && cursor.y <= viewport.y + viewport.height;

    if mouse.just_pressed(MouseButton::Right) && in_viewport {
        state.press_pos = Some(cursor);
        return;
    }

    if mouse.just_released(MouseButton::Right) {
        let Some(start) = state.press_pos.take() else { return };
        let delta = cursor - start;
        // Square-norm comparison avoids a sqrt in the hot path.
        if delta.length_squared() > DRAG_THRESHOLD_PX * DRAG_THRESHOLD_PX {
            return; // It was a drag (camera orbit) — no menu.
        }
        // Stamp the request onto the bridge; slint_main applies it on
        // the next overlay tick.
        let mut b = bridge.lock();
        b.request_viewport_context_menu = Some((cursor.x, cursor.y));
    }
}
