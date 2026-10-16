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
//!
//! ## How they read as real at a few hundred triangles
//!
//! The triangle budgets are small (see `BUDGET_*`: a tree is a few hundred,
//! a tuft of grass about fifty), so the realism is in the shading, which costs
//! nothing at draw time:
//! - **Foliage is lit as one volume.** The normals of a leaf clump lean toward
//!   the direction out of the crown, so a canopy shades as one rounded mass
//!   with soft light and dark sides, not as a heap of separately lit balls.
//! - **Baked ambient occlusion** darkens the inside and the underside of a
//!   crown, the root of a blade, the foot of a rock and the skirts of a conifer
//!   tier, and brightens the sunlit tips.
//! - **Silhouettes are broken up**: lumpy, many-sided clumps instead of
//!   spheres, zig-zag drooping conifer skirts, blades that taper to a point.
//! - **Colour is varied inside one mesh**: dry blades in a tuft, yellow-green
//!   new growth on a crown, strata and moss on rock, ridged and flared bark.

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
pub const GRASS_VARIANTS: u8 = 6;
pub const SHRUB_VARIANTS: u8 = 4;
pub const ROCK_VARIANTS: u8 = 6;
pub const CONIFER_VARIANTS: u8 = 3;
pub const BROADLEAF_VARIANTS: u8 = 3;

