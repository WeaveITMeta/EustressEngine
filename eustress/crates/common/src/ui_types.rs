//! Roblox-parity 2D UI types.
//!
//! `UDim` and `UDim2` describe a position or size as the sum of:
//! - **Scale** — a fraction of the parent's resolved pixel size (1.0 means
//!   "fill the parent's full extent on this axis", 0.5 means "half"),
//!   plus
//! - **Offset** — a pixel constant added on top of the scaled portion.
//!
//! For `BillboardGui::size` (the floating-3D-card type) the scale is
//! interpreted in studs (1.0 == 1 stud) — Roblox's BillboardGui uses
//! Scale-as-studs to make the card grow with the world rather than the
//! screen. The offset stays in canvas pixels.
//!
//! On-disk TOML form is strictly the 4-tuple
//! `[x_scale, x_offset, y_scale, y_offset]`. Legacy 2-tuple support has
//! been removed — every Position / Size in a TOML file MUST be a UDim2.

use bevy::reflect::Reflect;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One axis of a UDim2 — `value = Scale * parent_size + Offset`.
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Serialize, Deserialize, Default)]
pub struct UDim {
    pub scale: f32,
    pub offset: f32,
}

impl UDim {
    pub const fn new(scale: f32, offset: f32) -> Self { Self { scale, offset } }
    pub const fn from_offset(offset: f32) -> Self { Self { scale: 0.0, offset } }
    pub const fn from_scale(scale: f32) -> Self { Self { scale, offset: 0.0 } }
    /// Resolve to absolute pixels given the parent's pixel extent.
    pub fn to_pixels(self, parent_size: f32) -> f32 {
        self.scale * parent_size + self.offset
    }
}

/// Two-axis UDim — Roblox's `UDim2` for positions and sizes of GUI
/// elements. Round-trips through TOML as `[x_scale, x_offset, y_scale,
/// y_offset]`; the legacy `[width, height]` form (2 floats) is accepted
/// on read for backwards compatibility (those become pure-offset values).
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Default)]
pub struct UDim2 {
    pub x: UDim,
    pub y: UDim,
}

impl UDim2 {
    pub const fn new(x_scale: f32, x_offset: f32, y_scale: f32, y_offset: f32) -> Self {
        Self {
            x: UDim::new(x_scale, x_offset),
            y: UDim::new(y_scale, y_offset),
        }
    }

    /// Pure-pixels constructor — sets scale to 0 on both axes. Convenient
    /// for ports of code that previously stored `[f32; 2]` pixel sizes.
    pub const fn from_pixels(width_px: f32, height_px: f32) -> Self {
        Self::new(0.0, width_px, 0.0, height_px)
    }

    /// Pure-scale constructor — sets offset to 0 on both axes.
    pub const fn from_scale(x: f32, y: f32) -> Self {
        Self::new(x, 0.0, y, 0.0)
    }

    /// Resolve to absolute pixels given the parent's pixel extents.
    pub fn to_pixels(self, parent_w: f32, parent_h: f32) -> [f32; 2] {
        [self.x.to_pixels(parent_w), self.y.to_pixels(parent_h)]
    }

    /// Scratch-pad accessors for code that wants the raw 4-tuple.
    pub const fn as_array(&self) -> [f32; 4] {
        [self.x.scale, self.x.offset, self.y.scale, self.y.offset]
    }
}

// ── Serde: 4-tuple round-trip with 2-tuple legacy support ─────────────────

impl Serialize for UDim2 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Always serialise as the canonical 4-float array. The 2-float
        // legacy form is read-only — we don't write it back, since the
        // 4-tuple losslessly carries both Scale and Offset.
        self.as_array().serialize(s)
    }
}

impl<'de> Deserialize<'de> for UDim2 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Strict 4-float canonical form. We reject anything else loudly
        // so old 2-tuple `[w, h]` files surface an error at load instead
        // of silently round-tripping through a Scale=0 wrapper that
        // hides the schema drift.
        let v: [f32; 4] = <[f32; 4]>::deserialize(d)?;
        Ok(UDim2::new(v[0], v[1], v[2], v[3]))
    }
}
