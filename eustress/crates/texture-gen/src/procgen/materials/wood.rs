//! Wood: a growth-ring model of a sawn board shared by solid Wood (edge-glued
//! staves) and WoodPlanks (separate boards with gaps and butt joints). Grain
//! runs along u.
//!
//! Ring spacing is 6-12 mm, a fast-grown softwood. Finer rings would fall
//! below the 2 mm texel of a 4 m tile and alias rather than read as grain.

use crate::procgen::color::*;
use crate::procgen::noise::*;
use crate::procgen::{Ctx, Px, Surface};

/// Where a board was cut from its log.
struct Cut {
    /// Offset of the pith from the board's centre line, metres (across).
    pith: f32,
    /// Depth of the pith below the face, metres.
    depth: f32,
    /// Growth-ring spacing, metres.
    spacing: f32,
    seed: u32,
}

impl Cut {
    /// Flat-sawn boards have the pith under the face, so the face cuts the
    /// rings into long cathedral arches. Rift and quarter-sawn boards have it
    /// off to the side and near the face plane, so rings run as straight lines.
    fn new(id: u32, flat_sawn_share: f32) -> Cut {
        let flat = unit(hash(id ^ 0x11)) < flat_sawn_share;
        let side = if unit(hash(id ^ 0x16)) < 0.5 { -1.0 } else { 1.0 };
        Cut {
            pith: if flat {
                signed(hash(id ^ 0x12)) * 0.03
            } else {
                side * (0.12 + 0.25 * unit(hash(id ^ 0x12)))
            },
            depth: if flat {
                0.04 + 0.1 * unit(hash(id ^ 0x13))
            } else {
                0.005 + 0.05 * unit(hash(id ^ 0x13))
            },
            spacing: 0.006 + 0.006 * unit(hash(id ^ 0x14)),
            seed: hash(id ^ 0x15),
        }
    }

    /// Latewood density at `(u, v)` for a point `across_c` metres from the
    /// board's centre line.
    fn latewood(&self, u: f32, v: f32, across_c: f32) -> f32 {
        // The pith line rises and falls slowly along the board (about once
        // per tile), which draws the arches long and never closes them into
        // rings: the depth never reaches zero under the face.
        let depth = (self.depth + fbm2(u, v, 1, 1, 3, 0.5, self.seed) * 0.025).max(0.025);
        let wobble = fbm2(u, v, 3, 1, 3, 0.5, self.seed ^ 7) * 0.35;
        let r = ((across_c - self.pith).powi(2) + depth * depth).sqrt();
        let ring = r / self.spacing + wobble;
        let f = ring - ring.floor();
        // Earlywood darkens gradually into latewood, which ends sharply at
        // the next ring's start.
        smoothstep(0.45, 0.85, f) * smoothstep(1.0, 0.93, f)
    }
}

/// Dark pore streaks running with the grain: long along u, fine across.
fn pores(u: f32, v: f32, seed: u32) -> f32 {
    smoothstep(0.22, 0.55, fbm2(u, v, 24, 768, 2, 0.5, seed))
}

/// Solid wood: 32 edge-glued staves (12.5 cm on a 4 m tile) of one species,
/// 40 % flat-sawn and 60 % rift-sawn, tight glue lines, oiled finish. Staves
/// are offset half a width so the wrap line runs through a stave.
pub fn wood(ctx: &Ctx) -> Surface {
    const STAVES: f32 = 32.0;
    let stave_w = ctx.tile_m / STAVES;
    let palette = [
        (srgb(182, 136, 88), 3.0),
        (srgb(192, 148, 100), 2.0),
        (srgb(170, 124, 78), 2.0),
        (srgb(186, 140, 90), 1.5),
        (srgb(160, 114, 72), 1.0),
    ];
    Surface::from_fn(ctx, |u, v| {
        let sv = v * STAVES + 0.5;
        let s = sv.floor();
        let fs = sv - s;
        let id = hash2((s as i32).rem_euclid(STAVES as i32), 0, 0x5D0);
        let cut = Cut::new(id, 0.4);
        let late = cut.latewood(u, v, (fs - 0.5) * stave_w);
        let pore = pores(u, v, 0x5D1);
        let base = scale(pick(&palette, unit(hash(id ^ 1))), 0.93 + 0.14 * unit(hash(id ^ 2)));
        let along = fbm2(u, v, 6, 1, 3, 0.5, id ^ 3) * 0.08;
        let mut c = scale(base, (1.0 + along) * (1.0 - late * 0.32) * (1.0 - pore * 0.16));
        c = warmth(c, late * 0.04);
        let edge = fs.min(1.0 - fs) * stave_w;
        let glue = smoothstep(0.0005, 0.0, edge);
        c = scale(c, 1.0 - glue * 0.35);
        let h = late * 0.00004 - pore * 0.00007 - glue * 0.00012;
        let r = 0.58 + pore * 0.14 - late * 0.05 + fbm(u, v, 8, 3, 0.5, 0x5D2) * 0.05;
        Px { c, h, r, m: 0.0 }
    })
    .ao(0.004, 0.5, 0.25)
}

