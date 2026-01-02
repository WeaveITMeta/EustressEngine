// ============================================================================
// Mesh Optimization - Convert Point Clouds to Optimized Geometry
// ============================================================================
//
// Converts high-detail point clouds and meshes to optimized formats for
// Eustress Client. Supports:
// - Surface reconstruction from points
// - Mesh simplification (quadric error decimation)
// - UV unwrapping for texture baking
// - Normal map generation
// - LOD mesh generation
// - Instancing optimization
//
// Table of Contents:
// 1. Surface Reconstruction
// 2. Mesh Simplification
// 3. LOD Generation
// 4. Texture Optimization
// 5. Instancing Detection
// ============================================================================

use bevy::prelude::*;
use std::collections::HashMap;

// ============================================================================
// 1. Surface Reconstruction
// ============================================================================

/// Surface reconstruction method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconstructionMethod {
    /// Ball pivoting algorithm
    BallPivoting,
    /// Poisson surface reconstruction
    Poisson,
    /// Screened Poisson reconstruction
    ScreenedPoisson,
    /// Marching cubes (voxel-based)
    MarchingCubes,
    /// Alpha shapes
    AlphaShapes,
    /// Delaunay triangulation (2.5D heightmap)
    Delaunay,
}

/// Surface reconstruction settings
#[derive(Debug, Clone)]
pub struct ReconstructionSettings {
    pub method: ReconstructionMethod,
    /// Ball radius for ball pivoting
    pub ball_radius: f32,
    /// Octree depth for Poisson
    pub octree_depth: u8,
    /// Voxel size for marching cubes
    pub voxel_size: f32,
    /// Alpha value for alpha shapes
    pub alpha: f32,
    /// Minimum triangle quality (0-1)
    pub min_quality: f32,
    /// Remove non-manifold geometry
    pub manifold_only: bool,
}

impl Default for ReconstructionSettings {
    fn default() -> Self {
        Self {
            method: ReconstructionMethod::Poisson,
            ball_radius: 0.05,
            octree_depth: 8,
            voxel_size: 0.02,
            alpha: 0.1,
            min_quality: 0.3,
            manifold_only: true,
        }
    }
}

/// Reconstructed mesh data
#[derive(Debug, Clone)]
pub struct ReconstructedMesh {
    pub vertices: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub indices: Vec<u32>,
    pub uvs: Option<Vec<Vec2>>,
    pub colors: Option<Vec<[f32; 4]>>,
}

impl ReconstructedMesh {
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
    
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
    
    pub fn calculate_bounds(&self) -> (Vec3, Vec3) {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        
        for v in &self.vertices {
            min = min.min(*v);
            max = max.max(*v);
        }
        
        (min, max)
    }
}

/// Reconstruct surface from point cloud
pub fn reconstruct_surface(
    points: &[Vec3],
    normals: Option<&[Vec3]>,
    settings: &ReconstructionSettings,
) -> Result<ReconstructedMesh, String> {
    match settings.method {
        ReconstructionMethod::MarchingCubes => {
            marching_cubes_reconstruction(points, settings)
        }
        ReconstructionMethod::Delaunay => {
            delaunay_reconstruction(points, normals)
        }
        _ => {
            // Other methods require external libraries
            Err(format!("{:?} reconstruction not yet implemented", settings.method))
        }
    }
}

