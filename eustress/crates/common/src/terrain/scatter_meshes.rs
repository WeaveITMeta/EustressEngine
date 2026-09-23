//! Procedural meshes for terrain scatter: grass tufts, shrubs, rocks, and
//! conifer and broadleaf trees.
//!
//! The repository ships no vegetation or rock assets (only the primitive part
//! GLBs), so every built-in scatter kind is built here from code: a handful of
//! seeded variants per family, each with per-vertex normals and colours, so
//! one white `StandardMaterial` per family draws them all and the merged
//! batches of `scatter` can concatenate them freely. A `TerrainScatter` whose
//! Kind is Custom draws a mesh asset instead and never comes here.
//!
//! Every mesh stands on its origin with +Y up, sized at scale 1 as the
//! constants below say, and reaches [`GROUND_SINK`] below the origin so it
//! still meets ground that a coarser terrain LOD draws a little off the LOD-0
//! surface it was placed on. Colours are linear RGBA. The builders are pure
//! functions of the variant: the same variant is the same mesh on every
//! machine.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::realism::particle_sim::rng::SimRng;

/// Height of a grass tuft at scale 1, metres (its tallest blade reaches a
/// little past it).
pub const GRASS_HEIGHT: f32 = 0.45;
/// Height of a shrub at scale 1, metres.
pub const SHRUB_HEIGHT: f32 = 1.1;
/// Width of a rock at scale 1, metres. A rock scaled past
/// `scatter::LARGE_ROCK_SIZE` over this is placed as its own entity.
pub const ROCK_DIAMETER: f32 = 1.0;
/// How far every mesh reaches below its origin at scale 1, metres.
pub const GROUND_SINK: f32 = 0.08;

/// Variants built per family. Scatter picks one per instance by its seed.
pub const GRASS_VARIANTS: u8 = 4;
pub const SHRUB_VARIANTS: u8 = 3;
pub const ROCK_VARIANTS: u8 = 5;
pub const CONIFER_VARIANTS: u8 = 2;
pub const BROADLEAF_VARIANTS: u8 = 2;

/// The part of a tree trunk below its canopy at scale 1: what a tree's
/// capsule collider covers, since a body walking into a tree meets the trunk.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrunkShape {
    /// Trunk radius at the ground, metres.
    pub radius: f32,
    /// Height of the bare trunk, metres.
    pub height: f32,
}

/// A conifer's bare trunk: its lowest tier starts above it.
pub const CONIFER_TRUNK: TrunkShape = TrunkShape { radius: 0.22, height: 2.0 };
/// A broadleaf tree's bare trunk: its canopy lobes start above it.
pub const BROADLEAF_TRUNK: TrunkShape = TrunkShape { radius: 0.26, height: 3.0 };

/// One built-in scatter mesh: a family and one of its variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScatterMesh {
    Grass(u8),
    Shrub(u8),
    Rock(u8),
    Conifer(u8),
    Broadleaf(u8),
}

impl ScatterMesh {
    /// Build the mesh data. Variants past a family's count wrap around.
    pub fn build(self) -> ScatterMeshData {
        match self {
            Self::Grass(v) => grass_tuft(v % GRASS_VARIANTS),
            Self::Shrub(v) => shrub(v % SHRUB_VARIANTS),
            Self::Rock(v) => rock(v % ROCK_VARIANTS),
            Self::Conifer(v) => conifer(v % CONIFER_VARIANTS),
            Self::Broadleaf(v) => broadleaf(v % BROADLEAF_VARIANTS),
        }
    }

    /// The bare trunk of a tree mesh, `None` for anything else.
    pub fn trunk(self) -> Option<TrunkShape> {
        match self {
            Self::Conifer(_) => Some(CONIFER_TRUNK),
            Self::Broadleaf(_) => Some(BROADLEAF_TRUNK),
            _ => None,
        }
    }

    /// Every variant of every family, for building them all up front.
    pub fn all() -> impl Iterator<Item = ScatterMesh> {
        (0..GRASS_VARIANTS)
            .map(Self::Grass)
            .chain((0..SHRUB_VARIANTS).map(Self::Shrub))
            .chain((0..ROCK_VARIANTS).map(Self::Rock))
            .chain((0..CONIFER_VARIANTS).map(Self::Conifer))
            .chain((0..BROADLEAF_VARIANTS).map(Self::Broadleaf))
    }
}

