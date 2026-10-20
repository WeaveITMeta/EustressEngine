//! Keeps viewport drags from moving what the user did not mean to move.
//!
//! Three rules, shared by the Select, Move, Rotate and Scale tools:
//!
//! - **Nothing moves until the cursor does.** A press becomes a drag only
//!   after the cursor has travelled past a small dead zone, so a click, or a
//!   hand that twitches while clicking, moves nothing and records no undo
//!   step.
//! - **A body drag starts where the part already is.** Surface placement is
//!   absolute (the part lands where the cursor ray meets a surface), and the
//!   ray through a part meets the ground behind it, so on its own a drag
//!   would throw the part away from the camera the moment it began. The
//!   difference is captured when the drag goes live and faded out over the
//!   first stretch of cursor travel.
//! - **A drag can be taken back mid-gesture.** Escape, or Ctrl+Z while the
//!   button is still held, restores everything the drag touched and leaves
//!   the undo history alone.

use bevy::prelude::*;

/// Logical pixels a handle drag (move arrow or plane, rotate ring, scale
/// handle) must travel before it changes anything.
pub const HANDLE_DEAD_ZONE_PX: f32 = 3.0;

/// Logical pixels a body drag (grabbing the part itself) must travel before
/// the part moves. Larger than the handle dead zone because pressing on a
/// part is also how it gets clicked, and a click must never move it.
pub const BODY_DRAG_THRESHOLD_PX: f32 = 6.0;

/// Cursor travel, in logical pixels, over which the drag-start offset fades
/// to nothing.
pub const JUMP_FADE_PX: f32 = 140.0;

/// How much of the drag-start offset still applies once the cursor has
/// travelled `travel_px` since the drag went live: 1 at the start, 0 past
/// [`JUMP_FADE_PX`], smooth in between so the part eases onto the surface
/// under the cursor instead of snapping to it.
pub fn jump_fade(travel_px: f32) -> f32 {
    let t = (travel_px / JUMP_FADE_PX).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// A request, from Ctrl+Z or the Undo button pressed mid-drag, to cancel
/// the drag in flight instead of undoing the previous step.
///
/// It lives for two frames so the tool that owns the drag sees it whatever
/// the system order; the tool that cancels its drag consumes it, and an
/// unclaimed request expires on its own, so it can never cancel a later
/// drag by surprise.
#[derive(Resource, Default)]
pub struct DragCancelRequest {
    frames_left: u8,
}

impl DragCancelRequest {
    pub fn request(&mut self) {
        self.frames_left = 2;
    }

    pub fn pending(&self) -> bool {
        self.frames_left > 0
    }

    pub fn consume(&mut self) {
        self.frames_left = 0;
    }
}

/// Count an unclaimed cancel request down; see [`DragCancelRequest`].
pub fn age_drag_cancel_request(mut request: ResMut<DragCancelRequest>) {
    request.frames_left = request.frames_left.saturating_sub(1);
}

/// True while any viewport tool holds a drag, including a press on a part
/// that has not yet travelled past the threshold: undo during that gesture
/// cancels the gesture rather than reaching into the history.
pub fn drag_in_progress(world: &World) -> bool {
    let select = world
        .get_resource::<crate::select_tool::SelectToolState>()
        .map(|s| s.dragging)
        .unwrap_or(false);
    let moving = world
        .get_resource::<crate::move_tool::MoveToolState>()
        .map(|s| s.dragged_axis.is_some() || s.dragged_plane.is_some() || s.free_drag)
        .unwrap_or(false);
    let rotating = world
        .get_resource::<crate::rotate_tool::RotateToolState>()
        .map(|s| s.dragged_axis.is_some())
        .unwrap_or(false);
    let scaling = world
        .get_resource::<crate::scale_tool::ScaleToolState>()
        .map(|s| s.dragged_axis.is_some())
        .unwrap_or(false);
    select || moving || rotating || scaling
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_starts_whole_and_ends_at_zero() {
        assert!((jump_fade(0.0) - 1.0).abs() < 1e-6);
        assert_eq!(jump_fade(JUMP_FADE_PX), 0.0);
        assert_eq!(jump_fade(JUMP_FADE_PX * 3.0), 0.0);
        let mid = jump_fade(JUMP_FADE_PX * 0.5);
        assert!(mid > 0.4 && mid < 0.6, "mid = {mid}");
    }

    #[test]
    fn fade_never_increases() {
        let mut last = jump_fade(0.0);
        for px in 1..200 {
            let f = jump_fade(px as f32);
            assert!(f <= last + 1e-6);
            last = f;
        }
    }

    #[test]
    fn cancel_request_expires_unless_consumed() {
        let mut r = DragCancelRequest::default();
        assert!(!r.pending());
        r.request();
        assert!(r.pending());
        r.frames_left = r.frames_left.saturating_sub(1);
        assert!(r.pending());
        r.frames_left = r.frames_left.saturating_sub(1);
        assert!(!r.pending());
        r.request();
        r.consume();
        assert!(!r.pending());
    }
}
