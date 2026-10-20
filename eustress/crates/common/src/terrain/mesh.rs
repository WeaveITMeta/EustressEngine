//! Terrain mesh generation
//!
//! Generates chunk meshes with:
//! - Multi-octave Perlin noise for realistic height
//! - LOD-aware resolution
//! - Skirts for seamless LOD transitions
//! - Smooth normals
//! - Holes: on a sparse surface (`TerrainData::sparse_surface`) every quad
//!   with a corner on a hole is left out, with the skirt below it (see
//!   [`chunk_ground_quads`])

use bevy::prelude::*;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::asset::RenderAssetUsages;
use noise::{NoiseFn, Perlin, Fbm, MultiFractal};
use super::{TerrainConfig, TerrainData};
use super::height_query::material_weights_at_uv;
use super::material::{height_to_color, HeightBlendParams, TerrainMaterial};
use super::material_slots::TerrainSlotPalette;

// ── Vertex-colour realism knobs (see the baking block in HeightfieldShading::color) ──
// Flat per-material base colours read as "painted plastic". Three free,
// deterministic modulations bake depth into the vertex colour without a texture:
// curvature ambient-occlusion, slope self-shadow, and macro tonal variation.
/// Darkening per metre of local concavity (discrete Laplacian).
const AO_STRENGTH: f32 = 0.055;
/// Deepest valley/crevice shade (AO floor).
const AO_MIN: f32 = 0.62;
/// Brightest convex-ridge lift (AO ceiling).
const RIDGE_MAX: f32 = 1.08;
/// Shade of a near-vertical face (as `nrm.y` → 0).
const SLOPE_MIN: f32 = 0.72;
/// ± amplitude of the low-frequency tonal patchiness.
const MACRO_VARIATION: f32 = 0.13;
/// Patch wavelength of the tonal variation (~11 m).
const MACRO_FREQ: f32 = 1.0 / 11.0;

/// sRGB base colour of the material mix at a global height-cache UV: the
/// bilinear slot weights of the material map (`material_weights_at_uv`)
/// blending each slot's swatch from `data.slot_palette`, so a Space's
/// custom slots show in their own colour. A point with no material around
/// it reads as Grass, so no cell ever renders black.
///
/// Bilinear (not nearest) matters here: the worldgen export bakes a 3x3
/// smoothing kernel into its material mixes so boundaries are soft
/// gradients, and nearest sampling re-quantised that softness back into
/// hard blocks at mesh-vertex resolution. Bilinear interpolation between the
/// 4 nearest cells preserves the gradient.
fn material_mix_srgb(data: &TerrainData, u: f32, v: f32) -> [f32; 3] {
    let palette = &data.slot_palette;
    let weights = material_weights_at_uv(data, u, v);
    if weights.is_empty() {
        return palette.srgb(TerrainMaterial::Grass.to_u8());
    }
    let mut srgb = [0.0f32; 3];
    for &(slot, weight) in weights.as_slice() {
        let [r, g, b] = palette.srgb(slot);
        srgb[0] += r * weight;
        srgb[1] += g * weight;
        srgb[2] += b * weight;
    }
    srgb
}

/// The shade every baked terrain vertex colour shares: `ao` times the slope
/// self-shadow and macro value-noise, so heightfield ground and volumetric
/// surfaces that meet at a border are lit by one formula.
///
/// Every terrain vertex colour is `[base * shade, shade]`: the RGB is what
/// the vertex-colour `StandardMaterial` draws (an opaque surface ignores
/// alpha), and the alpha carries the shade alone, so the textured surface
/// material (`surface_material`) can darken its own albedo by it without
/// applying the swatch colour a second time.
fn baked_shade(ao: f32, normal: Vec3, world_x: f32, world_z: f32, seed: u32) -> f32 {
    let slope_shade = SLOPE_MIN + (1.0 - SLOPE_MIN) * normal.y.clamp(0.0, 1.0);
    let variation =
        1.0 + MACRO_VARIATION * hash_noise(world_x * MACRO_FREQ, world_z * MACRO_FREQ, seed ^ 0x9E37);
    (ao * slope_shade * variation).clamp(0.35, 1.2)
}

/// What the baked heightfield vertex colour reads, set up once per mesh.
///
/// The one definition of how heightfield ground is coloured: the heightfield
/// mesher calls it for every vertex, and the marching-cubes mesher
/// (`marching.rs`) calls it for every vertex where the heightfield term of
/// the terrain field wins, so the two meshes agree along a shared border.
pub(crate) struct HeightfieldShading<'a> {
    config: &'a TerrainConfig,
    data: &'a TerrainData,
    has_materials: bool,
    fallback_blend: HeightBlendParams,
}

impl<'a> HeightfieldShading<'a> {
    pub(crate) fn new(config: &'a TerrainConfig, data: &'a TerrainData) -> Self {
        Self {
            config,
            data,
            has_materials: !data.height_cache.is_empty() && data.has_material_layer(),
            fallback_blend: HeightBlendParams::default(),
        }
    }

