// =============================================================================
// Eustress Web - Pointer Tilt
// =============================================================================
// Parallax tilt for the hero render: the panel leans toward the cursor.
//
// Why it is written this way. Pointer position is a continuous value, and
// routing a continuous value through the reactive system means a re-render on
// every mousemove frame, which collapses under load. So the handlers write CSS
// custom properties straight onto the DOM node and never touch a signal. The
// compositor does the rest, since only `transform` changes.
//
// The tilt is motivated rather than decorative: a screenshot that responds to
// the cursor reads as a live surface, which is the claim the section is making.
// =============================================================================

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::MouseEvent;

/// Maximum rotation in degrees at the far edge of the element.
/// Small on purpose: past about 10deg the perspective distortion starts to
/// bend straight UI lines in the screenshot and it reads as a warp, not a tilt.
const MAX_TILT_DEG: f64 = 7.0;

/// How far the panel lifts toward the viewer, in pixels.
const LIFT_PX: f64 = 10.0;

/// Whether the visitor asked for reduced motion. Checked per interaction
/// rather than cached, so a mid-session preference change is honoured.
fn prefers_reduced_motion() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok().flatten())
        .map(|m| m.matches())
        .unwrap_or(false)
}

/// Write the tilt custom properties onto `el`.
fn set_tilt(el: &web_sys::HtmlElement, rx: f64, ry: f64, lift: f64) {
    let style = el.style();
    let _ = style.set_property("--tilt-rx", &format!("{:.3}deg", rx));
    let _ = style.set_property("--tilt-ry", &format!("{:.3}deg", ry));
    let _ = style.set_property("--tilt-lift", &format!("{:.2}px", lift));
}

/// Attach to the element that should tilt. Returns the two handlers to wire to
/// `on:mousemove` and `on:mouseleave`.
///
/// The element is located from the event itself (`currentTarget`), so no node
/// reference has to be threaded through and the helper works on any element.
pub fn on_tilt_move(ev: &MouseEvent) {
    if prefers_reduced_motion() {
        return;
    }
    let Some(target) = ev.current_target() else { return };
    let Ok(el) = target.dyn_into::<web_sys::HtmlElement>() else { return };

    let rect = el.get_bounding_client_rect();
    let (w, h) = (rect.width(), rect.height());
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    // Normalise the pointer to -1..1 across each axis, measured from centre.
    let nx = ((ev.client_x() as f64 - rect.left()) / w) * 2.0 - 1.0;
    let ny = ((ev.client_y() as f64 - rect.top()) / h) * 2.0 - 1.0;

    // Leaning TOWARD the cursor means the top edge drops as the pointer moves
    // down, so the X rotation is negated relative to the Y offset.
    let rx = -ny.clamp(-1.0, 1.0) * MAX_TILT_DEG;
    let ry = nx.clamp(-1.0, 1.0) * MAX_TILT_DEG;

    set_tilt(&el, rx, ry, LIFT_PX);
}

/// Return the panel to rest. The easing back is handled in CSS by the
/// transition on `transform`, so this only has to clear the values.
pub fn on_tilt_leave(ev: &MouseEvent) {
    let Some(target) = ev.current_target() else { return };
    let Ok(el) = target.dyn_into::<web_sys::HtmlElement>() else { return };
    set_tilt(&el, 0.0, 0.0, 0.0);
}
