//! Terrain mesh generation
//!
//! Generates chunk meshes with:
//! - Multi-octave Perlin noise for realistic height
//! - LOD-aware resolution
//! - Skirts for seamless LOD transitions
//! - Smooth normals

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
    let total_chunks_x = (config.chunks_x * 2 + 1) as f32;
    let total_chunks_z = (config.chunks_z * 2 + 1) as f32;

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
                    let world_u = ((chunk_pos.x as f32 + u + config.chunks_x as f32) / total_chunks_x).clamp(0.0, 1.0);
                    let world_v = ((chunk_pos.y as f32 + v + config.chunks_z as f32) / total_chunks_z).clamp(0.0, 1.0);
                    config.world_height(data.sample_height(world_u, world_v))
                }
            };
            heights.push(height);
        }
    }
    heights
}

/// Generate the heightfield mesh for a terrain chunk.
///
/// Systems that mesh chunks call [`super::generate_chunk_render_mesh`]
/// instead, which draws a chunk holding volumetric edits with marching cubes
/// at LOD 0 and comes here for everything else.
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
            let world_u = ((chunk_pos.x as f32 + u + config.chunks_x as f32) / total_chunks_x).clamp(0.0, 1.0);
            let world_v = ((chunk_pos.y as f32 + v + config.chunks_z as f32) / total_chunks_z).clamp(0.0, 1.0);

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

    // Generate indices for triangle list
    let quad_count = (resolution * resolution) as usize;
    let mut indices: Vec<u32> = Vec::with_capacity(quad_count * 6);
    
    for z in 0..resolution {
        for x in 0..resolution {
            let i = z * (resolution + 1) + x;
            
            // Two triangles per quad (counter-clockwise winding for front face)
            // Triangle 1: bottom-left, top-left, bottom-right
            indices.push(i);
            indices.push(i + resolution + 1);
            indices.push(i + 1);
            
            // Triangle 2: bottom-right, top-left, top-right
            indices.push(i + 1);
            indices.push(i + resolution + 1);
            indices.push(i + resolution + 2);
        }
    }
    
    // Add skirts for LOD seam hiding
    add_skirts(&mut positions, &mut normals, &mut uvs, &mut colors, &mut indices, resolution, size, height_scale);
    
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
fn add_skirts(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    colors: &mut Vec<[f32; 4]>,
    indices: &mut Vec<u32>,
    resolution: u32,
    size: f32,
    _height_scale: f32,
) {
    let skirt_depth = skirt_depth(size);
    let stride = resolution + 1;
    let base_vertex_count = positions.len() as u32;
    
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
