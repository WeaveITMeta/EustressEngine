//! Brick, concrete and stone.

use crate::procgen::color::*;
use crate::procgen::noise::*;
use crate::procgen::{Ctx, Px, Surface};
use std::f32::consts::TAU;

/// Running-bond brickwork: 16 bricks by 48 courses per tile, i.e. a
/// 25 × 8.3 cm module on a 4 m tile (a 21.5 × 6.5 cm brick in a 1 cm joint
/// scales to the same 3:1 proportion). Bricks vary in colour, size, set and
/// tilt; edges are rounded and chipped; joints are recessed and gritty.
pub fn brick(ctx: &Ctx) -> Surface {
    const COLS: f32 = 16.0;
    const ROWS: f32 = 48.0;
    let tile_cm = ctx.tile_m * 100.0;
    let cell_w = tile_cm / COLS;
    let cell_h = tile_cm / ROWS;
    let joint = cell_h * 0.12;
    let palette = [
        (srgb(152, 66, 46), 3.0),
        (srgb(168, 82, 56), 2.5),
        (srgb(136, 56, 42), 2.0),
        (srgb(176, 100, 70), 1.2),
        (srgb(160, 92, 66), 1.0),
        (srgb(120, 50, 38), 1.0),
        (srgb(92, 42, 34), 0.35),
    ];
    let mortar_c = srgb(176, 170, 158);
    Surface::from_fn(ctx, |u, v| {
        let bv = v * ROWS;
        let row = bv.floor();
        let fv = bv - row;
        let row_i = row as i32;
        let stagger = if row_i.rem_euclid(2) == 0 { 0.0 } else { 0.5 };
        // Quarter-brick phase keeps head joints off the u = 0 line.
        let bu = u * COLS + stagger + 0.25;
        let col = bu.floor();
        let fu = bu - col;
        let col_i = (col as i32).rem_euclid(COLS as i32);
        let id = hash2(col_i, row_i, 0xB41C);

        // Hand-laid irregularity: each brick sits a little off-centre and
        // its size varies by a couple of millimetres.
        let jx = signed(hash(id ^ 1)) * cell_w * 0.007;
        let jy = signed(hash(id ^ 2)) * cell_h * 0.014;
        let sx = signed(hash(id ^ 3)) * cell_w * 0.006;
        let sy = signed(hash(id ^ 4)) * cell_h * 0.012;
        let px = fu * cell_w - (cell_w * 0.5 + jx);
        let py = fv * cell_h - (cell_h * 0.5 + jy);
        let hx = cell_w * 0.5 - joint * 0.5 + sx;
        let hy = cell_h * 0.5 - joint * 0.5 + sy;
        let radius = joint * 0.35;
        let qx = px.abs() - (hx - radius);
        let qy = py.abs() - (hy - radius);
        let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
        let inside = qx.max(qy).min(0.0);
        let chip = fbm(u, v, 256, 3, 0.5, 0xC41F) * joint * 0.35
            + fbm(u, v, 512, 2, 0.5, 0xC420) * joint * 0.12;
        // Signed distance to the brick edge, cm; negative inside the brick.
        let d = outside + inside - radius + chip;

        // Face relief (metres): per-brick tilt, gentle undulation, pits.
        let tilt = (signed(hash(id ^ 5)) * px / hx + signed(hash(id ^ 6)) * py / hy) * 0.0005;
        let undul = fbm(u, v, 96, 3, 0.5, 0xFACE) * 0.0006;
        let pc = worley(u, v, 640, 640, 1.0, 0x9175);
        let pit = if unit(pc.id) < 0.22 {
            let r = 0.18 + 0.2 * unit(hash(pc.id));
            smoothstep(r, r * 0.4, pc.f1)
        } else {
            0.0
        };
        let face_h = tilt + undul - pit * 0.0007;
        let edge = (-d / (joint * 0.45)).clamp(0.0, 1.0);
        let brick_h = face_h - 0.0032 * (1.0 - edge).powi(2);
        let grit = fbm(u, v, 512, 3, 0.55, 0x3071);
        let mortar_h = -0.0058 + grit * 0.0005;
        let h = if d < 0.0 { brick_h.max(mortar_h) } else { mortar_h };
        let t_brick = smoothstep(0.06, -0.06, d);

        let base = pick(&palette, unit(hash(id ^ 7)));
        let value = 0.9 + 0.2 * unit(hash(id ^ 8));
        let warm = signed(hash(id ^ 9)) * 0.06;
        let mottle = fbm(u, v, 48, 4, 0.55, 0x707) * 0.2 + fbm(u, v, 192, 3, 0.5, 0x708) * 0.12;
        // Kiln flashing: about a third of the bricks darken toward one end.
        let flash = if unit(hash(id ^ 10)) < 0.35 {
            let dir = if unit(hash(id ^ 11)) < 0.5 { -1.0 } else { 1.0 };
            smoothstep(0.1, 1.0, dir * px / hx) * (0.2 + 0.2 * unit(hash(id ^ 12)))
        } else {
            0.0
        };
        let mut brick_c = warmth(scale(base, value * (1.0 + mottle) * (1.0 - flash)), warm);
        let sc = worley(u, v, 780, 780, 1.0, 0x5EC);
        let speck = if unit(sc.id) < 0.3 { smoothstep(0.32, 0.12, sc.f1) } else { 0.0 };
        let speck_c = if unit(hash(sc.id)) < 0.6 { scale(brick_c, 0.55) } else { scale(brick_c, 1.35) };
        brick_c = mix(brick_c, speck_c, speck * 0.7);
        brick_c = scale(brick_c, 1.0 - pit * 0.25);
        let weather = fbm(u, v, 4, 4, 0.5, 0x5007);
        brick_c = scale(brick_c, 1.0 + weather * 0.05);
        let mortar = scale(mortar_c, 1.0 + grit * 0.18 + weather * 0.04);
        let c = mix(mortar, brick_c, t_brick);

        let r_brick = 0.8 + fbm(u, v, 128, 3, 0.5, 0x2001) * 0.08 + pit * 0.06;
        Px { c, h, r: lerp(0.94, r_brick, t_brick), m: 0.0 }
    })
    .ao(0.012, 1.1, 0.45)
}

