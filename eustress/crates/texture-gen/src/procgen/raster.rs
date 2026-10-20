//! Thin wrapped marks drawn into a field: scratches and buffing swirls.
//! Every stamp wraps at the tile edge, so a mark that runs off one side
//! continues on the other.

use super::field::Field;
use super::noise::Rng;

/// Stamp a soft disc of `radius` texels at `(x, y)`, keeping the max.
pub fn stamp_max(f: &mut Field, x: f32, y: f32, radius: f32, value: f32) {
    let n = f.n as isize;
    let r = radius.max(0.35);
    let x0 = (x - r - 1.0).floor() as isize;
    let x1 = (x + r + 1.0).ceil() as isize;
    let y0 = (y - r - 1.0).floor() as isize;
    let y1 = (y + r + 1.0).ceil() as isize;
    for py in y0..=y1 {
        for px in x0..=x1 {
            let dx = px as f32 + 0.5 - x;
            let dy = py as f32 + 0.5 - y;
            let d = (dx * dx + dy * dy).sqrt();
            let cov = (r + 0.5 - d).clamp(0.0, 1.0);
            if cov <= 0.0 {
                continue;
            }
            let i = (py.rem_euclid(n) * n + px.rem_euclid(n)) as usize;
            let v = value * cov;
            if v > f.data[i] {
                f.data[i] = v;
            }
        }
    }
}

/// Straight scratches. `angle` returns each scratch's direction in radians
/// (0 = along u); lengths are in tile units. Intensity fades in and out
/// along the scratch so the ends taper.
pub fn scratches(
    n: usize,
    count: usize,
    seed: u64,
    len: (f32, f32),
    width_px: (f32, f32),
    mut angle: impl FnMut(&mut Rng) -> f32,
) -> Field {
    let mut f = Field::new(n, 0.0);
    let mut rng = Rng::new(seed);
    for _ in 0..count {
        let x = rng.f() * n as f32;
        let y = rng.f() * n as f32;
        let a = angle(&mut rng);
        let l = rng.range(len.0, len.1) * n as f32;
        let w = rng.range(width_px.0, width_px.1);
        let strength = rng.range(0.35, 1.0);
        let steps = (l * 2.0).ceil().max(2.0) as usize;
        let (dx, dy) = (a.cos(), a.sin());
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let fade = (t * (1.0 - t) * 4.0).min(1.0).powf(0.5);
            stamp_max(&mut f, x + dx * l * t, y + dy * l * t, w * 0.5, strength * fade);
        }
    }
    f
}

/// Short circular arcs, the swirl marks buffing leaves on polished metal.
/// Radii are in tile units.
pub fn swirls(n: usize, count: usize, seed: u64, radius: (f32, f32), width_px: f32) -> Field {
    let mut f = Field::new(n, 0.0);
    let mut rng = Rng::new(seed);
    for _ in 0..count {
        let cx = rng.f() * n as f32;
        let cy = rng.f() * n as f32;
        let r = rng.range(radius.0, radius.1) * n as f32;
        let a0 = rng.f() * std::f32::consts::TAU;
        let sweep = rng.range(0.25, 1.1) * if rng.f() < 0.5 { -1.0 } else { 1.0 };
        let strength = rng.range(0.3, 1.0);
        let steps = (r * sweep.abs() * 2.0).ceil().max(2.0) as usize;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let a = a0 + sweep * t;
            let fade = (t * (1.0 - t) * 4.0).min(1.0).powf(0.5);
            stamp_max(&mut f, cx + r * a.cos(), cy + r * a.sin(), width_px * 0.5, strength * fade);
        }
    }
    f
}
