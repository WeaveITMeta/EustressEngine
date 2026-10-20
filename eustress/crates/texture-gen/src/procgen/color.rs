//! Colour helpers. Colours are designed in sRGB (how people pick them) and
//! mixed in linear light (how light adds up).

use texture_gen::mips::{linear_to_srgb, srgb_to_linear};

pub type Rgb = [f32; 3];

/// Linear colour from 8-bit sRGB.
pub fn srgb(r: u8, g: u8, b: u8) -> Rgb {
    [
        srgb_to_linear(r as f32 / 255.0),
        srgb_to_linear(g as f32 / 255.0),
        srgb_to_linear(b as f32 / 255.0),
    ]
}

pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

pub fn scale(c: Rgb, k: f32) -> Rgb {
    [c[0] * k, c[1] * k, c[2] * k]
}

/// Shift toward warm (positive) or cool (negative) without changing much
/// else: a small red/blue tilt.
pub fn warmth(c: Rgb, k: f32) -> Rgb {
    [c[0] * (1.0 + k), c[1], c[2] * (1.0 - k)]
}

/// Pick from a weighted palette with a uniform `t` in `[0, 1)`.
pub fn pick(palette: &[(Rgb, f32)], t: f32) -> Rgb {
    let total: f32 = palette.iter().map(|p| p.1).sum();
    let mut acc = 0.0;
    for &(c, w) in palette {
        acc += w / total;
        if t < acc {
            return c;
        }
    }
    palette[palette.len() - 1].0
}

/// Linear colour to 8-bit sRGB.
pub fn to_srgb8(c: Rgb) -> [u8; 3] {
    [
        (linear_to_srgb(c[0]) * 255.0 + 0.5) as u8,
        (linear_to_srgb(c[1]) * 255.0 + 0.5) as u8,
        (linear_to_srgb(c[2]) * 255.0 + 0.5) as u8,
    ]
}

#[inline]
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
