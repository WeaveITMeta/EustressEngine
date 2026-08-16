// =============================================================================
// Eustress Web - QR Code
// =============================================================================
// Renders a QR code as inline SVG.
//
// The `qrcode` crate is pulled in with default-features off, so its `image` and
// `svg` render backends (and their dependency trees) stay out of the WASM
// bundle. We walk the module matrix and emit one <rect> per dark module, which
// is all the SVG backend would have done anyway.
// =============================================================================

use leptos::prelude::*;
use qrcode::types::Color;
use qrcode::{EcLevel, QrCode};

/// Quiet zone in modules. The QR spec requires 4; scanners are unreliable
/// without it because they cannot find the finder patterns against a busy page.
const QUIET_ZONE: u32 = 4;

/// Build the SVG path data for every dark module in `data`.
///
/// Returns `None` when the payload cannot be encoded (too long for any version).
/// Emitting one path beats one <rect> per module: a 29x29 code has ~440 dark
/// modules, and 440 rects is markedly slower to parse and paint on a phone.
fn qr_path(data: &str) -> Option<(String, u32)> {
    // Medium correction tolerates ~15% damage, which covers screen glare and a
    // partially obscured code without inflating the module count.
    let code = QrCode::with_error_correction_level(data, EcLevel::M).ok()?;
    let width = code.width() as u32;
    let colors = code.to_colors();

    let mut path = String::with_capacity(colors.len() * 12);
    for (i, color) in colors.iter().enumerate() {
        if *color != Color::Dark {
            continue;
        }
        let x = (i as u32 % width) + QUIET_ZONE;
        let y = (i as u32 / width) + QUIET_ZONE;
        // "M<x> <y>h1v1h-1z" — a 1x1 module square in module units.
        path.push_str(&format!("M{} {}h1v1h-1z", x, y));
    }

    Some((path, width + QUIET_ZONE * 2))
}

/// An inline SVG QR code.
///
/// `size_px` is the rendered edge length. The viewBox is in module units and
/// scales to it, so the code stays crisp at any size (no raster upscaling).
#[component]
pub fn QrCodeSvg(
    /// Payload to encode. Typically the mobile verification URL.
    #[prop(into)]
    data: String,
    /// Rendered edge length in CSS pixels.
    #[prop(default = 220)]
    size_px: u32,
) -> impl IntoView {
    let rendered = qr_path(&data);

    match rendered {
        Some((path, total)) => view! {
            <svg
                class="qr-code"
                width=size_px
                height=size_px
                viewBox=format!("0 0 {} {}", total, total)
                shape-rendering="crispEdges"
                role="img"
                aria-label="QR code linking to identity verification on your phone"
            >
                // White plate: QR scanners need a light background and a quiet
                // zone, which the dark page background would otherwise deny.
                <rect width=total height=total fill="#ffffff" />
                <path d=path fill="#000000" />
            </svg>
        }
        .into_any(),
        None => view! {
            <p class="qr-error">"Could not generate a QR code for this link."</p>
        }
        .into_any(),
    }
}
