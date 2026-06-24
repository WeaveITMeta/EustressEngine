//! Seven-wheel categorical color-picker state — the cosmetic, session-scoped
//! Bevy resources behind the status-bar color widget. Mirrors the
//! `DisplayUnit` pattern in [`crate::units`]: ECS-owned, no disk writes here;
//! persistence (if any) is the engine's user-settings responsibility.
//!
//! The wheel taxonomy itself lives in [`crate::brick_palette::Wheel`]; this
//! module only holds the picker's runtime state and the per-wheel swatch
//! query the UI renders.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::brick_palette::{Wheel, PALETTE};

/// Which wheel the two-step picker is currently drilled into. `None` = the
/// top-level list (the seven wheels + favorites) is shown.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActiveColorWheel(pub Option<Wheel>);

/// The user's favorite swatches (sRGB 0-255), most-recent first, capped at
/// [`ColorFavorites::CAP`]. Surfaced at the top of the picker's first step.
#[derive(Resource, Default, Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ColorFavorites(pub Vec<[u8; 3]>);

impl ColorFavorites {
    /// Maximum favorites shown in the picker's first step.
    pub const CAP: usize = 12;

    /// Toggle a color: remove it if already present, else push it to the
    /// front (most-recent-first) and trim to [`ColorFavorites::CAP`].
    pub fn toggle(&mut self, c: [u8; 3]) {
        if let Some(i) = self.0.iter().position(|x| *x == c) {
            self.0.remove(i);
        } else {
            self.0.insert(0, c);
            self.0.truncate(Self::CAP);
        }
    }

    /// True when `c` is already a favorite.
    pub fn contains(&self, c: [u8; 3]) -> bool {
        self.0.iter().any(|x| *x == c)
    }
}

/// The swatches that belong to one wheel, as `(name, r, g, b)` for the UI.
/// Sourced from [`PALETTE`]; the Python study can later supersede this with a
/// usage-derived, richer-named set written to a manifest the engine loads.
pub fn wheel_colors(w: Wheel) -> Vec<(&'static str, u8, u8, u8)> {
    PALETTE
        .iter()
        .filter(|e| e.wheel == w)
        .map(|e| (e.name, e.srgb[0], e.srgb[1], e.srgb[2]))
        .collect()
}

// ── Hexagon honeycomb color-wheel variants ──────────────────────────────────
//
// Every wheel renders in the BrickColor picker as the same 127-cell hexagon
// honeycomb (rows 7 -> 13 -> 7). The Stone wheel IS the hand-curated base
// palette ([`BASE_PALETTE`] — named, naturalistic swatches recovered from the
// original Slint picker). The other six wheels reuse the exact same 127
// `(name, x, y)` cells and apply a per-wheel HSL transform to each base color,
// so each wheel reads as a distinct, coherent thematic variant of one shared
// wheel rather than an independent procedural ring. The Slint `ColorSwatchCell`
// reads `r`/`g`/`b` (0-255) and positions the chip at the cell's `(x, y)` px
// inside the 286x264 honeycomb box.

/// One cell of a wheel's honeycomb.
#[derive(Debug, Clone, PartialEq)]
pub struct HoneycombCell {
    /// Hover label — the curated swatch name (e.g. `"Seraph Blue"`).
    pub name: String,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// Left/top px position inside the 286x264 honeycomb box.
    pub x: f32,
    pub y: f32,
}

/// A single curated base swatch: a name, an sRGB color, and a fixed `(x, y)`
/// position inside the 286x264 honeycomb box. The Stone wheel renders these
/// unchanged; the other wheels keep the name + position and transform the color.
struct Base {
    name: &'static str,
    r: u8,
    g: u8,
    b: u8,
    x: f32,
    y: f32,
}