/// Most triangles one variant of a family may have. Grass and shrubs are
/// merged by the thousand into a tile's mesh, rocks too until they are large,
/// and trees are an entity apiece (up to 2048 to a batch), so these hold what
/// a view of a forest costs. A test keeps every variant under its number.
pub const BUDGET_GRASS_TRIS: usize = 60;
pub const BUDGET_SHRUB_TRIS: usize = 400;
pub const BUDGET_ROCK_TRIS: usize = 200;
pub const BUDGET_CONIFER_TRIS: usize = 330;
pub const BUDGET_BROADLEAF_TRIS: usize = 460;

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
    ///
    /// The performance switch `scatter_meshes_v2` (see `utils::perf_on`)
    /// builds the earlier, cheaper meshes of [`legacy`] instead, so one
    /// binary can be timed with either.
    pub fn build(self) -> ScatterMeshData {
        if !crate::utils::perf_on("scatter_meshes_v2") {
            return legacy::build(self);
        }
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

    /// The most triangles a variant of this mesh's family may have.
    pub fn budget(self) -> usize {
        match self {
            Self::Grass(_) => BUDGET_GRASS_TRIS,
            Self::Shrub(_) => BUDGET_SHRUB_TRIS,
            Self::Rock(_) => BUDGET_ROCK_TRIS,
            Self::Conifer(_) => BUDGET_CONIFER_TRIS,
            Self::Broadleaf(_) => BUDGET_BROADLEAF_TRIS,
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

/// A hash of an integer lattice point to `[0, 1)`: the same on every machine.
fn hash01(seed: u64, x: i32, y: i32, z: i32) -> f32 {
    let mut h = seed
        ^ u64::from(x as u32).wrapping_mul(0x9E37_79B1_85EB_CA87)
        ^ u64::from(y as u32).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ u64::from(z as u32).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    (h >> 40) as f32 * (1.0 / (1u64 << 24) as f32)
}

/// Smooth value noise at `p`, in `[0, 1)`.
fn noise3(seed: u64, p: Vec3) -> f32 {
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (x0, y0, z0) = (p.x.floor(), p.y.floor(), p.z.floor());
    let (fx, fy, fz) = (smooth(p.x - x0), smooth(p.y - y0), smooth(p.z - z0));
    let (ix, iy, iz) = (x0 as i32, y0 as i32, z0 as i32);
    let h = |dx: i32, dy: i32, dz: i32| hash01(seed, ix + dx, iy + dy, iz + dz);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(h(0, 0, 0), h(1, 0, 0), fx);
    let x10 = lerp(h(0, 1, 0), h(1, 1, 0), fx);
    let x01 = lerp(h(0, 0, 1), h(1, 0, 1), fx);
    let x11 = lerp(h(0, 1, 1), h(1, 1, 1), fx);
    lerp(lerp(x00, x10, fy), lerp(x01, x11, fy), fz)
}

/// Three octaves of [`noise3`], in `[0, 1)`.
fn fbm(seed: u64, p: Vec3) -> f32 {
    noise3(seed, p) * 0.58 + noise3(seed ^ 0x9E37, p * 2.07) * 0.29 + noise3(seed ^ 0x79B9, p * 4.31) * 0.13
}

/// `normal` leaned `amount` of the way toward `outward`, unit length.
fn soften(normal: Vec3, outward: Vec3, amount: f32) -> Vec3 {
    (normal * (1.0 - amount) + outward * amount).try_normalize().unwrap_or(normal)
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

/// Append a lumpy leaf clump: the unit icosphere's twelve corners scaled by
/// `radii`, each pushed in or out by up to `lump` of its radius so no two
/// clumps share an outline, and moved to `center`. Smooth shaded, its normals
/// leaning `soft` of the way from the clump's own toward the direction out of
/// `crown`, which makes a canopy of many clumps shade as one volume. Coloured
/// by `color(point, normal)`.
#[allow(clippy::too_many_arguments)]
fn clump(
    out: &mut ScatterMeshData,
    seed: u64,
    center: Vec3,
    radii: Vec3,
    lump: f32,
    crown: Vec3,
    soft: f32,
    color: impl Fn(Vec3, Vec3) -> [f32; 4],
) {
    let (vertices, faces) = icosphere(0);
    let base = out.positions.len() as u32;
    for (i, v) in vertices.iter().enumerate() {
        let push = 1.0 + lump * (hash01(seed, i as i32, 7, 3) * 2.0 - 1.0);
        let point = center + *v * radii * push;
        // An ellipsoid's normal is the sphere's divided by the radii.
        let own = (*v / radii).try_normalize().unwrap_or(*v);
        let outward = (point - crown).try_normalize().unwrap_or(own);
        let normal = soften(own, outward, soft);
        out.vertex(point, normal, color(point, normal));
    }
    for [a, b, c] in faces {
        out.triangle(base + a, base + b, base + c);
    }
}

/// Append a tapered, capless tube from `a` (radius `ra`) to `b` (radius `rb`)
/// with `sides` faces, smooth shaded, coloured `color_a` to `color_b`; with
/// `ridge` above 0 the vertices round it alternate a shade lighter and darker,
/// which reads as bark furrows.
#[allow(clippy::too_many_arguments)]
fn limb(
    out: &mut ScatterMeshData,
    sides: u32,
    a: Vec3,
    ra: f32,
    b: Vec3,
    rb: f32,
    color_a: [f32; 4],
    color_b: [f32; 4],
    ridge: f32,
) {
    let along = b - a;
    let length = along.length().max(1e-4);
    let axis = along / length;
    let helper = if axis.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
    let u = axis.cross(helper).normalize();
    let v = axis.cross(u);
    // The side leans in by (ra - rb) over the length; its normal leans along.
    let lean = (ra - rb) / length;
    let base = out.positions.len() as u32;
    for (point, radius, color) in [(a, ra, color_a), (b, rb, color_b)] {
        for i in 0..sides {
            let angle = i as f32 / sides as f32 * TAU;
            let (sin, cos) = angle.sin_cos();
            let radial = u * cos + v * sin;
            let furrow = 1.0 + if i % 2 == 0 { ridge } else { -ridge };
            out.vertex(point + radial * radius, radial + axis * lean, shade(color, furrow));
        }
    }
    // Wound to face outward whichever way the frame turned.
    let probe_a = Vec3::from(out.positions[base as usize]);
    let probe_b = Vec3::from(out.positions[(base + sides) as usize]);
    let probe_c = Vec3::from(out.positions[(base + 1) as usize]);
    let face = (probe_b - probe_a).cross(probe_c - probe_a);
    let flip = face.dot(Vec3::from(out.normals[base as usize])) < 0.0;
    for i in 0..sides {
        let j = (i + 1) % sides;
        let (b0, b1, t0, t1) = (base + i, base + j, base + sides + i, base + sides + j);
        if flip {
            out.triangle(b0, b1, t0);
            out.triangle(b1, t1, t0);
        } else {
            out.triangle(b0, t0, b1);
            out.triangle(b1, t0, t1);
        }
    }
}

/// Append a trunk up +Y through `rings` of `(height, radius, shade)`, lowest
/// first, as stacked [`limb`]s coloured `low` at the first ring to `high` at
/// the last, furrowed, with the root flare and taper the radii give it.
fn trunk(out: &mut ScatterMeshData, sides: u32, rings: &[(f32, f32, f32)], low: [f32; 4], high: [f32; 4]) {
    let top = rings.last().map_or(1.0, |r| r.0).max(1e-3);
    let colour_at = |(y, _, k): (f32, f32, f32)| shade(mix(low, high, (y / top).clamp(0.0, 1.0)), k);
    for pair in rings.windows(2) {
        let (lo, hi) = (pair[0], pair[1]);
        limb(out, sides, Vec3::new(0.0, lo.0, 0.0), lo.1, Vec3::new(0.0, hi.0, 0.0), hi.1, colour_at(lo), colour_at(hi), 0.09);
    }
}

// ============================================================================
// Families
// ============================================================================

/// A tuft of tapering, curved blades fanning out from a common root: the
/// central ones tall and upright, the outer ones shorter and leaning, each
/// ending in a point, dark at the root and lighter at the tip, a few dry. Five
/// triangles a blade; drawn double sided.
fn grass_tuft(variant: u8) -> ScatterMeshData {
    let mut rng = variant_rng(0x6752_4153, variant);
    let mut out = ScatterMeshData::default();
    let blades = 8 + u32::from(variant % 3);
    // Some variants lean toward summer-dry.
    let dry_share = [0.10, 0.18, 0.06, 0.30, 0.14, 0.22][usize::from(variant) % 6];
    let root_colour = srgb(0.09, 0.19, 0.04);
    let fresh_tip = srgb(0.46, 0.64, 0.17);
    let olive_tip = srgb(0.40, 0.52, 0.14);
    let dry_tip = srgb(0.72, 0.64, 0.30);
    for blade in 0..blades {
        let central = blade < 3;
        let heading = blade as f32 / blades as f32 * TAU + rng.uniform() * 0.9;
        let (sin, cos) = heading.sin_cos();
        let out_dir = Vec3::new(cos, 0.0, sin);
        let side = Vec3::new(-sin, 0.0, cos);
        let root = out_dir * (if central { rng.uniform() * 0.02 } else { 0.02 + rng.uniform() * 0.08 });
        let height = GRASS_HEIGHT * if central { 0.95 + rng.uniform() * 0.3 } else { 0.5 + rng.uniform() * 0.45 };
        let half_width = 0.020 + rng.uniform() * 0.014;
        let lean = if central { 0.05 + rng.uniform() * 0.12 } else { 0.18 + rng.uniform() * 0.32 };
        let curl = (rng.uniform() - 0.5) * 0.35;
        let dry = rng.uniform() < dry_share;
        let tip_colour = if dry {
            dry_tip
        } else {
            mix(fresh_tip, olive_tip, rng.uniform())
        };
        let tint = 0.85 + rng.uniform() * 0.3;
        let first = out.positions.len() as u32;
        const ROWS: u32 = 3;
        for s in 0..ROWS {
            let t = s as f32 / ROWS as f32;
            // The blade bends further out the higher it gets, and a little
            // to one side.
            let centre = root
                + out_dir * (lean * height * t * t)
                + side * (curl * height * t * t)
                + Vec3::Y * (-GROUND_SINK + (height + GROUND_SINK) * t);
            let tangent = (out_dir * (2.0 * lean * height * t)
                + side * (2.0 * curl * height * t)
                + Vec3::Y * (height + GROUND_SINK))
                .normalize();
            // Mostly up: lit like the ground under it rather than by the
            // blade's own facing, which flips as the view moves round it.
            let normal = (tangent.cross(side).normalize() * 0.35 + Vec3::Y * 0.65).normalize();
            let w = half_width * (1.0 - 0.7 * t);
            // Root in shade, brightening up the blade.
            let ao = 0.5 + 0.5 * t.sqrt();
            let colour = shade(mix(root_colour, tip_colour, t), tint * ao.min(1.0));
            out.vertex(centre - side * w, normal, colour);
            out.vertex(centre + side * w, normal, colour);
        }
        // The tip: one vertex, where the blade comes to a point.
        let t = 1.0;
        let tip = root
            + out_dir * (lean * height)
            + side * (curl * height)
            + Vec3::Y * height;
        let tangent = (out_dir * (2.0 * lean * height) + side * (2.0 * curl * height) + Vec3::Y * (height + GROUND_SINK)).normalize();
        let normal = (tangent.cross(side).normalize() * 0.35 + Vec3::Y * 0.65).normalize();
        let tip_vertex = out.vertex(tip, normal, shade(tip_colour, tint * t));
        for s in 0..ROWS - 1 {
            let (l0, r0) = (first + s * 2, first + s * 2 + 1);
            let (l1, r1) = (l0 + 2, r0 + 2);
            out.triangle(l0, r0, r1);
            out.triangle(l0, r1, l1);
        }
        let (l, r) = (first + (ROWS - 1) * 2, first + (ROWS - 1) * 2 + 1);
        out.triangle(l, r, tip_vertex);
    }
    out
}

/// A rounded bush of many small leaf clumps packed in a dome, a few thin
/// stems showing at its foot: dark and dense inside, new growth lighter and
/// yellower on the outside and top. Variants run from fresh to dry to russet.
fn shrub(variant: u8) -> ScatterMeshData {
    let mut rng = variant_rng(0x5348_5242, variant);
    let mut out = ScatterMeshData::default();
    // (inside, outside, new growth), sRGB.
    let (dark, light, growth) = match variant % 4 {
        0 => (srgb(0.08, 0.20, 0.07), srgb(0.22, 0.42, 0.12), srgb(0.46, 0.60, 0.16)),
        1 => (srgb(0.07, 0.17, 0.08), srgb(0.17, 0.34, 0.12), srgb(0.38, 0.52, 0.17)),
        2 => (srgb(0.14, 0.18, 0.07), srgb(0.30, 0.38, 0.13), srgb(0.55, 0.55, 0.20)),
        _ => (srgb(0.18, 0.12, 0.06), srgb(0.40, 0.28, 0.10), srgb(0.62, 0.42, 0.14)),
    };
    let stem_low = srgb(0.20, 0.14, 0.09);
    let stem_high = srgb(0.30, 0.22, 0.14);
    let clumps = 14 + u32::from(variant % 3) * 2;
    // The dome the clumps pack: wider than tall, resting on the ground.
    let dome = Vec3::new(0.50, SHRUB_HEIGHT * 0.48, 0.50);
    let crown = Vec3::new(0.0, SHRUB_HEIGHT * 0.36, 0.0);
    let golden = 2.399_963_2;
    for i in 0..clumps {
        // Fibonacci spiral over the dome, the lowest clumps nearly on the
        // ground, some pulled in toward the middle.
        let f = (i as f32 + 0.5) / clumps as f32;
        let y = 1.0 - f.sqrt() * 1.30;
        let ring = (1.0 - y * y).max(0.0).sqrt();
        let angle = i as f32 * golden + rng.uniform() * 0.5;
        let depth = 0.66 + rng.uniform() * 0.34;
        let dir = Vec3::new(angle.cos() * ring, y, angle.sin() * ring);
        let r = SHRUB_HEIGHT * (0.15 + rng.uniform() * 0.06) * (1.0 + (1.0 - y) * 0.18);
        let center = crown + dir * dome * depth;
        let center = Vec3::new(center.x, center.y.max(r * 0.60 - GROUND_SINK), center.z);
        let hue = rng.uniform();
        let tint = 0.88 + rng.uniform() * 0.24;
        let seed = rng.next_u64();
        clump(&mut out, seed, center, Vec3::new(r, r * 0.82, r), 0.22, crown, 0.55, |point, normal| {
            // Dark inside and low, lighter out and up, a streak of new growth
            // on the sunlit top.
            let outer = ((point - crown) / dome).length().clamp(0.0, 1.2) / 1.2;
            let up = (point.y / SHRUB_HEIGHT).clamp(0.0, 1.0);
            let lit = (0.15 + 0.5 * outer + 0.35 * up + 0.2 * normal.y.max(0.0)).clamp(0.0, 1.0);
            let base = mix(dark, light, lit);
            let fresh = (up * outer * (0.4 + hue)).clamp(0.0, 0.8);
            shade(mix(base, growth, fresh * 0.55), tint)
        });
    }
    // Stems at the foot, where the clumps leave the ground bare.
    for i in 0..3 {
        let angle = i as f32 / 3.0 * TAU + rng.uniform() * 1.2;
        let reach = 0.10 + rng.uniform() * 0.14;
        let foot = Vec3::new(angle.cos() * reach * 0.4, -GROUND_SINK, angle.sin() * reach * 0.4);
        let tip = Vec3::new(angle.cos() * reach, 0.30 + rng.uniform() * 0.12, angle.sin() * reach);
        limb(&mut out, 4, foot, 0.025, tip, 0.012, stem_low, stem_high, 0.0);
    }
    out
}

/// A boulder: a convex polyhedron cut by a dozen random planes from a rounded
/// block, its corners nudged by noise so no face is perfectly flat, flat
/// shaded so the planes read as split rock, sunk a little into the ground.
/// Coloured by variant (granite, basalt, sandstone, slate, mossy granite,
/// pale limestone) with strata, a darker foot, moss on the faces that look
/// up, and the odd lichen fleck.
fn rock(variant: u8) -> ScatterMeshData {
    let mut rng = variant_rng(0x524F_434B, variant);
    let (vertices, faces) = icosphere(1);
    let radius = ROCK_DIAMETER * 0.5;
    // How tall the block is against its width: sandstone lies flat, granite
    // and limestone stand as boulders.
    let (squash_low, squash_high) = [(0.72, 0.95), (0.58, 0.78), (0.48, 0.62), (0.64, 0.86), (0.66, 0.88), (0.80, 1.02)][usize::from(variant) % 6];
    let squash = squash_low + rng.uniform() * (squash_high - squash_low);
    let (stretch_x, stretch_z) = (0.85 + rng.uniform() * 0.35, 0.85 + rng.uniform() * 0.35);
    // The cutting planes as (unit normal, distance from the middle): a dozen
    // around the sides, a flattened top and a flat buried base.
    let mut planes: Vec<(Vec3, f32)> = (0..10 + usize::from(variant % 3))
        .map(|_| (random_direction(&mut rng), radius * (0.70 + rng.uniform() * 0.30)))
        .collect();
    let tilt = Vec3::new((rng.uniform() - 0.5) * 0.4, 1.0, (rng.uniform() - 0.5) * 0.4).normalize();
    planes.push((tilt, radius * squash * (0.75 + rng.uniform() * 0.2)));
    planes.push((Vec3::NEG_Y, radius * squash * 0.55));
    let seed = rng.next_u64();
    let shaped: Vec<Vec3> = vertices
        .iter()
        .map(|v| {
            // Where the ray from the middle through this corner leaves the
            // block: the nearest plane it runs into.
            let mut reach = radius * 1.2;
            for (normal, distance) in &planes {
                let along = v.dot(*normal);
                if along > 0.02 {
                    reach = reach.min(distance / along);
                }
            }
            let p = *v * reach;
            // A touch of noise, so a face is not exactly a plane.
            let grain = 1.0 + (fbm(seed, p * (3.1 / radius)) - 0.5) * 0.10;
            let p = p * grain;
            Vec3::new(p.x * stretch_x, p.y * squash, p.z * stretch_z)
        })
        .collect();
    let lift = radius * squash * 0.30;
    let shaped: Vec<Vec3> = shaped.into_iter().map(|p| p + Vec3::Y * lift).collect();

    // (base colour, moss share, strata strength) per variant.
    let (base, moss_share, strata) = match variant % 6 {
        0 => (srgb(0.47, 0.45, 0.43), 0.10, 0.07),
        1 => (srgb(0.21, 0.21, 0.23), 0.0, 0.05),
        2 => (srgb(0.60, 0.49, 0.36), 0.0, 0.11),
        3 => (srgb(0.33, 0.36, 0.41), 0.06, 0.09),
        4 => (srgb(0.43, 0.43, 0.39), 0.50, 0.06),
        _ => (srgb(0.66, 0.63, 0.55), 0.06, 0.08),
    };
    let moss = srgb(0.14, 0.26, 0.09);
    let lichen = srgb(0.55, 0.57, 0.42);
    let phase = rng.uniform() * TAU;
    let mut out = ScatterMeshData::default();
    // One vertex per corner per face: flat facets read as rock.
    for [a, b, c] in faces {
        let (pa, pb, pc) = (shaped[a as usize], shaped[b as usize], shaped[c as usize]);
        let normal = (pb - pa).cross(pc - pa);
        let unit = normal.try_normalize().unwrap_or(Vec3::Y);
        let centre = (pa + pb + pc) / 3.0;
        let centre_height = centre.y;
        // A darker foot, brighter toward the top, strata across it, and each
        // face a little its own shade.
        let foot = 0.58 + 0.42 * ((centre_height + radius * 0.2) / (radius * 1.1)).clamp(0.0, 1.0);
        let bands = 1.0 + strata * (centre_height * 16.0 / radius + phase).sin();
        let face_hash = hash01(seed, (centre.x * 977.0) as i32, (centre.y * 977.0) as i32, (centre.z * 977.0) as i32);
        let mut colour = shade(base, foot * bands * (0.93 + face_hash * 0.14));
        // Moss on the faces that look up, in patches.
        let patch = fbm(seed ^ 0x6D6F, centre * (4.0 / radius));
        if unit.y > 0.45 && patch > 1.0 - moss_share * 1.4 {
            colour = mix(colour, shade(moss, 0.5 + face_hash * 0.45), 0.7);
        } else if face_hash > 0.975 {
            colour = mix(colour, lichen, 0.16);
        }
        let ia = out.vertex(pa, normal, colour);
        let ib = out.vertex(pb, normal, colour);
        let ic = out.vertex(pc, normal, colour);
        out.triangle(ia, ib, ic);
    }
    out
}

/// A spruce-like tree: a flared, furrowed trunk under stacked skirts of
/// drooping branches, each skirt a zig-zag ring of long and short tips round
/// a shaded middle and a sunlit crown, narrowing up to the leader.
fn conifer(variant: u8) -> ScatterMeshData {
    let mut rng = variant_rng(0x434F_4E49, variant);
    let mut out = ScatterMeshData::default();
    let bark_low = srgb(0.20, 0.13, 0.08);
    let bark_high = srgb(0.30, 0.21, 0.13);
    let height = 9.0 + f32::from(variant) * 1.5 + rng.uniform();
    // The lowest tier's tips droop, so it starts a little above the bare trunk.
    let canopy_start = CONIFER_TRUNK.height + 0.3;
    // The trunk runs up into the canopy, so no gap shows between the tiers.
    trunk(
        &mut out,
        6,
        &[
            (-GROUND_SINK * 3.0, CONIFER_TRUNK.radius * 1.45, 0.55),
            (0.45, CONIFER_TRUNK.radius * 1.05, 0.85),
            (height * 0.5, 0.11, 1.0),
            (height + 0.5, 0.012, 1.0),
        ],
        bark_low,
        bark_high,
    );
    let tiers = 8 + u32::from(variant % 2);
    // (inner, outer tips, sunlit crown), sRGB.
    let (needle_dark, needle_light, needle_new) = match variant % 3 {
        0 => (srgb(0.05, 0.15, 0.08), srgb(0.13, 0.31, 0.15), srgb(0.30, 0.45, 0.20)),
        1 => (srgb(0.06, 0.14, 0.09), srgb(0.12, 0.27, 0.17), srgb(0.24, 0.40, 0.22)),
        _ => (srgb(0.06, 0.16, 0.07), srgb(0.17, 0.33, 0.13), srgb(0.36, 0.48, 0.16)),
    };
    let span = height - canopy_start;
    const SIDES: u32 = 8;
    for tier in 0..tiers {
        let f = tier as f32 / (tiers - 1) as f32;
        let base_y = canopy_start + span * f * 0.80;
        let tier_height = span * (0.30 - f * 0.11);
        let radius = 2.4 * (1.0 - f * 0.80) * (0.90 + rng.uniform() * 0.2);
        let phase = rng.uniform() * TAU;
        let tint = 0.9 + rng.uniform() * 0.2;
        let growth = f * f * 0.5;
        let apex_y = base_y + tier_height;
        // The ring the skirt hangs from, halfway up the tier and well in.
        let mid_y = base_y + tier_height * 0.46;
        let mid_r = radius * 0.50;
        let tip_colour = shade(mix(needle_light, needle_new, growth), tint);
        let inner_colour = shade(mix(needle_dark, needle_light, 0.25), tint);
        let crown_colour = shade(mix(needle_light, needle_new, 0.5 + growth), tint);

        // Outer ring: tips alternately long and short, drooping.
        let outer_first = out.positions.len() as u32;
        let mid_first = outer_first + SIDES * 2;
        // Band normals are the slope's: outward and up, from the rise and the
        // run between the two rings.
        for i in 0..SIDES {
            let angle = phase + i as f32 / SIDES as f32 * TAU;
            let (sin, cos) = angle.sin_cos();
            let long = i % 2 == 0;
            let r = radius * if long { 1.0 } else { 0.70 + rng.uniform() * 0.08 };
            let droop = if long { radius * 0.10 } else { radius * 0.02 };
            let y = base_y - droop;
            // Slope of the band from this tip up to the mid ring.
            let (rise, run) = (mid_y - y, mid_r - r);
            let normal = Vec3::new(cos * rise, -run, sin * rise);
            // Underside shade: the band faces down where it droops.
            out.vertex(Vec3::new(cos * r, y, sin * r), normal, shade(tip_colour, if long { 1.0 } else { 0.86 }));
            let (rise2, run2) = (apex_y - mid_y, 0.0 - mid_r);
            let _ = (rise2, run2);
            // The mid ring vertex of the lower band, shaded.
            out.vertex(Vec3::new(cos * mid_r, mid_y, sin * mid_r), normal, shade(inner_colour, 0.8));
        }
        // Lower band: tip i, mid i, tip i+1, mid i+1.
        for i in 0..SIDES {
            let j = (i + 1) % SIDES;
            let (t0, m0) = (outer_first + i * 2, outer_first + i * 2 + 1);
            let (t1, m1) = (outer_first + j * 2, outer_first + j * 2 + 1);
            out.triangle(t0, m0, t1);
            out.triangle(t1, m0, m1);
        }
        let _ = mid_first;
        // Upper band: the mid ring up to the apex, one apex per face with its
        // normal half way round so the crown shades without a pinch.
        let upper_first = out.positions.len() as u32;
        for i in 0..SIDES {
            let angle = phase + i as f32 / SIDES as f32 * TAU;
            let (sin, cos) = angle.sin_cos();
            let (rise, run) = (apex_y - mid_y, -mid_r);
            let normal = Vec3::new(cos * rise, -run, sin * rise);
            out.vertex(Vec3::new(cos * mid_r, mid_y, sin * mid_r), normal, inner_colour);
        }
        for i in 0..SIDES {
            let angle = phase + (i as f32 + 0.5) / SIDES as f32 * TAU;
            let (sin, cos) = angle.sin_cos();
            let (rise, run) = (apex_y - mid_y, -mid_r);
            let normal = Vec3::new(cos * rise, -run, sin * rise);
            out.vertex(Vec3::new(0.0, apex_y, 0.0), normal, crown_colour);
        }
        for i in 0..SIDES {
            let j = (i + 1) % SIDES;
            out.triangle(upper_first + i, upper_first + SIDES + i, upper_first + j);
        }
    }
    out
}

/// A round-crowned tree: a flared, furrowed trunk forking into a few boughs
/// under a crown of many lumpy leaf clumps packed in a shell round a hollow
/// middle, dark inside and below, lighter and yellower on the sunlit top.
fn broadleaf(variant: u8) -> ScatterMeshData {
    let mut rng = variant_rng(0x4252_4F41, variant);
    let mut out = ScatterMeshData::default();
    let bark_low = srgb(0.24, 0.17, 0.11);
    let bark_high = srgb(0.34, 0.26, 0.18);
    let base = BROADLEAF_TRUNK.height;
    let crown_y = base + 2.4 + f32::from(variant) * 0.6;
    let crown = Vec3::new(0.0, crown_y, 0.0);
    let radii = Vec3::new(2.6 + f32::from(variant) * 0.2, 2.0 + f32::from(variant) * 0.15, 2.6 + f32::from(variant) * 0.2);
    trunk(
        &mut out,
        7,
        &[
            (-GROUND_SINK * 3.0, BROADLEAF_TRUNK.radius * 1.55, 0.55),
            (0.40, BROADLEAF_TRUNK.radius * 1.12, 0.85),
            (base * 0.6, BROADLEAF_TRUNK.radius * 0.92, 1.0),
            (base + 0.9, 0.15, 1.0),
        ],
        bark_low,
        bark_high,
    );
    // (inside, lit), sRGB, and the yellower new growth.
    let (leaf_dark, leaf_light, leaf_new) = match variant % 3 {
        0 => (srgb(0.09, 0.24, 0.07), srgb(0.28, 0.48, 0.14), srgb(0.52, 0.62, 0.16)),
        1 => (srgb(0.08, 0.22, 0.08), srgb(0.22, 0.42, 0.13), srgb(0.44, 0.58, 0.17)),
        _ => (srgb(0.12, 0.24, 0.07), srgb(0.34, 0.48, 0.13), srgb(0.58, 0.58, 0.16)),
    };
    // Boughs from the top of the trunk out under the crown.
    let boughs = 4;
    for i in 0..boughs {
        let angle = i as f32 / boughs as f32 * TAU + rng.uniform() * 0.9;
        let from = Vec3::new(0.0, base + 0.2 + rng.uniform() * 0.5, 0.0);
        let to = Vec3::new(angle.cos() * radii.x * 0.55, crown_y - radii.y * 0.30 + rng.uniform() * 0.6, angle.sin() * radii.z * 0.55);
        limb(&mut out, 4, from, 0.12, to, 0.04, bark_high, bark_high, 0.0);
    }
    let clumps = 15 + u32::from(variant) * 2;
    let golden = 2.399_963_2;
    for i in 0..clumps {
        // Fibonacci spiral over the crown's shell, lower part left open
        // toward the trunk.
        let f = (i as f32 + 0.5) / clumps as f32;
        let y = 1.0 - f * 1.55;
        let ring = (1.0 - y * y).max(0.0).sqrt();
        let angle = i as f32 * golden + rng.uniform() * 0.6;
        let dir = Vec3::new(angle.cos() * ring, y, angle.sin() * ring);
        let shell = 0.66 + rng.uniform() * 0.30;
        let r = 1.00 + rng.uniform() * 0.50;
        let center = crown + dir * radii * shell;
        let hue = rng.uniform();
        let tint = 0.88 + rng.uniform() * 0.24;
        let seed = rng.next_u64();
        clump(&mut out, seed, center, Vec3::new(r, r * 0.80, r), 0.26, crown, 0.6, |point, normal| {
            // Dark deep inside and underneath, light on the outside and top,
            // new growth where it is both.
            let outer = ((point - crown) / radii).length().clamp(0.0, 1.3) / 1.3;
            let top = ((point.y - (crown_y - radii.y)) / (radii.y * 2.0)).clamp(0.0, 1.0);
            let lit = (0.10 + 0.45 * outer + 0.45 * top + 0.15 * normal.y.max(0.0)).clamp(0.0, 1.0);
            let base = mix(leaf_dark, leaf_light, lit);
            let fresh = (top * outer * (0.3 + hue)).clamp(0.0, 0.8);
            shade(mix(base, leaf_new, fresh * 0.6), tint)
        });
    }
    out
}

/// The meshes scatter drew before the realism rewrite: plain lobes, tubes and
/// cones, 30 to 500 triangles each. Kept behind the `scatter_meshes_v2`
/// performance switch so a frame-time measurement can compare the two sets in
/// one binary (`EUSTRESS_PERF_OFF=scatter_meshes_v2`).
mod legacy {
    use super::*;

    const GRASS_VARIANTS: u8 = 4;
    const SHRUB_VARIANTS: u8 = 3;
    const ROCK_VARIANTS: u8 = 5;
    const CONIFER_VARIANTS: u8 = 2;
    const BROADLEAF_VARIANTS: u8 = 2;

    pub(super) fn build(mesh: ScatterMesh) -> ScatterMeshData {
        match mesh {
            ScatterMesh::Grass(v) => grass_tuft(v % GRASS_VARIANTS),
            ScatterMesh::Shrub(v) => shrub(v % SHRUB_VARIANTS),
            ScatterMesh::Rock(v) => rock(v % ROCK_VARIANTS),
            ScatterMesh::Conifer(v) => conifer(v % CONIFER_VARIANTS),
            ScatterMesh::Broadleaf(v) => broadleaf(v % BROADLEAF_VARIANTS),
        }
    }

    /// Every variant of the earlier set.
    #[cfg(test)]
    pub(super) fn all() -> impl Iterator<Item = ScatterMesh> {
        (0..GRASS_VARIANTS)
            .map(ScatterMesh::Grass)
            .chain((0..SHRUB_VARIANTS).map(ScatterMesh::Shrub))
            .chain((0..ROCK_VARIANTS).map(ScatterMesh::Rock))
            .chain((0..CONIFER_VARIANTS).map(ScatterMesh::Conifer))
            .chain((0..BROADLEAF_VARIANTS).map(ScatterMesh::Broadleaf))
    }

    /// Append an ellipsoid lobe: a unit icosphere scaled by `radii` and moved
    /// to `center`, smooth shaded, coloured by `color(point, normal)`.
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

    /// Append a tapered, capless tube around +Y from `y0` (radius `r0`) to
    /// `y1` (radius `r1`) with `sides` faces, smooth shaded.
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

    /// A tuft of curved, tapering blades leaning out from a common root, dark
    /// at the base and lighter at the tips. Drawn double sided.
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
        // No triangle collapsed to a line or a point: it would cost a draw
        // for nothing, and a flat-shaded one has no normal to light.
        for t in data.indices.chunks_exact(3) {
            let (a, b, c) = (Vec3::from(data.positions[t[0] as usize]), Vec3::from(data.positions[t[1] as usize]), Vec3::from(data.positions[t[2] as usize]));
            assert!((b - a).cross(c - a).length() > 1e-9, "{mesh:?} has a degenerate triangle {a} {b} {c}");
        }
    }

    #[test]
    fn every_builder_makes_valid_normals_colours_and_indices() {
        for mesh in ScatterMesh::all() {
            assert_valid(mesh, &mesh.build());
        }
    }

    #[test]
    fn the_earlier_set_behind_the_switch_is_valid_and_has_cheaper_conifers() {
        for mesh in legacy::all() {
            assert_valid(mesh, &legacy::build(mesh));
        }
        // What the switch is for: the earlier conifers cost fewer triangles.
        assert!(legacy::build(ScatterMesh::Conifer(0)).triangle_count() < ScatterMesh::Conifer(0).build().triangle_count());
    }

    #[test]
    fn every_variant_stays_inside_its_triangle_budget() {
        for mesh in ScatterMesh::all() {
            let tris = mesh.build().triangle_count();
            assert!(tris <= mesh.budget(), "{mesh:?} has {tris} triangles, over its budget of {}", mesh.budget());
        }
    }

    #[test]
    fn builders_are_deterministic_and_variants_differ() {
        for mesh in ScatterMesh::all() {
            assert_eq!(mesh.build(), mesh.build(), "{mesh:?} changed between builds");
        }
        assert_ne!(ScatterMesh::Rock(0).build(), ScatterMesh::Rock(1).build());
        assert_ne!(ScatterMesh::Grass(0).build(), ScatterMesh::Grass(1).build());
        assert_ne!(ScatterMesh::Conifer(0).build(), ScatterMesh::Conifer(1).build());
        assert_ne!(ScatterMesh::Broadleaf(0).build(), ScatterMesh::Broadleaf(2).build());
        assert_ne!(ScatterMesh::Shrub(0).build(), ScatterMesh::Shrub(3).build());
        assert_eq!(ScatterMesh::Rock(ROCK_VARIANTS).build(), ScatterMesh::Rock(0).build(), "variants wrap");
    }

    #[test]
    fn meshes_stand_on_their_origin_at_their_nominal_size() {
        let extent = |data: &ScatterMeshData| {
            data.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| {
                (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
            })
        };
        for v in 0..GRASS_VARIANTS {
            let (lo, hi) = extent(&ScatterMesh::Grass(v).build());
            assert!(lo.y < 0.0 && lo.y >= -GROUND_SINK - 1e-4, "a tuft reaches just below the ground, {lo}");
            assert!(hi.y > GRASS_HEIGHT * 0.5 && hi.y < GRASS_HEIGHT * 1.5, "tuft height {}", hi.y);
            assert!((hi.x - lo.x).max(hi.z - lo.z) < GRASS_HEIGHT * 1.6, "a tuft fans out only so far, {lo} .. {hi}");
        }
        for v in 0..SHRUB_VARIANTS {
            let (lo, hi) = extent(&ScatterMesh::Shrub(v).build());
            assert!(lo.y >= -GROUND_SINK * 3.0 && lo.y < 0.0, "shrub {v} stands on the ground, {lo}");
            assert!(hi.y > SHRUB_HEIGHT * 0.7 && hi.y < SHRUB_HEIGHT * 1.4, "shrub {v} height {}", hi.y);
        }
        for v in 0..ROCK_VARIANTS {
            let (lo, hi) = extent(&ScatterMesh::Rock(v).build());
            assert!(lo.y < 0.0 && hi.y > 0.0, "rock {v} is partly buried, {lo} .. {hi}");
            assert!((hi.x - lo.x) < ROCK_DIAMETER * 1.6 && (hi.z - lo.z) < ROCK_DIAMETER * 1.6, "rock {v} width {} by {}", hi.x - lo.x, hi.z - lo.z);
            assert!((hi.x - lo.x) > ROCK_DIAMETER * 0.6, "rock {v} is not a pebble, width {}", hi.x - lo.x);
        }
        for v in 0..CONIFER_VARIANTS {
            let tree = ScatterMesh::Conifer(v);
            let (lo, hi) = extent(&tree.build());
            let trunk = tree.trunk().expect("a tree has a trunk");
            assert!(lo.y < 0.0, "{tree:?} trunk reaches into the ground");
            assert!(hi.y > trunk.height * 3.5, "{tree:?} is only {} m tall", hi.y);
            assert!(hi.x - lo.x > 3.0 && hi.x - lo.x < 6.5, "{tree:?} is {} m across", hi.x - lo.x);
        }
        for v in 0..BROADLEAF_VARIANTS {
            let tree = ScatterMesh::Broadleaf(v);
            let (lo, hi) = extent(&tree.build());
            let trunk = tree.trunk().expect("a tree has a trunk");
            assert!(lo.y < 0.0, "{tree:?} trunk reaches into the ground");
            assert!(hi.y > trunk.height * 2.0, "{tree:?} is only {} m tall", hi.y);
            assert!(hi.x - lo.x > 4.0 && hi.x - lo.x < 9.0, "{tree:?} is {} m across", hi.x - lo.x);
        }
    }

    #[test]
    fn a_tree_has_bare_trunk_below_its_canopy() {
        // A capsule collider covers the trunk, so no foliage may hang into the
        // space a body walks through: every vertex below the trunk's height
        // lies within the trunk's own radius of its axis (a little over, for
        // the root flare).
        for tree in [ScatterMesh::Conifer(0), ScatterMesh::Conifer(2), ScatterMesh::Broadleaf(0), ScatterMesh::Broadleaf(2)] {
            let trunk = tree.trunk().expect("a tree has a trunk");
            for p in &tree.build().positions {
                let p = Vec3::from(*p);
                if p.y < trunk.height * 0.9 {
                    let across = (p.x * p.x + p.z * p.z).sqrt();
                    assert!(across <= trunk.radius * 1.9, "{tree:?} has foliage {across} m from the axis at height {}", p.y);
                }
            }
        }
    }

    #[test]
    fn foliage_is_darker_inside_than_out() {
        // The baked occlusion that gives the volume: the canopy's lowest,
        // innermost vertices are darker than its highest outer ones.
        let luma = |c: &[f32; 4]| c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11;
        for tree in [ScatterMesh::Broadleaf(0), ScatterMesh::Broadleaf(1), ScatterMesh::Conifer(0)] {
            let data = tree.build();
            let trunk = tree.trunk().expect("a tree has a trunk");
            let canopy: Vec<(Vec3, f32)> = data
                .positions
                .iter()
                .zip(&data.colors)
                .map(|(p, c)| (Vec3::from(*p), luma(c)))
                .filter(|(p, _)| p.y > trunk.height + 1.0)
                .collect();
            let top = canopy.iter().map(|(p, _)| p.y).fold(f32::MIN, f32::max);
            let bottom = canopy.iter().map(|(p, _)| p.y).fold(f32::MAX, f32::min);
            let mean = |lo: f32, hi: f32| {
                let band: Vec<f32> = canopy.iter().filter(|(p, _)| p.y >= lo && p.y <= hi).map(|(_, l)| *l).collect();
                band.iter().sum::<f32>() / band.len().max(1) as f32
            };
            let third = (top - bottom) / 3.0;
            assert!(
                mean(top - third, top) > mean(bottom, bottom + third),
                "{tree:?}: the top of the crown ({}) is not lighter than its underside ({})",
                mean(top - third, top),
                mean(bottom, bottom + third)
            );
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
