//! What a ray or contact on a road met: the road instance and the surface
//! under the hit (carriageway, shoulder, kerb, sidewalk, or paint on the
//! carriageway) with the material it is made of, so a tyre's grip and its
//! sound follow the surface.
//!
//! A designed road's collider sits on its surface entity, a junction's on
//! its root; neither is an instance. Each carries a [`RoadHitMap`]: the
//! triangles of every surface it draws, paint included, bucketed on a grid,
//! and what each surface is made of. A junction also carries
//! [`RoadJunctionRoads`], its legs' roads, so a hit in its box reports the
//! road whose leg it lies on.

use std::collections::HashMap;

use bevy::prelude::*;

use super::material::TerrainMaterial;
use super::road_design::P2;
use super::road_mesh::{MeshData, Surface};

/// Side of the grid cells a map buckets its triangles in, metres.
const HIT_CELL: f32 = 4.0;
/// How far above or below a surface a hit still lies on it, metres: the
/// collider and the drawn surface are one mesh, paint floats 12 mm over it.
const HIT_SLACK: f32 = 0.08;

/// One kind of road surface, as a hit reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitSurface {
    Carriageway,
    Shoulder,
    Kerb,
    Sidewalk,
    /// A painted marking on the carriageway (a line, crosswalk or stop bar).
    Paint,
}

impl HitSurface {
    pub fn name(self) -> &'static str {
        match self {
            Self::Carriageway => "Carriageway",
            Self::Shoulder => "Shoulder",
            Self::Kerb => "Kerb",
            Self::Sidewalk => "Sidewalk",
            Self::Paint => "Paint",
        }
    }

    fn of(surface: Surface) -> Self {
        match surface {
            Surface::Carriageway => Self::Carriageway,
            Surface::Shoulder => Self::Shoulder,
            Surface::Kerb => Self::Kerb,
            Surface::Sidewalk => Self::Sidewalk,
            Surface::MarkingWhite | Surface::MarkingYellow => Self::Paint,
        }
    }
}

/// What a hit on a road's collider lies on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadHit {
    pub surface: HitSurface,
    pub material: TerrainMaterial,
}

/// Every surface a road or junction draws, for reading what a hit lies on.
#[derive(Component, Clone, Debug, Default)]
pub struct RoadHitMap {
    positions: Vec<[f32; 3]>,
    /// Each triangle's corners and the index of its surface in `surfaces`.
    tris: Vec<([u32; 3], u8)>,
    surfaces: Vec<(HitSurface, TerrainMaterial)>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

fn cell(x: f32, z: f32) -> (i32, i32) {
    ((x / HIT_CELL).floor() as i32, (z / HIT_CELL).floor() as i32)
}

impl RoadHitMap {
    /// The map of `parts`, each surface's mesh (world positions) with the
    /// material it is made of.
    pub fn new<'a>(parts: impl IntoIterator<Item = (Surface, TerrainMaterial, &'a MeshData)>) -> Self {
        let mut map = Self::default();
        for (surface, material, mesh) in parts {
            let kind = (HitSurface::of(surface), material);
            let k = match map.surfaces.iter().position(|s| *s == kind) {
                Some(k) => k,
                None => {
                    map.surfaces.push(kind);
                    map.surfaces.len() - 1
                }
            };
            let base = map.positions.len() as u32;
            map.positions.extend_from_slice(&mesh.positions);
            for t in mesh.indices.chunks_exact(3) {
                let corners = [base + t[0], base + t[1], base + t[2]];
                let index = map.tris.len() as u32;
                map.tris.push((corners, k as u8));
                let ps = corners.map(|i| map.positions[i as usize]);
                let (lo, hi) = (
                    cell(ps.iter().map(|p| p[0]).fold(f32::MAX, f32::min), ps.iter().map(|p| p[2]).fold(f32::MAX, f32::min)),
                    cell(ps.iter().map(|p| p[0]).fold(f32::MIN, f32::max), ps.iter().map(|p| p[2]).fold(f32::MIN, f32::max)),
                );
                for x in lo.0..=hi.0 {
                    for z in lo.1..=hi.1 {
                        map.grid.entry((x, z)).or_default().push(index);
                    }
                }
            }
        }
        map
    }

    /// What lies at world point `p`, on or within [`HIT_SLACK`] of a
    /// surface: paint wherever paint covers the carriageway there, else the
    /// surface nearest in height (the kerb's face against the gutter).
    pub fn at(&self, p: Vec3) -> Option<RoadHit> {
        let mut best: Option<(f32, usize)> = None;
        for &t in self.grid.get(&cell(p.x, p.z))? {
            let (corners, k) = self.tris[t as usize];
            let [a, b, c] = corners.map(|i| self.positions[i as usize]);
            let den = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
            if den.abs() < 1e-9 {
                continue;
            }
            let w0 = ((b[2] - c[2]) * (p.x - c[0]) + (c[0] - b[0]) * (p.z - c[2])) / den;
            let w1 = ((c[2] - a[2]) * (p.x - c[0]) + (a[0] - c[0]) * (p.z - c[2])) / den;
            let w2 = 1.0 - w0 - w1;
            if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                continue;
            }
            let gap = (w0 * a[1] + w1 * b[1] + w2 * c[1] - p.y).abs();
            if gap > HIT_SLACK {
                continue;
            }
            let k = k as usize;
            let paint = self.surfaces[k].0 == HitSurface::Paint;
            let better = match best {
                None => true,
                Some((g, b)) => {
                    let best_paint = self.surfaces[b].0 == HitSurface::Paint;
                    (paint && !best_paint) || (paint == best_paint && gap < g)
                }
            };
            #[cfg(test)]
            let better = if super::mutant("g07_paint") { best.is_none_or(|(g, _)| gap < g) } else { better };
            if better {
                best = Some((gap, k));
            }
        }
        best.map(|(_, k)| RoadHit { surface: self.surfaces[k].0, material: self.surfaces[k].1 })
    }
}