/// Simple marching cubes reconstruction
fn marching_cubes_reconstruction(
    points: &[Vec3],
    settings: &ReconstructionSettings,
) -> Result<ReconstructedMesh, String> {
    if points.is_empty() {
        return Err("No points provided".to_string());
    }
    
    // Calculate bounds
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for p in points {
        min = min.min(*p);
        max = max.max(*p);
    }
    
    let size = max - min;
    let voxel_size = settings.voxel_size;
    let grid_size = (
        (size.x / voxel_size).ceil() as usize + 2,
        (size.y / voxel_size).ceil() as usize + 2,
        (size.z / voxel_size).ceil() as usize + 2,
    );
    
    // Create density grid
    let mut grid = vec![0.0f32; grid_size.0 * grid_size.1 * grid_size.2];
    let influence_radius = voxel_size * 2.0;
    let influence_sq = influence_radius * influence_radius;
    
    // Accumulate point influence
    for point in points {
        let local = (*point - min) / voxel_size;
        let ix = local.x.floor() as i32;
        let iy = local.y.floor() as i32;
        let iz = local.z.floor() as i32;
        
        // Influence nearby voxels
        for dx in -2..=2 {
            for dy in -2..=2 {
                for dz in -2..=2 {
                    let vx = (ix + dx).max(0) as usize;
                    let vy = (iy + dy).max(0) as usize;
                    let vz = (iz + dz).max(0) as usize;
                    
                    if vx >= grid_size.0 || vy >= grid_size.1 || vz >= grid_size.2 {
                        continue;
                    }
                    
                    let voxel_center = min + Vec3::new(
                        (vx as f32 + 0.5) * voxel_size,
                        (vy as f32 + 0.5) * voxel_size,
                        (vz as f32 + 0.5) * voxel_size,
                    );
                    
                    let dist_sq = (*point - voxel_center).length_squared();
                    if dist_sq < influence_sq {
                        let weight = 1.0 - (dist_sq / influence_sq).sqrt();
                        let idx = vx + vy * grid_size.0 + vz * grid_size.0 * grid_size.1;
                        grid[idx] += weight;
                    }
                }
            }
        }
    }
    
    // Marching cubes (simplified - generates vertices at voxel edges)
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let iso_level = 0.5;
    
    for z in 0..grid_size.2 - 1 {
        for y in 0..grid_size.1 - 1 {
            for x in 0..grid_size.0 - 1 {
                let idx = |dx: usize, dy: usize, dz: usize| {
                    (x + dx) + (y + dy) * grid_size.0 + (z + dz) * grid_size.0 * grid_size.1
                };
                
                // Sample 8 corners
                let corners = [
                    grid[idx(0, 0, 0)],
                    grid[idx(1, 0, 0)],
                    grid[idx(1, 1, 0)],
                    grid[idx(0, 1, 0)],
                    grid[idx(0, 0, 1)],
                    grid[idx(1, 0, 1)],
                    grid[idx(1, 1, 1)],
                    grid[idx(0, 1, 1)],
                ];
                
                // Calculate cube index
                let mut cube_index = 0u8;
                for (i, &c) in corners.iter().enumerate() {
                    if c > iso_level {
                        cube_index |= 1 << i;
                    }
                }
                
                // Skip empty or full cubes
                if cube_index == 0 || cube_index == 255 {
                    continue;
                }
                
                // Generate triangles (simplified - just center point)
                let center = min + Vec3::new(
                    (x as f32 + 0.5) * voxel_size,
                    (y as f32 + 0.5) * voxel_size,
                    (z as f32 + 0.5) * voxel_size,
                );
                
                let base_idx = vertices.len() as u32;
                
                // Add a small cube at surface voxels (placeholder for proper MC)
                let s = voxel_size * 0.4;
                vertices.extend_from_slice(&[
                    center + Vec3::new(-s, -s, -s),
                    center + Vec3::new( s, -s, -s),
                    center + Vec3::new( s,  s, -s),
                    center + Vec3::new(-s,  s, -s),
                    center + Vec3::new(-s, -s,  s),
                    center + Vec3::new( s, -s,  s),
                    center + Vec3::new( s,  s,  s),
                    center + Vec3::new(-s,  s,  s),
                ]);
                
                // Cube faces
                let cube_indices: [u32; 36] = [
                    0,1,2, 2,3,0, // front
                    4,6,5, 6,4,7, // back
                    0,4,5, 5,1,0, // bottom
                    2,6,7, 7,3,2, // top
                    0,3,7, 7,4,0, // left
                    1,5,6, 6,2,1, // right
                ];
                
                for i in cube_indices {
                    indices.push(base_idx + i);
                }
            }
        }
    }
    
    // Calculate normals
    let mut normals = vec![Vec3::ZERO; vertices.len()];
    for tri in indices.chunks(3) {
        if tri.len() < 3 { continue; }
        let v0 = vertices[tri[0] as usize];
        let v1 = vertices[tri[1] as usize];
        let v2 = vertices[tri[2] as usize];
        let normal = (v1 - v0).cross(v2 - v0).normalize_or_zero();
        normals[tri[0] as usize] += normal;
        normals[tri[1] as usize] += normal;
        normals[tri[2] as usize] += normal;
    }
    for n in &mut normals {
        *n = n.normalize_or_zero();
    }
    
    Ok(ReconstructedMesh {
        vertices,
        normals,
        indices,
        uvs: None,
        colors: None,
    })
}

