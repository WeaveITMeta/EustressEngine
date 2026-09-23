//! Fluid boundaries as particles (Akinci et al. 2012, "Versatile rigid-fluid
//! coupling for incompressible SPH").
//!
//! The reflecting domain walls and every obstacle surface are sampled into
//! one set of boundary particles. Each carries a volume V_b = 1 / sum_k W_bk
//! over its boundary neighbours, which corrects for uneven sampling (edges,
//! corners, overlapping bodies), so a fluid particle sees
//!
//!   rho_i += sum_b rho0 V_b W_ib
//!   a_i   -= sum_b rho0 V_b (p_i / rho_i^2) grad W_ib
//!
//! The reaction of each term is the force the fluid exerts on that body.

use bevy_math::Vec3;
use rayon::prelude::*;

use super::grid::CellGrid;
use super::params::{Boundary, Obstacle, ObstacleShape, SimParams};
use crate::kernel::CubicSpline;

/// `owner` of a boundary particle that belongs to a domain wall.
pub const WALL: u32 = u32::MAX;

/// Hard cap on boundary samples (a huge part crossing a small domain is
/// clipped to the domain first, so this only bounds pathological cases).
const MAX_SAMPLES: usize = 400_000;

#[derive(Clone, Debug, Default)]
pub struct BoundarySet {
    pub pos: Vec<Vec3>,
    /// Akinci volume V_b (m^3).
    pub vol: Vec<f32>,
    /// Surface velocity (m/s).
    pub vel: Vec<Vec3>,
    /// Obstacle index, or [`WALL`].
    pub owner: Vec<u32>,
    pub grid: CellGrid,
}

impl BoundarySet {
    pub fn is_empty(&self) -> bool {
        self.pos.is_empty()
    }

    /// Resample walls and obstacles at `spacing` and recompute volumes.
    pub fn rebuild(&mut self, params: &SimParams, obstacles: &[Obstacle], spacing: f32, kernel: CubicSpline) {
        let half = params.half();
        let mut pos = Vec::new();
        let mut vel = Vec::new();
        let mut owner = Vec::new();
        sample_walls(params, spacing, &mut pos);
        vel.resize(pos.len(), Vec3::ZERO);
        owner.resize(pos.len(), WALL);
        let reach = half + Vec3::splat(kernel.h);
        for (oi, o) in obstacles.iter().enumerate() {
            let start = pos.len();
            sample_obstacle(o, spacing, reach, &mut pos);
            // Keep only what a fluid particle inside the domain can feel.
            let mut w = start;
            for r in start..pos.len() {
                if pos[r].abs().cmple(reach).all() {
                    pos[w] = pos[r];
                    w += 1;
                }
            }
            pos.truncate(w);
            vel.resize(pos.len(), o.velocity);
            owner.resize(pos.len(), oi as u32);
            if pos.len() > MAX_SAMPLES {
                pos.truncate(MAX_SAMPLES);
                vel.truncate(MAX_SAMPLES);
                owner.truncate(MAX_SAMPLES);
                break;
            }
        }
        let periodic = [
            params.boundary[0] == Boundary::Periodic,
            params.boundary[1] == Boundary::Periodic,
            params.boundary[2] == Boundary::Periodic,
        ];
        let order = self.grid.build(&pos, -half, params.domain_size, kernel.h, periodic);
        self.pos = order.iter().map(|&i| pos[i as usize]).collect();
        self.vel = order.iter().map(|&i| vel[i as usize]).collect();
        self.owner = order.iter().map(|&i| owner[i as usize]).collect();
        let h2 = kernel.h * kernel.h;
        let (grid, bpos) = (&self.grid, &self.pos);
        self.vol = (0..bpos.len())
            .into_par_iter()
            .map(|b| {
                let xb = bpos[b];
                let mut sum = 0.0f32;
                grid.for_each_candidate(xb, |k| {
                    let r2 = grid.delta(xb, bpos[k]).length_squared();
                    if r2 < h2 {
                        sum += kernel.w(r2.sqrt());
                    }
                });
                if sum > 0.0 { 1.0 / sum } else { 0.0 }
            })
            .collect();
    }
}