/// The hand-curated 127-swatch base palette (recovered from the original Slint
/// BrickColor picker). Naturalistic, named colors laid out as a centred hexagon
/// honeycomb (rows 7 -> 13 -> 7). This is the canonical Stone wheel and the
/// source every other wheel transforms.
const BASE_PALETTE: [Base; 127] = [
    Base { name: "Old Growth", r: 22, g: 56, b: 34, x: 66.0, y: 0.0 },
    Base { name: "Fern Hollow", r: 42, g: 75, b: 48, x: 88.0, y: 0.0 },
    Base { name: "Hemlock", r: 30, g: 68, b: 58, x: 110.0, y: 0.0 },
    Base { name: "Deep Spruce", r: 18, g: 72, b: 65, x: 132.0, y: 0.0 },
    Base { name: "Fjord Water", r: 24, g: 78, b: 88, x: 154.0, y: 0.0 },
    Base { name: "Storm Petrel", r: 36, g: 60, b: 98, x: 176.0, y: 0.0 },
    Base { name: "Winter Night", r: 20, g: 32, b: 58, x: 198.0, y: 0.0 },
    Base { name: "Meadow Grass", r: 68, g: 120, b: 56, x: 55.0, y: 19.0 },
    Base { name: "Clover Field", r: 50, g: 130, b: 68, x: 77.0, y: 19.0 },
    Base { name: "River Moss", r: 78, g: 140, b: 95, x: 99.0, y: 19.0 },
    Base { name: "Tidal Pool", r: 38, g: 112, b: 110, x: 121.0, y: 19.0 },
    Base { name: "Lake Depth", r: 40, g: 82, b: 128, x: 143.0, y: 19.0 },
    Base { name: "Deep Current", r: 32, g: 68, b: 120, x: 165.0, y: 19.0 },
    Base { name: "Ocean Trench", r: 28, g: 56, b: 105, x: 187.0, y: 19.0 },
    Base { name: "Dusk Horizon", r: 58, g: 62, b: 98, x: 209.0, y: 19.0 },
    Base { name: "Sage Brush", r: 142, g: 152, b: 125, x: 44.0, y: 38.0 },
    Base { name: "Spring Canopy", r: 118, g: 165, b: 72, x: 66.0, y: 38.0 },
    Base { name: "Lichen", r: 130, g: 175, b: 128, x: 88.0, y: 38.0 },
    Base { name: "Glacier Melt", r: 98, g: 185, b: 172, x: 110.0, y: 38.0 },
    Base { name: "Clear Sky", r: 120, g: 180, b: 210, x: 132.0, y: 38.0 },
    Base { name: "Forget-Me-Not", r: 98, g: 140, b: 195, x: 154.0, y: 38.0 },
    Base { name: "Wisteria Bloom", r: 135, g: 130, b: 185, x: 176.0, y: 38.0 },
    Base { name: "Wild Heather", r: 135, g: 92, b: 145, x: 198.0, y: 38.0 },
    Base { name: "Elderberry", r: 110, g: 62, b: 95, x: 220.0, y: 38.0 },
    Base { name: "Olive Grove", r: 108, g: 130, b: 62, x: 33.0, y: 57.0 },
    Base { name: "New Leaf", r: 148, g: 190, b: 70, x: 55.0, y: 57.0 },
    Base { name: "Pistachio Shell", r: 140, g: 176, b: 110, x: 77.0, y: 57.0 },
    Base { name: "Seafoam", r: 110, g: 195, b: 162, x: 99.0, y: 57.0 },
    Base { name: "Shallow Lagoon", r: 72, g: 178, b: 176, x: 121.0, y: 57.0 },
    Base { name: "Raindrop", r: 160, g: 200, b: 215, x: 143.0, y: 57.0 },
    Base { name: "Lavender Sprig", r: 158, g: 130, b: 185, x: 165.0, y: 57.0 },
    Base { name: "Thistle", r: 180, g: 150, b: 178, x: 187.0, y: 57.0 },
    Base { name: "Plum Blossom", r: 185, g: 115, b: 170, x: 209.0, y: 57.0 },
    Base { name: "Dried Rose", r: 190, g: 145, b: 160, x: 231.0, y: 57.0 },
    Base { name: "Moss Stone", r: 110, g: 112, b: 52, x: 22.0, y: 76.0 },
    Base { name: "Golden Fern", r: 168, g: 185, b: 62, x: 44.0, y: 76.0 },
    Base { name: "Morning Dew", r: 185, g: 215, b: 180, x: 66.0, y: 76.0 },
    Base { name: "Creek Water", r: 82, g: 178, b: 174, x: 88.0, y: 76.0 },
    Base { name: "Glacier Blue", r: 95, g: 195, b: 210, x: 110.0, y: 76.0 },
    Base { name: "Mountain Iris", r: 96, g: 88, b: 160, x: 132.0, y: 76.0 },
    Base { name: "Harbor Blue", r: 72, g: 118, b: 155, x: 154.0, y: 76.0 },
    Base { name: "Twilight Violet", r: 108, g: 68, b: 155, x: 176.0, y: 76.0 },
    Base { name: "Dried Lavender", r: 165, g: 155, b: 175, x: 198.0, y: 76.0 },
    Base { name: "Fig Skin", r: 162, g: 120, b: 148, x: 220.0, y: 76.0 },
    Base { name: "Mulberry", r: 108, g: 48, b: 58, x: 242.0, y: 76.0 },
    Base { name: "Hay Field", r: 210, g: 210, b: 130, x: 11.0, y: 95.0 },
    Base { name: "Wheat Stalk", r: 225, g: 205, b: 160, x: 33.0, y: 95.0 },
    Base { name: "Beeswax", r: 235, g: 210, b: 100, x: 55.0, y: 95.0 },
    Base { name: "Buttercream", r: 245, g: 238, b: 200, x: 77.0, y: 95.0 },
    Base { name: "Lamb's Wool", r: 242, g: 240, b: 232, x: 99.0, y: 95.0 },
    Base { name: "Fresh Snow", r: 248, g: 246, b: 242, x: 121.0, y: 95.0 },
    Base { name: "Raw Silk", r: 238, g: 225, b: 202, x: 143.0, y: 95.0 },
    Base { name: "Petal Pink", r: 240, g: 210, b: 215, x: 165.0, y: 95.0 },
    Base { name: "Cherry Blossom", r: 235, g: 180, b: 190, x: 187.0, y: 95.0 },
    Base { name: "Wild Rose", r: 218, g: 132, b: 145, x: 209.0, y: 95.0 },
    Base { name: "Peony", r: 225, g: 158, b: 178, x: 231.0, y: 95.0 },
    Base { name: "Foxglove", r: 210, g: 115, b: 155, x: 253.0, y: 95.0 },
    Base { name: "Dijon", r: 195, g: 165, b: 55, x: 0.0, y: 114.0 },
    Base { name: "Marigold", r: 228, g: 190, b: 42, x: 22.0, y: 114.0 },
    Base { name: "Dandelion", r: 240, g: 220, b: 60, x: 44.0, y: 114.0 },
    Base { name: "Warm Butter", r: 242, g: 232, b: 162, x: 66.0, y: 114.0 },
    Base { name: "Parchment", r: 235, g: 222, b: 178, x: 88.0, y: 114.0 },
    Base { name: "Bone White", r: 245, g: 242, b: 232, x: 110.0, y: 114.0 },
    Base { name: "Linen Cloth", r: 240, g: 232, b: 220, x: 132.0, y: 114.0 },
    Base { name: "Sandstone", r: 228, g: 218, b: 198, x: 154.0, y: 114.0 },
    Base { name: "Desert Blush", r: 210, g: 178, b: 165, x: 176.0, y: 114.0 },
    Base { name: "Coral Reef", r: 218, g: 138, b: 120, x: 198.0, y: 114.0 },
    Base { name: "Pomegranate", r: 190, g: 52, b: 68, x: 220.0, y: 114.0 },
    Base { name: "Cranberry", r: 168, g: 34, b: 62, x: 242.0, y: 114.0 },
    Base { name: "Garnet", r: 148, g: 28, b: 48, x: 264.0, y: 114.0 },
    Base { name: "Raw Honey", r: 218, g: 172, b: 42, x: 11.0, y: 133.0 },
    Base { name: "Autumn Gold", r: 195, g: 160, b: 55, x: 33.0, y: 133.0 },
    Base { name: "Persimmon", r: 210, g: 120, b: 38, x: 55.0, y: 133.0 },
    Base { name: "Apricot Flesh", r: 228, g: 190, b: 155, x: 77.0, y: 133.0 },
    Base { name: "Ripe Peach", r: 235, g: 188, b: 152, x: 99.0, y: 133.0 },
    Base { name: "Terra Rosa", r: 210, g: 120, b: 82, x: 121.0, y: 133.0 },
    Base { name: "Sunburnt Clay", r: 195, g: 105, b: 78, x: 143.0, y: 133.0 },
    Base { name: "Dried Poppy", r: 185, g: 78, b: 72, x: 165.0, y: 133.0 },
    Base { name: "Barn Red", r: 155, g: 42, b: 38, x: 187.0, y: 133.0 },
    Base { name: "Brick Dust", r: 165, g: 55, b: 45, x: 209.0, y: 133.0 },
    Base { name: "Paprika", r: 180, g: 52, b: 28, x: 231.0, y: 133.0 },
    Base { name: "Salmon Run", r: 210, g: 128, b: 108, x: 253.0, y: 133.0 },
    Base { name: "Burnt Sienna", r: 175, g: 82, b: 36, x: 22.0, y: 152.0 },
    Base { name: "Pumpkin Rind", r: 195, g: 98, b: 38, x: 44.0, y: 152.0 },
    Base { name: "Cinnamon Bark", r: 180, g: 110, b: 50, x: 66.0, y: 152.0 },
    Base { name: "Adobe Wall", r: 200, g: 145, b: 108, x: 88.0, y: 152.0 },
    Base { name: "Weathered Cedar", r: 168, g: 128, b: 108, x: 110.0, y: 152.0 },
    Base { name: "Clay Pot", r: 185, g: 125, b: 95, x: 132.0, y: 152.0 },
    Base { name: "Red Sandstone", r: 148, g: 68, b: 48, x: 154.0, y: 152.0 },
    Base { name: "Madder Root", r: 155, g: 35, b: 52, x: 176.0, y: 152.0 },
    Base { name: "Dried Blood", r: 112, g: 32, b: 40, x: 198.0, y: 152.0 },
    Base { name: "Oxblood", r: 98, g: 22, b: 22, x: 220.0, y: 152.0 },
    Base { name: "Black Cherry", r: 82, g: 18, b: 28, x: 242.0, y: 152.0 },
    Base { name: "Toffee", r: 160, g: 105, b: 32, x: 33.0, y: 171.0 },
    Base { name: "Buckskin", r: 195, g: 168, b: 128, x: 55.0, y: 171.0 },
    Base { name: "Saddle Leather", r: 168, g: 130, b: 88, x: 77.0, y: 171.0 },
    Base { name: "Raw Sienna", r: 150, g: 90, b: 48, x: 99.0, y: 171.0 },
    Base { name: "Dry Sand", r: 215, g: 192, b: 148, x: 121.0, y: 171.0 },
    Base { name: "River Stone", r: 198, g: 190, b: 170, x: 143.0, y: 171.0 },
    Base { name: "Driftwood", r: 175, g: 158, b: 132, x: 165.0, y: 171.0 },
    Base { name: "Antique Bronze", r: 175, g: 115, b: 52, x: 187.0, y: 171.0 },
    Base { name: "Tarnished Copper", r: 158, g: 105, b: 58, x: 209.0, y: 171.0 },
    Base { name: "Dark Mahogany", r: 88, g: 38, b: 18, x: 231.0, y: 171.0 },
    Base { name: "Wet Earth", r: 115, g: 82, b: 55, x: 44.0, y: 190.0 },
    Base { name: "Roasted Bean", r: 95, g: 68, b: 48, x: 66.0, y: 190.0 },
    Base { name: "Hazelnut Shell", r: 155, g: 128, b: 98, x: 88.0, y: 190.0 },
    Base { name: "Fallen Chestnut", r: 130, g: 72, b: 48, x: 110.0, y: 190.0 },
    Base { name: "Black Walnut", r: 72, g: 48, b: 32, x: 132.0, y: 190.0 },
    Base { name: "Peat Soil", r: 52, g: 38, b: 25, x: 154.0, y: 190.0 },
    Base { name: "Dark Bark", r: 85, g: 55, b: 28, x: 176.0, y: 190.0 },
    Base { name: "Pine Cone", r: 110, g: 72, b: 38, x: 198.0, y: 190.0 },
    Base { name: "Acorn Cap", r: 145, g: 98, b: 58, x: 220.0, y: 190.0 },
    Base { name: "Fieldstone", r: 128, g: 125, b: 118, x: 55.0, y: 209.0 },
    Base { name: "Morning Fog", r: 178, g: 182, b: 175, x: 77.0, y: 209.0 },
    Base { name: "Dry Clay", r: 148, g: 135, b: 120, x: 99.0, y: 209.0 },
    Base { name: "Overcast", r: 118, g: 118, b: 115, x: 121.0, y: 209.0 },
    Base { name: "Dove Feather", r: 155, g: 152, b: 148, x: 143.0, y: 209.0 },
    Base { name: "Slate Cliff", r: 105, g: 115, b: 125, x: 165.0, y: 209.0 },
    Base { name: "Birch Bark", r: 198, g: 195, b: 188, x: 187.0, y: 209.0 },
    Base { name: "Wet Shale", r: 62, g: 68, b: 72, x: 209.0, y: 209.0 },
    Base { name: "Basalt", r: 52, g: 52, b: 48, x: 66.0, y: 228.0 },
    Base { name: "Starless Night", r: 15, g: 14, b: 18, x: 88.0, y: 228.0 },
    Base { name: "Soot", r: 28, g: 26, b: 24, x: 110.0, y: 228.0 },
    Base { name: "Deep Slate", r: 48, g: 55, b: 62, x: 132.0, y: 228.0 },
    Base { name: "Anthracite", r: 58, g: 58, b: 55, x: 154.0, y: 228.0 },
    Base { name: "Cast Iron", r: 68, g: 68, b: 65, x: 176.0, y: 228.0 },
    Base { name: "Volcanic Ash", r: 45, g: 48, b: 48, x: 198.0, y: 228.0 },
];

