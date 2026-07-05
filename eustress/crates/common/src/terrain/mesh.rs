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
use super::material::{height_to_color, HeightBlendParams};

// ── Vertex-colour realism knobs (see the baking block in generate_chunk_mesh) ──
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

// ── Splat-bucket display colours (sRGB) for the vertex-colour blend ──
// The 4-bucket splat is [grass, rock, dirt, snow]. Bucket 3 conflates
// snow + water + ice + glacier (see TerrainMaterial::splat_bucket); it's
// resolved PER-VERTEX by altitude below (peaks → snow, lowlands → water).
// Temperate worlds are almost all water (rivers/lakes) and essentially no
// snow, so colouring bucket 3 as flat snow-white made every waterway read as
// blinding white confetti — water-blue for the lowlands fixes that.
const BUCKET_GRASS: [f32; 3] = [0.34, 0.52, 0.24];
const BUCKET_ROCK: [f32; 3] = [0.48, 0.44, 0.40];
const BUCKET_DIRT: [f32; 3] = [0.55, 0.42, 0.30];
const BUCKET_SNOW: [f32; 3] = [0.92, 0.93, 0.96];
const BUCKET_WATER: [f32; 3] = [0.20, 0.40, 0.58];
/// Fraction of the height band above which bucket-3 reads as snow, not water.
const SNOW_ALT_FRAC: f32 = 0.72;

/// Bilinearly-sample the 4 splat weights `[grass, rock, dirt, snow]` at a
/// global height-cache UV. Returns zeros when no splatmap has been loaded
/// (the mesh then falls back to a height-band colour). Layout:
/// `splat_cache[pixel*4 + c]`.
///
/// Bilinear (not nearest) matters here: `export.rs` already bakes a 3x3
/// smoothing kernel into the splatmap PNG so material boundaries are soft
/// gradients, not one-hot steps — but NEAREST sampling re-quantised that
/// softness back into hard blocks at mesh-vertex resolution, which is what
/// produced the visibly jagged, non-blending material transitions. Bilinear
/// interpolation between the 4 nearest cache pixels preserves the gradient.
fn sample_splat_weights(data: &TerrainData, u: f32, v: f32) -> [f32; 4] {
    let w = data.cache_width as usize;
    let h = data.cache_height as usize;
    if data.splat_cache.is_empty() || w == 0 || h == 0 {
        return [0.0; 4];
    }
    let px = u.clamp(0.0, 1.0) * (w.saturating_sub(1)) as f32;
    let pz = v.clamp(0.0, 1.0) * (h.saturating_sub(1)) as f32;

    let x0 = px.floor() as usize;
    let z0 = pz.floor() as usize;
    let x1 = (x0 + 1).min(w.saturating_sub(1));
    let z1 = (z0 + 1).min(h.saturating_sub(1));
    let fx = px - x0 as f32;
    let fz = pz - z0 as f32;

    let sample = |x: usize, z: usize| -> [f32; 4] {
        let idx = (z * w + x) * 4;
        if idx + 3 < data.splat_cache.len() {
            [
                data.splat_cache[idx],
                data.splat_cache[idx + 1],
                data.splat_cache[idx + 2],
                data.splat_cache[idx + 3],
            ]
        } else {
            [0.0; 4]
        }
    };

    let s00 = sample(x0, z0);
    let s10 = sample(x1, z0);
    let s01 = sample(x0, z1);
    let s11 = sample(x1, z1);

    let mut out = [0.0f32; 4];
    for c in 0..4 {
        let top = s00[c] + (s10[c] - s00[c]) * fx;
        let bottom = s01[c] + (s11[c] - s01[c]) * fx;
        out[c] = top + (bottom - top) * fz;
    }
    out
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

/// Generate mesh for a terrain chunk
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
    let has_splat = !use_procedural && !data.splat_cache.is_empty();
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
    let fallback_blend = HeightBlendParams::default();

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

            // Sample height (procedural or from cached heightmap).
            let height = if use_procedural {
                noise_context.sample_height(world_x, world_z)
            } else {
                data.sample_height(world_u, world_v) * height_scale
            };

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
                    data.sample_height((world_u - du).clamp(0.0, 1.0), world_v) * height_scale,
                    data.sample_height((world_u + du).clamp(0.0, 1.0), world_v) * height_scale,
                    data.sample_height(world_u, (world_v - dv).clamp(0.0, 1.0)) * height_scale,
                    data.sample_height(world_u, (world_v + dv).clamp(0.0, 1.0)) * height_scale,
                )
            };
            let ddx = (hr - hl) / (2.0 * step_m);
            let ddz = (hup - hd) / (2.0 * step_m);
            let nrm = Vec3::new(-ddx, 1.0, -ddz).normalize();
            normals.push(nrm.to_array());

            // Base material colour. From the splatmap (grass/rock/dirt +
            // snow-or-water) when present, else a height-band fallback so
            // procedural / splat-less terrain keeps its previous look. Bucket 3
            // is snow+water conflated by splat_bucket(); resolve it per-vertex
            // by altitude (peaks → snow, lowlands → water) so rivers/lakes read
            // as water instead of blinding snow-white. The terrain
            // StandardMaterial base_color is white, so this vertex colour shows
            // through directly.
            let lin = if has_splat {
                let w = sample_splat_weights(data, world_u, world_v);
                let sum = (w[0] + w[1] + w[2] + w[3]).max(1e-4);
                let bucket3 = if height >= height_scale * SNOW_ALT_FRAC {
                    BUCKET_SNOW
                } else {
                    BUCKET_WATER
                };
                let cols = [BUCKET_GRASS, BUCKET_ROCK, BUCKET_DIRT, bucket3];
                let mut srgb = [0.0f32; 3];
                for (bkt, col) in cols.iter().enumerate() {
                    let wn = w[bkt] / sum;
                    srgb[0] += col[0] * wn;
                    srgb[1] += col[1] * wn;
                    srgb[2] += col[2] * wn;
                }
                Color::srgb(srgb[0], srgb[1], srgb[2]).to_linear()
            } else {
                height_to_color(height, &fallback_blend).to_linear()
            };

            // Bake cheap realism into the vertex colour (see the knob consts):
            //  1. curvature AO — Laplacian `mean(neighbours) - h` is >0 in
            //     concavities (darken → occlusion) and <0 on ridges (lift);
            //  2. slope shade — scale by `nrm.y` (cos slope) for soft cliff
            //     self-shadow;
            //  3. macro value-noise — low-frequency world-space patchiness so
            //     the uniform per-material fill stops reading as flat paint.
            let laplacian = (hl + hr + hd + hup) * 0.25 - height;
            let ao = (1.0 - laplacian * AO_STRENGTH).clamp(AO_MIN, RIDGE_MAX);
            let slope_shade = SLOPE_MIN + (1.0 - SLOPE_MIN) * nrm.y.clamp(0.0, 1.0);
            let variation = 1.0
                + MACRO_VARIATION
                    * hash_noise(world_x * MACRO_FREQ, world_z * MACRO_FREQ, seed ^ 0x9E37);
            let shade = (ao * slope_shade * variation).clamp(0.35, 1.2);
            colors.push([lin.red * shade, lin.green * shade, lin.blue * shade, 1.0]);
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
    // Skirt depth proportional to chunk size (5% of chunk size, minimum 2 units)
    let skirt_depth = -(size * 0.05).max(2.0);
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