    /// Linear RGBA of the heightfield surface at one vertex, the baked shade
    /// in alpha (see [`baked_shade`]). `world_u`, `world_v` is the vertex's
    /// global height-cache UV, `height` its surface height, `neighbours` the
    /// surface heights one sample step away at `[-X, +X, -Z, +Z]`, and
    /// `normal` its unit surface normal.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn color(
        &self,
        world_u: f32,
        world_v: f32,
        world_x: f32,
        world_z: f32,
        height: f32,
        neighbours: [f32; 4],
        normal: Vec3,
    ) -> [f32; 4] {
        let config = self.config;
        let [hl, hr, hd, hup] = neighbours;

        // Base material colour. From the material map when the terrain has
        // one, every slot at its own base colour, else a height-band
        // fallback so procedural terrain keeps its look. The terrain
        // StandardMaterial base_color is white, so this vertex colour shows
        // through directly.
        let lin = if self.has_materials {
            let [r, g, b] = material_mix_srgb(self.data, world_u, world_v);
            Color::srgb(r, g, b).to_linear()
        } else {
            height_to_color(height, &self.fallback_blend).to_linear()
        };

        // Bake cheap realism into the vertex colour (see the knob consts):
        //  1. curvature AO: Laplacian `mean(neighbours) - h` is >0 in
        //     concavities (darken for occlusion) and <0 on ridges (lift);
        //  2. slope shade: scale by `normal.y` (cos slope) for soft cliff
        //     self-shadow;
        //  3. macro value-noise: low-frequency world-space patchiness so
        //     the uniform per-material fill stops reading as flat paint.
        let laplacian = (hl + hr + hd + hup) * 0.25 - height;
        let ao = (1.0 - laplacian * AO_STRENGTH).clamp(AO_MIN, RIDGE_MAX);
        let shade = baked_shade(ao, normal, world_x, world_z, config.seed);
        [lin.red * shade, lin.green * shade, lin.blue * shade, shade]
    }
}

/// Linear RGBA of a vertex on a surface a volumetric edit made (a cave
/// wall, an overhang): `material`'s swatch in `palette` (its slot is its
/// discriminant) under the same slope shade and macro variation the
/// heightfield bakes, the shade in alpha (see [`baked_shade`]). There is no
/// curvature AO, since there is no height raster there to take a Laplacian
/// of.
pub(crate) fn material_vertex_color(
    palette: &TerrainSlotPalette,
    material: TerrainMaterial,
    world_x: f32,
    world_z: f32,
    normal: Vec3,
    seed: u32,
) -> [f32; 4] {
    let lin = palette.color(material.to_u8()).to_linear();
    let shade = baked_shade(1.0, normal, world_x, world_z, seed);
    [lin.red * shade, lin.green * shade, lin.blue * shade, shade]
}

/// How far (a negative Y offset) chunk skirts hang below the chunk border:
/// 5% of the chunk size, at least 2 units.
pub(crate) fn skirt_depth(chunk_size: f32) -> f32 {
    -(chunk_size * 0.05).max(2.0)
}

/// Pre-allocated noise generators for terrain height sampling.
/// Created once per chunk instead of once per vertex.
struct TerrainNoiseContext {
    perlin: Perlin,
    perlin3: Perlin,
    base_terrain: Fbm<Perlin>,
    ridge_perlin: Perlin,
    height_scale: f32,
}

impl TerrainNoiseContext {
    fn new(seed: u32, height_scale: f32) -> Self {
        Self {
            perlin: Perlin::new(seed),
            perlin3: Perlin::new(seed + 2000),
            base_terrain: Fbm::new(seed + 100)
                .set_octaves(4)
                .set_frequency(0.001)
                .set_lacunarity(2.0)
                .set_persistence(0.5),
            ridge_perlin: Perlin::new(seed + 3000),
            height_scale,
        }
    }

    /// Sample height at a world position using cached noise generators
    fn sample_height(&self, x: f32, z: f32) -> f32 {
        // Layer 1: Continental/Biome mask (very large scale)
        let continent_freq = 0.0003;
        let continent = self.perlin.get([x as f64 * continent_freq, z as f64 * continent_freq]) as f32;
        let continent = (continent + 1.0) * 0.5;

        // Layer 2: Base terrain shape (medium scale)
        let base = self.base_terrain.get([x as f64, z as f64]) as f32;

        // Layer 3: Mountain ridges (using ridged multifractal)
        let mountain_height = self.sample_mountain_ridges(x, z);

        // Layer 4: Fine detail (small scale noise)
        let detail_freq = 0.008;
        let detail = self.perlin3.get([x as f64 * detail_freq, z as f64 * detail_freq]) as f32 * 0.1;

        // Combine layers based on biome
        let mountain_mask = (continent * 1.5 - 0.3).clamp(0.0, 1.0);
        let mountain_mask = mountain_mask * mountain_mask;
        let plains_mask = 1.0 - mountain_mask;
        let hills_mask = (1.0 - (mountain_mask - 0.5).abs() * 2.0).clamp(0.0, 1.0);

        let mut height = 0.0;

        // Flat plains with gentle undulation
        let plains_height = base * 0.05 + detail * 0.5;
        height += plains_height * plains_mask;

        // Rolling hills
        let hills_height = base * 0.15 + detail;
        height += hills_height * hills_mask * 0.5;

        // Mountains with ridges
        let mountains_height = mountain_height * 0.8 + base * 0.2;
        height += mountains_height * mountain_mask;

        // Add subtle detail everywhere
        height += detail * 0.3;

        // Ensure some flat areas at sea level
        if height < 0.02 && plains_mask > 0.7 {
            height = height * 0.3;
        }

        height * self.height_scale
    }

