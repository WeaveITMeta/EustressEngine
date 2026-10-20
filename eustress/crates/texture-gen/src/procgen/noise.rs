//! Periodic noise.
//!
//! Every function takes tile coordinates `u, v` in `[0, 1)` and an integer
//! lattice frequency, and wraps its lattice at that frequency. The value at
//! `u = 1` is therefore exactly the value at `u = 0`, for every octave and
//! every layer built on top, which is what makes the generated maps tile
//! without a seam.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

/// Highest lattice frequency (cycles per tile) a fractal octave may reach.
/// Octaves above it would be finer than the texels can hold and only add
/// per-texel sparkle, so the fractal sums stop before them. Set from the
/// output resolution by [`set_resolution`].
static MAX_FREQ: AtomicU32 = AtomicU32::new(820);

/// Cap octaves at 0.4 cycles per texel (2.5 texels per cycle).
pub fn set_resolution(n: usize) {
    MAX_FREQ.store(((n as f32) * 0.4) as u32, Ordering::Relaxed);
}

#[inline]
fn max_freq() -> i32 {
    MAX_FREQ.load(Ordering::Relaxed) as i32
}

#[inline]
pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

#[inline]
pub fn hash2(ix: i32, iy: i32, seed: u32) -> u32 {
    hash((ix as u32) ^ hash((iy as u32) ^ hash(seed)))
}

/// Uniform in `[0, 1)`.
#[inline]
pub fn unit(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// Uniform in `[-1, 1)`.
#[inline]
pub fn signed(h: u32) -> f32 {
    unit(h) * 2.0 - 1.0
}

fn gradients() -> &'static [[f32; 2]; 256] {
    static G: OnceLock<[[f32; 2]; 256]> = OnceLock::new();
    G.get_or_init(|| {
        let mut g = [[0.0; 2]; 256];
        for (i, v) in g.iter_mut().enumerate() {
            let a = (i as f32 + 0.5) * std::f32::consts::TAU / 256.0;
            *v = [a.cos(), a.sin()];
        }
        g
    })
}

#[inline]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Gradient noise on a lattice that wraps every `px` cells in x and `py` in y.
/// Roughly in `[-1, 1]`.
pub fn perlin(x: f32, y: f32, px: i32, py: i32, seed: u32) -> f32 {
    let xf = x.floor();
    let yf = y.floor();
    let ix = xf as i32;
    let iy = yf as i32;
    let fx = x - xf;
    let fy = y - yf;
    let g = gradients();
    let corner = |i: i32, j: i32, dx: f32, dy: f32| {
        let h = hash2(i.rem_euclid(px), j.rem_euclid(py), seed);
        let gr = g[(h & 255) as usize];
        gr[0] * dx + gr[1] * dy
    };
    let n00 = corner(ix, iy, fx, fy);
    let n10 = corner(ix + 1, iy, fx - 1.0, fy);
    let n01 = corner(ix, iy + 1, fx, fy - 1.0);
    let n11 = corner(ix + 1, iy + 1, fx - 1.0, fy - 1.0);
    let a = fade(fx);
    let b = fade(fy);
    let top = n00 + (n10 - n00) * a;
    let bottom = n01 + (n11 - n01) * a;
    (top + (bottom - top) * b) * std::f32::consts::SQRT_2
}

/// Fractal sum of periodic Perlin layers starting at `fx`×`fy` cycles per
/// tile and doubling each octave. Normalised to roughly `[-1, 1]`; typical
/// values sit within ±0.5.
pub fn fbm2(u: f32, v: f32, fx: u32, fy: u32, octaves: u32, gain: f32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let (mut ax, mut ay) = (fx as i32, fy as i32);
    for o in 0..octaves {
        if o > 0 && ax.max(ay) > max_freq() {
            break;
        }
        sum += amp * perlin(u * ax as f32, v * ay as f32, ax, ay, seed.wrapping_add(o.wrapping_mul(0x9E37_79B9)));
        norm += amp;
        amp *= gain;
        ax *= 2;
        ay *= 2;
    }
    sum / norm
}

