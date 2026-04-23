//! # Cursor Badge — workaround for the Slint custom-cursor blocker
//!
//! Slint 1.x doesn't expose the underlying `winit` image-cursor
//! binding, so we can't set an OS-level custom cursor per tool. As a
//! workaround, we render a 16×16 badge **inside the viewport** at the
//! cursor's current position, offset down-right so it sits adjacent
//! to the OS cursor without covering it.
//!
//! Not identical to the spec (the OS cursor ideally IS the badge),
//! but it's the highest-fidelity signal available with Slint today.
//! When Slint upstream ships `set-mouse-cursor-image`, we swap this
//! for the proper OS cursor + remove the Slint-side follower.
//!
//! ## State flow
//!
//! ```text
//! ActiveModalTool (Res) ──▶ sync_cursor_badge_state ──▶ CursorBadgeState
//!                                                            │
//! mouse cursor position (window) ────────────────────────────┤
//!                                                            ▼
//!                                                    Slint cursor_badge.slint
//!                                                    (visible-when, cursor-x, cursor-y, badge-icon)
//! ```

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

#[derive(Resource, Default, Debug, Clone)]
pub struct CursorBadgeState {
    pub visible: bool,
    /// Viewport-relative cursor position in pixels.
    pub cursor_x: f32,
    pub cursor_y: f32,
    /// Which badge SVG to render — one of the 6 shipped
    /// cursor-badge assets (`cursor-badge-gap-fill.svg` etc.).
    /// Empty string = no badge for this tool.
    pub icon_path: String,
}

pub struct CursorBadgePlugin;

impl Plugin for CursorBadgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorBadgeState>()
            .add_systems(Update, sync_cursor_badge_state);
    }
}

fn sync_cursor_badge_state(
    mut state: ResMut<CursorBadgeState>,
    active: Res<crate::modal_tool::ActiveModalTool>,
    windows: Query<&Window, With<PrimaryWindow>>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
) {
    // No active tool → hide.
    let Some(tool_id) = active.id() else {
        state.visible = false;
        state.icon_path.clear();
        return;
    };

    // Map tool id → badge icon. Tools that don't have a badge
    // (Select / Move / Rotate / Scale) render no badge.
    let icon_path = match tool_id {
        "gap_fill"             => "assets/icons/ui/cursor-badge-gap-fill.svg",
        "resize_align"         => "assets/icons/ui/cursor-badge-resize-align.svg",
        "edge_align"           => "assets/icons/ui/cursor-badge-edge-align.svg",
        "part_swap_positions"  => "assets/icons/ui/cursor-badge-part-swap.svg",
        "model_reflect"        => "assets/icons/ui/cursor-badge-mirror.svg",
        "material_flip"        => "assets/icons/ui/cursor-badge-material-flip.svg",
        _ => "",
    };

    if icon_path.is_empty() {
        state.visible = false;
        state.icon_path.clear();
        return;
    }
    state.icon_path = icon_path.to_string();

    // Resolve cursor position — clamp to inside the viewport bounds
    // so the badge doesn't trail into the panel chrome.
    let Ok(window) = windows.single() else {
        state.visible = false;
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        state.visible = false;
        return;
    };
    if let Some(vb) = viewport_bounds.as_deref() {
        let scale = window.scale_factor() as f32;
        if !vb.contains_logical(cursor, scale) {
            state.visible = false;
            return;
        }
    }
    state.cursor_x = cursor.x;
    state.cursor_y = cursor.y;
    state.visible = true;
}
