//! Uniform cell grid for neighbour search.
//!
//! Built by a counting sort every substep. The solver permutes its particle
//! arrays into cell order after each build, so a cell's particles are one
//! contiguous index range (cache-friendly neighbour loops, and the
//! permutation is deterministic). Periodic axes wrap, with minimum-image
//! displacements.

use bevy_math::Vec3;

#[derive(Clone, Debug, Default)]
pub struct CellGrid {
    /// Actual cell edge per axis (>= the requested cell size).
    pub cell: Vec3,
    pub dims: [i32; 3],
    /// Domain minimum corner.
    pub origin: Vec3,
    pub size: Vec3,
    pub periodic: [bool; 3],
    /// Prefix sums: particles of cell `c` are `cell_start[c]..cell_start[c+1]`.
    pub cell_start: Vec<u32>,
    /// Neighbour cell offsets per axis, deduplicated for tiny periodic grids.
    offsets: [Vec<i32>; 3],
}

impl CellGrid {
    /// Bin `positions` and return the permutation that sorts them by cell.
    /// After the caller applies it, cell ranges index the arrays directly.
    pub fn build(
        &mut self,
        positions: &[Vec3],
        origin: Vec3,
        size: Vec3,
        min_cell: f32,
        periodic: [bool; 3],
    ) -> Vec<u32> {
        let min_cell = min_cell.max(size.max_element() * 1e-6).max(f32::MIN_POSITIVE);
        self.origin = origin;
        self.size = size;
        self.periodic = periodic;
        for a in 0..3 {
            // Round down so every cell is at least `min_cell` wide: then all
            // neighbours within that radius sit in the 27 adjacent cells.
            let n = (size[a] / min_cell).floor().max(1.0);
            // Keep the grid bounded no matter how small the cell requested.
            let n = n.min(1024.0) as i32;
            self.dims[a] = n;
            self.cell[a] = size[a] / n as f32;
            self.offsets[a] = match (periodic[a], n) {
                (true, 1) => vec![0],
                (true, 2) => vec![0, 1],
                _ => vec![-1, 0, 1],
            };
        }
        let ncells = (self.dims[0] * self.dims[1] * self.dims[2]) as usize;
        let mut counts = vec![0u32; ncells + 1];
        let cells: Vec<u32> = positions.iter().map(|&p| self.cell_index(p) as u32).collect();
        for &c in &cells {
            counts[c as usize + 1] += 1;
        }
        for c in 0..ncells {
            counts[c + 1] += counts[c];
        }
        self.cell_start = counts.clone();
        let mut cursor = counts;
        let mut order = vec![0u32; positions.len()];
        for (i, &c) in cells.iter().enumerate() {
            let slot = &mut cursor[c as usize];
            order[*slot as usize] = i as u32;
            *slot += 1;
        }
        order
    }

    #[inline]
    fn coord(&self, p: Vec3, a: usize) -> i32 {
        let c = ((p[a] - self.origin[a]) / self.cell[a]).floor() as i32;
        c.clamp(0, self.dims[a] - 1)
    }

    #[inline]
    pub fn cell_index(&self, p: Vec3) -> usize {
        let (x, y, z) = (self.coord(p, 0), self.coord(p, 1), self.coord(p, 2));
        ((z * self.dims[1] + y) * self.dims[0] + x) as usize
    }

    /// Minimum-image displacement a - b.
    #[inline]
    pub fn delta(&self, a: Vec3, b: Vec3) -> Vec3 {
        let mut d = a - b;
        for ax in 0..3 {
            if self.periodic[ax] {
                let l = self.size[ax];
                if d[ax] > 0.5 * l {
                    d[ax] -= l;
                } else if d[ax] < -0.5 * l {
                    d[ax] += l;
                }
            }
        }
        d
    }

    /// Visit every particle index whose cell neighbours `p`'s cell. The
    /// callback filters by distance itself (it usually needs the distance).
    #[inline]
    pub fn for_each_candidate(&self, p: Vec3, mut f: impl FnMut(usize)) {
        let c = [self.coord(p, 0), self.coord(p, 1), self.coord(p, 2)];
        for &oz in &self.offsets[2] {
            let Some(z) = self.wrap(c[2] + oz, 2) else { continue };
            for &oy in &self.offsets[1] {
                let Some(y) = self.wrap(c[1] + oy, 1) else { continue };
                for &ox in &self.offsets[0] {
                    let Some(x) = self.wrap(c[0] + ox, 0) else { continue };
                    let cell = ((z * self.dims[1] + y) * self.dims[0] + x) as usize;
                    let (s, e) = (self.cell_start[cell], self.cell_start[cell + 1]);
                    for j in s..e {
                        f(j as usize);
                    }
                }
            }
        }
    }

    #[inline]
    fn wrap(&self, v: i32, a: usize) -> Option<i32> {
        let n = self.dims[a];
        if v >= 0 && v < n {
            Some(v)
        } else if self.periodic[a] {
            Some(v.rem_euclid(n))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_exactly_the_brute_force_neighbours() {
        let size = Vec3::new(1.0, 0.6, 0.8);
        let origin = -size * 0.5;
        let mut pts = Vec::new();
        let mut s = 12345u64;
        for _ in 0..600 {
            let mut r = || {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (s >> 40) as f32 / (1u64 << 24) as f32
            };
            pts.push(origin + Vec3::new(r(), r(), r()) * size);
        }
        for periodic in [[false; 3], [true, false, true], [true; 3]] {
            let mut g = CellGrid::default();
            let order = g.build(&pts, origin, size, 0.13, periodic);
            let sorted: Vec<Vec3> = order.iter().map(|&i| pts[i as usize]).collect();
            // Rebuild on the sorted array so ranges index it directly.
            let _ = g.build(&sorted, origin, size, 0.13, periodic);
            for i in 0..sorted.len() {
                let mut found = Vec::new();
                g.for_each_candidate(sorted[i], |j| {
                    if j != i && g.delta(sorted[i], sorted[j]).length() < 0.13 {
                        found.push(j);
                    }
                });
                found.sort();
                found.dedup();
                let mut brute: Vec<usize> = (0..sorted.len())
                    .filter(|&j| j != i && g.delta(sorted[i], sorted[j]).length() < 0.13)
                    .collect();
                brute.sort();
                assert_eq!(found, brute, "periodic={periodic:?} i={i}");
            }
        }
    }
}