/// Floorboards: 24 rows of mixed-width boards (13-20 cm) along u, each row
/// split into 2-4 boards of random length with staggered butt joints, 1.2 mm
/// gaps, eased edges, slight cupping, knots in about a third of the boards,
/// satin finish. The first row is centred on the wrap line.
pub fn wood_planks(ctx: &Ctx) -> Surface {
    const ROWS: usize = 24;
    let palette = [
        (srgb(150, 104, 62), 3.0),
        (srgb(164, 118, 72), 2.5),
        (srgb(136, 92, 54), 2.0),
        (srgb(174, 130, 84), 1.5),
        (srgb(126, 84, 50), 1.2),
        (srgb(152, 106, 72), 1.0),
    ];
    let tile = ctx.tile_m;
    // Row boundaries in tile units: mixed widths that still sum to one tile.
    let mut bounds = [0.0f32; ROWS + 1];
    let mut total = 0.0;
    for (i, b) in bounds.iter_mut().enumerate().skip(1) {
        total += 0.8 + 0.4 * unit(hash2(i as i32, 0, 0xD5));
        *b = total;
    }
    for b in bounds.iter_mut() {
        *b /= total;
    }
    let phase_v = bounds[1] * 0.5;
    Surface::from_fn(ctx, |u, v| {
        let tv = (v + phase_v).rem_euclid(1.0);
        let mut row = 0;
        while row + 1 < ROWS && tv >= bounds[row + 1] {
            row += 1;
        }
        let board_w = (bounds[row + 1] - bounds[row]) * tile;
        let across = (tv - bounds[row]) * tile;
        let rh = hash2(row as i32, 0, 0xD0);
        let k = 2 + (rh % 3) as usize;
        let phase_u = unit(hash(rh ^ 1));
        let mut cuts = [0.0f32; 5];
        let mut sum = 0.0;
        for i in 0..k {
            sum += 0.6 + unit(hash(rh ^ (10 + i as u32)));
            cuts[i + 1] = sum;
        }
        for c in cuts.iter_mut().take(k + 1) {
            *c /= sum;
        }
        let t = (u - phase_u).rem_euclid(1.0);
        let mut seg = 0;
        while seg + 1 < k && t >= cuts[seg + 1] {
            seg += 1;
        }
        let along = (t - cuts[seg]) * tile;
        let len = (cuts[seg + 1] - cuts[seg]) * tile;
        let id = hash2(row as i32, seg as i32, 0xD1);
        let cut = Cut::new(id, 0.65);

        let mut across_c = across - board_w * 0.5;
        // Knot: rings swirl around it and its core darkens.
        let mut knot = 0.0;
        if unit(hash(id ^ 0x20)) < 0.33 {
            let ka = (0.15 + 0.7 * unit(hash(id ^ 0x21))) * len;
            let kb = signed(hash(id ^ 0x22)) * 0.3 * board_w;
            let kr = 0.006 + 0.008 * unit(hash(id ^ 0x23));
            let dx = (along - ka) / 1.6;
            let dy = across_c - kb;
            let d = (dx * dx + dy * dy).sqrt();
            let pull = (-(d / (kr * 3.5)).powi(2)).exp();
            across_c += dy.signum() * pull * kr * 2.5;
            knot = smoothstep(kr, kr * 0.4, d);
        }
        let late = cut.latewood(u, v, across_c);
        let pore = pores(u, v, 0xD2);

        let base = scale(pick(&palette, unit(hash(id ^ 1))), 0.9 + 0.2 * unit(hash(id ^ 2)));
        let base = warmth(base, signed(hash(id ^ 3)) * 0.04);
        let along_var = fbm2(u, v, 8, 2, 3, 0.5, id ^ 4) * 0.08;
        let mut c = scale(base, (1.0 + along_var) * (1.0 - late * 0.32) * (1.0 - pore * 0.18));
        c = mix(c, scale(base, 0.38), knot * 0.85);

        let edge = across.min(board_w - across).min(along).min(len - along);
        let gap = smoothstep(0.0009, 0.0005, edge);
        let ease = smoothstep(0.0028, 0.0009, edge);
        let cup = ((across / board_w - 0.5) * 2.0).powi(2);
        let mut h = -0.0004 * cup + late * 0.00004 - pore * 0.00006 - ease * ease * 0.0012;
        h = h * (1.0 - gap) + gap * -0.004;
        c = mix(c, scale(base, 0.22), gap);
        let wear = fbm(u, v, 6, 4, 0.5, 0xD3) * 0.06;
        let r = (0.44 + pore * 0.16 - late * 0.04 + wear + knot * 0.1) * (1.0 - gap) + gap * 0.92;
        Px { c, h, r, m: 0.0 }
    })
    .ao(0.008, 0.9, 0.4)
}
