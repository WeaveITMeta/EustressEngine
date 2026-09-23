//! Grass, fabric and ice.

use crate::procgen::color::*;
use crate::procgen::field::Field;
use crate::procgen::noise::*;
use crate::procgen::{Ctx, Px, Surface};
use std::f32::consts::{PI, TAU};

/// A mown lawn seen from above: about 350,000 individual blades drawn over a
/// dark thatch with a depth buffer, so the top blades win and the gaps between
/// them fall into shadow. Blades stand mostly upright, so from above each
/// shows 2-5.5 cm of foreshortened length; they are 3-5 mm wide, taper and
/// curve. Colour varies per blade and drifts toward straw in patches.
pub fn grass(ctx: &Ctx) -> Surface {
    let n = ctx.n;
    let texel = ctx.texel_m();
    let blade_palette = [
        (srgb(70, 112, 36), 3.0),
        (srgb(84, 128, 42), 3.0),
        (srgb(98, 138, 48), 2.0),
        (srgb(60, 98, 32), 2.0),
        (srgb(112, 142, 56), 1.2),
        (srgb(128, 136, 60), 0.6),
        (srgb(146, 136, 82), 0.25),
        (srgb(104, 86, 52), 0.12),
    ];
    let straw = srgb(156, 144, 88);
    let thatch = srgb(58, 52, 34);

    let mut z = vec![0.0f32; n * n];
    let mut col = vec![[0.0f32; 3]; n * n];
    let mut rough = vec![0.95f32; n * n];
    // Thatch and soil under the blades.
    let under = Surface::from_fn(ctx, |u, v| {
        let t = fbm(u, v, 128, 3, 0.5, 0x6A50);
        Px { c: scale(thatch, 1.0 + t * 0.3), h: -0.012 + t * 0.002, r: 0.95, m: 0.0 }
    });
    for i in 0..n * n {
        z[i] = under.height.data[i];
        col[i] = under.albedo[i];
    }

    let mut rng = Rng::new(0x6A55);
    let count = n * n / 12;
    for _ in 0..count {
        let mut x = rng.f() * n as f32;
        let mut y = rng.f() * n as f32;
        // Straw-toned patches are small and faint: anything broad and strong
        // would repeat visibly every tile across a large lawn.
        let dry = smoothstep(0.15, 0.5, fbm(x / n as f32, y / n as f32, 10, 3, 0.5, 0x6A01));
        let len_m = rng.range(0.02, 0.055) * (1.0 - 0.2 * dry);
        let len_px = len_m / texel;
        let theta = rng.f() * TAU;
        let bend = rng.normal() * 0.35;
        let width_px = rng.range(0.003, 0.005) / texel;
        let base_z = rng.range(-0.010, 0.0);
        let rise = rng.range(0.4, 0.9);
        let mut c = pick(&blade_palette, rng.f());
        c = mix(c, straw, dry * rng.range(0.15, 0.45));
        c = scale(c, rng.range(0.85, 1.12));
        let blade_r = rng.range(0.55, 0.72);
        let steps = (len_px * 2.0).ceil().max(2.0) as usize;
        let step = len_px / steps as f32;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let a = theta + bend * t;
            if s > 0 {
                x += a.cos() * step;
                y += a.sin() * step;
            }
            let r = (width_px * 0.5 * (1.0 - 0.85 * t.powf(1.5))).max(0.3);
            let bz = base_z + t * len_m * rise * 0.5;
            let shade = 0.72 + 0.4 * t;
            let x0 = (x - r - 1.0).floor() as isize;
            let x1 = (x + r + 1.0).ceil() as isize;
            let y0 = (y - r - 1.0).floor() as isize;
            let y1 = (y + r + 1.0).ceil() as isize;
            for py in y0..=y1 {
                for px in x0..=x1 {
                    let dx = px as f32 + 0.5 - x;
                    let dy = py as f32 + 0.5 - y;
                    let d = (dx * dx + dy * dy).sqrt();
                    if d > r + 0.5 {
                        continue;
                    }
                    let across = (d / (r + 0.5)).min(1.0);
                    let zz = bz + (1.0 - across * across) * 0.0004;
                    let idx = (py.rem_euclid(n as isize) * n as isize + px.rem_euclid(n as isize)) as usize;
                    if zz > z[idx] {
                        z[idx] = zz;
                        col[idx] = scale(c, shade * (0.9 + 0.1 * (1.0 - across)));
                        rough[idx] = blade_r;
                    }
                }
            }
        }
    }

    let height = Field { n, data: z };
    // Deep in the sward is dark whatever the local slopes say.
    let depth_occ = height.map(|h| 0.35 + 0.65 * smoothstep(-0.012, 0.004, h));
    Surface {
        albedo: col,
        height,
        roughness: Field { n, data: rough },
        metallic: Field::new(n, 0.0),
        ao_radius_m: 0.015,
        ao_strength: 1.0,
        cavity: 0.6,
        normal_strength: 0.8,
        occlusion: Some(depth_occ),
    }
}