    /// Generate realistic mountain ridges using ridged multifractal noise
    fn sample_mountain_ridges(&self, x: f32, z: f32) -> f32 {
        let mut height = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = 0.0008_f32;
        let mut weight = 1.0;

        for i in 0..5 {
            let noise = self.ridge_perlin.get([
                x as f64 * frequency as f64 + i as f64 * 100.0,
                z as f64 * frequency as f64 + i as f64 * 100.0
            ]) as f32;

            let mut ridge = 1.0 - noise.abs();
            ridge = ridge * ridge;
            ridge *= weight;
            weight = ridge.clamp(0.0, 1.0);

            height += ridge * amplitude;
            amplitude *= 0.5;
            frequency *= 2.2;
        }

        height = height / 2.0;
        height = height.powf(1.3);
        height
    }
}

/// World-space surface heights of a chunk's vertex grid at `resolution`,
/// laid out `z * (resolution + 1) + x`. Vertex `(x, z)` sits at
/// `(x, z) * chunk_size / resolution` from the chunk entity's corner.
///
/// The single definition of where a chunk's ground is: the render mesh takes
/// its vertex heights from here, and the physics collider (`collider.rs`)
/// samples it at LOD 0, so what a body stands on cannot drift from what the
/// full-detail mesh shows.
pub fn chunk_height_grid(
    chunk_pos: IVec2,
    resolution: u32,
    config: &TerrainConfig,
    data: &TerrainData,
) -> Vec<f32> {
    let resolution = resolution.max(1);
    let size = config.chunk_size;
    let stride = (resolution + 1) as usize;
    let mut heights = Vec::with_capacity(stride * stride);

    let noise = data
        .height_cache
        .is_empty()
        .then(|| TerrainNoiseContext::new(config.seed, config.height_scale));

    for z in 0..=resolution {
        for x in 0..=resolution {
            let u = x as f32 / resolution as f32;
            let v = z as f32 / resolution as f32;
            let height = match &noise {
                Some(ctx) => ctx.sample_height(
                    chunk_pos.x as f32 * size + u * size,
                    chunk_pos.y as f32 * size + v * size,
                ),
                None => {
                    let uv = config.chunk_point_uv(chunk_pos, u, v);
                    config.world_height(data.sample_height(uv.x.clamp(0.0, 1.0), uv.y.clamp(0.0, 1.0)))
                }
            };
            heights.push(height);
        }
    }
    heights
}

/// Which quads of chunk `chunk_pos`'s vertex grid at `resolution` have
/// ground, one flag per quad laid out `z * resolution + x` (quad `(x, z)`
/// spans vertices `x..=x + 1` by `z..=z + 1` of [`chunk_height_grid`]'s
/// grid). On a sparse surface (`TerrainData::sparse_surface`) a quad has none
/// when any of its four corners stands on a hole
/// (`TerrainData::point_is_hole`). `None` when every quad has ground, which
/// off a sparse surface is always.
///
/// The single definition of which ground a chunk keeps: the render mesh draws
/// only these quads at every LOD, the LOD-0 collider (`collider.rs`) is built
/// from exactly the quads LOD 0 keeps, and [`ground_at_world`] answers from
/// them for raycasts and scatter.
pub fn chunk_ground_quads(chunk_pos: IVec2, resolution: u32, config: &TerrainConfig, data: &TerrainData) -> Option<Vec<bool>> {
    if !data.sparse_surface || data.material_cache.is_empty() {
        return None;
    }
    let resolution = resolution.max(1);
    let stride = resolution as usize + 1;
    let mut holes = Vec::with_capacity(stride * stride);
    for z in 0..=resolution {
        for x in 0..=resolution {
            // The vertex's own `u, v`, computed as `chunk_height_grid` does.
            let u = x as f32 / resolution as f32;
            let v = z as f32 / resolution as f32;
            holes.push(data.point_is_hole(config, chunk_pos, u, v));
        }
    }
    if !holes.contains(&true) {
        return None;
    }
    let mut quads = Vec::with_capacity(resolution as usize * resolution as usize);
    for z in 0..resolution as usize {
        for x in 0..resolution as usize {
            let i = z * stride + x;
            quads.push(!(holes[i] || holes[i + 1] || holes[i + stride] || holes[i + stride + 1]));
        }
    }
    Some(quads)
}

/// The two triangles of the quad whose first vertex is `i` in a vertex grid
/// `stride` vertices wide, split along the `(x, z + 1)-(x + 1, z)` diagonal
/// and wound counter-clockwise seen from above (Bevy's front face):
/// bottom-left, top-left, bottom-right, then bottom-right, top-left,
/// top-right. The render mesh and the collider of a chunk with holes both
/// triangulate through this, and parry's heightfield splits along the same
/// diagonal.
#[inline]
fn quad_triangles(i: u32, stride: u32) -> [[u32; 3]; 2] {
    [[i, i + stride, i + 1], [i + 1, i + stride, i + stride + 1]]
}