/// Triangle-list mesh data with a normal and a linear RGBA colour per
/// vertex: what the builders produce and the merged batches concatenate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScatterMeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl ScatterMeshData {
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    fn vertex(&mut self, position: Vec3, normal: Vec3, color: [f32; 4]) -> u32 {
        let index = self.positions.len() as u32;
        self.positions.push(position.to_array());
        self.normals.push(normal.try_normalize().unwrap_or(Vec3::Y).to_array());
        self.colors.push(color);
        index
    }

    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend([a, b, c]);
    }

    /// Append `other` placed by `transform`, whose scale must be uniform (a
    /// non-uniform scale would need the inverse transpose for the normals),
    /// with its colours' RGB multiplied by `tint`.
    pub fn append_transformed(&mut self, other: &ScatterMeshData, transform: &Transform, tint: f32) {
        let base = self.positions.len() as u32;
        self.positions.extend(other.positions.iter().map(|p| transform.transform_point(Vec3::from(*p)).to_array()));
        self.normals.extend(
            other
                .normals
                .iter()
                .map(|n| (transform.rotation * Vec3::from(*n)).try_normalize().unwrap_or(Vec3::Y).to_array()),
        );
        self.colors.extend(other.colors.iter().map(|c| [c[0] * tint, c[1] * tint, c[2] * tint, c[3]]));
        self.indices.extend(other.indices.iter().map(|i| base + i));
    }

    /// The Bevy mesh of this data.
    pub fn into_mesh(self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Linear RGBA of an sRGB colour.
fn srgb(r: f32, g: f32, b: f32) -> [f32; 4] {
    let c = Color::srgb(r, g, b).to_linear();
    [c.red, c.green, c.blue, 1.0]
}

fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, 1.0]
}

