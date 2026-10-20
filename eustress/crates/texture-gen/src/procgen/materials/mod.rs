//! The bundled material library, one generator per textured material.
//!
//! Scale: every design assumes the engine maps one tile onto `Ctx::tile_m`
//! metres of surface (4 m, the engine's `TILE_WORLD_SIZE`), so bricks are
//! brick-sized, boards board-sized and grass blades blade-sized. Periodicity:
//! every repeating structure uses an integer count per tile and every noise
//! layer an integer lattice frequency, so each map wraps exactly.

mod masonry;
mod metal;
mod organic;
mod wood;

use super::{Ctx, Surface};

pub struct Material {
    /// Prefix of the three output files, as the `.mat.toml` files reference them.
    pub stem: &'static str,
    pub build: fn(&Ctx) -> Surface,
}

pub const LIBRARY: &[Material] = &[
    Material { stem: "brick", build: masonry::brick },
    Material { stem: "concrete", build: masonry::concrete },
    Material { stem: "granite", build: masonry::granite },
    Material { stem: "marble", build: masonry::marble },
    Material { stem: "slate", build: masonry::slate },
    Material { stem: "sand", build: masonry::sand },
    Material { stem: "wood", build: wood::wood },
    Material { stem: "wood_planks", build: wood::wood_planks },
    Material { stem: "metal", build: metal::metal },
    Material { stem: "diamond_plate", build: metal::diamond_plate },
    Material { stem: "corroded_metal", build: metal::corroded_metal },
    Material { stem: "foil", build: metal::foil },
    Material { stem: "gold", build: metal::gold },
    Material { stem: "silver", build: metal::silver },
    Material { stem: "bronze", build: metal::bronze },
    Material { stem: "grass", build: organic::grass },
    Material { stem: "fabric", build: organic::fabric },
    Material { stem: "ice", build: organic::ice },
];