/// The LOD-0 ground of chunk `chunk_pos` on a sparse surface with holes in
/// it, as triangles: the LOD-0 vertex grid at [`chunk_height_grid`]'s
/// heights, positions local to the chunk entity exactly as the render mesh
/// places them, and the two triangles of every quad [`chunk_ground_quads`]
/// keeps, split and wound as the render mesh splits and winds them. `None`
/// when every quad has ground; no triangles when none has. What the collider
/// of such a chunk is built from.
pub fn chunk_ground_triangles(chunk_pos: IVec2, config: &TerrainConfig, data: &TerrainData) -> Option<(Vec<Vec3>, Vec<[u32; 3]>)> {
    let resolution = config.resolution_for_lod(0).max(1);
    let quads = chunk_ground_quads(chunk_pos, resolution, config, data)?;
    let grid = chunk_height_grid(chunk_pos, resolution, config, data);
    let size = config.chunk_size;
    let stride = resolution + 1;
    let mut positions = Vec::with_capacity(grid.len());
    for z in 0..=resolution {
        for x in 0..=resolution {
            let u = x as f32 / resolution as f32;
            let v = z as f32 / resolution as f32;
            positions.push(Vec3::new(u * size, grid[(z * stride + x) as usize], v * size));
        }
    }
    let mut triangles = Vec::new();
    for z in 0..resolution {
        for x in 0..resolution {
            if quads[(z * resolution + x) as usize] {
                triangles.extend(quad_triangles(z * stride + x, stride));
            }
        }
    }
    Some((positions, triangles))
}

/// Whether world XZ `(world_x, world_z)` has ground under it: it lies in a
/// quad the LOD-0 mesh of its chunk keeps (see [`chunk_ground_quads`]).
/// Every point of a full surface has ground; on a sparse one, no point off
/// the chunk grid does. Terrain raycasts and scatter ask this, so neither
/// lands on ground the meshes and colliders leave out.
pub fn ground_at_world(config: &TerrainConfig, data: &TerrainData, world_x: f32, world_z: f32) -> bool {
    if !data.sparse_surface || data.material_cache.is_empty() {
        return true;
    }
    let (fx, fz) = (world_x / config.chunk_size, world_z / config.chunk_size);
    if !(fx.is_finite() && fz.is_finite()) {
        return false;
    }
    let chunk = IVec2::new(fx.floor() as i32, fz.floor() as i32);
    if !config.contains_chunk(chunk) {
        return false;
    }
    let resolution = config.resolution_for_lod(0).max(1);
    // The quad along one axis: the fraction past the chunk's corner in quads.
    let quad = |f: f32, start: i32| (((f - start as f32) * resolution as f32).floor().max(0.0) as u32).min(resolution - 1);
    let (qx, qz) = (quad(fx, chunk.x), quad(fz, chunk.y));
    [(qx, qz), (qx + 1, qz), (qx, qz + 1), (qx + 1, qz + 1)].into_iter().all(|(x, z)| {
        !data.point_is_hole(config, chunk, x as f32 / resolution as f32, z as f32 / resolution as f32)
    })
}

/// Generate the heightfield mesh for a terrain chunk.
///
/// Systems that mesh chunks call [`super::generate_chunk_render_mesh`]
/// instead, which draws a chunk holding volumetric edits with marching cubes
/// at LOD 0 and comes here for everything else.
///
/// On a sparse surface only the quads [`chunk_ground_quads`] keeps are drawn,
/// and only their skirts hang (see `add_skirts`). The vertex buffers keep
/// every vertex, so a chunk that keeps no quad yields a mesh without indices.
pub fn generate_chunk_mesh(
    chunk_pos: IVec2,
    lod: u32,
    config: &TerrainConfig,
    data: &TerrainData,
    meshes: &mut Assets<Mesh>,
) -> Handle<Mesh> {
    let resolution = config.resolution_for_lod(lod);
    let size = config.chunk_size;
    let height_scale = config.height_scale;
    let seed = config.seed;
    
    // Generate vertices
    let vertex_count = ((resolution + 1) * (resolution + 1)) as usize;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vertex_count);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(vertex_count);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(vertex_count);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(vertex_count);

    // Create noise context once per chunk (NOT per vertex)
    let noise_context = TerrainNoiseContext::new(seed, height_scale);
    let use_procedural = data.height_cache.is_empty();
    let shading = HeightfieldShading::new(config, data);
    let total_chunks_x = (config.chunks_x * 2 + 1) as f32;
    let total_chunks_z = (config.chunks_z * 2 + 1) as f32;

    // World-space step for SEAMLESS normals: one mesh cell, expressed in the
    // global height-cache UV space. Sampling the shared global field (rather
    // than chunk-local vertices) means a border vertex gets the SAME normal
    // from either adjacent chunk → no lighting seam at chunk boundaries.
    let terrain_w_m = (total_chunks_x * size).max(1.0);
    let terrain_d_m = (total_chunks_z * size).max(1.0);
    let step_m = (size / resolution as f32).max(0.001);
    let du = step_m / terrain_w_m;
    let dv = step_m / terrain_d_m;

    // Vertex heights come from the shared grid so the LOD-0 collider, which
    // samples the same function, matches this mesh exactly.
    let grid = chunk_height_grid(chunk_pos, resolution, config, data);
    let grid_stride = resolution as usize + 1;

    // Height sampling
    for z in 0..=resolution {
        for x in 0..=resolution {
            let u = x as f32 / resolution as f32;
            let v = z as f32 / resolution as f32;

            // World position for this vertex
            let world_x = chunk_pos.x as f32 * size + u * size;
            let world_z = chunk_pos.y as f32 * size + v * size;

            // Global height-cache UV (data path — also drives colour + normals).
            let uv = config.chunk_point_uv(chunk_pos, u, v);
            let world_u = uv.x.clamp(0.0, 1.0);
            let world_v = uv.y.clamp(0.0, 1.0);

            // Surface height (procedural or from the cached heightmap).
            let height = grid[z as usize * grid_stride + x as usize];

            // Local position within chunk
            let local_x = u * size;
            let local_z = v * size;

            positions.push([local_x, height, local_z]);
            uvs.push([u, v]);

            // Neighbour heights (metres) — sampled ONCE and reused for both the
            // seamless normal and the baked ambient-occlusion below. Central
            // difference over a GLOBAL function of world coordinates (the noise
            // context for procedural, the shared height cache for disk data),
            // never the chunk-local grid — so a border vertex gets identical
            // values from either neighbouring chunk (no shading seam, and the
            // AO stays continuous across chunk borders too).
            let (hl, hr, hd, hup) = if use_procedural {
                (
                    noise_context.sample_height(world_x - step_m, world_z),
                    noise_context.sample_height(world_x + step_m, world_z),
                    noise_context.sample_height(world_x, world_z - step_m),
                    noise_context.sample_height(world_x, world_z + step_m),
                )
            } else {
                (
                    config.world_height(data.sample_height((world_u - du).clamp(0.0, 1.0), world_v)),
                    config.world_height(data.sample_height((world_u + du).clamp(0.0, 1.0), world_v)),
                    config.world_height(data.sample_height(world_u, (world_v - dv).clamp(0.0, 1.0))),
                    config.world_height(data.sample_height(world_u, (world_v + dv).clamp(0.0, 1.0))),
                )
            };
            let ddx = (hr - hl) / (2.0 * step_m);
            let ddz = (hup - hd) / (2.0 * step_m);
            let nrm = Vec3::new(-ddx, 1.0, -ddz).normalize();
            normals.push(nrm.to_array());
            colors.push(shading.color(world_u, world_v, world_x, world_z, height, [hl, hr, hd, hup], nrm));
        }
    }

    // Generate indices for triangle list: two triangles per quad that has
    // ground, which off a sparse surface is every quad.
    let ground = chunk_ground_quads(chunk_pos, resolution, config, data);
    let quad_count = (resolution * resolution) as usize;
    let mut indices: Vec<u32> = Vec::with_capacity(quad_count * 6);

    for z in 0..resolution {
        for x in 0..resolution {
            if ground.as_ref().is_some_and(|quads| !quads[(z * resolution + x) as usize]) {
                continue;
            }
            let i = z * (resolution + 1) + x;
            for triangle in quad_triangles(i, resolution + 1) {
                indices.extend_from_slice(&triangle);
            }
        }
    }

    // Add skirts for LOD seam hiding
    add_skirts(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut colors,
        &mut indices,
        resolution,
        size,
        height_scale,
        ground.as_deref(),
    );
    
    // Build mesh
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    
    meshes.add(mesh)
}