/// Heavy plain-weave cloth: 384 warp by 384 weft threads per tile (about
/// 1 cm each), round threads that rise over and dip under their crossings by
/// varying amounts, slubs that vary thickness along each thread, heathered
/// per-thread tone, fibre fuzz running with each thread, and dark gaps between
/// them. The weave is offset half a thread so the wrap line runs mid-thread.
pub fn fabric(ctx: &Ctx) -> Surface {
    const THREADS: f32 = 384.0;
    let t_i = THREADS as i32;
    let warp_c = srgb(182, 172, 154);
    let weft_c = srgb(166, 156, 140);
    let gap_c = srgb(92, 86, 76);
    Surface::from_fn(ctx, |u, v| {
        let tx = u * THREADS + 0.5;
        let ty = v * THREADS + 0.5;
        let i = (tx.floor() as i32).rem_euclid(t_i);
        let j = (ty.floor() as i32).rem_euclid(t_i);
        let ax = tx - tx.floor() - 0.5;
        let ay = ty - ty.floor() - 0.5;
        // Slubs: per-thread base thickness plus variation along the thread.
        let warp_th = 0.74 + 0.14 * unit(hash2(i, 0, 0xFA)) + 0.08 * perlin(v * 48.0, i as f32 + 0.5, 48, t_i, 0xFB);
        let weft_th = 0.74 + 0.14 * unit(hash2(0, j, 0xFC)) + 0.08 * perlin(u * 48.0, j as f32 + 0.5, 48, t_i, 0xFD);
        // Over/under: warp i crosses over weft j when i + j is even. How far
        // a thread rises over a crossing varies crossing to crossing.
        let crossing = 0.8 + 0.4 * unit(hash2(i, j, 0xF3));
        let warp_lift = (0.5 + 0.5 * (PI * (ty - 0.5 - i.rem_euclid(2) as f32)).cos()) * crossing;
        let weft_lift = (0.5 + 0.5 * (PI * (tx - 0.5 - (j + 1).rem_euclid(2) as f32)).cos()) * crossing;
        let pw = 1.0 - (ax / (0.5 * warp_th)).powi(2);
        let pf = 1.0 - (ay / (0.5 * weft_th)).powi(2);
        let fuzz_w = fbm2(u, v, 512, 32, 2, 0.5, 0xFE);
        let fuzz_f = fbm2(u, v, 32, 512, 2, 0.5, 0xFF);
        let zw = if pw > 0.0 { warp_lift * 0.6 + pw.sqrt() * 0.4 + fuzz_w * 0.08 } else { -1.0 };
        let zf = if pf > 0.0 { weft_lift * 0.6 + pf.sqrt() * 0.4 + fuzz_f * 0.08 } else { -1.0 };
        let (c, z) = if zw < 0.0 && zf < 0.0 {
            (gap_c, -0.3)
        } else if zw >= zf {
            let tone = 0.93 + 0.14 * unit(hash2(i, 1, 0xF0));
            (scale(warp_c, tone * (0.72 + 0.28 * pw.max(0.0).sqrt()) * (1.0 + fuzz_w * 0.12)), zw)
        } else {
            let tone = 0.93 + 0.14 * unit(hash2(1, j, 0xF1));
            (scale(weft_c, tone * (0.72 + 0.28 * pf.max(0.0).sqrt()) * (1.0 + fuzz_f * 0.12)), zf)
        };
        let c = scale(c, 1.0 + fbm(u, v, 6, 4, 0.5, 0xF2) * 0.05);
        let r = if z < 0.0 { 0.98 } else { 0.9 + (fuzz_w + fuzz_f) * 0.03 };
        Px { c, h: z * 0.0016, r, m: 0.0 }
    })
    .ao(0.008, 0.9, 0.4)
}