/// The roads whose legs meet at a junction, each with its leg's foot,
/// outward heading (radians) and half width to its kerb (metres), so a hit
/// in the box reports the road it lies on. Refreshed whenever the roads'
/// entities change, since a junction outlives a respawned spline.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct RoadJunctionRoads(pub Vec<(Entity, P2, f64, f64)>);

impl RoadJunctionRoads {
    /// The road a hit at world `p` lies on: the leg whose corridor it is in,
    /// furthest along it; else the leg whose centreline passes nearest.
    pub fn road_at(&self, p: Vec3) -> Option<Entity> {
        let p = P2::new(f64::from(p.x), f64::from(p.z));
        let mut inside: Option<(f64, Entity)> = None;
        let mut nearest: Option<(f64, Entity)> = None;
        for &(road, foot, heading, half) in &self.0 {
            let (dx, dz) = (p.x - foot.x, p.z - foot.z);
            let along = dx * heading.cos() + dz * heading.sin();
            let across = -dx * heading.sin() + dz * heading.cos();
            if along >= 0.0 && across.abs() <= half && inside.is_none_or(|(a, _)| along > a) {
                inside = Some((along, road));
            }
            let off = if along >= 0.0 { across.abs() } else { dx.hypot(dz) };
            if nearest.is_none_or(|(d, _)| off < d) {
                nearest = Some((off, road));
            }
        }
        inside.or(nearest).map(|(_, road)| road)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10 m square of carriageway at height 0 and a white line 0.15 m wide
    /// across it 12 mm up, and a kerb strip along one edge at 0.15 m.
    fn square() -> RoadHitMap {
        let quad = |x0: f32, z0: f32, x1: f32, z1: f32, y: f32| MeshData {
            positions: vec![[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]],
            normals: vec![[0.0, 1.0, 0.0]; 4],
            uvs: vec![[0.0, 0.0]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
        };
        let road = quad(0.0, 0.0, 10.0, 10.0, 0.0);
        let line = quad(4.925, 0.0, 5.075, 10.0, 0.012);
        let kerb = quad(10.0, 0.0, 10.3, 10.0, 0.15);
        RoadHitMap::new([
            (Surface::Carriageway, TerrainMaterial::Asphalt, &road),
            (Surface::MarkingWhite, TerrainMaterial::Asphalt, &line),
            (Surface::Kerb, TerrainMaterial::Concrete, &kerb),
        ])
    }

    #[test]
    fn a_hit_reads_the_surface_under_it() {
        let map = square();
        let on = |x: f32, y: f32, z: f32| map.at(Vec3::new(x, y, z)).map(|h| (h.surface, h.material));
        assert_eq!(on(2.0, 0.0, 3.0), Some((HitSurface::Carriageway, TerrainMaterial::Asphalt)));
        assert_eq!(on(5.0, 0.0, 3.0), Some((HitSurface::Paint, TerrainMaterial::Asphalt)), "the line, though the ray met the carriageway under it");
        assert_eq!(on(10.15, 0.15, 3.0), Some((HitSurface::Kerb, TerrainMaterial::Concrete)));
        assert_eq!(on(2.0, 1.0, 3.0), None, "a metre over the road is not on it");
        assert_eq!(on(-3.0, 0.0, 3.0), None, "off the road");
    }

    #[test]
    fn a_hit_in_a_junction_reports_the_leg_it_lies_on() {
        let (a, b) = (Entity::from_raw_u32(1).unwrap(), Entity::from_raw_u32(2).unwrap());
        // Road a leaves east from (0, 0), road b north (+z) from (0, 0), both 4 m to the kerb.
        let roads = RoadJunctionRoads(vec![(a, P2::new(0.0, 0.0), 0.0, 4.0), (b, P2::new(0.0, 0.0), std::f64::consts::FRAC_PI_2, 4.0)]);
        assert_eq!(roads.road_at(Vec3::new(9.0, 0.0, 1.0)), Some(a));
        assert_eq!(roads.road_at(Vec3::new(1.0, 0.0, 9.0)), Some(b));
        assert_eq!(roads.road_at(Vec3::new(-6.0, 0.0, -1.0)), Some(a), "behind every leg: the nearest centreline");
    }
}
