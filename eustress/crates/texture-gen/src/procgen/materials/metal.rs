//! Metals. Albedo here is the metal's specular colour (F0).

use crate::procgen::color::*;
use crate::procgen::field::Field;
use crate::procgen::noise::*;
use crate::procgen::raster;
use crate::procgen::{Ctx, Px, Surface};

/// Brushed steel: fine streaks along u from the abrasive, a scatter of
/// longer scratches mostly with the brushing and a few across it, and faint
/// handling smudges.
pub fn metal(ctx: &Ctx) -> Surface {
    let base = srgb(192, 194, 197);
    let scratches = raster::scratches(ctx.n, 900, 0xE1, (0.01, 0.09), (0.6, 1.1), |rng| {
        if rng.f() < 0.8 { rng.normal() * 0.05 } else { rng.f() * std::f32::consts::TAU }
    });
    Surface::from_fn(ctx, |u, v| {
        let brush = fbm2(u, v, 2, 256, 3, 0.6, 0xE2) * 0.7 + fbm2(u, v, 8, 512, 2, 0.5, 0xE3) * 0.3;
        let smudge = fbm(u, v, 4, 4, 0.5, 0xE4);
        let s = scratches.sample(u, v);
        let c = scale(base, (1.0 + brush * 0.05 + smudge * 0.025) * (1.0 + s * 0.06));
        let r = (0.34 + brush * 0.08 + smudge * 0.05 - s * 0.06).clamp(0.05, 1.0);
        let h = brush * 0.000015 - s * 0.00002;
        Px { c, h, r, m: 1.0 }
    })
    .ao(0.002, 0.05, 0.0)
}

/// Tread plate: raised lugs 6.25 cm apart alternating ±45° (64 × 64 per tile),
/// about 1.6 mm high with steep flanks and rounded crowns, each lug a little
/// different in height and wear, on a mill-finish plate with shallow dents,
/// scratches and dirt packed in around the lug bases.
pub fn diamond_plate(ctx: &Ctx) -> Surface {
    const CELLS: f32 = 64.0;
    let cells_i = CELLS as i32;
    let base = srgb(188, 191, 196);
    let dirt_c = srgb(84, 82, 76);
    let scratches = raster::scratches(ctx.n, 700, 0xF0, (0.01, 0.06), (0.6, 1.0), |rng| {
        rng.f() * std::f32::consts::TAU
    });
    Surface::from_fn(ctx, |u, v| {
        let cx = u * CELLS;
        let cy = v * CELLS;
        let ix = cx.floor();
        let iy = cy.floor();
        let lx = cx - ix - 0.5;
        let ly = cy - iy - 0.5;
        let lug_id = hash2((ix as i32).rem_euclid(cells_i), (iy as i32).rem_euclid(cells_i), 0xF5);
        let even = (ix as i32 + iy as i32).rem_euclid(2) == 0;
        let (dx, dy) = if even { (0.7071, 0.7071) } else { (0.7071, -0.7071) };
        let half_len = 0.27 + signed(hash(lug_id)) * 0.01;
        let radius = 0.085 + signed(hash(lug_id ^ 1)) * 0.005;
        let t = (lx * dx + ly * dy).clamp(-half_len, half_len);
        let qx = lx - t * dx;
        let qy = ly - t * dy;
        let dist = (qx * qx + qy * qy).sqrt();
        let sd = dist - radius;
        let crown = (1.0 - (dist / radius).powi(2)).max(0.0);
        let lug = smoothstep(0.02, -0.015, sd);
        // Worn lugs have flatter, lower crowns.
        let worn = unit(hash(lug_id ^ 2));
        let h_lug = lug * (0.0011 + 0.0002 * signed(hash(lug_id ^ 3)) + 0.0004 * crown.sqrt() * (1.0 - worn * 0.6));
        let dent_cell = worley(u, v, 9, 9, 1.0, 0xF6);
        let dent = if unit(dent_cell.id) < 0.3 { smoothstep(0.45, 0.0, dent_cell.f1).powi(2) } else { 0.0 };
        let wave = fbm(u, v, 4, 3, 0.5, 0xF7);

        let mill = fbm2(u, v, 3, 256, 3, 0.5, 0xF1);
        let dirt_mask = smoothstep(0.0, 0.3, fbm(u, v, 5, 4, 0.5, 0xF2) + 0.1);
        let near_lug = smoothstep(0.07, 0.0, sd.max(0.0)) * (1.0 - lug);
        let dirt = ((near_lug * 0.8 + fbm(u, v, 40, 3, 0.5, 0xF3).max(0.0) * 0.5) * dirt_mask).clamp(0.0, 1.0);
        let s = scratches.sample(u, v) * (1.0 - lug);
        let wear = lug * crown * (0.5 + 0.5 * worn);

        let mut c = scale(base, 1.0 + mill * 0.04 + fbm(u, v, 6, 4, 0.5, 0xF4) * 0.03);
        c = scale(c, 1.0 + wear * 0.1 + s * 0.05);
        c = mix(c, dirt_c, dirt * 0.55);
        let h = h_lug + mill * 0.00001 - s * 0.00002 - dent * 0.0006 + wave * 0.0003;
        let r = (0.38 + mill * 0.06 - wear * 0.2 + dirt * 0.35 - s * 0.04).clamp(0.05, 1.0);
        Px { c, h, r, m: 1.0 - dirt * 0.6 }
    })
    .ao(0.01, 0.8, 0.3)
}

