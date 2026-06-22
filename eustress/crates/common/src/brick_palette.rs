//! Native Eustress BrickColor palette — sRGB(0-255)-keyed swatches grouped
//! into the seven color wheels, plus a Roblox-BrickColor -> Eustress-token
//! mapping and a pure-Rust sRGB -> OKLCH helper.
//!
//! This extends the legacy `BrickColorValue` concept (an `i32` palette index,
//! [`crate::classes::BrickColorValue`]): an index resolves to a
//! [`PaletteEntry`] here when a concrete RGB / OKLCH is needed. The table is
//! the *authoring* vocabulary for the categorical color picker — the exact
//! imported Roblox token + sRGB are preserved verbatim in an instance's
//! `[metadata]` by the importer, so this palette never has to be an
//! exhaustive 1:1 mirror of Roblox's ~1000-entry set.
//!
//! Pure Rust, no `rbx_*` / bevy deps. The OKLCH math is Ottosson's, applied
//! with the sRGB EOTF decode first; it MUST agree with the importer's
//! `color_manifest::srgb_to_oklch` and the Python `tools/color_wheel`
//! pipeline so OKLCH values are comparable across all three.

use serde::{Deserialize, Serialize};

/// The seven Eustress color wheels — the categorical axes of the picker.
/// `good <-> evil` (Halo/Aether/Verdure vs Char/Hex/Umbra) crossed with
/// `abstract <-> realistic` (Aether/Hex abstract; Verdure/Char realistic),
/// with Halo/Umbra the light/dark poles and Stone the neutral core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Wheel {
    Aether,
    Halo,
    Verdure,
    Stone,
    Char,
    Hex,
    Umbra,
}

impl Wheel {
    /// All seven wheels in canonical order (top-level picker order).
    pub const ALL: [Wheel; 7] = [
        Wheel::Aether,
        Wheel::Halo,
        Wheel::Verdure,
        Wheel::Stone,
        Wheel::Char,
        Wheel::Hex,
        Wheel::Umbra,
    ];

    /// Stable lowercase id used on the wire (Slint, manifest, settings).
    pub const fn id_str(self) -> &'static str {
        match self {
            Wheel::Aether => "aether",
            Wheel::Halo => "halo",
            Wheel::Verdure => "verdure",
            Wheel::Stone => "stone",
            Wheel::Char => "char",
            Wheel::Hex => "hex",
            Wheel::Umbra => "umbra",
        }
    }

    /// Parse the stable id back to a wheel.
    pub fn from_id(s: &str) -> Option<Wheel> {
        Wheel::ALL.into_iter().find(|w| w.id_str() == s)
    }

    /// Title-case label for the UI.
    pub const fn display_name(self) -> &'static str {
        match self {
            Wheel::Aether => "Aether",
            Wheel::Halo => "Halo",
            Wheel::Verdure => "Verdure",
            Wheel::Stone => "Stone",
            Wheel::Char => "Char",
            Wheel::Hex => "Hex",
            Wheel::Umbra => "Umbra",
        }
    }
}

/// One palette swatch. `id` doubles as the Eustress token and, for entries
/// seeded from Roblox's palette, the Roblox BrickColor number (so a Roblox
/// token round-trips 1:1 when it overlaps). Native Eustress swatches use ids
/// >= [`NATIVE_ID_BASE`] to stay clear of Roblox's numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteEntry {
    pub id: u16,
    pub name: &'static str,
    pub srgb: [u8; 3],
    pub wheel: Wheel,
}

impl PaletteEntry {
    pub const fn new(id: u16, name: &'static str, srgb: [u8; 3], wheel: Wheel) -> Self {
        Self { id, name, srgb, wheel }
    }

    /// sRGB as 0..1 floats (the engine's `color_rgba` convention).
    pub fn srgb_f32(&self) -> [f32; 3] {
        [
            self.srgb[0] as f32 / 255.0,
            self.srgb[1] as f32 / 255.0,
            self.srgb[2] as f32 / 255.0,
        ]
    }

    /// OKLCH `[L, C, hue_deg]` of this swatch.
    pub fn oklch(&self) -> [f32; 3] {
        srgb_u8_to_oklch(self.srgb)
    }
}

/// Roblox's "Medium stone grey" — the canonical default part color
/// (sRGB 163,162,165). MUST match `classes.rs` / `brick_color_value.rs`.
pub const DEFAULT_INDEX: u16 = 194;

/// Native Eustress swatch ids start here (above Roblox's ~1032 range) so a
/// real Roblox BrickColor number never collides with a Eustress-authored one.
pub const NATIVE_ID_BASE: u16 = 9000;

