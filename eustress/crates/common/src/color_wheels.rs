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

#[cfg(test)]
mod tests {
    use super::*;

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