/// Legacy wrapper — kept for external callers that don't have a TerrainNoiseContext.
/// Internally creates a one-off context. For bulk mesh generation, prefer
/// TerrainNoiseContext::sample_height() which amortises the allocation cost.
#[allow(dead_code)]
fn sample_perlin_height(x: f32, z: f32, seed: u32, scale: f32) -> f32 {
    let context = TerrainNoiseContext::new(seed, scale);
    context.sample_height(x, z)
}

/// Legacy wrapper for mountain ridge sampling.
/// Prefer TerrainNoiseContext::sample_mountain_ridges() for bulk generation.
#[allow(dead_code)]
fn sample_mountain_ridges(x: f32, z: f32, seed: u32) -> f32 {
    let perlin = Perlin::new(seed);
    let mut height = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 0.0008_f32;
    let mut weight = 1.0;
    for i in 0..5 {
        let noise = perlin.get([
            x as f64 * frequency as f64 + i as f64 * 100.0,
            z as f64 * frequency as f64 + i as f64 * 100.0,
        ]) as f32;
        let mut ridge = 1.0 - noise.abs();
        ridge = ridge * ridge;
        ridge *= weight;
        weight = ridge.clamp(0.0, 1.0);
        height += ridge * amplitude;
        amplitude *= 0.5;
        frequency *= 2.2;
    }
    height = height / 2.0;
    height.powf(1.3)
}

/// Fast hash-based noise fallback (for no-deps mode or quick sampling)
#[allow(dead_code)]
fn hash_noise(x: f32, z: f32, seed: u32) -> f32 {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let fx = x - x.floor();
    let fz = z - z.floor();
    
    // Smoothstep interpolation
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uz = fz * fz * (3.0 - 2.0 * fz);
    
    // Corner values with seed
    let v00 = hash2d(ix, iz, seed);
    let v10 = hash2d(ix + 1, iz, seed);
    let v01 = hash2d(ix, iz + 1, seed);
    let v11 = hash2d(ix + 1, iz + 1, seed);
    
    // Bilinear interpolation
    let v0 = v00 + (v10 - v00) * ux;
    let v1 = v01 + (v11 - v01) * ux;
    
    v0 + (v1 - v0) * uz
}

/// Hash function for 2D coordinates
fn hash2d(x: i32, z: i32, seed: u32) -> f32 {
    let n = x.wrapping_mul(374761393)
        .wrapping_add(z.wrapping_mul(668265263))
        .wrapping_add(seed as i32);
    let n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    let n = n ^ (n >> 16);
    (n as f32 / i32::MAX as f32).abs() * 2.0 - 1.0  // -1 to 1
}