/// Curated starter palette across the seven wheels. Roblox-sourced entries
/// keep their real BrickColor number + sRGB; native Eustress swatches
/// (id >= NATIVE_ID_BASE) fill out wheels the classic Roblox palette
/// under-represents (ethereal lights, chaotic darks, deep voids). The Python
/// `tools/color_wheel` study produces the richer, usage-derived membership;
/// this is the always-available authoring fallback.
pub const PALETTE: &[PaletteEntry] = &[
    // -- Stone (neutral core) -----------------------------------------
    PaletteEntry::new(194, "Medium stone grey", [163, 162, 165], Wheel::Stone),
    PaletteEntry::new(199, "Dark stone grey", [99, 95, 98], Wheel::Stone),
    PaletteEntry::new(135, "Sand blue", [116, 134, 156], Wheel::Stone),
    PaletteEntry::new(9001, "Slate", [112, 118, 126], Wheel::Stone),
    PaletteEntry::new(9002, "Ash grey", [142, 140, 138], Wheel::Stone),
    // -- Halo (light - good) ------------------------------------------
    PaletteEntry::new(1, "White", [242, 243, 243], Wheel::Halo),
    PaletteEntry::new(208, "Light stone grey", [229, 228, 223], Wheel::Halo),
    PaletteEntry::new(9010, "Halo gold", [253, 240, 200], Wheel::Halo),
    PaletteEntry::new(9011, "Seraph blue", [220, 240, 255], Wheel::Halo),
    PaletteEntry::new(9012, "Grace", [255, 246, 238], Wheel::Halo),
    // -- Aether (abstract - good): bright neons -----------------------
    PaletteEntry::new(23, "Bright blue", [13, 105, 172], Wheel::Aether),
    PaletteEntry::new(24, "Bright yellow", [245, 205, 48], Wheel::Aether),
    PaletteEntry::new(107, "Bright bluish green", [0, 143, 156], Wheel::Aether),
    PaletteEntry::new(1010, "Really blue", [0, 0, 255], Wheel::Aether),
    PaletteEntry::new(9020, "Aether cyan", [60, 230, 220], Wheel::Aether),
    PaletteEntry::new(9021, "Prism green", [80, 240, 120], Wheel::Aether),
    // -- Verdure (realistic - good): natural --------------------------
    PaletteEntry::new(37, "Bright green", [75, 151, 75], Wheel::Verdure),
    PaletteEntry::new(28, "Dark green", [40, 127, 71], Wheel::Verdure),
    PaletteEntry::new(141, "Earth green", [39, 70, 45], Wheel::Verdure),
    PaletteEntry::new(151, "Sand green", [120, 144, 130], Wheel::Verdure),
    PaletteEntry::new(9030, "Moss", [90, 110, 70], Wheel::Verdure),
    PaletteEntry::new(9031, "Loam", [120, 100, 74], Wheel::Verdure),
    // -- Char (realistic - evil): suffering, ash ----------------------
    PaletteEntry::new(192, "Reddish brown", [105, 64, 40], Wheel::Char),
    PaletteEntry::new(217, "Brown", [124, 92, 70], Wheel::Char),
    PaletteEntry::new(153, "Sand red", [149, 121, 119], Wheel::Char),
    PaletteEntry::new(9040, "Soot", [58, 52, 48], Wheel::Char),
    PaletteEntry::new(9041, "Rust", [123, 63, 40], Wheel::Char),
    PaletteEntry::new(9042, "Dried blood", [96, 42, 38], Wheel::Char),
    // -- Hex (abstract - evil): chaotic, toxic ------------------------
    PaletteEntry::new(21, "Bright red", [196, 40, 28], Wheel::Hex),
    PaletteEntry::new(1004, "Really red", [255, 0, 0], Wheel::Hex),
    PaletteEntry::new(104, "Bright violet", [107, 50, 124], Wheel::Hex),
    PaletteEntry::new(1032, "Hot pink", [255, 0, 191], Wheel::Hex),
    PaletteEntry::new(9050, "Venom", [120, 230, 40], Wheel::Hex),
    PaletteEntry::new(9051, "Bruise", [88, 30, 110], Wheel::Hex),
    // -- Umbra (dark - evil): deep voids ------------------------------
    PaletteEntry::new(26, "Black", [27, 42, 53], Wheel::Umbra),
    PaletteEntry::new(1003, "Really black", [17, 17, 17], Wheel::Umbra),
    PaletteEntry::new(9060, "Void", [12, 10, 22], Wheel::Umbra),
    PaletteEntry::new(9061, "Pitch", [20, 18, 16], Wheel::Umbra),
    PaletteEntry::new(9062, "Abyss", [10, 16, 28], Wheel::Umbra),
];

/// Look up a palette entry by its (Roblox or native) index.
pub fn entry_for_index(index: i32) -> Option<&'static PaletteEntry> {
    let id = u16::try_from(index).ok()?;
    PALETTE.iter().find(|e| e.id == id)
}