/// Delaunay triangulation for heightmap-like data
fn delaunay_reconstruction(
    points: &[Vec3],
    normals: Option<&[Vec3]>,
) -> Result<ReconstructedMesh, String> {
    if points.len() < 3 {
        return Err("Need at least 3 points".to_string());
    }
    
    // Simple 2.5D triangulation (project to XZ, triangulate, use Y as height)
    // This is a placeholder - real implementation would use proper Delaunay
    
    let mut vertices = points.to_vec();
    let mut mesh_normals = normals.map(|n| n.to_vec())
        .unwrap_or_else(|| vec![Vec3::Y; vertices.len()]);
    
    // Sort points by X then Z for simple triangulation
    let mut sorted_indices: Vec<usize> = (0..points.len()).collect();
    sorted_indices.sort_by(|&a, &b| {
        let pa = points[a];
        let pb = points[b];
        pa.x.partial_cmp(&pb.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(pa.z.partial_cmp(&pb.z).unwrap_or(std::cmp::Ordering::Equal))
    });
    
    // Simple grid-based triangulation
    let mut indices = Vec::new();
    let grid_size = (points.len() as f32).sqrt() as usize;
    
    if grid_size > 1 {
        for row in 0..grid_size - 1 {
            for col in 0..grid_size - 1 {
                let i0 = row * grid_size + col;
                let i1 = i0 + 1;
                let i2 = i0 + grid_size;
                let i3 = i2 + 1;
                
                if i3 < sorted_indices.len() {
                    indices.push(sorted_indices[i0] as u32);
                    indices.push(sorted_indices[i1] as u32);
                    indices.push(sorted_indices[i2] as u32);
                    
                    indices.push(sorted_indices[i1] as u32);
                    indices.push(sorted_indices[i3] as u32);
                    indices.push(sorted_indices[i2] as u32);
                }
            }
        }
    }
    
    Ok(ReconstructedMesh {
        vertices,
        normals: mesh_normals,
        indices,
        uvs: None,
        colors: None,
    })
}

// ============================================================================
// 2. Mesh Simplification
// ============================================================================

/// Mesh simplification settings
#[derive(Debug, Clone)]
pub struct SimplificationSettings {
    /// Target triangle count (0 = use ratio)
    pub target_triangles: usize,
    /// Target ratio (0.5 = half triangles)
    pub target_ratio: f32,
    /// Preserve boundary edges
    pub preserve_boundaries: bool,
    /// Preserve UV seams
    pub preserve_uv_seams: bool,
    /// Maximum error threshold
    pub max_error: f32,
    /// Lock vertices at sharp edges
    pub preserve_sharp_edges: bool,
    /// Sharp edge angle threshold (degrees)
    pub sharp_angle: f32,
}

impl Default for SimplificationSettings {
    fn default() -> Self {
        Self {
            target_triangles: 0,
            target_ratio: 0.5,
            preserve_boundaries: true,
            preserve_uv_seams: true,
            max_error: 0.01,
            preserve_sharp_edges: true,
            sharp_angle: 30.0,
        }
    }
}

/// Simplify mesh using quadric error metrics
pub fn simplify_mesh(
    mesh: &ReconstructedMesh,
    settings: &SimplificationSettings,
) -> Result<ReconstructedMesh, String> {
    let target = if settings.target_triangles > 0 {
        settings.target_triangles
    } else {
        ((mesh.triangle_count() as f32) * settings.target_ratio) as usize
    };
    
    if target >= mesh.triangle_count() {
        return Ok(mesh.clone());
    }
    
    // Quadric error simplification (simplified implementation)
    let mut vertices = mesh.vertices.clone();
    let mut indices = mesh.indices.clone();
    let mut normals = mesh.normals.clone();
    
    // Build edge collapse priority queue
    let mut edge_costs: Vec<(f32, usize, usize)> = Vec::new();
    
    for tri in indices.chunks(3) {
        if tri.len() < 3 { continue; }
        let edges = [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])];
        
        for (i0, i1) in edges {
            let v0 = vertices[i0 as usize];
            let v1 = vertices[i1 as usize];
            let cost = (v1 - v0).length();  // Simple distance cost
            edge_costs.push((cost, i0 as usize, i1 as usize));
        }
    }
    
    edge_costs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    
    // Collapse edges until target reached
    let mut collapsed = std::collections::HashSet::new();
    let mut current_tris = mesh.triangle_count();
    
    for (cost, i0, i1) in edge_costs {
        if current_tris <= target { break; }
        if cost > settings.max_error { break; }
        if collapsed.contains(&i0) || collapsed.contains(&i1) { continue; }
        
        // Collapse i1 into i0
        let mid = (vertices[i0] + vertices[i1]) / 2.0;
        vertices[i0] = mid;
        normals[i0] = (normals[i0] + normals[i1]).normalize_or_zero();
        collapsed.insert(i1);
        
        // Update indices
        for idx in &mut indices {
            if *idx == i1 as u32 {
                *idx = i0 as u32;
            }
        }
        
        // Remove degenerate triangles
        let mut new_indices = Vec::new();
        for tri in indices.chunks(3) {
            if tri.len() < 3 { continue; }
            if tri[0] != tri[1] && tri[1] != tri[2] && tri[2] != tri[0] {
                new_indices.extend_from_slice(tri);
            }
        }
        indices = new_indices;
        current_tris = indices.len() / 3;
    }
    
    // Compact vertices
    let mut vertex_map: HashMap<usize, usize> = HashMap::new();
    let mut new_vertices = Vec::new();
    let mut new_normals = Vec::new();
    
    for idx in &mut indices {
        let old_idx = *idx as usize;
        let new_idx = *vertex_map.entry(old_idx).or_insert_with(|| {
            let i = new_vertices.len();
            new_vertices.push(vertices[old_idx]);
            new_normals.push(normals[old_idx]);
            i
        });
        *idx = new_idx as u32;
    }
    
    Ok(ReconstructedMesh {
        vertices: new_vertices,
        normals: new_normals,
        indices,
        uvs: None,
        colors: None,
    })
}