/// Lake ice: clear blue-white with deeper-looking bluer areas, fracture lines
/// at two scales, trapped air bubbles, patches of rough white frost, and a
/// slow undulation in the surface.
pub fn ice(ctx: &Ctx) -> Surface {
    let clear = srgb(208, 224, 236);
    let deep = srgb(170, 196, 218);
    let white = srgb(236, 242, 247);
    Surface::from_fn(ctx, |u, v| {
        let depth = fbm(u, v, 3, 5, 0.55, 0x1C);
        let mut c = mix(clear, deep, smoothstep(-0.2, 0.4, depth) * 0.35);
        // Fractures follow the cell borders of a slightly warped lattice, so
        // they run straight-ish but never ruler-straight.
        let wu = u + fbm(u, v, 16, 3, 0.5, 0x25) * 0.006;
        let wv = v + fbm(u, v, 16, 3, 0.5, 0x26) * 0.006;
        let big = worley(wu, wv, 7, 7, 1.0, 0x1D);
        let w_big = 0.01 * (0.6 + 0.8 * (fbm(u, v, 48, 2, 0.5, 0x1E) + 0.5).clamp(0.0, 1.0));
        let crack_big = smoothstep(w_big, 0.0, big.f2 - big.f1);
        let small = worley(wu, wv, 26, 26, 1.0, 0x1F);
        let crack_small = smoothstep(0.012, 0.0, small.f2 - small.f1) * smoothstep(0.0, 0.3, fbm(u, v, 6, 3, 0.5, 0x20));
        let crack = (crack_big + crack_small * 0.7).min(1.0);
        let b = worley(u, v, 170, 170, 1.0, 0x21);
        let r_b = 0.08 + 0.2 * unit(hash(b.id));
        let bubble_p = 0.14 * (0.5 + smoothstep(-0.2, 0.3, fbm(u, v, 8, 3, 0.5, 0x22)));
        let (rim, core) = if unit(b.id) < bubble_p {
            (
                smoothstep(r_b, r_b * 0.7, b.f1) * smoothstep(r_b * 0.4, r_b * 0.75, b.f1),
                smoothstep(r_b * 0.7, 0.0, b.f1),
            )
        } else {
            (0.0, 0.0)
        };
        let frost = smoothstep(0.12, 0.42, fbm(u, v, 7, 4, 0.55, 0x23));
        let grain = fbm(u, v, 256, 2, 0.5, 0x24);
        c = mix(c, white, (crack * 0.7 + rim * 0.55 + core * 0.2).clamp(0.0, 1.0));
        c = mix(c, scale(white, 1.0 + grain * 0.04), frost * 0.75);
        let h = depth * 0.0005 - crack * 0.0003 + frost * (0.0001 + grain * 0.00008);
        let r = 0.04 + depth.abs() * 0.02 + crack * 0.25 + frost * (0.38 + grain * 0.08);
        Px { c, h, r: r.clamp(0.02, 1.0), m: 0.0 }
    })
    .ao(0.004, 0.3, 0.15)
}