/// Cast concrete: cloudy tone variation, fine sand grain, a little exposed
/// aggregate, bug holes where air was trapped against the form, and the odd
/// hairline crack.
pub fn concrete(ctx: &Ctx) -> Surface {
    let base = srgb(150, 148, 142);
    Surface::from_fn(ctx, |u, v| {
        let broad = fbm(u, v, 3, 5, 0.55, 0xC0);
        let blotch = fbm(u, v, 12, 4, 0.55, 0xC8);
        let mid = fbm(u, v, 24, 4, 0.5, 0xC1);
        let fine = fbm(u, v, 256, 3, 0.5, 0xC2);
        let agg = worley(u, v, 200, 200, 1.0, 0xC3);
        let agg_r = 0.25 + 0.2 * unit(hash(agg.id));
        let stone = if unit(agg.id) < 0.35 { smoothstep(agg_r, agg_r * 0.7, agg.f1) } else { 0.0 };
        let stone_tone = signed(hash(agg.id ^ 3)) * 0.12;
        let hole = worley(u, v, 280, 280, 1.0, 0xC4);
        let hole_r = 0.1 + 0.3 * unit(hash(hole.id)).powi(2);
        let void = if unit(hole.id) < 0.012 { smoothstep(hole_r, hole_r * 0.35, hole.f1) } else { 0.0 };
        let cr = worley(u, v, 6, 6, 1.0, 0xC5);
        let crack_w = 0.004 + 0.004 * (fbm(u, v, 64, 2, 0.5, 0xC6) + 0.5).clamp(0.0, 1.0);
        let crack_mask = smoothstep(0.08, 0.3, fbm(u, v, 5, 3, 0.5, 0xC7));
        let crack = smoothstep(crack_w, 0.0, cr.f2 - cr.f1) * crack_mask;

        // Tone variation lives mostly at 30 cm and below; broad tile-scale
        // shading stays faint so a large slab does not show the tile grid.
        let mut c = scale(base, 1.0 + broad * 0.08 + blotch * 0.12 + mid * 0.08 + fine * 0.07);
        c = warmth(c, broad * 0.03);
        c = scale(c, 1.0 + stone * stone_tone);
        c = scale(c, 1.0 - void * 0.15 - crack * 0.35);
        let h = mid * 0.0006 + fine * 0.00025 + stone * 0.00015 - void * 0.0012 - crack * 0.0008;
        let r = 0.86 + fine * 0.06 + broad * 0.03 + void * 0.08;
        Px { c, h, r: r.clamp(0.0, 1.0), m: 0.0 }
    })
    .ao(0.006, 0.9, 0.5)
}