/// sRGB(0-255) for an index, falling back to the default grey when unknown.
pub fn srgb_for_index(index: i32) -> [u8; 3] {
    entry_for_index(index)
        .or_else(|| entry_for_index(DEFAULT_INDEX as i32))
        .map(|e| e.srgb)
        .unwrap_or([163, 162, 165])
}

/// Map a Roblox BrickColor token (+ the sRGB it imported as) to the nearest
/// Eustress palette entry: exact id match when the number overlaps, else the
/// perceptually-nearest swatch by OKLCH.
pub fn roblox_brickcolor_to_eustress(token: u16, imported_srgb: [u8; 3]) -> &'static PaletteEntry {
    if let Some(e) = entry_for_index(token as i32) {
        return e;
    }
    nearest_by_oklch(imported_srgb)
}

/// Perceptually-nearest palette entry to an sRGB(0-255) color (OKLab dist^2).
pub fn nearest_by_oklch(srgb: [u8; 3]) -> &'static PaletteEntry {
    let target = srgb_u8_to_oklch(srgb);
    PALETTE
        .iter()
        .min_by(|a, b| oklch_dist(a.oklch(), target).total_cmp(&oklch_dist(b.oklch(), target)))
        .unwrap_or(&PALETTE[0])
}

/// Squared OKLab distance between two OKLCH colors.
fn oklch_dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let al = oklch_to_oklab(a);
    let bl = oklch_to_oklab(b);
    let d = [al[0] - bl[0], al[1] - bl[1], al[2] - bl[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// Pure-Rust sRGB(0-255) -> OKLCH `[L, C, hue_deg]` (Ottosson). The sRGB EOTF
/// is decoded *before* the linear-sRGB -> LMS matrices. Achromatic colors get
/// `hue = 0`. Mirror of `color_manifest::srgb_to_oklch` (0..1 input) and the
/// Python `oklch.py` so all three agree.
pub fn srgb_u8_to_oklch(c: [u8; 3]) -> [f32; 3] {
    fn lin(u: f32) -> f32 {
        if u <= 0.04045 {
            u / 12.92
        } else {
            ((u + 0.055) / 1.055).powf(2.4)
        }
    }
    let r = lin(c[0] as f32 / 255.0);
    let g = lin(c[1] as f32 / 255.0);
    let b = lin(c[2] as f32 / 255.0);
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();
    let ok_l = 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_;
    let ok_a = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_;
    let ok_b = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_;
    let chroma = (ok_a * ok_a + ok_b * ok_b).sqrt();
    let hue = if chroma < 1e-4 {
        0.0
    } else {
        ok_b.atan2(ok_a).to_degrees().rem_euclid(360.0)
    };
    [ok_l, chroma, hue]
}

/// OKLCH `[L, C, hue_deg]` -> OKLab `[L, a, b]`.
fn oklch_to_oklab(c: [f32; 3]) -> [f32; 3] {
    let h = c[2].to_radians();
    [c[0], c[1] * h.cos(), c[1] * h.sin()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_index_is_medium_stone_grey() {
        assert_eq!(srgb_for_index(DEFAULT_INDEX as i32), [163, 162, 165]);
    }

    #[test]
    fn unknown_index_falls_back_to_default() {
        assert_eq!(srgb_for_index(123_456), [163, 162, 165]);
    }

    #[test]
    fn every_wheel_has_at_least_one_swatch() {
        for w in Wheel::ALL {
            assert!(PALETTE.iter().any(|e| e.wheel == w), "wheel {:?} empty", w);
        }
    }

    #[test]
    fn wheel_id_round_trips() {
        for w in Wheel::ALL {
            assert_eq!(Wheel::from_id(w.id_str()), Some(w));
        }
    }

    #[test]
    fn black_is_dark_white_is_light() {
        let black = srgb_u8_to_oklch([0, 0, 0]);
        let white = srgb_u8_to_oklch([255, 255, 255]);
        assert!(black[0] < 0.05, "black L = {}", black[0]);
        assert!(white[0] > 0.95, "white L = {}", white[0]);
    }

    #[test]
    fn roblox_token_exact_match_wins() {
        let e = roblox_brickcolor_to_eustress(194, [0, 0, 0]);
        assert_eq!(e.id, 194);
    }

    #[test]
    fn nearest_picks_a_close_swatch_for_unknown_token() {
        // Pure red is not number 7777; nearest should land in a warm/red swatch.
        let e = roblox_brickcolor_to_eustress(7777, [250, 10, 10]);
        assert!(e.srgb[0] > e.srgb[2], "expected a reddish nearest, got {:?}", e);
    }
}
