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
    
    // Height sampling
    for z in 0..=resolution {
        for x in 0..=resolution {
            let u = x as f32 / resolution as f32;
            let v = z as f32 / resolution as f32;
            
            // World position for this vertex
            let world_x = chunk_pos.x as f32 * size + u * size;
            let world_z = chunk_pos.y as f32 * size + v * size;
            
            // Sample height (procedural or from data)
            let height = if data.height_cache.is_empty() {
                // Procedural height using multi-octave Perlin noise
                sample_perlin_height(world_x, world_z, seed, height_scale)
            } else {
                // Sample from cached heightmap (world UV)
                let total_chunks_x = (config.chunks_x * 2 + 1) as f32;
                let total_chunks_z = (config.chunks_z * 2 + 1) as f32;
                let world_u = ((chunk_pos.x as f32 + u + config.chunks_x as f32) / total_chunks_x).clamp(0.0, 1.0);
                let world_v = ((chunk_pos.y as f32 + v + config.chunks_z as f32) / total_chunks_z).clamp(0.0, 1.0);
                data.sample_height(world_u, world_v) * height_scale
            };
            
            // Local position within chunk
            let local_x = u * size;
            let local_z = v * size;
            
            positions.push([local_x, height, local_z]);
            uvs.push([u, v]);
            
            // Placeholder normal (will be calculated after)
            normals.push([0.0, 1.0, 0.0]);
        }
    }
    
    // Calculate normals from neighboring vertices
    calculate_normals(&mut normals, &positions, resolution);
    
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
    add_skirts(&mut positions, &mut normals, &mut uvs, &mut indices, resolution, size, height_scale);
    
    // Build mesh
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    
    meshes.add(mesh)
}

/// Sample height using realistic terrain generation
/// Creates varied terrain with plains, rolling hills, and mountain ranges
fn sample_perlin_height(x: f32, z: f32, seed: u32, scale: f32) -> f32 {
    let perlin = Perlin::new(seed);
    let perlin2 = Perlin::new(seed + 1000);
    let perlin3 = Perlin::new(seed + 2000);
    
    // ==========================================================
    // Layer 1: Continental/Biome mask (very large scale)
    // Determines where mountains vs plains are located
    // ==========================================================
    let continent_freq = 0.0003;  // Very large features
    let continent = perlin.get([x as f64 * continent_freq, z as f64 * continent_freq]) as f32;
    let continent = (continent + 1.0) * 0.5;  // 0 to 1
    
    // Create distinct biome regions (reserved for future biome-based terrain)
    let biome_freq = 0.0006;
    let _biome = perlin2.get([x as f64 * biome_freq, z as f64 * biome_freq]) as f32;
    let _biome = (_biome + 1.0) * 0.5;
    
    // ==========================================================
    // Layer 2: Base terrain shape (medium scale)
    // ==========================================================
    let base_freq = 0.001;
    let base_terrain: Fbm<Perlin> = Fbm::new(seed + 100)
        .set_octaves(4)
        .set_frequency(base_freq)
        .set_lacunarity(2.0)
        .set_persistence(0.5);
    let base = base_terrain.get([x as f64, z as f64]) as f32;
    
    // ==========================================================
    // Layer 3: Mountain ridges (using ridged multifractal)
    // ==========================================================
    let mountain_height = sample_mountain_ridges(x, z, seed + 3000);
    
    // ==========================================================
    // Layer 4: Fine detail (small scale noise)
    // ==========================================================
    let detail_freq = 0.008;
    let detail = perlin3.get([x as f64 * detail_freq, z as f64 * detail_freq]) as f32 * 0.1;
    
    // ==========================================================
    // Combine layers based on biome
    // ==========================================================
    
    // Mountain influence: high where continent and biome align
    let mountain_mask = (continent * 1.5 - 0.3).clamp(0.0, 1.0);
    let mountain_mask = mountain_mask * mountain_mask;  // Sharper falloff
    
    // Plains are where mountains aren't
    let plains_mask = 1.0 - mountain_mask;
    
    // Rolling hills in transition zones
    let hills_mask = (1.0 - (mountain_mask - 0.5).abs() * 2.0).clamp(0.0, 1.0);
    
    // Combine terrain types
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
        height = height * 0.3;  // Flatten low plains
    }
    
    height * scale
}

/// Generate realistic mountain ridges using ridged multifractal noise
fn sample_mountain_ridges(x: f32, z: f32, seed: u32) -> f32 {
    let perlin = Perlin::new(seed);
    
    let mut height = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 0.0008;  // Start with large mountain ranges
    let mut weight = 1.0;
    
    // Ridged multifractal - creates sharp mountain ridges
    for i in 0..5 {
        let noise = perlin.get([
            x as f64 * frequency as f64 + i as f64 * 100.0,
            z as f64 * frequency as f64 + i as f64 * 100.0
        ]) as f32;
        
        // Ridged: absolute value creates valleys, invert for peaks
        let mut ridge = 1.0 - noise.abs();
        ridge = ridge * ridge;  // Square for sharper ridges
        
        // Weight by previous octave for more natural look
        ridge *= weight;
        weight = ridge.clamp(0.0, 1.0);
        
        height += ridge * amplitude;
        
        amplitude *= 0.5;
        frequency *= 2.2;  // Slightly higher lacunarity for mountains
    }
    
    // Normalize and shape
    height = height / 2.0;  // Normalize roughly to 0-1
    
    // Apply power curve for more dramatic peaks
    height = height.powf(1.3);
    
    height
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

/// Calculate smooth normals from vertex positions
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
    }
    
    // Top edge (z = resolution)
    for x in 0..=resolution {
        let idx = (resolution * stride + x) as usize;
        let pos = positions[idx];
        positions.push([pos[0], pos[1] + skirt_depth, pos[2]]);
        normals.push(normals[idx]);
        uvs.push(uvs[idx]);
    }
    
    // Left edge (x = 0)
    for z in 0..=resolution {
        let idx = (z * stride) as usize;
        let pos = positions[idx];
        positions.push([pos[0], pos[1] + skirt_depth, pos[2]]);
        normals.push(normals[idx]);
        uvs.push(uvs[idx]);
    }
    
    // Right edge (x = resolution)
    for z in 0..=resolution {
        let idx = (z * stride + resolution) as usize;
        let pos = positions[idx];
        positions.push([pos[0], pos[1] + skirt_depth, pos[2]]);
        normals.push(normals[idx]);
        uvs.push(uvs[idx]);
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
