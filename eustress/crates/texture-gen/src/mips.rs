//! Mip chains for material maps.
//!
//! Each level is a 2×2 box filter of the one above it, done in the space that
//! suits the map:
//!
//! * colour maps are averaged in linear light, so a mip of fine bright-on-dark
//!   detail (mortar lines, grain) keeps its brightness instead of darkening;
//! * normal maps are averaged as vectors and re-normalised, so distant surfaces
//!   keep unit-length normals at full 8-bit precision;
//! * data maps (roughness, metallic, occlusion, depth) are averaged as-is.
//!
//! Odd dimensions clamp the second tap to the last row/column.

use std::sync::OnceLock;

/// How a map's texels are filtered between levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MapKind {
    /// sRGB-encoded colour: base colour, emissive.
    Color,
    /// Tangent-space normal map, XYZ packed into RGB.
    Normal,
    /// Linear data: metallic/roughness, occlusion, depth.
    Data,
}

/// Levels in a full chain down to 1×1.
pub fn mip_level_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// Appends every level below `level0` (tightly packed RGBA8, `width`×`height`)
/// and returns the whole chain with its level count. Level 0 comes first,
/// then each smaller level, which is the layout wgpu expects for a
/// single-layer texture created with that many mip levels.
pub fn build_mip_chain_rgba8(
    mut chain: Vec<u8>,
    width: u32,
    height: u32,
    kind: MapKind,
) -> (Vec<u8>, u32) {
    let levels = mip_level_count(width, height);
    let expected = width as usize * height as usize * 4;
    if chain.len() != expected || levels <= 1 {
        return (chain, 1);
    }
    let total: usize = (0..levels)
        .map(|l| {
            let (w, h) = level_size(width, height, l);
            w as usize * h as usize * 4
        })
        .sum();
    chain.reserve_exact(total - chain.len());

    let mut src_start = 0usize;
    let (mut sw, mut sh) = (width as usize, height as usize);
    for level in 1..levels {
        let (dw, dh) = level_size(width, height, level);
        let (dw, dh) = (dw as usize, dh as usize);
        let src_len = sw * sh * 4;
        let dst_start = chain.len();
        chain.resize(dst_start + dw * dh * 4, 0);
        let (head, dst) = chain.split_at_mut(dst_start);
        let src = &head[src_start..src_start + src_len];
        downsample(src, sw, sh, dst, dw, dh, kind);
        src_start = dst_start;
        sw = dw;
        sh = dh;
    }
    (chain, levels)
}

fn level_size(width: u32, height: u32, level: u32) -> (u32, u32) {
    ((width >> level).max(1), (height >> level).max(1))
}

fn downsample(src: &[u8], sw: usize, sh: usize, dst: &mut [u8], dw: usize, dh: usize, kind: MapKind) {
    let lut = srgb_to_linear_lut();
    for y in 0..dh {
        let y0 = (y * 2).min(sh - 1);
        let y1 = (y * 2 + 1).min(sh - 1);
        for x in 0..dw {
            let x0 = (x * 2).min(sw - 1);
            let x1 = (x * 2 + 1).min(sw - 1);
            let taps = [
                (y0 * sw + x0) * 4,
                (y0 * sw + x1) * 4,
                (y1 * sw + x0) * 4,
                (y1 * sw + x1) * 4,
            ];
            let o = (y * dw + x) * 4;
            match kind {
                MapKind::Color => {
                    for c in 0..3 {
                        let sum: f32 = taps.iter().map(|&t| lut[src[t + c] as usize]).sum();
                        dst[o + c] = linear_to_srgb_u8(sum * 0.25);
                    }
                    dst[o + 3] = avg_u8(src, &taps, 3);
                }
                MapKind::Normal => {
                    let mut v = [0.0f32; 3];
                    for &t in &taps {
                        for c in 0..3 {
                            v[c] += src[t + c] as f32 * (2.0 / 255.0) - 1.0;
                        }
                    }
                    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                    let v = if len > 1e-6 { [v[0] / len, v[1] / len, v[2] / len] } else { [0.0, 0.0, 1.0] };
                    for c in 0..3 {
                        dst[o + c] = ((v[c] * 0.5 + 0.5) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                    }
                    dst[o + 3] = avg_u8(src, &taps, 3);
                }
                MapKind::Data => {
                    for c in 0..4 {
                        dst[o + c] = avg_u8(src, &taps, c);
                    }
                }
            }
        }
    }
}

#[inline]
fn avg_u8(src: &[u8], taps: &[usize; 4], c: usize) -> u8 {
    let sum: u32 = taps.iter().map(|&t| src[t + c] as u32).sum();
    ((sum + 2) / 4) as u8
}

fn srgb_to_linear_lut() -> &'static [f32; 256] {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut t = [0.0f32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = srgb_to_linear(i as f32 / 255.0);
        }
        t
    })
}

/// sRGB transfer function, decode direction.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB transfer function, encode direction.
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn linear_to_srgb_u8(c: f32) -> u8 {
    static LUT: OnceLock<Vec<u8>> = OnceLock::new();
    const STEPS: usize = 4096;
    let lut = LUT.get_or_init(|| {
        (0..=STEPS)
            .map(|i| (linear_to_srgb(i as f32 / STEPS as f32) * 255.0 + 0.5) as u8)
            .collect()
    });
    lut[(c.clamp(0.0, 1.0) * STEPS as f32 + 0.5) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_count_matches_wgpu() {
        assert_eq!(mip_level_count(2048, 2048), 12);
        assert_eq!(mip_level_count(1, 1), 1);
        assert_eq!(mip_level_count(1000, 700), 10);
    }

    #[test]
    fn chain_has_every_level_and_ends_at_one_texel() {
        let w = 64u32;
        let h = 16u32;
        let base = vec![128u8; (w * h * 4) as usize];
        let (chain, levels) = build_mip_chain_rgba8(base, w, h, MapKind::Data);
        assert_eq!(levels, 7);
        let expected: usize = (0..levels)
            .map(|l| ((w >> l).max(1) * (h >> l).max(1) * 4) as usize)
            .sum();
        assert_eq!(chain.len(), expected);
        assert!(chain.iter().all(|&b| b == 128));
    }

    #[test]
    fn colour_mips_average_in_linear_light() {
        // A black/white checker must average to linear 0.5 (sRGB 188), not
        // sRGB 128, or every distant bright-on-dark detail goes grey-dark.
        let mut base = Vec::new();
        for y in 0..2 {
            for x in 0..2 {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                base.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let (chain, levels) = build_mip_chain_rgba8(base, 2, 2, MapKind::Color);
        assert_eq!(levels, 2);
        let px = &chain[16..20];
        assert!((px[0] as i32 - 188).abs() <= 1, "got {}", px[0]);
    }

    #[test]
    fn normal_mips_stay_unit_length() {
        // Two opposite 45° tilts average to straight up, re-normalised.
        let tilt = |x: f32| (((x * 0.5 + 0.5) * 255.0).round()) as u8;
        let a = [tilt(0.7071), 128, tilt(0.7071), 255];
        let b = [tilt(-0.7071), 128, tilt(0.7071), 255];
        let mut base = Vec::new();
        base.extend_from_slice(&a);
        base.extend_from_slice(&b);
        base.extend_from_slice(&a);
        base.extend_from_slice(&b);
        let (chain, _) = build_mip_chain_rgba8(base, 2, 2, MapKind::Normal);
        let px = &chain[16..20];
        assert!((px[0] as i32 - 128).abs() <= 1);
        assert!(px[2] >= 254);
    }
}