/// Points on a 2D lattice over [-a, a] x [-b, b] (inclusive ends).
fn face_lattice(a: f32, b: f32, s: f32) -> (usize, usize) {
    let na = ((2.0 * a / s).round() as usize).max(1) + 1;
    let nb = ((2.0 * b / s).round() as usize).max(1) + 1;
    (na, nb)
}

/// Reflecting domain faces. A point on an edge or corner is produced once,
/// by the lowest-index axis whose face contains it.
fn sample_walls(params: &SimParams, s: f32, out: &mut Vec<Vec3>) {
    let half = params.half();
    let reflect = |a: usize| params.boundary[a] == Boundary::Reflect;
    let counts: [usize; 3] = [0, 1, 2].map(|a| ((params.domain_size[a] / s).round() as usize).max(1) + 1);
    let coord = |a: usize, i: usize| -half[a] + params.domain_size[a] * i as f32 / (counts[a] - 1) as f32;
    for a in 0..3 {
        if !reflect(a) {
            continue;
        }
        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
        for side in [0, counts[a] - 1] {
            for ib in 0..counts[b] {
                for ic in 0..counts[c] {
                    let on_b_face = (ib == 0 || ib == counts[b] - 1) && reflect(b) && b < a;
                    let on_c_face = (ic == 0 || ic == counts[c] - 1) && reflect(c) && c < a;
                    if on_b_face || on_c_face {
                        continue;
                    }
                    let mut p = Vec3::ZERO;
                    p[a] = coord(a, side);
                    p[b] = coord(b, ib);
                    p[c] = coord(c, ic);
                    out.push(p);
                }
            }
        }
    }
}