/// Calculate smooth normals from vertex positions.
///
/// Superseded by the seamless (global-function) central-difference normals
/// computed inline in `generate_chunk_mesh` — this chunk-LOCAL version is
/// what produced the visible rectangular shading seams at chunk borders.
/// Kept for reference / potential reuse (e.g. a future GPU compute path).
#[allow(dead_code)]
fn calculate_normals(normals: &mut Vec<[f32; 3]>, positions: &[[f32; 3]], resolution: u32) {
    let stride = (resolution + 1) as usize;
    
    for z in 0..=resolution as usize {
        for x in 0..=resolution as usize {
            let idx = z * stride + x;
            
            // Get neighboring heights
            let h_left = if x > 0 { positions[idx - 1][1] } else { positions[idx][1] };
            let h_right = if x < resolution as usize { positions[idx + 1][1] } else { positions[idx][1] };
            let h_down = if z > 0 { positions[idx - stride][1] } else { positions[idx][1] };
            let h_up = if z < resolution as usize { positions[idx + stride][1] } else { positions[idx][1] };
            
            // Calculate normal from height differences
            let dx = h_right - h_left;
            let dz = h_up - h_down;
            
            let normal = Vec3::new(-dx, 2.0, -dz).normalize();
            normals[idx] = normal.to_array();
        }
    }
}