// ============================================================================
// 3. LOD Generation
// ============================================================================

/// LOD mesh set
#[derive(Debug, Clone)]
pub struct MeshLODSet {
    pub lods: Vec<ReconstructedMesh>,
    pub distances: Vec<f32>,
}

/// Generate LOD meshes
pub fn generate_mesh_lods(
    mesh: &ReconstructedMesh,
    lod_count: usize,
    ratios: &[f32],
) -> Result<MeshLODSet, String> {
    let mut lods = Vec::with_capacity(lod_count);
    let mut distances = Vec::with_capacity(lod_count);
    
    // LOD 0 is original
    lods.push(mesh.clone());
    distances.push(0.0);
    
    // Generate simplified LODs
    for (i, &ratio) in ratios.iter().enumerate().take(lod_count - 1) {
        let settings = SimplificationSettings {
            target_ratio: ratio,
            ..Default::default()
        };
        
        let simplified = simplify_mesh(mesh, &settings)?;
        lods.push(simplified);
        
        // Distance thresholds (exponential)
        distances.push(10.0 * (2.0_f32).powi(i as i32 + 1));
    }
    
    Ok(MeshLODSet { lods, distances })
}

// ============================================================================
// 4. Texture Optimization
// ============================================================================

/// Bake point cloud colors to texture
pub fn bake_colors_to_texture(
    mesh: &ReconstructedMesh,
    points: &[Vec3],
    colors: &[[f32; 4]],
    texture_size: u32,
) -> Result<Vec<u8>, String> {
    if mesh.uvs.is_none() {
        return Err("Mesh needs UVs for texture baking".to_string());
    }
    
    let uvs = mesh.uvs.as_ref().unwrap();
    let mut texture = vec![0u8; (texture_size * texture_size * 4) as usize];
    
    // Build spatial index for points
    let cell_size = 0.1;
    let mut spatial_hash: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
    
    for (i, point) in points.iter().enumerate() {
        let cell = (
            (point.x / cell_size).floor() as i32,
            (point.y / cell_size).floor() as i32,
            (point.z / cell_size).floor() as i32,
        );
        spatial_hash.entry(cell).or_default().push(i);
    }
    
    // For each triangle, sample colors
    for tri in mesh.indices.chunks(3) {
        if tri.len() < 3 { continue; }
        
        let v0 = mesh.vertices[tri[0] as usize];
        let v1 = mesh.vertices[tri[1] as usize];
        let v2 = mesh.vertices[tri[2] as usize];
        
        let uv0 = uvs[tri[0] as usize];
        let uv1 = uvs[tri[1] as usize];
        let uv2 = uvs[tri[2] as usize];
        
        // Rasterize triangle in UV space
        let min_u = uv0.x.min(uv1.x).min(uv2.x).max(0.0);
        let max_u = uv0.x.max(uv1.x).max(uv2.x).min(1.0);
        let min_v = uv0.y.min(uv1.y).min(uv2.y).max(0.0);
        let max_v = uv0.y.max(uv1.y).max(uv2.y).min(1.0);
        
        let start_x = (min_u * texture_size as f32) as u32;
        let end_x = (max_u * texture_size as f32) as u32;
        let start_y = (min_v * texture_size as f32) as u32;
        let end_y = (max_v * texture_size as f32) as u32;
        
        for py in start_y..=end_y {
            for px in start_x..=end_x {
                let u = (px as f32 + 0.5) / texture_size as f32;
                let v = (py as f32 + 0.5) / texture_size as f32;
                
                // Barycentric coordinates
                let uv = Vec2::new(u, v);
                if let Some((w0, w1, w2)) = barycentric_2d(uv0, uv1, uv2, uv) {
                    if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                        // Interpolate world position
                        let world_pos = v0 * w0 + v1 * w1 + v2 * w2;
                        
                        // Find nearest point color
                        let cell = (
                            (world_pos.x / cell_size).floor() as i32,
                            (world_pos.y / cell_size).floor() as i32,
                            (world_pos.z / cell_size).floor() as i32,
                        );
                        
                        let mut best_color = [1.0, 1.0, 1.0, 1.0];
                        let mut best_dist = f32::INFINITY;
                        
                        for dx in -1..=1 {
                            for dy in -1..=1 {
                                for dz in -1..=1 {
                                    let neighbor = (cell.0 + dx, cell.1 + dy, cell.2 + dz);
                                    if let Some(indices) = spatial_hash.get(&neighbor) {
                                        for &i in indices {
                                            let dist = (points[i] - world_pos).length_squared();
                                            if dist < best_dist {
                                                best_dist = dist;
                                                best_color = colors[i];
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        
                        // Write to texture
                        let tex_idx = ((py * texture_size + px) * 4) as usize;
                        if tex_idx + 3 < texture.len() {
                            texture[tex_idx] = (best_color[0] * 255.0) as u8;
                            texture[tex_idx + 1] = (best_color[1] * 255.0) as u8;
                            texture[tex_idx + 2] = (best_color[2] * 255.0) as u8;
                            texture[tex_idx + 3] = (best_color[3] * 255.0) as u8;
                        }
                    }
                }
            }
        }
    }
    
    Ok(texture)
}

fn barycentric_2d(a: Vec2, b: Vec2, c: Vec2, p: Vec2) -> Option<(f32, f32, f32)> {
    let v0 = c - a;
    let v1 = b - a;
    let v2 = p - a;
    
    let dot00 = v0.dot(v0);
    let dot01 = v0.dot(v1);
    let dot02 = v0.dot(v2);
    let dot11 = v1.dot(v1);
    let dot12 = v1.dot(v2);
    
    let inv_denom = 1.0 / (dot00 * dot11 - dot01 * dot01);
    if !inv_denom.is_finite() { return None; }
    
    let u = (dot11 * dot02 - dot01 * dot12) * inv_denom;
    let v = (dot00 * dot12 - dot01 * dot02) * inv_denom;
    
    Some((1.0 - u - v, v, u))
}

// ============================================================================
// 5. Instancing Detection
// ============================================================================

/// Detect repeated geometry for instancing
pub fn detect_instances(
    meshes: &[ReconstructedMesh],
    similarity_threshold: f32,
) -> Vec<InstanceGroup> {
    let mut groups: Vec<InstanceGroup> = Vec::new();
    
    for (i, mesh) in meshes.iter().enumerate() {
        let signature = mesh_signature(mesh);
        
        // Find matching group
        let mut found = false;
        for group in &mut groups {
            if (group.signature - signature).abs() < similarity_threshold {
                group.instances.push(i);
                found = true;
                break;
            }
        }
        
        if !found {
            groups.push(InstanceGroup {
                signature,
                prototype_index: i,
                instances: vec![i],
            });
        }
    }
    
    groups
}

#[derive(Debug, Clone)]
pub struct InstanceGroup {
    pub signature: f32,
    pub prototype_index: usize,
    pub instances: Vec<usize>,
}

fn mesh_signature(mesh: &ReconstructedMesh) -> f32 {
    // Simple signature based on vertex count and bounds
    let (min, max) = mesh.calculate_bounds();
    let size = max - min;
    
    mesh.vertex_count() as f32 * 0.001 + 
    size.x + size.y + size.z
}