/// sRGB 0-255 -> HSL (`h` degrees, `s`/`l` in 0..1).
fn rgb_to_hsl(rgb: [u8; 3]) -> (f32, f32, f32) {
    let r = rgb[0] as f32 / 255.0;
    let g = rgb[1] as f32 / 255.0;
    let b = rgb[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d.abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if (max - r).abs() < 1e-6 {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if (max - g).abs() < 1e-6 {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    (h.rem_euclid(360.0), s.clamp(0.0, 1.0), l)
}

/// HSL (`h` degrees, `s`/`l` in 0..1) -> sRGB 0-255.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let h = h.rem_euclid(360.0);
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let to_u8 = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [to_u8(r1), to_u8(g1), to_u8(b1)]
}

/// Hue interpolation along the shorter arc.
fn hue_toward(h: f32, target: f32, t: f32) -> f32 {
    let mut d = (target - h).rem_euclid(360.0);
    if d > 180.0 {
        d -= 360.0;
    }
    (h + d * t).rem_euclid(360.0)
}

/// Apply a wheel's thematic HSL transform to one base color. Stone returns the
/// base unchanged; every other wheel is a distinct, coherent recolor of the
/// same curated swatch so the wheels read as variants of one shared palette.
fn transform_color(wheel: Wheel, rgb: [u8; 3]) -> [u8; 3] {
    // Stone is the curated palette, verbatim.
    if matches!(wheel, Wheel::Stone) {
        return rgb;
    }
    let (h, s, l) = rgb_to_hsl(rgb);
    let (h, s, l) = match wheel {
        // abstract · good — neon: brighten + heavily saturate.
        Wheel::Aether => (h, (s * 1.35 + 0.20).min(1.0), (l * 0.85 + 0.20).clamp(0.0, 0.72)),
        // light · good — pastel: lighten + desaturate toward near-white.
        Wheel::Halo => (h, s * 0.40, (l * 0.45 + 0.55).clamp(0.0, 0.96)),
        // realistic · good — living: bias hue toward green + modest desaturate.
        Wheel::Verdure => (hue_toward(h, 120.0, 0.35), (s * 0.80).clamp(0.0, 1.0), l * 0.95),
        // realistic · evil — ash/earth: darken + warm (amber) bias.
        Wheel::Char => (hue_toward(h, 28.0, 0.45), (s * 0.65).clamp(0.0, 1.0), (l * 0.55).clamp(0.0, 0.40)),
        // abstract · evil — toxic: saturate + darken + magenta/violet bias.
        Wheel::Hex => (hue_toward(h, 300.0, 0.40), (s * 1.30 + 0.15).min(1.0), (l * 0.60).clamp(0.0, 0.42)),
        // dark · evil — void: heavy darken + slight desaturate.
        Wheel::Umbra => (h, s * 0.75, (l * 0.32).clamp(0.0, 0.18)),
        Wheel::Stone => (h, s, l),
    };
    hsl_to_rgb(h, s, l)
}

/// Generate the 127-cell hexagon honeycomb for `wheel` — the curated base
/// palette ([`BASE_PALETTE`]) put through the wheel's thematic transform. Cell
/// names and `(x, y)` positions are identical across all seven wheels; only the
/// colors differ.
pub fn wheel_honeycomb(wheel: Wheel) -> Vec<HoneycombCell> {
    BASE_PALETTE
        .iter()
        .map(|b| {
            let [r, g, bl] = transform_color(wheel, [b.r, b.g, b.b]);
            HoneycombCell {
                name: b.name.to_string(),
                r,
                g,
                b: bl,
                x: b.x,
                y: b.y,
            }
        })
        .collect()
}

/// The name of the [`BASE_PALETTE`] swatch nearest to `rgb` by squared RGB
/// distance. Drives the BrickColor field's displayed value (a swatch name like
/// `"Seraph Blue"`) instead of a raw `"r, g, b"` triple.
pub fn nearest_base_name(rgb: [u8; 3]) -> &'static str {
    let mut best = BASE_PALETTE[0].name;
    let mut best_d = i32::MAX;
    for b in BASE_PALETTE.iter() {
        let dr = b.r as i32 - rgb[0] as i32;
        let dg = b.g as i32 - rgb[1] as i32;
        let db = b.b as i32 - rgb[2] as i32;
        let d = dr * dr + dg * dg + db * db;
        if d < best_d {
            best_d = d;
            best = b.name;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn honeycomb_has_127_cells_per_wheel() {
        for w in Wheel::ALL {
            assert_eq!(wheel_honeycomb(w).len(), 127, "wheel {:?}", w);
        }
    }

    #[test]
    fn honeycomb_cells_stay_in_box() {
        // The honeycomb layout box is 286x264 px (13 cols x 22px wide).
        for w in Wheel::ALL {
            for c in wheel_honeycomb(w) {
                assert!(c.x >= 0.0 && c.x <= 286.0, "x {}", c.x);
                assert!(c.y >= 0.0 && c.y <= 264.0, "y {}", c.y);
            }
        }
    }

    #[test]
    fn stone_is_the_curated_base_palette() {
        // Stone renders BASE_PALETTE verbatim — names, colors, and positions.
        let stone = wheel_honeycomb(Wheel::Stone);
        assert_eq!(stone.len(), BASE_PALETTE.len());
        for (cell, base) in stone.iter().zip(BASE_PALETTE.iter()) {
            assert_eq!(cell.name, base.name);
            assert_eq!([cell.r, cell.g, cell.b], [base.r, base.g, base.b]);
            assert_eq!((cell.x, cell.y), (base.x, base.y));
        }
    }

    #[test]
    fn wheels_share_names_and_positions() {
        // Every wheel reuses Stone's 127 names + (x, y); only colors differ.
        let stone = wheel_honeycomb(Wheel::Stone);
        for w in Wheel::ALL {
            let cells = wheel_honeycomb(w);
            for (c, s) in cells.iter().zip(stone.iter()) {
                assert_eq!(c.name, s.name);
                assert_eq!((c.x, c.y), (s.x, s.y));
            }
        }
    }

    #[test]
    fn nearest_base_name_exact_and_near() {
        // An exact base color resolves to its own name.
        assert_eq!(nearest_base_name([24, 78, 88]), "Fjord Water");
        // A near color snaps to the closest base swatch.
        assert_eq!(nearest_base_name([23, 77, 87]), "Fjord Water");
    }

    #[test]
    fn umbra_is_darker_than_halo() {
        let avg = |w: Wheel| {
            let cells = wheel_honeycomb(w);
            let sum: u32 = cells
                .iter()
                .map(|c| c.r as u32 + c.g as u32 + c.b as u32)
                .sum();
            sum as f32 / (cells.len() as f32 * 3.0)
        };
        assert!(avg(Wheel::Umbra) < avg(Wheel::Halo));
    }

    #[test]
    fn favorites_cap_and_dedupe() {
        let mut f = ColorFavorites::default();
        for i in 0..20u8 {
            f.toggle([i, 0, 0]);
        }
        assert_eq!(f.0.len(), ColorFavorites::CAP);
        let first = f.0[0];
        f.toggle(first);
        assert!(!f.contains(first));
    }

    #[test]
    fn every_wheel_yields_colors() {
        for w in Wheel::ALL {
            assert!(!wheel_colors(w).is_empty());
        }
    }
}