/// Add skirts to hide LOD seams between chunks at different LOD levels
///
/// Skirts are vertical strips extending downward from chunk edges that
/// prevent gaps from appearing when adjacent chunks have different resolutions.
///
/// Each skirt segment hangs below the border edge of one border quad, and
/// `ground` (the chunk's [`chunk_ground_quads`], `None` when every quad has
/// ground) leaves it out whenever that quad is left out. That is exactly when
/// one of the segment's two top vertices belongs to no kept quad of the chunk:
/// a hole on either top vertex drops every quad around that vertex, and a
/// hole on either inner corner drops both border quads around the top vertex
/// beside it. So no skirt hangs from ground the chunk does not draw. Every
/// skirt vertex is still added, so the vertex count does not depend on the
/// holes.
#[allow(clippy::too_many_arguments)]
fn add_skirts(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    colors: &mut Vec<[f32; 4]>,
    indices: &mut Vec<u32>,
    resolution: u32,
    size: f32,
    _height_scale: f32,
    ground: Option<&[bool]>,
) {
    let skirt_depth = skirt_depth(size);
    let stride = resolution + 1;
    let base_vertex_count = positions.len() as u32;
    // Whether the segment below the border edge of quad `(x, z)` hangs.
    let hangs = |x: u32, z: u32| ground.map_or(true, |quads| quads[(z * resolution + x) as usize]);
    
    // Add skirt vertices for each edge
    // Bottom edge (z = 0)
    for x in 0..=resolution {
        let idx = x as usize;
        let pos = positions[idx];
        positions.push([pos[0], pos[1] + skirt_depth, pos[2]]);
        normals.push(normals[idx]);
        uvs.push(uvs[idx]);
        colors.push(colors[idx]);
    }
    
    // Top edge (z = resolution)
    for x in 0..=resolution {
        let idx = (resolution * stride + x) as usize;
        let pos = positions[idx];
        positions.push([pos[0], pos[1] + skirt_depth, pos[2]]);
        normals.push(normals[idx]);
        uvs.push(uvs[idx]);
        colors.push(colors[idx]);
    }
    
    // Left edge (x = 0)
    for z in 0..=resolution {
        let idx = (z * stride) as usize;
        let pos = positions[idx];
        positions.push([pos[0], pos[1] + skirt_depth, pos[2]]);
        normals.push(normals[idx]);
        uvs.push(uvs[idx]);
        colors.push(colors[idx]);
    }
    
    // Right edge (x = resolution)
    for z in 0..=resolution {
        let idx = (z * stride + resolution) as usize;
        let pos = positions[idx];
        positions.push([pos[0], pos[1] + skirt_depth, pos[2]]);
        normals.push(normals[idx]);
        uvs.push(uvs[idx]);
        colors.push(colors[idx]);
    }
    
    // Generate skirt triangles connecting edge vertices to skirt vertices
    // Skirts face outward from the chunk (away from center)
    
    // Bottom edge triangles (face -Z direction)
    let bottom_skirt_start = base_vertex_count;
    for x in 0..resolution {
        if !hangs(x, 0) {
            continue;
        }
        let top_left = x;
        let top_right = x + 1;
        let bottom_left = bottom_skirt_start + x;
        let bottom_right = bottom_skirt_start + x + 1;
        
        // CCW winding facing -Z
        indices.push(top_left);
        indices.push(top_right);
        indices.push(bottom_left);
        
        indices.push(top_right);
        indices.push(bottom_right);
        indices.push(bottom_left);
    }
    
    // Top edge triangles (face +Z direction)
    let top_skirt_start = bottom_skirt_start + stride;
    for x in 0..resolution {
        if !hangs(x, resolution - 1) {
            continue;
        }
        let top_left = resolution * stride + x;
        let top_right = resolution * stride + x + 1;
        let bottom_left = top_skirt_start + x;
        let bottom_right = top_skirt_start + x + 1;
        
        // CCW winding facing +Z
        indices.push(top_left);
        indices.push(bottom_left);
        indices.push(top_right);
        
        indices.push(top_right);
        indices.push(bottom_left);
        indices.push(bottom_right);
    }
    
    // Left edge triangles (face -X direction)
    let left_skirt_start = top_skirt_start + stride;
    for z in 0..resolution {
        if !hangs(0, z) {
            continue;
        }
        let top_top = z * stride;
        let top_bottom = (z + 1) * stride;
        let bottom_top = left_skirt_start + z;
        let bottom_bottom = left_skirt_start + z + 1;
        
        // CCW winding facing -X
        indices.push(top_top);
        indices.push(bottom_top);
        indices.push(top_bottom);
        
        indices.push(top_bottom);
        indices.push(bottom_top);
        indices.push(bottom_bottom);
    }
    
    // Right edge triangles (face +X direction)
    let right_skirt_start = left_skirt_start + stride;
    for z in 0..resolution {
        if !hangs(resolution - 1, z) {
            continue;
        }
        let top_top = z * stride + resolution;
        let top_bottom = (z + 1) * stride + resolution;
        let bottom_top = right_skirt_start + z;
        let bottom_bottom = right_skirt_start + z + 1;
        
        // CCW winding facing +X
        indices.push(top_top);
        indices.push(top_bottom);
        indices.push(bottom_top);

        indices.push(top_bottom);
        indices.push(bottom_bottom);
        indices.push(bottom_top);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::terrain::material::{material_cell, MaterialCell, MATERIAL_SLOT_NONE};

    const NO_MATERIAL: MaterialCell = [MATERIAL_SLOT_NONE, MATERIAL_SLOT_NONE, 0, 0];

    /// 3 x 3 chunks of 32 m at 8 cells: a 24 x 24 raster sloping gently
    /// along X, all Grass. LOD 0 meshes a chunk at 8 quads a side, 4 m each.
    fn config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 8,
            chunks_x: 1,
            chunks_z: 1,
            height_scale: 10.0,
            ..TerrainConfig::default()
        }
    }

    fn grassy(config: &TerrainConfig) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        let w = data.cache_width as usize;
        for (i, h) in data.height_cache.iter_mut().enumerate() {
            *h = 0.3 + 0.01 * (i % w) as f32;
        }
        data.material_cache = vec![material_cell(TerrainMaterial::Grass.to_u8()); data.height_cache.len()];
        data
    }

    /// Raster index of the cell LOD-0 vertex `(x, z)` of `chunk` stands on.
    fn vertex_cell(config: &TerrainConfig, data: &TerrainData, chunk: IVec2, x: u32, z: u32) -> usize {
        let r = config.resolution_for_lod(0) as f32;
        let uv = config.chunk_point_uv(chunk, x as f32 / r, z as f32 / r);
        let (cx, cz) = data.cell_at_uv(uv.x.clamp(0.0, 1.0), uv.y.clamp(0.0, 1.0));
        cz * data.cache_width as usize + cx
    }

    /// Makes the cell under LOD-0 vertex `(x, z)` of `chunk` a hole, after
    /// checking that no other vertex of the chunk stands on it, so the quads
    /// it takes are the ones around that vertex.
    fn hole_under(config: &TerrainConfig, data: &mut TerrainData, chunk: IVec2, x: u32, z: u32) {
        let r = config.resolution_for_lod(0);
        let cell = vertex_cell(config, data, chunk, x, z);
        let sharing = (0..=r)
            .flat_map(|b| (0..=r).map(move |a| (a, b)))
            .filter(|&(a, b)| vertex_cell(config, data, chunk, a, b) == cell)
            .count();
        assert_eq!(sharing, 1, "vertex ({x}, {z}) shares its cell with another vertex");
        data.material_cache[cell] = NO_MATERIAL;
    }

    /// The quads the LOD-`lod` mesh of `chunk` draws, as `(x, z)`, and the
    /// skirt segments it hangs.
    fn drawn(chunk: IVec2, lod: u32, config: &TerrainConfig, data: &TerrainData) -> (HashSet<(u32, u32)>, usize) {
        let mut meshes = Assets::<Mesh>::default();
        let handle = generate_chunk_mesh(chunk, lod, config, data, &mut meshes);
        let mesh = meshes.get(&handle).expect("mesh was just added");
        let stride = config.resolution_for_lod(lod) + 1;
        assert_eq!(mesh.count_vertices(), (stride * stride + 4 * stride) as usize, "every vertex is kept");
        let indices: &[u32] = match mesh.indices() {
            Some(Indices::U32(values)) => values.as_slice(),
            _ => panic!("terrain mesh indices are U32"),
        };
        assert_eq!(indices.len() % 6, 0, "whole quads and whole skirt segments");
        let mut quads = HashSet::new();
        let mut skirts = 0;
        for pair in indices.chunks_exact(6) {
            // A skirt segment's pair of triangles uses a skirt vertex.
            if pair.iter().all(|&i| i < stride * stride) {
                assert!(quads.insert((pair[0] % stride, pair[0] / stride)), "a quad is drawn twice");
            } else {
                skirts += 1;
            }
        }
        (quads, skirts)
    }

    fn every_quad(resolution: u32) -> HashSet<(u32, u32)> {
        (0..resolution).flat_map(|z| (0..resolution).map(move |x| (x, z))).collect()
    }

    #[test]
    fn a_hole_drops_exactly_the_quads_touching_its_vertex() {
        let config = config();
        let r = config.resolution_for_lod(0);
        let mut data = grassy(&config);
        data.sparse_surface = true;
        hole_under(&config, &mut data, IVec2::ZERO, 2, 3);

        let (quads, skirts) = drawn(IVec2::ZERO, 0, &config, &data);
        let mut expected = every_quad(r);
        for quad in [(1, 2), (2, 2), (1, 3), (2, 3)] {
            assert!(expected.remove(&quad));
        }
        assert_eq!(quads, expected);
        assert_eq!(skirts, 4 * r as usize, "no border quad went, so every skirt segment hangs");
        let kept = chunk_ground_quads(IVec2::ZERO, r, &config, &data).expect("the chunk has a hole");
        assert_eq!(kept.iter().filter(|kept| !**kept).count(), 4);
        // The chunk beside it stands on none of the hole.
        assert_eq!(chunk_ground_quads(IVec2::new(1, 0), r, &config, &data), None);
    }

    #[test]
    fn a_hole_at_the_border_takes_the_skirt_below_its_quads() {
        let config = config();
        let r = config.resolution_for_lod(0);
        // On the -Z border, and one row in: either way two border quads go,
        // and so do the two skirt segments below them.
        for (x, z, gone, dropped) in [(3, 0, [(2, 0), (3, 0)], 2), (6, 1, [(5, 0), (6, 0)], 4)] {
            let mut data = grassy(&config);
            data.sparse_surface = true;
            hole_under(&config, &mut data, IVec2::ZERO, x, z);
            let (quads, skirts) = drawn(IVec2::ZERO, 0, &config, &data);
            assert_eq!(quads.len(), (r * r) as usize - dropped, "hole at ({x}, {z})");
            assert!(gone.iter().all(|quad| !quads.contains(quad)), "hole at ({x}, {z})");
            assert_eq!(skirts, 4 * r as usize - 2, "hole at ({x}, {z})");
        }
    }

    #[test]
    fn a_chunk_without_ground_keeps_no_quad_at_any_lod() {
        let config = config();
        let mut data = grassy(&config);
        data.sparse_surface = true;
        // Every cell of chunk (0, 0)'s tile is a hole; the chunks around it
        // keep their ground.
        let side = config.chunk_resolution;
        let tile = config.chunk_grid_index(IVec2::ZERO).expect("on the grid") * side;
        let w = data.cache_width as usize;
        for z in tile.y..tile.y + side {
            for x in tile.x..tile.x + side {
                data.material_cache[z as usize * w + x as usize] = NO_MATERIAL;
            }
        }
        assert!(!data.chunk_has_ground(&config, IVec2::ZERO));
        assert!(data.chunk_has_ground(&config, IVec2::new(1, 0)));
        for lod in 0..config.lod_levels {
            assert_eq!(drawn(IVec2::ZERO, lod, &config, &data), (HashSet::new(), 0), "LOD {lod}");
        }
        let (positions, triangles) = chunk_ground_triangles(IVec2::ZERO, &config, &data).expect("the chunk has holes");
        assert!(triangles.is_empty());
        assert_eq!(positions.len(), 81);
        assert!(!drawn(IVec2::new(1, 0), 0, &config, &data).0.is_empty());
    }

    #[test]
    fn a_full_surface_keeps_every_quad_whatever_its_materials() {
        let config = config();
        let r = config.resolution_for_lod(0);
        let mut data = grassy(&config);
        data.material_cache.fill(NO_MATERIAL);
        assert!(!data.sparse_surface);
        assert_eq!(chunk_ground_quads(IVec2::ZERO, r, &config, &data), None);
        assert_eq!(chunk_ground_triangles(IVec2::ZERO, &config, &data), None);
        assert_eq!(drawn(IVec2::ZERO, 0, &config, &data), (every_quad(r), 4 * r as usize));
        assert!(data.chunk_has_ground(&config, IVec2::ZERO));
        assert!(ground_at_world(&config, &data, 10.0, 10.0));
    }

    #[test]
    fn the_ground_triangles_are_the_kept_quads_over_the_mesh_vertices() {
        let config = config();
        let r = config.resolution_for_lod(0);
        let mut data = grassy(&config);
        data.sparse_surface = true;
        hole_under(&config, &mut data, IVec2::ZERO, 2, 3);
        let (positions, triangles) = chunk_ground_triangles(IVec2::ZERO, &config, &data).expect("the chunk has a hole");
        assert_eq!(triangles.len(), 2 * (r * r - 4) as usize);

        // The vertices the render mesh draws, and its surface triangles.
        let mut meshes = Assets::<Mesh>::default();
        let handle = generate_chunk_mesh(IVec2::ZERO, 0, &config, &data, &mut meshes);
        let mesh = meshes.get(&handle).expect("mesh was just added");
        let mesh_positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|values| values.as_float3())
            .expect("terrain mesh positions are Float32x3");
        for (i, p) in positions.iter().enumerate() {
            assert_eq!(p.to_array(), mesh_positions[i], "vertex {i}");
        }
        let indices: &[u32] = match mesh.indices() {
            Some(Indices::U32(values)) => values.as_slice(),
            _ => panic!("terrain mesh indices are U32"),
        };
        let surface: Vec<u32> = triangles.iter().flatten().copied().collect();
        assert_eq!(indices[..surface.len()], surface[..]);
    }

    #[test]
    fn ground_at_world_follows_the_kept_quads() {
        let config = config();
        let mut data = grassy(&config);
        hole_under(&config, &mut data, IVec2::ZERO, 2, 3);
        // 4 m quads: quad (1, 2) of chunk (0, 0), around (6, 10), touches the
        // hole's vertex; quad (4, 4), around (18, 18), does not.
        assert!(ground_at_world(&config, &data, 6.0, 10.0), "a full surface has ground everywhere");
        data.sparse_surface = true;
        assert!(!ground_at_world(&config, &data, 6.0, 10.0));
        assert!(ground_at_world(&config, &data, 18.0, 18.0));
        assert!(!ground_at_world(&config, &data, 500.0, 18.0), "off the grid");
    }
}