/// Weathered steel: rust patches with crisp, ragged edges over pitted
/// mill-scale steel. Thin rust is an orange-ochre stain; thick rust is raised,
/// darker, and broken into flakes with dark cracks between them. Run-off
/// streaks trail down the face below the rust.
pub fn corroded_metal(ctx: &Ctx) -> Surface {
    let n = ctx.n;
    let steel = srgb(108, 110, 114);
    let rust_dark = srgb(66, 38, 24);
    let rust_mid = srgb(118, 60, 34);
    let rust_orange = srgb(156, 84, 42);
    let ochre = srgb(156, 114, 64);
    let rust_raw = Field::from_fn(n, |u, v| {
        fbm(u, v, 4, 6, 0.55, 0xA1) + fbm(u, v, 16, 4, 0.5, 0xA2) * 0.35 + fbm(u, v, 64, 3, 0.5, 0xAB) * 0.12
    });
    let cover_field = rust_raw.map(|m| smoothstep(0.0, 0.06, m + 0.05));
    let trail = downward_trail(&cover_field, (0.3 / ctx.texel_m()) as usize);
    Surface::from_fn(ctx, |u, v| {
        let m = rust_raw.sample(u, v) + 0.05;
        let rust = smoothstep(0.0, 0.06, m);
        let thick = smoothstep(0.08, 0.3, m);
        let streak = trail.sample(u, v) * smoothstep(-0.1, 0.35, fbm2(u, v, 128, 3, 3, 0.5, 0xA4));
        let rn = fbm(u, v, 96, 4, 0.55, 0xA5);
        let stain = mix(rust_orange, ochre, smoothstep(-0.2, 0.4, fbm(u, v, 12, 3, 0.5, 0xA6)));
        let mut scale_c = mix(rust_dark, rust_mid, smoothstep(-0.25, 0.15, rn));
        scale_c = mix(scale_c, rust_orange, smoothstep(0.15, 0.45, rn) * 0.6);
        let rust_c = mix(stain, scale_c, thick);
        // Flakes: thick rust cracks into plates, each lifted a different amount.
        let fl = worley(u, v, 150, 150, 1.0, 0xAC);
        let crack = smoothstep(0.07, 0.0, fl.f2 - fl.f1) * thick;
        let lift = unit(hash(fl.id)) * thick;
        let mill = fbm(u, v, 8, 4, 0.5, 0xA7);
        let metal_c = mix(scale(steel, 1.0 + mill * 0.12), srgb(96, 104, 116), smoothstep(0.1, 0.4, mill) * 0.4);
        let p = worley(u, v, 300, 300, 1.0, 0xA8);
        let pit = if unit(p.id) < 0.025 + 0.2 * rust * (1.0 - thick) {
            smoothstep(0.32, 0.08, p.f1)
        } else {
            0.0
        };
        let cover = (rust + streak * 0.45).clamp(0.0, 1.0);
        let mut c = mix(metal_c, rust_c, cover);
        c = scale(c, (1.0 - pit * 0.4 * (1.0 - thick)) * (1.0 - crack * 0.55));
        let grain = fbm(u, v, 256, 3, 0.55, 0xA9);
        let h = rust * 0.0002 + thick * (0.0003 + lift * 0.0005 + grain * 0.0002) - crack * 0.0003 - pit * 0.0004 * (1.0 - thick);
        let r = lerp(0.4 + mill * 0.08 + pit * 0.2, 0.84 + grain * 0.1, cover);
        Px { c, h, r: r.clamp(0.0, 1.0), m: 1.0 - cover.powf(0.8) }
    })
    .ao(0.008, 0.8, 0.4)
}

