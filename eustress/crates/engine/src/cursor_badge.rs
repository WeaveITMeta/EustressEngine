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

/// NOTE FOR WHOEVER PICKS THIS UP: [`CursorBadgeState`] currently has **no
/// renderer**. This system computes `icon_path` + `visible` every frame and
/// nothing anywhere reads them — `grep -rn CursorBadgeState src/` returns only
/// this file. No badge has ever appeared on screen, for modal tools or anything
/// else, so do not treat "the badge will show it" as working feedback.
///
/// Rendering it is not a one-liner: `WinitPlugin` is disabled (Slint owns the
/// window), so Bevy's `CursorIcon` does not apply, and the obvious Slint route
/// — a `TouchArea` over `viewport-sizer` carrying `mouse-cursor` — would sit on
/// top of the transparent hole the 3D scene is composited through and risks
/// swallowing viewport clicks. The safe route is a small absolutely-positioned
/// `Image` in the Slint overlay that follows the cursor and never takes input.
///
/// Feedback that IS live today for the paint modes: the ribbon button lights up
/// (`selected: root.current-tool == "lock"` etc.) and `lock_tool` draws a
/// wireframe box around the part under the cursor.
fn sync_cursor_badge_state(
    mut state: ResMut<CursorBadgeState>,
    active: Res<crate::modal_tool::ActiveModalTool>,
    windows: Query<&Window, With<PrimaryWindow>>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    // Gizmo hover states — a hovered transform handle shows a matching
    // badge so the user knows the handle is live BEFORE clicking (AAA
    // audit: "gizmo handles show no cursor feedback").
    move_state: Option<Res<crate::move_tool::MoveToolState>>,
    scale_state: Option<Res<crate::scale_tool::ScaleToolState>>,
    rotate_state: Option<Res<crate::rotate_tool::RotateToolState>>,
    // Paint modes live on `StudioState.current_tool`, NOT in `ActiveModalTool`
    // — they are not ModalTools. Without this the badge could never show for
    // Lock / Unlock / Anchor, so entering one of those modes changed nothing
    // on screen and read as "the button does nothing".
    studio_state: Option<Res<crate::ui::StudioState>>,
) {
    // Modal tools take precedence; otherwise fall through to gizmo-hover
    // badges for the transform tools.
    let icon_path = if let Some(tool_id) = active.id() {
        match tool_id {
            "gap_fill"             => "assets/icons/ui/cursor-badge-gap-fill.svg",
            "resize_align"         => "assets/icons/ui/cursor-badge-resize-align.svg",
            "edge_align"           => "assets/icons/ui/cursor-badge-edge-align.svg",
            "part_swap_positions"  => "assets/icons/ui/cursor-badge-part-swap.svg",
            "model_reflect"        => "assets/icons/ui/cursor-badge-mirror.svg",
            "material_flip"        => "assets/icons/ui/cursor-badge-material-flip.svg",
            // Decal/Texture surface placement — reuse the material-flip
            // badge (a surface-paint glyph) as the "paste onto face" cursor.
            "surface_place"        => "assets/icons/ui/cursor-badge-material-flip.svg",
            _ => "",
        }
    } else if move_state
        .as_deref()
        .map(|s| s.hovered_axis.is_some() || s.hovered_plane.is_some() || s.dragged_axis.is_some())
        .unwrap_or(false)
    {
        "assets/icons/ui/cursor-badge-move.svg"
    } else if scale_state
        .as_deref()
        .map(|s| s.hovered_axis.is_some() || s.dragged_axis.is_some())
        .unwrap_or(false)
    {
        "assets/icons/ui/cursor-badge-scale.svg"
    } else if rotate_state
        .as_deref()
        // RotateToolState tracks no hover — badge appears while dragging.
        .map(|s| s.dragged_axis.is_some())
        .unwrap_or(false)
    {
        "assets/icons/ui/cursor-badge-rotate.svg"
    } else {
        // Paint modes. These persist until Escape, so unlike the gizmo badges
        // above the cursor carries the mode for as long as it is armed — that
        // is the only on-screen signal that a click is about to flip a flag
        // rather than select.
        match studio_state.as_deref().map(|s| s.current_tool) {
            Some(crate::ui::Tool::Lock)   => "assets/icons/ui/lock.svg",
            Some(crate::ui::Tool::Unlock) => "assets/icons/ui/unlock.svg",
            Some(crate::ui::Tool::Anchor) => "assets/icons/ui/anchor.svg",
            _ => "",
        }
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