/// Polished grey granite: interlocking crystals of white, grey and pink
/// feldspar, quartz and dark mica at about a centimetre, with finer dark
/// grains between them and the odd polishing pit.
pub fn granite(ctx: &Ctx) -> Surface {
    let minerals = [
        (srgb(210, 205, 198), 5.5),
        (srgb(188, 183, 177), 3.5),
        (srgb(192, 166, 158), 1.2),
        (srgb(152, 152, 156), 2.0),
        (srgb(126, 126, 132), 0.8),
        (srgb(46, 46, 50), 0.9),
    ];
    Surface::from_fn(ctx, |u, v| {
        let g = worley(u, v, 400, 400, 1.0, 0x6A);
        let mut c = pick(&minerals, unit(g.id));
        c = scale(c, 1.0 + fbm(u, v, 512, 2, 0.5, 0x6B) * 0.08 + signed(hash(g.id ^ 1)) * 0.05);
        let f = worley(u, v, 800, 800, 1.0, 0x6C);
        let dark = if unit(f.id) < 0.08 { smoothstep(0.42, 0.2, f.f1) } else { 0.0 };
        c = mix(c, srgb(34, 34, 38), dark * 0.8);
        let boundary = smoothstep(0.08, 0.0, g.f2 - g.f1);
        c = scale(c, 1.0 - boundary * 0.06);
        c = scale(c, 1.0 + fbm(u, v, 6, 4, 0.5, 0x6D) * 0.06);
        let p = worley(u, v, 500, 500, 1.0, 0x6E);
        let pit = if unit(p.id) < 0.05 { smoothstep(0.2, 0.08, p.f1) } else { 0.0 };
        let h = -boundary * 0.00003 - pit * 0.0003 + fbm(u, v, 32, 3, 0.5, 0x6F) * 0.00005;
        let r = 0.2 + boundary * 0.06 + pit * 0.4 + unit(hash(g.id ^ 2)) * 0.06;
        Px { c, h, r, m: 0.0 }
    })
    .ao(0.003, 0.4, 0.2)
}

/// Carrara-style marble, polished: warm white with grey mottling, soft wispy
/// grey veining along a turbulent network, a few sharper dark veins with
/// feathery side branches. Everything is drawn in domain-warped coordinates,
/// and the directional vein wave uses the integer wave vector (2, 3) cycles
/// per tile, so the pattern stays periodic.
pub fn marble(ctx: &Ctx) -> Surface {
    let white = srgb(238, 237, 233);
    let mottle_c = srgb(212, 212, 214);
    let soft_c = srgb(176, 178, 184);
    let dark_c = srgb(98, 102, 110);
    let feather_c = srgb(150, 152, 158);
    Surface::from_fn(ctx, |u, v| {
        let pu = u + fbm(u, v, 2, 5, 0.55, 0x3A) * 0.3;
        let pv = v + fbm(u, v, 2, 5, 0.55, 0x3B) * 0.3;
        let mottle = fbm(pu, pv, 6, 5, 0.55, 0x3C) * 0.7 + fbm(u, v, 18, 3, 0.5, 0x3D) * 0.3;
        let mut c = mix(white, mottle_c, smoothstep(-0.15, 0.4, mottle) * 0.6);
        // Soft network veining, fading in and out.
        let net = ridged(pu, pv, 3, 5, 0.55, 0x3E);
        let net_mask = smoothstep(-0.1, 0.35, fbm(u, v, 3, 3, 0.5, 0x3F));
        let soft = smoothstep(0.62, 0.9, net) * net_mask;
        // Dominant diagonal vein system.
        let s = (TAU * (2.0 * pu + 3.0 * pv + fbm(pu, pv, 4, 5, 0.55, 0x40) * 0.9)).sin();
        let band = 1.0 - s.abs();
        let main_mask = smoothstep(-0.05, 0.3, fbm(u, v, 4, 4, 0.5, 0x41));
        let halo = band.powf(5.0) * main_mask;
        let width = 18.0 + 40.0 * (fbm(u, v, 8, 3, 0.5, 0x42) + 0.5).clamp(0.0, 1.0);
        let dark = band.powf(width) * main_mask;
        // Feather veins branching off near the main veins.
        let feather = smoothstep(0.86, 0.97, ridged(pu, pv, 14, 3, 0.5, 0x43)) * halo.powf(0.5);
        c = mix(c, soft_c, (soft * 0.55 + halo * 0.45).clamp(0.0, 1.0));
        c = mix(c, feather_c, (feather * 0.55).clamp(0.0, 1.0));
        c = mix(c, dark_c, (dark * 0.9).clamp(0.0, 1.0));
        let r = 0.1 + mottle.abs() * 0.04 + dark * 0.05 + soft * 0.02;
        let h = -dark * 0.00002 + fbm(u, v, 24, 3, 0.5, 0x44) * 0.00002;
        Px { c, h, r, m: 0.0 }
    })
    .ao(0.002, 0.1, 0.05)
}