/// Average of `f` over the `len` texels above each texel (wrapping), so
/// anything in `f` leaves a trail running down the texture.
fn downward_trail(f: &Field, len: usize) -> Field {
    let n = f.n;
    let len = len.clamp(1, n);
    let mut out = Field::new(n, 0.0);
    for x in 0..n {
        let mut acc: f32 = (1..=len).map(|k| f.at(x as isize, -(k as isize))).sum();
        for y in 0..n {
            out.data[y * n + x] = acc / len as f32;
            acc += f.at(x as isize, y as isize) - f.at(x as isize, y as isize - len as isize);
        }
    }
    out
}

/// Crumpled aluminium foil: three scales of steep creased facets (each Voronoi
/// cell folds up or down to a sharp crease along its border; cells of about
/// 7, 2.7 and 1 cm). The relief is what makes foil read as foil, because each
/// facet catches the light at its own angle.
pub fn foil(ctx: &Ctx) -> Surface {
    let base = srgb(226, 228, 231);
    let tile = ctx.tile_m;
    Surface::from_fn(ctx, |u, v| {
        // Height of a folded cell in metres: slope about 0.35 on each side.
        let fold = |cell: Cell, k: u32, cells: f32| {
            let s = if unit(hash(cell.id ^ k)) < 0.6 { 1.0 } else { -0.8 };
            s * (cell.f2 - cell.f1).min(0.8) * 0.35 * tile / cells
        };
        let a = worley(u, v, 60, 60, 1.0, 0xF10);
        let b = worley(u, v, 150, 150, 1.0, 0xF11);
        let c3 = worley(u, v, 380, 380, 1.0, 0xF12);
        let h = fold(a, 1, 60.0) + fold(b, 2, 150.0) * 0.6 + fold(c3, 3, 380.0) * 0.4;
        let crease = smoothstep(0.05, 0.0, a.f2 - a.f1) + smoothstep(0.05, 0.0, b.f2 - b.f1) * 0.5;
        let c = scale(base, 1.0 - crease * 0.03 + fbm(u, v, 8, 3, 0.5, 0xF13) * 0.02);
        let r = (0.12 + crease * 0.06 + fbm(u, v, 32, 3, 0.5, 0xF14).abs() * 0.05).clamp(0.02, 1.0);
        Px { c, h, r, m: 1.0 }
    })
    .ao(0.01, 0.35, 0.1)
}

/// Polished precious metal: faint orange-peel, buffing swirls, a few fine
/// scratches, handling smudges and patches of tarnish or patina.
fn polished(ctx: &Ctx, base: Rgb, rough: f32, tarnish: Rgb, tarnish_amt: f32, seed: u32) -> Surface {
    let sw = raster::swirls(ctx.n, 2600, seed as u64, (0.01, 0.05), 0.8);
    let sc = raster::scratches(ctx.n, 260, seed as u64 ^ 0x5C, (0.005, 0.04), (0.5, 0.9), |rng| {
        rng.f() * std::f32::consts::TAU
    });
    Surface::from_fn(ctx, |u, v| {
        let peel = fbm(u, v, 64, 3, 0.5, seed ^ 2);
        let smudge = smoothstep(0.1, 0.45, fbm(u, v, 6, 4, 0.5, seed ^ 3));
        let tarn = smoothstep(0.15, 0.5, fbm(u, v, 4, 4, 0.5, seed ^ 4)) * tarnish_amt;
        let s = sw.sample(u, v);
        let k = sc.sample(u, v);
        let mut c = scale(base, 1.0 + fbm(u, v, 5, 3, 0.5, seed ^ 5) * 0.03 + s * 0.02);
        c = mix(c, tarnish, tarn);
        let r = (rough + smudge * 0.07 + s * 0.06 + k * 0.1 + tarn * 0.3).clamp(0.03, 1.0);
        let h = peel * 0.00001 - s * 0.000006 - k * 0.00001;
        Px { c, h, r, m: 1.0 }
    })
    .ao(0.001, 0.0, 0.0)
}

pub fn gold(ctx: &Ctx) -> Surface {
    polished(ctx, srgb(255, 226, 152), 0.16, srgb(214, 172, 100), 0.18, 0x601D)
}

pub fn silver(ctx: &Ctx) -> Surface {
    polished(ctx, srgb(244, 242, 236), 0.12, srgb(176, 164, 142), 0.28, 0x5117)
}

pub fn bronze(ctx: &Ctx) -> Surface {
    polished(ctx, srgb(214, 150, 92), 0.28, srgb(122, 104, 74), 0.35, 0xB20)
}