fn shade(c: [f32; 4], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

/// The generator of one variant of one family, so two families' variant 0
/// differ.
fn variant_rng(family: u64, variant: u8) -> SimRng {
    SimRng::new(family, u64::from(variant))
}

/// A unit direction uniform on the sphere.
fn random_direction(rng: &mut SimRng) -> Vec3 {
    let z = rng.uniform() * 2.0 - 1.0;
    let a = rng.uniform() * TAU;
    let r = (1.0 - z * z).max(0.0).sqrt();
    Vec3::new(r * a.cos(), z, r * a.sin())
}

/// Unit icosphere: shared vertices on the sphere and counter-clockwise
/// triangles seen from outside, each face split `subdivisions` times.
fn icosphere(subdivisions: u32) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let t = (1.0 + 5f32.sqrt()) * 0.5;
    let mut vertices: Vec<Vec3> = [
        (-1.0, t, 0.0),
        (1.0, t, 0.0),
        (-1.0, -t, 0.0),
        (1.0, -t, 0.0),
        (0.0, -1.0, t),
        (0.0, 1.0, t),
        (0.0, -1.0, -t),
        (0.0, 1.0, -t),
        (t, 0.0, -1.0),
        (t, 0.0, 1.0),
        (-t, 0.0, -1.0),
        (-t, 0.0, 1.0),
    ]
    .into_iter()
    .map(|(x, y, z)| Vec3::new(x, y, z).normalize())
    .collect();
    let mut faces: Vec<[u32; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..subdivisions {
        let mut midpoints: HashMap<(u32, u32), u32> = HashMap::new();
        let mut midpoint = |a: u32, b: u32, vertices: &mut Vec<Vec3>| -> u32 {
            let key = (a.min(b), a.max(b));
            *midpoints.entry(key).or_insert_with(|| {
                let middle = ((vertices[a as usize] + vertices[b as usize]) * 0.5).normalize();
                vertices.push(middle);
                (vertices.len() - 1) as u32
            })
        };
        let mut next = Vec::with_capacity(faces.len() * 4);
        for [a, b, c] in faces {
            let ab = midpoint(a, b, &mut vertices);
            let bc = midpoint(b, c, &mut vertices);
            let ca = midpoint(c, a, &mut vertices);
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        faces = next;
    }
    (vertices, faces)
}

/// Append an ellipsoid lobe: a unit icosphere scaled by `radii` and moved to
/// `center`, smooth shaded, coloured by `color(point, normal)`.
fn lobe(
    out: &mut ScatterMeshData,
    subdivisions: u32,
    center: Vec3,
    radii: Vec3,
    color: impl Fn(Vec3, Vec3) -> [f32; 4],
) {
    let (vertices, faces) = icosphere(subdivisions);
    let base = out.positions.len() as u32;
    for v in &vertices {
        let point = center + *v * radii;
        // An ellipsoid's normal is the sphere's divided by the radii.
        let normal = (*v / radii).try_normalize().unwrap_or(*v);
        out.vertex(point, normal, color(point, normal));
    }
    for [a, b, c] in faces {
        out.triangle(base + a, base + b, base + c);
    }
}

/// Append a tapered, capless tube around +Y from `y0` (radius `r0`) to `y1`
/// (radius `r1`) with `sides` faces, smooth shaded.
#[allow(clippy::too_many_arguments)]
fn tube(out: &mut ScatterMeshData, sides: u32, y0: f32, r0: f32, y1: f32, r1: f32, bottom: [f32; 4], top: [f32; 4]) {
    let base = out.positions.len() as u32;
    // The side leans in by (r0 - r1) over the height; its normal leans up.
    let lean = (r0 - r1) / (y1 - y0).max(1e-4);
    for (y, r, color) in [(y0, r0, bottom), (y1, r1, top)] {
        for i in 0..sides {
            let a = i as f32 / sides as f32 * TAU;
            let (sin, cos) = a.sin_cos();
            out.vertex(Vec3::new(cos * r, y, sin * r), Vec3::new(cos, lean, sin), color);
        }
    }
    for i in 0..sides {
        let j = (i + 1) % sides;
        let (b0, b1, t0, t1) = (base + i, base + j, base + sides + i, base + sides + j);
        out.triangle(b0, t0, b1);
        out.triangle(b1, t0, t1);
    }
}

/// Append a cone around +Y: its base ring of `radius` at `y0`, its apex at
/// `y1`, `sides` faces, a flat cap underneath. Coloured `rim` at the base
/// ring and `tip` at the apex.
fn cone(out: &mut ScatterMeshData, sides: u32, y0: f32, y1: f32, radius: f32, rim: [f32; 4], tip: [f32; 4]) {
    let height = (y1 - y0).max(1e-4);
    let base = out.positions.len() as u32;
    // Side normals: perpendicular to the slant, (cos * h, r, sin * h).
    for i in 0..sides {
        let a = i as f32 / sides as f32 * TAU;
        let (sin, cos) = a.sin_cos();
        out.vertex(Vec3::new(cos * radius, y0, sin * radius), Vec3::new(cos * height, radius, sin * height), rim);
    }
    // One apex per face, its normal half way round, so the tip shades
    // without a pinch.
    for i in 0..sides {
        let a = (i as f32 + 0.5) / sides as f32 * TAU;
        let (sin, cos) = a.sin_cos();
        out.vertex(Vec3::new(0.0, y1, 0.0), Vec3::new(cos * height, radius, sin * height), tip);
    }
    for i in 0..sides {
        let j = (i + 1) % sides;
        out.triangle(base + i, base + sides + i, base + j);
    }
    // The cap, facing down, so a tier seen from below is not hollow.
    let cap = out.positions.len() as u32;
    let under = shade(rim, 0.6);
    for i in 0..sides {
        let a = i as f32 / sides as f32 * TAU;
        let (sin, cos) = a.sin_cos();
        out.vertex(Vec3::new(cos * radius, y0, sin * radius), Vec3::NEG_Y, under);
    }
    let centre = out.vertex(Vec3::new(0.0, y0, 0.0), Vec3::NEG_Y, under);
    for i in 0..sides {
        let j = (i + 1) % sides;
        out.triangle(cap + i, cap + j, centre);
    }
}

// ============================================================================
// Families
// ============================================================================

/// A tuft of curved, tapering blades leaning out from a common root, dark at
/// the base and lighter at the tips. Drawn double sided.
fn grass_tuft(variant: u8) -> ScatterMeshData {
    const SEGMENTS: u32 = 3;
    let mut rng = variant_rng(0x6752_4153, variant);
    let mut out = ScatterMeshData::default();
    let blades = 5 + u32::from(variant % 3);
    let root_colour = srgb(0.16, 0.30, 0.08);
    let tip_colour = srgb(0.55, 0.68, 0.24);
    for blade in 0..blades {
        let heading = blade as f32 / blades as f32 * TAU + rng.uniform() * 0.8;
        let (sin, cos) = heading.sin_cos();
        let out_dir = Vec3::new(cos, 0.0, sin);
        let side = Vec3::new(-sin, 0.0, cos);
        let root = out_dir * (rng.uniform() * 0.06);
        let height = GRASS_HEIGHT * (0.7 + rng.uniform() * 0.5);
        let half_width = 0.018 + rng.uniform() * 0.012;
        let lean = 0.15 + rng.uniform() * 0.25;
        let tint = 0.85 + rng.uniform() * 0.3;
        let first = out.positions.len() as u32;
        for s in 0..=SEGMENTS {
            let t = s as f32 / SEGMENTS as f32;
            // The blade bends further out the higher it gets.
            let centre = root + out_dir * (lean * height * t * t) + Vec3::Y * (-GROUND_SINK + (height + GROUND_SINK) * t);
            let tangent = (out_dir * (2.0 * lean * height * t) + Vec3::Y * (height + GROUND_SINK)).normalize();
            // Mostly up: lit like the ground under it rather than by the
            // blade's own facing, which flips as the view moves round it.
            let normal = (tangent.cross(side).normalize() * 0.35 + Vec3::Y * 0.65).normalize();
            let w = half_width * (1.0 - t) + 0.002;
            let colour = shade(mix(root_colour, tip_colour, t), tint);
            out.vertex(centre - side * w, normal, colour);
            out.vertex(centre + side * w, normal, colour);
        }
        for s in 0..SEGMENTS {
            let (l0, r0) = (first + s * 2, first + s * 2 + 1);
            let (l1, r1) = (l0 + 2, r0 + 2);
            out.triangle(l0, r0, r1);
            out.triangle(l0, r1, l1);
        }
    }
    out
}

/// A low, rounded bush of a few overlapping lobes.
fn shrub(variant: u8) -> ScatterMeshData {
    let mut rng = variant_rng(0x5348_5242, variant);
    let mut out = ScatterMeshData::default();
    let lobes = 3 + u32::from(variant);
    let dark = srgb(0.12, 0.26, 0.09);
    let light = srgb(0.30, 0.48, 0.16);
    for i in 0..lobes {
        let a = i as f32 / lobes as f32 * TAU + rng.uniform() * 0.9;
        let spread = if i == 0 { 0.0 } else { 0.18 + rng.uniform() * 0.2 };
        let r = SHRUB_HEIGHT * (0.28 + rng.uniform() * 0.14);
        let center = Vec3::new(a.cos() * spread, r * 0.75 + rng.uniform() * 0.15, a.sin() * spread);
        let tint = 0.85 + rng.uniform() * 0.3;
        lobe(&mut out, 1, center, Vec3::new(r, r * 0.85, r), |point, normal| {
            // Darker low down and on the undersides.
            let up = (point.y / SHRUB_HEIGHT).clamp(0.0, 1.0) * 0.6 + normal.y.max(0.0) * 0.4;
            shade(mix(dark, light, up), tint)
        });
    }
    out
}

/// A lumpy, flattened, faceted boulder: an icosphere pushed out by a few
/// seeded bumps, about a quarter of its height below the origin.
fn rock(variant: u8) -> ScatterMeshData {
    let mut rng = variant_rng(0x524F_434B, variant);
    let (vertices, faces) = icosphere(1);
    let bumps: Vec<(Vec3, f32)> = (0..6).map(|_| (random_direction(&mut rng), rng.uniform() * 0.4 - 0.18)).collect();
    let squash = 0.55 + rng.uniform() * 0.25;
    let stretch = 0.85 + rng.uniform() * 0.3;
    let radius = ROCK_DIAMETER * 0.5;
    let lift = radius * squash * 0.5;
    let shaped: Vec<Vec3> = vertices
        .iter()
        .map(|v| {
            let push: f32 = bumps.iter().map(|(dir, amount)| amount * v.dot(*dir).max(0.0).powi(3)).sum();
            // Bumps that overlap could otherwise turn a side inside out.
            let p = *v * (1.0 + push.clamp(-0.3, 0.5)) * radius;
            Vec3::new(p.x * stretch, p.y * squash + lift, p.z)
        })
        .collect();
    let warm = rng.uniform();
    let base = mix(srgb(0.42, 0.42, 0.44), srgb(0.50, 0.45, 0.39), warm);
    let mut out = ScatterMeshData::default();
    // One vertex per corner per face: flat facets read as rock.
    for [a, b, c] in faces {
        let (pa, pb, pc) = (shaped[a as usize], shaped[b as usize], shaped[c as usize]);
        let normal = (pb - pa).cross(pc - pa);
        let centre_height = (pa.y + pb.y + pc.y) / 3.0;
        let k = 0.7 + 0.3 * (centre_height / (radius * 1.5)).clamp(0.0, 1.0) + (rng.uniform() - 0.5) * 0.12;
        let colour = shade(base, k);
        let ia = out.vertex(pa, normal, colour);
        let ib = out.vertex(pb, normal, colour);
        let ic = out.vertex(pc, normal, colour);
        out.triangle(ia, ib, ic);
    }
    out
}

/// A spruce-like tree: a tapered trunk and stacked cones narrowing upward.
fn conifer(variant: u8) -> ScatterMeshData {
    let mut rng = variant_rng(0x434F_4E49, variant);
    let mut out = ScatterMeshData::default();
    let bark_low = srgb(0.24, 0.16, 0.10);
    let bark_high = srgb(0.32, 0.22, 0.14);
    let height = 9.0 + f32::from(variant) * 1.5 + rng.uniform();
    let canopy_start = CONIFER_TRUNK.height;
    // The trunk runs up into the canopy, so no gap shows between the tiers.
    tube(&mut out, 7, -GROUND_SINK * 3.0, CONIFER_TRUNK.radius, height * 0.7, 0.07, bark_low, bark_high);
    let tiers = 4 + u32::from(variant);
    let needle_dark = srgb(0.07, 0.20, 0.10);
    let needle_light = srgb(0.16, 0.36, 0.17);
    let span = height - canopy_start;
    for tier in 0..tiers {
        let f = tier as f32 / tiers as f32;
        let y0 = canopy_start + span * f * 0.78;
        let y1 = (y0 + span * (0.42 - f * 0.12)).min(height);
        let radius = 2.3 * (1.0 - f * 0.72) * (0.9 + rng.uniform() * 0.2);
        let tint = 0.9 + rng.uniform() * 0.2;
        cone(&mut out, 9, y0, y1, radius, shade(needle_dark, tint), shade(needle_light, tint));
    }
    out
}

/// A round-crowned tree: a tapered trunk under a cluster of canopy lobes.
fn broadleaf(variant: u8) -> ScatterMeshData {
    let mut rng = variant_rng(0x4252_4F41, variant);
    let mut out = ScatterMeshData::default();
    let bark_low = srgb(0.28, 0.20, 0.13);
    let bark_high = srgb(0.36, 0.27, 0.18);
    let crown = BROADLEAF_TRUNK.height + 2.4 + f32::from(variant) * 0.6;
    tube(&mut out, 7, -GROUND_SINK * 3.0, BROADLEAF_TRUNK.radius, crown, 0.1, bark_low, bark_high);
    let leaf_dark = srgb(0.12, 0.30, 0.09);
    let leaf_light = srgb(0.34, 0.55, 0.18);
    let lobes = 5 + u32::from(variant);
    for i in 0..lobes {
        let a = i as f32 / lobes as f32 * TAU + rng.uniform() * 0.7;
        let spread = if i == 0 { 0.0 } else { 1.0 + rng.uniform() * 0.7 };
        let r = 1.4 + rng.uniform() * 0.6;
        let center = Vec3::new(a.cos() * spread, crown + (rng.uniform() - 0.3) * 1.2, a.sin() * spread);
        let tint = 0.88 + rng.uniform() * 0.24;
        lobe(&mut out, 1, center, Vec3::new(r, r * 0.8, r), |_, normal| {
            shade(mix(leaf_dark, leaf_light, normal.y * 0.5 + 0.5), tint)
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_valid(mesh: ScatterMesh, data: &ScatterMeshData) {
        assert!(data.triangle_count() > 0, "{mesh:?} has no triangles");
        assert_eq!(data.indices.len() % 3, 0, "{mesh:?} indices are not whole triangles");
        assert_eq!(data.normals.len(), data.vertex_count(), "{mesh:?} normals");
        assert_eq!(data.colors.len(), data.vertex_count(), "{mesh:?} colours");
        for &i in &data.indices {
            assert!((i as usize) < data.vertex_count(), "{mesh:?} index {i} out of range");
        }
        for (p, n) in data.positions.iter().zip(&data.normals) {
            let n = Vec3::from(*n);
            assert!(Vec3::from(*p).is_finite(), "{mesh:?} position {p:?}");
            assert!(n.is_finite() && (n.length() - 1.0).abs() < 1e-3, "{mesh:?} normal {n} is not unit length");
        }
        for c in &data.colors {
            assert!(c.iter().all(|x| x.is_finite() && *x >= 0.0), "{mesh:?} colour {c:?}");
        }
    }

    #[test]
    fn every_builder_makes_valid_normals_colours_and_indices() {
        for mesh in ScatterMesh::all() {
            assert_valid(mesh, &mesh.build());
        }
    }

    #[test]
    fn builders_are_deterministic_and_variants_differ() {
        for mesh in ScatterMesh::all() {
            assert_eq!(mesh.build(), mesh.build(), "{mesh:?} changed between builds");
        }
        assert_ne!(ScatterMesh::Rock(0).build(), ScatterMesh::Rock(1).build());
        assert_ne!(ScatterMesh::Grass(0).build(), ScatterMesh::Grass(1).build());
        assert_eq!(ScatterMesh::Rock(ROCK_VARIANTS).build(), ScatterMesh::Rock(0).build(), "variants wrap");
    }

    #[test]
    fn meshes_stand_on_their_origin_at_their_nominal_size() {
        let extent = |data: &ScatterMeshData| {
            data.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| {
                (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
            })
        };
        let (lo, hi) = extent(&ScatterMesh::Grass(0).build());
        assert!(lo.y < 0.0 && lo.y >= -GROUND_SINK - 1e-4, "a tuft reaches just below the ground, {lo}");
        assert!(hi.y > GRASS_HEIGHT * 0.5 && hi.y < GRASS_HEIGHT * 1.5, "tuft height {}", hi.y);
        let (lo, hi) = extent(&ScatterMesh::Rock(2).build());
        assert!(lo.y < 0.0 && hi.y > 0.0, "a rock is partly buried, {lo} .. {hi}");
        assert!((hi.x - lo.x) < ROCK_DIAMETER * 2.0, "rock width {}", hi.x - lo.x);
        for tree in [ScatterMesh::Conifer(0), ScatterMesh::Broadleaf(1)] {
            let (lo, hi) = extent(&tree.build());
            let trunk = tree.trunk().expect("a tree has a trunk");
            assert!(lo.y < 0.0, "{tree:?} trunk reaches into the ground");
            assert!(hi.y > trunk.height * 2.0, "{tree:?} is only {} m tall", hi.y);
        }
    }

    #[test]
    fn appending_moves_rotates_and_reindexes() {
        let tuft = ScatterMesh::Grass(1).build();
        let mut merged = ScatterMeshData::default();
        merged.append_transformed(&tuft, &Transform::IDENTITY, 1.0);
        let moved = Transform::from_xyz(10.0, 2.0, -3.0).with_rotation(Quat::from_rotation_y(1.0)).with_scale(Vec3::splat(2.0));
        merged.append_transformed(&tuft, &moved, 0.5);
        assert_eq!(merged.vertex_count(), tuft.vertex_count() * 2);
        assert_eq!(merged.indices.len(), tuft.indices.len() * 2);
        let n = tuft.vertex_count();
        assert_eq!(merged.indices[tuft.indices.len()], tuft.indices[0] + n as u32);
        let p = Vec3::from(merged.positions[n]);
        assert!((p - moved.transform_point(Vec3::from(tuft.positions[0]))).length() < 1e-4);
        assert!((merged.colors[n][1] - tuft.colors[0][1] * 0.5).abs() < 1e-6, "tinted");
        assert_valid(ScatterMesh::Grass(1), &merged);
    }
}