/// Natural cleft slate: cool dark grey with a faint cleavage grain, split into
/// thin laminae whose broken edges are ragged, not smooth. Layers shift tone
/// slightly; a little iron staining.
pub fn slate(ctx: &Ctx) -> Surface {
    let base = srgb(66, 70, 76);
    Surface::from_fn(ctx, |u, v| {
        // Ragged break lines: high-frequency noise perturbs the layer field
        // right where the steps are cut.
        let jag = fbm(u, v, 48, 3, 0.5, 0x51) * 0.12 + fbm(u, v, 192, 2, 0.5, 0x52) * 0.04;
        let layers = fbm(u, v, 2, 4, 0.5, 0x53) * 2.2 + fbm(u, v, 8, 3, 0.5, 0x54) * 0.4 + jag;
        let q = layers * 3.0;
        let li = q.floor();
        let lf = q - li;
        let step = li + smoothstep(0.92, 1.0, lf);
        let edge = smoothstep(0.9, 0.97, lf) * smoothstep(1.0, 0.97, lf);
        let layer_id = hash((li as i32) as u32 ^ 0x77);
        let cleave = fbm2(u, v, 96, 24, 3, 0.5, 0x55);
        let grain = fbm(u, v, 384, 2, 0.5, 0x56);
        let mut c = scale(base, 1.0 + signed(layer_id) * 0.04 + cleave * 0.05 + grain * 0.05);
        let tint = unit(hash(layer_id));
        if tint < 0.3 {
            c = mix(c, srgb(64, 74, 70), 0.3);
        } else if tint < 0.5 {
            c = mix(c, srgb(76, 68, 78), 0.25);
        }
        c = scale(c, 1.0 + fbm(u, v, 12, 3, 0.5, 0x57) * 0.06 + edge * 0.1);
        let rust = smoothstep(0.42, 0.62, fbm(u, v, 10, 3, 0.5, 0x58)) * 0.2;
        c = mix(c, srgb(104, 84, 66), rust);
        let h = step * 0.0006 + cleave * 0.00008 + grain * 0.00004;
        let r = 0.6 + cleave * 0.06 + grain * 0.04 + edge * 0.08 + rust * 0.06;
        Px { c, h, r, m: 0.0 }
    })
    .ao(0.008, 0.8, 0.3)
}

/// Dry sand: asymmetric wind ripples about 11 cm apart (a long gentle stoss
/// face and a short steep lee face) that fade in and out, fine grain, and a
/// scatter of darker and lighter grains. Ripple phase is 34u + 7v cycles.
pub fn sand(ctx: &Ctx) -> Surface {
    let base = srgb(206, 182, 140);
    Surface::from_fn(ctx, |u, v| {
        let warp = fbm(u, v, 2, 4, 0.5, 0x61) * 1.6 + fbm(u, v, 9, 3, 0.5, 0x62) * 0.25;
        let phase = 34.0 * u + 7.0 * v + warp;
        let t = phase - phase.floor();
        let ripple = if t < 0.72 { smoothstep(0.0, 0.72, t) } else { smoothstep(1.0, 0.72, t) };
        let amp = 0.6 + 0.4 * smoothstep(-0.3, 0.3, fbm(u, v, 6, 3, 0.5, 0x63));
        let grain = fbm(u, v, 512, 2, 0.6, 0x64);
        let coarse = worley(u, v, 760, 760, 1.0, 0x65);
        let dark_grain = if unit(coarse.id) < 0.035 { smoothstep(0.45, 0.2, coarse.f1) } else { 0.0 };
        let light_grain = if unit(coarse.id) > 0.97 { smoothstep(0.45, 0.2, coarse.f1) } else { 0.0 };
        let mut c = scale(
            base,
            1.0 + grain * 0.08 + (ripple - 0.5) * 0.08 * amp + fbm(u, v, 5, 4, 0.5, 0x66) * 0.035,
        );
        c = mix(c, srgb(132, 112, 86), dark_grain * 0.45);
        c = mix(c, srgb(232, 224, 208), light_grain * 0.35);
        let h = ripple * amp * 0.004 + grain * 0.0002;
        let r = 0.92 + grain * 0.04;
        Px { c, h, r, m: 0.0 }
    })
    .ao(0.01, 0.5, 0.25)
}
