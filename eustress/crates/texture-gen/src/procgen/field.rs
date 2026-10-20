//! Square scalar/colour buffers with wrap-around addressing, and the passes
//! that turn a height field into a normal map and an ambient-occlusion map.
//!
//! Every neighbourhood read wraps at the tile edge, so a derived map is as
//! periodic as the height it came from.

use std::thread;

/// A square `n`×`n` scalar field, row-major, `y` down the texture.
#[derive(Clone)]
pub struct Field {
    pub n: usize,
    pub data: Vec<f32>,
}

impl Field {
    pub fn new(n: usize, value: f32) -> Self {
        Field { n, data: vec![value; n * n] }
    }

    /// Fill from `f(u, v)` at texel centres, in parallel.
    pub fn from_fn(n: usize, f: impl Fn(f32, f32) -> f32 + Sync) -> Self {
        let mut data = vec![0.0; n * n];
        par_rows(&mut data, n, |y, row| {
            let v = (y as f32 + 0.5) / n as f32;
            for (x, out) in row.iter_mut().enumerate() {
                *out = f((x as f32 + 0.5) / n as f32, v);
            }
        });
        Field { n, data }
    }

    #[inline]
    pub fn at(&self, x: isize, y: isize) -> f32 {
        let n = self.n as isize;
        self.data[(y.rem_euclid(n) * n + x.rem_euclid(n)) as usize]
    }

    /// Bilinear sample at tile coordinates, wrapping.
    pub fn sample(&self, u: f32, v: f32) -> f32 {
        let x = u * self.n as f32 - 0.5;
        let y = v * self.n as f32 - 0.5;
        let x0 = x.floor();
        let y0 = y.floor();
        let fx = x - x0;
        let fy = y - y0;
        let (x0, y0) = (x0 as isize, y0 as isize);
        let a = self.at(x0, y0) + (self.at(x0 + 1, y0) - self.at(x0, y0)) * fx;
        let b = self.at(x0, y0 + 1) + (self.at(x0 + 1, y0 + 1) - self.at(x0, y0 + 1)) * fx;
        a + (b - a) * fy
    }

    pub fn map(&self, f: impl Fn(f32) -> f32 + Sync) -> Field {
        let mut out = self.clone();
        par_rows(&mut out.data, self.n, |_, row| {
            for v in row.iter_mut() {
                *v = f(*v);
            }
        });
        out
    }
}

/// Run `f(row_index, row)` over every row of an `n`-wide buffer on all cores.
pub fn par_rows<T: Send>(data: &mut [T], n: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let threads = thread::available_parallelism().map(|t| t.get()).unwrap_or(4).max(1);
    let rows = data.len() / n;
    let per = rows.div_ceil(threads).max(1);
    thread::scope(|s| {
        for (chunk_i, chunk) in data.chunks_mut(per * n).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (r, row) in chunk.chunks_mut(n).enumerate() {
                    f(chunk_i * per + r, row);
                }
            });
        }
    });
}

/// Tangent-space normal map from a height field in metres, OpenGL convention
/// (+X right, +Y up the texture, +Z out), which is what glTF and Bevy expect.
/// Image rows run down the texture, so the green channel carries `+dh/dy`.
pub fn normal_map(h: &Field, texel_m: f32, strength: f32) -> Vec<[f32; 3]> {
    let n = h.n;
    let mut out = vec![[0.0f32; 3]; n * n];
    let scale = strength / (8.0 * texel_m);
    par_rows(&mut out, n, |y, row| {
        let y = y as isize;
        for (x, o) in row.iter_mut().enumerate() {
            let x = x as isize;
            let tl = h.at(x - 1, y - 1);
            let t = h.at(x, y - 1);
            let tr = h.at(x + 1, y - 1);
            let l = h.at(x - 1, y);
            let r = h.at(x + 1, y);
            let bl = h.at(x - 1, y + 1);
            let b = h.at(x, y + 1);
            let br = h.at(x + 1, y + 1);
            let dhdx = ((tr + 2.0 * r + br) - (tl + 2.0 * l + bl)) * scale;
            let dhdy = ((bl + 2.0 * b + br) - (tl + 2.0 * t + tr)) * scale;
            let (nx, ny, nz) = (-dhdx, dhdy, 1.0);
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            *o = [nx / len, ny / len, nz / len];
        }
    });
    out
}

/// Horizon-based ambient occlusion from a height field in metres: for each
/// texel, how much of the hemisphere the surrounding relief blocks within
/// `radius_m`. 1 = open, 0 = fully enclosed.
pub fn ambient_occlusion(h: &Field, texel_m: f32, radius_m: f32, strength: f32) -> Field {
    const DIRS: usize = 12;
    const STEPS: usize = 8;
    let n = h.n;
    let radius_px = (radius_m / texel_m).max(1.5);
    // Offsets are integer texel steps, denser near the centre.
    let mut taps: Vec<Vec<(isize, isize, f32)>> = Vec::with_capacity(DIRS);
    for d in 0..DIRS {
        let a = (d as f32 + 0.37) * std::f32::consts::TAU / DIRS as f32;
        let (dx, dy) = (a.cos(), a.sin());
        let mut dir = Vec::with_capacity(STEPS);
        let mut last = (0isize, 0isize);
        for s in 0..STEPS {
            let t = (s as f32 + 1.0) / STEPS as f32;
            let dist = (radius_px * t * t).max(1.0);
            let o = ((dx * dist).round() as isize, (dy * dist).round() as isize);
            if o != last && o != (0, 0) {
                let dm = ((o.0 * o.0 + o.1 * o.1) as f32).sqrt() * texel_m;
                dir.push((o.0, o.1, dm));
                last = o;
            }
        }
        taps.push(dir);
    }
    let mut out = Field::new(n, 1.0);
    par_rows(&mut out.data, n, |y, row| {
        let y = y as isize;
        for (x, o) in row.iter_mut().enumerate() {
            let x = x as isize;
            let h0 = h.at(x, y);
            let mut occ = 0.0;
            for dir in &taps {
                let mut max_sin: f32 = 0.0;
                for &(ox, oy, dm) in dir {
                    let dh = h.at(x + ox, y + oy) - h0;
                    if dh > 0.0 {
                        let t = dh / dm;
                        max_sin = max_sin.max(t / (1.0 + t * t).sqrt());
                    }
                }
                occ += max_sin;
            }
            *o = (1.0 - strength * occ / DIRS as f32).clamp(0.0, 1.0);
        }
    });
    out
}