/// Surface samples of an obstacle, limited to the box |p| <= reach (domain
/// coordinates) so large bodies are only sampled where the fluid can be.
fn sample_obstacle(o: &Obstacle, s: f32, reach: Vec3, out: &mut Vec<Vec3>) {
    let to_domain = |local: Vec3| o.center + o.rotation * local;
    // Bounding sphere of the reachable region, in the obstacle's frame, to
    // skip lattice rows that cannot land inside it.
    let inv = o.rotation.inverse();
    let local_center = inv * (-o.center);
    let local_radius = reach.length();
    let e = o.half_extents;
    match o.shape {
        ObstacleShape::Box => {
            // Index window of a face lattice that can fall within the
            // reachable sphere (so a 500 m plate costs only its patch).
            let window = |axis: usize, n: usize| -> (usize, usize) {
                let step = 2.0 * e[axis] / (n - 1).max(1) as f32;
                if step <= 0.0 {
                    return (0, n - 1);
                }
                let lo = ((local_center[axis] - local_radius + e[axis]) / step).ceil().max(0.0) as usize;
                let hi = (((local_center[axis] + local_radius + e[axis]) / step).floor().max(0.0) as usize).min(n - 1);
                (lo, hi)
            };
            for a in 0..3 {
                let (b, c) = ((a + 1) % 3, (a + 2) % 3);
                let (nb, nc) = face_lattice(e[b], e[c], s);
                let (b_lo, b_hi) = window(b, nb);
                let (c_lo, c_hi) = window(c, nc);
                if b_lo > b_hi || c_lo > c_hi {
                    continue;
                }
                for sign in [-1.0f32, 1.0] {
                    if (sign * e[a] - local_center[a]).abs() > local_radius {
                        continue;
                    }
                    for ib in b_lo..=b_hi {
                        let vb = -e[b] + 2.0 * e[b] * ib as f32 / (nb - 1).max(1) as f32;
                        for ic in c_lo..=c_hi {
                            // Edges shared with a lower-index face come from that face.
                            let edge_b = (ib == 0 || ib == nb - 1) && b < a;
                            let edge_c = (ic == 0 || ic == nc - 1) && c < a;
                            if edge_b || edge_c {
                                continue;
                            }
                            let vc = -e[c] + 2.0 * e[c] * ic as f32 / (nc - 1).max(1) as f32;
                            let mut p = Vec3::ZERO;
                            p[a] = sign * e[a];
                            p[b] = vb;
                            p[c] = vc;
                            out.push(to_domain(p));
                        }
                    }
                }
            }
        }
        ObstacleShape::Sphere => {
            let r = e.x.max(0.0);
            let n = ((4.0 * std::f32::consts::PI * r * r / (s * s)).round() as usize).clamp(8, 100_000);
            let golden = std::f32::consts::PI * (3.0 - 5.0f32.sqrt());
            for i in 0..n {
                let y = 1.0 - 2.0 * (i as f32 + 0.5) / n as f32;
                let rad = (1.0 - y * y).max(0.0).sqrt();
                let th = golden * i as f32;
                out.push(to_domain(Vec3::new(rad * th.cos(), y, rad * th.sin()) * r));
            }
        }
        ObstacleShape::Cylinder => {
            let (r, hh) = (e.x.max(0.0), e.y.max(0.0));
            let rings = ((2.0 * hh / s).round() as usize).max(1) + 1;
            let around = ((std::f32::consts::TAU * r / s).round() as usize).max(6);
            for iy in 0..rings {
                let y = -hh + 2.0 * hh * iy as f32 / (rings - 1).max(1) as f32;
                if (y - local_center.y).abs() > local_radius {
                    continue;
                }
                for k in 0..around {
                    let th = std::f32::consts::TAU * k as f32 / around as f32;
                    out.push(to_domain(Vec3::new(r * th.cos(), y, r * th.sin())));
                }
            }
            // Caps: concentric rings inside the rim.
            let radial = (r / s).round() as usize;
            for sign in [-1.0f32, 1.0] {
                if (sign * hh - local_center.y).abs() > local_radius {
                    continue;
                }
                out.push(to_domain(Vec3::new(0.0, sign * hh, 0.0)));
                for ir in 1..radial {
                    let rr = r * ir as f32 / radial as f32;
                    let n = ((std::f32::consts::TAU * rr / s).round() as usize).max(6);
                    for k in 0..n {
                        let th = std::f32::consts::TAU * k as f32 / n as f32;
                        out.push(to_domain(Vec3::new(rr * th.cos(), sign * hh, rr * th.sin())));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_math::Quat;

    /// Walls sampled with no duplicate points, and every volume positive.
    #[test]
    fn wall_samples_are_unique_with_positive_volume() {
        let params = SimParams { domain_size: Vec3::new(0.2, 0.3, 0.1), ..Default::default() };
        let s = 0.01;
        let mut set = BoundarySet::default();
        set.rebuild(&params, &[], s, CubicSpline::new(2.4 * s));
        let mut keys: Vec<[i64; 3]> = set
            .pos
            .iter()
            .map(|p| [(p.x / s * 10.0).round() as i64, (p.y / s * 10.0).round() as i64, (p.z / s * 10.0).round() as i64])
            .collect();
        let n = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), n, "duplicate wall samples");
        // Surface area / s^2, roughly.
        let area = 2.0 * (0.2 * 0.3 + 0.3 * 0.1 + 0.2 * 0.1);
        let expect = area / (s * s);
        assert!((n as f32 - expect).abs() < 0.1 * expect, "{n} samples vs ~{expect}");
        assert!(set.vol.iter().all(|&v| v > 0.0));
    }

    /// A large part crossing the domain is only sampled where it overlaps.
    #[test]
    fn big_obstacle_is_clipped_to_the_domain() {
        let params = SimParams { domain_size: Vec3::splat(0.5), boundary: [Boundary::Periodic; 3], ..Default::default() };
        let plate = Obstacle {
            shape: ObstacleShape::Box,
            center: Vec3::new(0.0, -0.3, 0.0),
            rotation: Quat::IDENTITY,
            half_extents: Vec3::new(256.0, 0.1, 256.0),
            velocity: Vec3::ZERO,
        };
        let s = 0.02;
        let mut set = BoundarySet::default();
        set.rebuild(&params, &[plate], s, CubicSpline::new(2.4 * s));
        assert!(!set.is_empty());
        assert!(set.pos.len() < 10_000, "{} samples: not clipped", set.pos.len());
    }
}
