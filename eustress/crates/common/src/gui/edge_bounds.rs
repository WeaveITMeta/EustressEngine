//! # Bounds for the rectangles a ScreenGui hands to Slint
//!
//! Slint's software renderer keeps a rectangle's physical edges in 16-bit
//! integers. An edge past 32,767 physical px panics inside euclid (BOXHEAD #75,
//! the same limit as the text-height crash). A GUI element's position and size
//! come from UDim2 `Scale` times the parent's size plus `Offset`, and nothing
//! stops a script writing a huge one. So every rectangle is clamped, edge by
//! edge, to a bound around the viewport. An edge that is clamped lies off the
//! screen on the side it was clamped to, so what is visible does not change.

/// The most a rectangle edge may sit from the origin, in logical px. At up to
/// 4x scale an edge stays under the renderer's 16-bit limit: 4,000 x 4 =
/// 16,000 physical px, and a width at most doubles that to 32,000.
pub const EDGE_LIMIT: f32 = 4000.0;

/// The largest font size handed to Slint, in logical px. A text box's height
/// is its element's (bounded), but the line height grows with the font size.
pub const FONT_LIMIT: f32 = 1000.0;

/// The bound for a viewport of `(width, height)` logical px: [`EDGE_LIMIT`], or
/// the viewport itself when it is bigger, so every visible edge lies inside.
pub fn edge_bound(viewport: (f32, f32)) -> f32 {
    let biggest = viewport.0.max(viewport.1);
    if biggest.is_finite() {
        EDGE_LIMIT.max(biggest)
    } else {
        EDGE_LIMIT
    }
}

/// The two edges of one axis, each kept within `bound` either side of the
/// origin, in order (`low <= high`). A NaN edge (an infinity times zero) is 0,
/// and an infinite edge is the bound.
pub fn bounded_edges(low: f32, high: f32, bound: f32) -> (f32, f32) {
    // Clamped by hand: `f32::clamp` panics when its bounds are out of order.
    let clamp = |v: f32| if v.is_nan() { 0.0 } else { v.max(-bound).min(bound) };
    let (a, b) = (clamp(low), clamp(high));
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// A font size Slint can draw: a finite size from 0 to [`FONT_LIMIT`].
pub fn bounded_font_size(size: f32) -> f32 {
    if size.is_nan() {
        0.0
    } else {
        size.clamp(0.0, FONT_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_huge_edge_is_clamped_and_a_visible_one_is_not() {
        assert_eq!(bounded_edges(-1e30, 1e30, EDGE_LIMIT), (-EDGE_LIMIT, EDGE_LIMIT));
        assert_eq!(bounded_edges(10.0, 110.0, EDGE_LIMIT), (10.0, 110.0));
        assert_eq!(bounded_edges(f32::INFINITY, f32::NEG_INFINITY, EDGE_LIMIT), (-EDGE_LIMIT, EDGE_LIMIT));
        assert_eq!(bounded_edges(f32::NAN, f32::INFINITY, EDGE_LIMIT), (0.0, EDGE_LIMIT));
    }

    #[test]
    fn the_edges_come_out_in_order() {
        assert_eq!(bounded_edges(5.0, -5.0, EDGE_LIMIT), (-5.0, 5.0));
    }

    #[test]
    fn the_bound_keeps_the_renderer_edge_under_its_16_bit_limit() {
        // A 1920 x 1080 window at 4x scale: the widest span Slint adds up.
        let bound = edge_bound((1920.0, 1080.0));
        assert_eq!(bound, EDGE_LIMIT);
        assert!(2.0 * bound * 4.0 < 32_767.0);
        // A bigger viewport raises the bound to itself, so its edges stay visible.
        assert_eq!(edge_bound((5120.0, 1440.0)), 5120.0);
        assert_eq!(edge_bound((f32::INFINITY, 10.0)), EDGE_LIMIT);
    }

    #[test]
    fn a_font_size_is_a_drawable_size() {
        assert_eq!(bounded_font_size(14.0), 14.0);
        assert_eq!(bounded_font_size(1e9), FONT_LIMIT);
        assert_eq!(bounded_font_size(-3.0), 0.0);
        assert_eq!(bounded_font_size(f32::NAN), 0.0);
    }
}