#[inline]
pub fn fbm(u: f32, v: f32, freq: u32, octaves: u32, gain: f32, seed: u32) -> f32 {
    fbm2(u, v, freq, freq, octaves, gain, seed)
}

/// Ridged fractal: sharp creases where the underlying noise crosses zero.
/// In `[0, 1]`, 1 on the ridge.
pub fn ridged(u: f32, v: f32, freq: u32, octaves: u32, gain: f32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let mut f = freq as i32;
    for o in 0..octaves {
        if o > 0 && f > max_freq() {
            break;
        }
        let n = perlin(u * f as f32, v * f as f32, f, f, seed.wrapping_add(o.wrapping_mul(0x9E37_79B9)));
        let r = 1.0 - n.abs();
        sum += amp * r * r;
        norm += amp;
        amp *= gain;
        f *= 2;
    }
    sum / norm
}

/// Result of a cellular (Worley) lookup, distances in cell units.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    /// Distance to the nearest feature point.
    pub f1: f32,
    /// Distance to the second nearest.
    pub f2: f32,
    /// Stable hash of the nearest point's cell (wrapped), for per-cell values.
    pub id: u32,
    /// Offset from the nearest feature point to the sample, cell units.
    pub dx: f32,
    pub dy: f32,
}

/// Cellular noise with `nx`×`ny` cells per tile, wrapping at the tile edge.
/// `jitter` in `[0, 1]` scatters each feature point inside its cell.
pub fn worley(u: f32, v: f32, nx: u32, ny: u32, jitter: f32, seed: u32) -> Cell {
    let (nxi, nyi) = (nx as i32, ny as i32);
    let x = u * nx as f32;
    let y = v * ny as f32;
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let mut best = Cell { f1: f32::MAX, f2: f32::MAX, id: 0, dx: 0.0, dy: 0.0 };
    for j in -1..=1 {
        for i in -1..=1 {
            let cx = ix + i;
            let cy = iy + j;
            let h = hash2(cx.rem_euclid(nxi), cy.rem_euclid(nyi), seed);
            let px = cx as f32 + 0.5 + jitter * (unit(h) - 0.5);
            let py = cy as f32 + 0.5 + jitter * (unit(hash(h)) - 0.5);
            let dx = x - px;
            let dy = y - py;
            let d = (dx * dx + dy * dy).sqrt();
            if d < best.f1 {
                best.f2 = best.f1;
                best.f1 = d;
                best.id = hash(h ^ 0x51ED_270B);
                best.dx = dx;
                best.dy = dy;
            } else if d < best.f2 {
                best.f2 = d;
            }
        }
    }
    best
}

/// SplitMix64, for deterministic scatter (blades, scratches, bubbles).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 * (1.0 / 16_777_216.0)
    }

    /// Uniform in `[a, b)`.
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }

    /// Approximately normal (Irwin–Hall, 4 draws), mean 0, sd ~1.
    pub fn normal(&mut self) -> f32 {
        (self.f() + self.f() + self.f() + self.f() - 2.0) * 1.732
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perlin_wraps_exactly() {
        for i in 0..50 {
            let y = i as f32 / 50.0 * 7.0;
            for f in [1i32, 3, 7, 64] {
                let a = perlin(0.0, y, f, 7, 9);
                let b = perlin(f as f32, y, f, 7, 9);
                assert!((a - b).abs() < 1e-5, "freq {f}: {a} vs {b}");
            }
        }
    }

    #[test]
    fn worley_wraps_exactly() {
        for i in 0..50 {
            let v = i as f32 / 50.0;
            let a = worley(0.0, v, 17, 9, 1.0, 3);
            let b = worley(1.0, v, 17, 9, 1.0, 3);
            assert!((a.f1 - b.f1).abs() < 1e-4);
            assert_eq!(a.id, b.id);
        }
    }
}
