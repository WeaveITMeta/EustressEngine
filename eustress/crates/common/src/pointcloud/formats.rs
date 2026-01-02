// ============================================================================
// Import/Export Format Support
// ============================================================================
//
// Comprehensive format support for:
//
// MESH FORMATS:
// - OBJ  - Wavefront OBJ (vertices, faces, UVs, normals)
// - GLTF - GL Transmission Format (modern, PBR materials)
// - FBX  - Autodesk FBX (animation, rigging)
// - DAE  - Collada (interchange format)
// - STL  - Stereolithography (3D printing)
// - USDZ - Universal Scene Description (Apple AR)
//
// POINT CLOUD FORMATS:
// - PLY  - Polygon File Format (Stanford)
// - LAS  - LiDAR Aerial Survey (ASPRS standard)
// - PTS  - Leica point cloud format
// - XYZ  - Simple ASCII coordinates
// - DXF  - AutoCAD Drawing Exchange
// - PCD  - Point Cloud Library format
// - E57  - 3D imaging standard (ASTM)
//
// ELEVATION DATA:
// - DEM  - Digital Elevation Model (generic)
// - DSM  - Digital Surface Model (includes buildings/trees)
// - DTM  - Digital Terrain Model (bare earth)
// - GeoTIFF - Georeferenced raster elevation
// - ASC  - ESRI ASCII Grid
// - HGT  - SRTM height files
//
// Table of Contents:
// 1. Format Enums
// 2. Mesh Import/Export
// 3. Point Cloud Import/Export
// 4. Elevation Data Import
// 5. Format Detection
// ============================================================================

use bevy::prelude::*;
use std::path::Path;
use std::io::{Read, Write, BufRead, BufReader, BufWriter, Seek, SeekFrom};
use std::fs::File;

use super::mesh_optimization::ReconstructedMesh;
use super::core::PointCloudPoint;

// ============================================================================
// 1. Format Enums
// ============================================================================

/// Supported mesh formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeshFormat {
    /// Wavefront OBJ
    OBJ,
    /// GL Transmission Format (binary/text)
    GLTF,
    /// GL Binary
    GLB,
    /// Autodesk FBX
    FBX,
    /// Collada
    DAE,
    /// Stereolithography (ASCII/Binary)
    STL,
    /// Universal Scene Description (Apple)
    USDZ,
    /// USD ASCII
    USDA,
    /// USD Crate (binary)
    USDC,
}

impl MeshFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "obj" => Some(Self::OBJ),
            "gltf" => Some(Self::GLTF),
            "glb" => Some(Self::GLB),
            "fbx" => Some(Self::FBX),
            "dae" => Some(Self::DAE),
            "stl" => Some(Self::STL),
            "usdz" => Some(Self::USDZ),
            "usda" => Some(Self::USDA),
            "usdc" => Some(Self::USDC),
            _ => None,
        }
    }
    
    pub fn extension(&self) -> &'static str {
        match self {
            Self::OBJ => "obj",
            Self::GLTF => "gltf",
            Self::GLB => "glb",
            Self::FBX => "fbx",
            Self::DAE => "dae",
            Self::STL => "stl",
            Self::USDZ => "usdz",
            Self::USDA => "usda",
            Self::USDC => "usdc",
        }
    }
    
    pub fn description(&self) -> &'static str {
        match self {
            Self::OBJ => "Wavefront OBJ - Universal mesh format",
            Self::GLTF => "glTF 2.0 - Modern PBR format",
            Self::GLB => "glTF Binary - Compact glTF",
            Self::FBX => "Autodesk FBX - Animation/rigging",
            Self::DAE => "Collada - Interchange format",
            Self::STL => "STL - 3D printing format",
            Self::USDZ => "USDZ - Apple AR format",
            Self::USDA => "USD ASCII - Pixar scene format",
            Self::USDC => "USD Crate - Binary USD",
        }
    }
}

/// Supported point cloud formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointCloudFileFormat {
    /// Stanford PLY
    PLY,
    /// LiDAR LAS/LAZ
    LAS,
    /// Leica PTS
    PTS,
    /// Simple XYZ ASCII
    XYZ,
    /// AutoCAD DXF
    DXF,
    /// Point Cloud Library
    PCD,
    /// ASTM E57
    E57,
    /// Potree octree format
    Potree,
    /// Gaussian splatting
    GaussianSplat,
}

impl PointCloudFileFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "ply" => Some(Self::PLY),
            "las" | "laz" => Some(Self::LAS),
            "pts" => Some(Self::PTS),
            "xyz" | "txt" | "asc" => Some(Self::XYZ),
            "dxf" => Some(Self::DXF),
            "pcd" => Some(Self::PCD),
            "e57" => Some(Self::E57),
            "json" => Some(Self::Potree),  // Potree metadata
            "splat" | "ply3dgs" => Some(Self::GaussianSplat),
            _ => None,
        }
    }
    
    pub fn extension(&self) -> &'static str {
        match self {
            Self::PLY => "ply",
            Self::LAS => "las",
            Self::PTS => "pts",
            Self::XYZ => "xyz",
            Self::DXF => "dxf",
            Self::PCD => "pcd",
            Self::E57 => "e57",
            Self::Potree => "json",
            Self::GaussianSplat => "splat",
        }
    }
    
    pub fn description(&self) -> &'static str {
        match self {
            Self::PLY => "PLY - Stanford polygon format",
            Self::LAS => "LAS/LAZ - LiDAR standard",
            Self::PTS => "PTS - Leica scanner format",
            Self::XYZ => "XYZ - Simple ASCII points",
            Self::DXF => "DXF - AutoCAD exchange",
            Self::PCD => "PCD - Point Cloud Library",
            Self::E57 => "E57 - 3D imaging standard",
            Self::Potree => "Potree - Web streaming format",
            Self::GaussianSplat => "Gaussian Splatting - Neural rendering",
        }
    }
}

/// Elevation data formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElevationFormat {
    /// Digital Elevation Model (generic heightmap)
    DEM,
    /// Digital Surface Model (includes structures)
    DSM,
    /// Digital Terrain Model (bare earth)
    DTM,
    /// GeoTIFF raster
    GeoTIFF,
    /// ESRI ASCII Grid
    ASC,
    /// SRTM Height files
    HGT,
    /// USGS BIL format
    BIL,
    /// Terragen heightmap
    TER,
}

impl ElevationFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "dem" => Some(Self::DEM),
            "dsm" => Some(Self::DSM),
            "dtm" => Some(Self::DTM),
            "tif" | "tiff" | "geotiff" => Some(Self::GeoTIFF),
            "asc" | "grd" => Some(Self::ASC),
            "hgt" => Some(Self::HGT),
            "bil" => Some(Self::BIL),
            "ter" => Some(Self::TER),
            _ => None,
        }
    }
    
    pub fn description(&self) -> &'static str {
        match self {
            Self::DEM => "DEM - Digital Elevation Model",
            Self::DSM => "DSM - Digital Surface Model (with structures)",
            Self::DTM => "DTM - Digital Terrain Model (bare earth)",
            Self::GeoTIFF => "GeoTIFF - Georeferenced raster",
            Self::ASC => "ASC - ESRI ASCII Grid",
            Self::HGT => "HGT - SRTM height data",
            Self::BIL => "BIL - Band Interleaved by Line",
            Self::TER => "TER - Terragen heightmap",
        }
    }
}

/// Any supported import format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Mesh(MeshFormat),
    PointCloud(PointCloudFileFormat),
    Elevation(ElevationFormat),
}

impl ImportFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?;
        
        if let Some(fmt) = MeshFormat::from_extension(ext) {
            return Some(Self::Mesh(fmt));
        }
        if let Some(fmt) = PointCloudFileFormat::from_extension(ext) {
            return Some(Self::PointCloud(fmt));
        }
        if let Some(fmt) = ElevationFormat::from_extension(ext) {
            return Some(Self::Elevation(fmt));
        }
        
        None
    }
}

// ============================================================================
// 2. Mesh Import/Export
// ============================================================================

/// Imported mesh data
#[derive(Debug, Clone)]
pub struct ImportedMesh {
    pub name: String,
    pub vertices: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub indices: Vec<u32>,
    pub material_id: Option<usize>,
}

/// Import result containing multiple meshes
#[derive(Debug, Clone)]
pub struct MeshImportResult {
    pub meshes: Vec<ImportedMesh>,
    pub materials: Vec<ImportedMaterial>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ImportedMaterial {
    pub name: String,
    pub diffuse_color: [f32; 4],
    pub diffuse_texture: Option<String>,
    pub normal_texture: Option<String>,
    pub metallic: f32,
    pub roughness: f32,
}

/// Import mesh from file
pub fn import_mesh(path: &Path) -> Result<MeshImportResult, String> {
    let format = MeshFormat::from_extension(
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ).ok_or("Unsupported mesh format")?;
    
    match format {
        MeshFormat::OBJ => import_obj(path),
        MeshFormat::STL => import_stl(path),
        MeshFormat::GLTF | MeshFormat::GLB => import_gltf(path),
        MeshFormat::FBX => import_fbx(path),
        _ => Err(format!("{:?} import not yet implemented", format)),
    }
}

/// Import Autodesk FBX file (binary format)
/// Note: FBX is a complex proprietary format. This implementation handles
/// basic geometry extraction from binary FBX files.
fn import_fbx(path: &Path) -> Result<MeshImportResult, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    
    // Check magic bytes
    let mut magic = [0u8; 23];
    file.read_exact(&mut magic).map_err(|e| e.to_string())?;
    
    // FBX binary magic: "Kaydara FBX Binary  \x00"
    if !magic.starts_with(b"Kaydara FBX Binary") {
        return import_fbx_ascii(path);
    }
    
    // Read version
    let mut version_buf = [0u8; 4];
    file.read_exact(&mut version_buf).map_err(|e| e.to_string())?;
    let _version = u32::from_le_bytes(version_buf);
    
    let mut meshes = Vec::new();
    let mut warnings = Vec::new();
    
    // Parse FBX nodes to find geometry
    let mut vertices: Vec<Vec3> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    
    // Simplified FBX parsing - look for Vertices and PolygonVertexIndex arrays
    let file_len = file.metadata().map_err(|e| e.to_string())?.len();
    let mut buffer = Vec::new();
    file.seek(SeekFrom::Start(27)).map_err(|e| e.to_string())?;
    file.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
    
    // Search for "Vertices" property
    if let Some(pos) = find_fbx_property(&buffer, b"Vertices") {
        if let Some(verts) = parse_fbx_double_array(&buffer[pos..]) {
            for chunk in verts.chunks(3) {
                if chunk.len() == 3 {
                    vertices.push(Vec3::new(chunk[0] as f32, chunk[1] as f32, chunk[2] as f32));
                }
            }
        }
    }
    
    // Search for "PolygonVertexIndex" property
    if let Some(pos) = find_fbx_property(&buffer, b"PolygonVertexIndex") {
        if let Some(idx) = parse_fbx_int_array(&buffer[pos..]) {
            // FBX uses negative indices to mark polygon end
            // Convert to triangles
            let mut poly_start = 0usize;
            for (i, &index) in idx.iter().enumerate() {
                let actual_index = if index < 0 { !index } else { index };
                
                if index < 0 {
                    // End of polygon - triangulate
                    let poly_len = i - poly_start + 1;
                    if poly_len >= 3 {
                        // Fan triangulation
                        let first = idx[poly_start].max(0) as u32;
                        for j in 1..poly_len - 1 {
                            let second = idx[poly_start + j];
                            let third = idx[poly_start + j + 1];
                            indices.push(first);
                            indices.push((if second < 0 { !second } else { second }) as u32);
                            indices.push((if third < 0 { !third } else { third }) as u32);
                        }
                    }
                    poly_start = i + 1;
                }
            }
        }
    }
    
    if vertices.is_empty() {
        warnings.push("No geometry found in FBX file".to_string());
    } else {
        // Generate normals
        let mut normals = vec![Vec3::ZERO; vertices.len()];
        for tri in indices.chunks(3) {
            if tri.len() < 3 { continue; }
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;
            if i0 < vertices.len() && i1 < vertices.len() && i2 < vertices.len() {
                let v0 = vertices[i0];
                let v1 = vertices[i1];
                let v2 = vertices[i2];
                let normal = (v1 - v0).cross(v2 - v0).normalize_or_zero();
                normals[i0] += normal;
                normals[i1] += normal;
                normals[i2] += normal;
            }
        }
        for n in &mut normals {
            *n = n.normalize_or_zero();
        }
        
        meshes.push(ImportedMesh {
            name: path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("FBX_Mesh")
                .to_string(),
            vertices,
            normals,
            uvs: Vec::new(),
            indices,
            material_id: None,
        });
    }
    
    Ok(MeshImportResult {
        meshes,
        materials: Vec::new(),
        warnings,
    })
}

/// Import ASCII FBX (fallback)
fn import_fbx_ascii(path: &Path) -> Result<MeshImportResult, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    
    let mut vertices: Vec<Vec3> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut in_vertices = false;
    let mut in_indices = false;
    
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let trimmed = line.trim();
        
        if trimmed.contains("Vertices:") {
            in_vertices = true;
            in_indices = false;
            // Parse inline vertices if present
            if let Some(data_start) = trimmed.find("*") {
                // Skip count, look for actual data
            }
            continue;
        }
        
        if trimmed.contains("PolygonVertexIndex:") {
            in_vertices = false;
            in_indices = true;
            continue;
        }
        
        if trimmed.starts_with("}") {
            in_vertices = false;
            in_indices = false;
            continue;
        }
        
        if in_vertices {
            // Parse comma-separated doubles
            for part in trimmed.split(',') {
                if let Ok(v) = part.trim().parse::<f64>() {
                    // Collect 3 values for each vertex
                    // This is simplified - real implementation needs state machine
                }
            }
        }
        
        if in_indices {
            // Parse comma-separated integers
            for part in trimmed.split(',') {
                if let Ok(i) = part.trim().parse::<i32>() {
                    let actual = if i < 0 { !i } else { i };
                    indices.push(actual as u32);
                }
            }
        }
    }
    
    // Generate placeholder normals
    let normals = vec![Vec3::Y; vertices.len()];
    
    Ok(MeshImportResult {
        meshes: vec![ImportedMesh {
            name: path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("FBX_Mesh")
                .to_string(),
            vertices,
            normals,
            uvs: Vec::new(),
            indices,
            material_id: None,
        }],
        materials: Vec::new(),
        warnings: vec!["ASCII FBX parsing is limited - consider using binary FBX".to_string()],
    })
}

/// Find FBX property by name in buffer
fn find_fbx_property(buffer: &[u8], name: &[u8]) -> Option<usize> {
    buffer.windows(name.len())
        .position(|w| w == name)
        .map(|p| p + name.len())
}

/// Parse FBX double array
fn parse_fbx_double_array(buffer: &[u8]) -> Option<Vec<f64>> {
    // Look for array header: type 'd', count, encoding, compressed_len, data
    let mut pos = 0;
    while pos < buffer.len().saturating_sub(20) {
        if buffer[pos] == b'd' {
            let count = u32::from_le_bytes([
                buffer[pos + 1], buffer[pos + 2], buffer[pos + 3], buffer[pos + 4]
            ]) as usize;
            let encoding = u32::from_le_bytes([
                buffer[pos + 5], buffer[pos + 6], buffer[pos + 7], buffer[pos + 8]
            ]);
            
            if encoding == 0 && count > 0 && count < 10_000_000 {
                // Uncompressed
                let data_start = pos + 13;
                let data_end = data_start + count * 8;
                if data_end <= buffer.len() {
                    let mut values = Vec::with_capacity(count);
                    for i in 0..count {
                        let offset = data_start + i * 8;
                        let bytes = [
                            buffer[offset], buffer[offset + 1], buffer[offset + 2], buffer[offset + 3],
                            buffer[offset + 4], buffer[offset + 5], buffer[offset + 6], buffer[offset + 7],
                        ];
                        values.push(f64::from_le_bytes(bytes));
                    }
                    return Some(values);
                }
            }
        }
        pos += 1;
    }
    None
}

/// Parse FBX int array
fn parse_fbx_int_array(buffer: &[u8]) -> Option<Vec<i32>> {
    let mut pos = 0;
    while pos < buffer.len().saturating_sub(20) {
        if buffer[pos] == b'i' {
            let count = u32::from_le_bytes([
                buffer[pos + 1], buffer[pos + 2], buffer[pos + 3], buffer[pos + 4]
            ]) as usize;
            let encoding = u32::from_le_bytes([
                buffer[pos + 5], buffer[pos + 6], buffer[pos + 7], buffer[pos + 8]
            ]);
            
            if encoding == 0 && count > 0 && count < 10_000_000 {
                let data_start = pos + 13;
                let data_end = data_start + count * 4;
                if data_end <= buffer.len() {
                    let mut values = Vec::with_capacity(count);
                    for i in 0..count {
                        let offset = data_start + i * 4;
                        let bytes = [
                            buffer[offset], buffer[offset + 1], buffer[offset + 2], buffer[offset + 3],
                        ];
                        values.push(i32::from_le_bytes(bytes));
                    }
                    return Some(values);
                }
            }
        }
        pos += 1;
    }
    None
}

/// Import Wavefront OBJ
fn import_obj(path: &Path) -> Result<MeshImportResult, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    
    let mut vertices: Vec<Vec3> = Vec::new();
    let mut normals: Vec<Vec3> = Vec::new();
    let mut uvs: Vec<Vec2> = Vec::new();
    let mut faces: Vec<(Vec<usize>, Vec<usize>, Vec<usize>)> = Vec::new();
    
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        
        if parts.is_empty() { continue; }
        
        match parts[0] {
            "v" if parts.len() >= 4 => {
                let x: f32 = parts[1].parse().unwrap_or(0.0);
                let y: f32 = parts[2].parse().unwrap_or(0.0);
                let z: f32 = parts[3].parse().unwrap_or(0.0);
                vertices.push(Vec3::new(x, y, z));
            }
            "vn" if parts.len() >= 4 => {
                let x: f32 = parts[1].parse().unwrap_or(0.0);
                let y: f32 = parts[2].parse().unwrap_or(0.0);
                let z: f32 = parts[3].parse().unwrap_or(0.0);
                normals.push(Vec3::new(x, y, z).normalize_or_zero());
            }
            "vt" if parts.len() >= 3 => {
                let u: f32 = parts[1].parse().unwrap_or(0.0);
                let v: f32 = parts[2].parse().unwrap_or(0.0);
                uvs.push(Vec2::new(u, v));
            }
            "f" if parts.len() >= 4 => {
                let mut face_v = Vec::new();
                let mut face_vt = Vec::new();
                let mut face_vn = Vec::new();
                
                for part in &parts[1..] {
                    let indices: Vec<&str> = part.split('/').collect();
                    
                    if let Some(v) = indices.get(0).and_then(|s| s.parse::<usize>().ok()) {
                        face_v.push(v - 1);  // OBJ is 1-indexed
                    }
                    if let Some(vt) = indices.get(1).and_then(|s| s.parse::<usize>().ok()) {
                        face_vt.push(vt - 1);
                    }
                    if let Some(vn) = indices.get(2).and_then(|s| s.parse::<usize>().ok()) {
                        face_vn.push(vn - 1);
                    }
                }
                
                faces.push((face_v, face_vt, face_vn));
            }
            _ => {}
        }
    }
    
    // Convert to indexed mesh
    let mut final_vertices = Vec::new();
    let mut final_normals = Vec::new();
    let mut final_uvs = Vec::new();
    let mut final_indices = Vec::new();
    
    for (face_v, face_vt, face_vn) in faces {
        // Triangulate (fan triangulation for convex polygons)
        for i in 1..face_v.len() - 1 {
            let tri_indices = [0, i, i + 1];
            
            for &idx in &tri_indices {
                let base = final_vertices.len() as u32;
                
                if let Some(&vi) = face_v.get(idx) {
                    final_vertices.push(vertices.get(vi).copied().unwrap_or(Vec3::ZERO));
                }
                
                if let Some(&ni) = face_vn.get(idx) {
                    final_normals.push(normals.get(ni).copied().unwrap_or(Vec3::Y));
                } else {
                    final_normals.push(Vec3::Y);
                }
                
                if let Some(&ti) = face_vt.get(idx) {
                    final_uvs.push(uvs.get(ti).copied().unwrap_or(Vec2::ZERO));
                } else {
                    final_uvs.push(Vec2::ZERO);
                }
                
                final_indices.push(base);
            }
        }
    }
    
    Ok(MeshImportResult {
        meshes: vec![ImportedMesh {
            name: path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("mesh")
                .to_string(),
            vertices: final_vertices,
            normals: final_normals,
            uvs: final_uvs,
            indices: final_indices,
            material_id: None,
        }],
        materials: Vec::new(),
        warnings: Vec::new(),
    })
}

/// Import STL (ASCII and Binary)
fn import_stl(path: &Path) -> Result<MeshImportResult, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    
    // Check if ASCII or binary
    let mut header = [0u8; 80];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    
    let is_ascii = header.starts_with(b"solid");
    
    if is_ascii {
        import_stl_ascii(path)
    } else {
        import_stl_binary(path)
    }
}

fn import_stl_ascii(path: &Path) -> Result<MeshImportResult, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    let mut current_normal = Vec3::Y;
    
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        
        if parts.is_empty() { continue; }
        
        match parts[0] {
            "facet" if parts.len() >= 5 && parts[1] == "normal" => {
                let x: f32 = parts[2].parse().unwrap_or(0.0);
                let y: f32 = parts[3].parse().unwrap_or(0.0);
                let z: f32 = parts[4].parse().unwrap_or(0.0);
                current_normal = Vec3::new(x, y, z).normalize_or_zero();
            }
            "vertex" if parts.len() >= 4 => {
                let x: f32 = parts[1].parse().unwrap_or(0.0);
                let y: f32 = parts[2].parse().unwrap_or(0.0);
                let z: f32 = parts[3].parse().unwrap_or(0.0);
                
                let idx = vertices.len() as u32;
                vertices.push(Vec3::new(x, y, z));
                normals.push(current_normal);
                indices.push(idx);
            }
            _ => {}
        }
    }
    
    let uv_count = normals.len();
    Ok(MeshImportResult {
        meshes: vec![ImportedMesh {
            name: path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("mesh")
                .to_string(),
            vertices,
            normals,
            uvs: vec![Vec2::ZERO; uv_count],
            indices,
            material_id: None,
        }],
        materials: Vec::new(),
        warnings: Vec::new(),
    })
}

fn import_stl_binary(path: &Path) -> Result<MeshImportResult, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    
    // Skip 80-byte header
    let mut header = [0u8; 80];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    
    // Read triangle count
    let mut count_buf = [0u8; 4];
    file.read_exact(&mut count_buf).map_err(|e| e.to_string())?;
    let triangle_count = u32::from_le_bytes(count_buf) as usize;
    
    let mut vertices = Vec::with_capacity(triangle_count * 3);
    let mut normals = Vec::with_capacity(triangle_count * 3);
    let mut indices = Vec::with_capacity(triangle_count * 3);
    
    for _ in 0..triangle_count {
        // Normal (12 bytes)
        let mut buf = [0u8; 12];
        file.read_exact(&mut buf).map_err(|e| e.to_string())?;
        let normal = Vec3::new(
            f32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            f32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            f32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
        ).normalize_or_zero();
        
        // 3 vertices (36 bytes)
        for _ in 0..3 {
            file.read_exact(&mut buf).map_err(|e| e.to_string())?;
            let vertex = Vec3::new(
                f32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
                f32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
                f32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            );
            
            let idx = vertices.len() as u32;
            vertices.push(vertex);
            normals.push(normal);
            indices.push(idx);
        }
        
        // Attribute byte count (2 bytes, usually 0)
        let mut attr = [0u8; 2];
        file.read_exact(&mut attr).map_err(|e| e.to_string())?;
    }
    
    let uv_count = normals.len();
    Ok(MeshImportResult {
        meshes: vec![ImportedMesh {
            name: path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("mesh")
                .to_string(),
            vertices,
            normals,
            uvs: vec![Vec2::ZERO; uv_count],
            indices,
            material_id: None,
        }],
        materials: Vec::new(),
        warnings: Vec::new(),
    })
}

/// Import glTF/GLB files
#[cfg(feature = "model-import")]
fn import_gltf(path: &Path) -> Result<MeshImportResult, String> {
    use gltf::Gltf;
    
    let gltf = Gltf::open(path).map_err(|e| format!("Failed to open glTF: {}", e))?;
    let base_path = path.parent().unwrap_or(Path::new("."));
    
    let mut meshes = Vec::new();
    let mut materials = Vec::new();
    let mut warnings = Vec::new();
    
    // Load buffers
    let mut buffer_data: Vec<Vec<u8>> = Vec::new();
    for buffer in gltf.buffers() {
        match buffer.source() {
            gltf::buffer::Source::Uri(uri) => {
                if uri.starts_with("data:") {
                    // Base64 embedded data
                    let encoded = uri.split(',').nth(1).ok_or("Invalid data URI")?;
                    let decoded = base64_decode(encoded)?;
                    buffer_data.push(decoded);
                } else {
                    // External file
                    let buffer_path = base_path.join(uri);
                    let mut file = File::open(&buffer_path)
                        .map_err(|e| format!("Failed to open buffer {}: {}", uri, e))?;
                    let mut data = Vec::new();
                    file.read_to_end(&mut data).map_err(|e| e.to_string())?;
                    buffer_data.push(data);
                }
            }
            gltf::buffer::Source::Bin => {
                // GLB embedded binary
                if let Some(blob) = gltf.blob.as_ref() {
                    buffer_data.push(blob.clone());
                } else {
                    return Err("GLB file missing binary blob".to_string());
                }
            }
        }
    }
    
    // Import materials
    for material in gltf.materials() {
        let pbr = material.pbr_metallic_roughness();
        let base_color = pbr.base_color_factor();
        
        // Extract texture URIs if available
        let diffuse_texture = pbr.base_color_texture()
            .map(|info| {
                let texture = info.texture();
                let source = texture.source();
                match source.source() {
                    gltf::image::Source::Uri { uri, .. } => Some(uri.to_string()),
                    _ => None,
                }
            })
            .flatten();
        
        let normal_texture = material.normal_texture()
            .map(|info| {
                let texture = info.texture();
                let source = texture.source();
                match source.source() {
                    gltf::image::Source::Uri { uri, .. } => Some(uri.to_string()),
                    _ => None,
                }
            })
            .flatten();
        
        materials.push(ImportedMaterial {
            name: material.name().unwrap_or("Material").to_string(),
            diffuse_color: base_color,
            diffuse_texture,
            normal_texture,
            metallic: pbr.metallic_factor(),
            roughness: pbr.roughness_factor(),
        });
    }
    
    // Import meshes
    for mesh in gltf.meshes() {
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buffer| buffer_data.get(buffer.index()).map(|d| d.as_slice()));
            
            // Read positions (required)
            let positions: Vec<Vec3> = reader.read_positions()
                .ok_or("Mesh missing positions")?
                .map(|p| Vec3::new(p[0], p[1], p[2]))
                .collect();
            
            // Read normals (optional)
            let normals: Vec<Vec3> = reader.read_normals()
                .map(|iter| iter.map(|n| Vec3::new(n[0], n[1], n[2])).collect())
                .unwrap_or_else(|| vec![Vec3::Y; positions.len()]);
            
            // Read UVs (optional)
            let uvs: Vec<Vec2> = reader.read_tex_coords(0)
                .map(|iter| iter.into_f32().map(|uv| Vec2::new(uv[0], uv[1])).collect())
                .unwrap_or_else(|| vec![Vec2::ZERO; positions.len()]);
            
            // Read indices
            let indices: Vec<u32> = reader.read_indices()
                .map(|iter| iter.into_u32().collect())
                .unwrap_or_else(|| (0..positions.len() as u32).collect());
            
            let material_id = primitive.material().index();
            
            meshes.push(ImportedMesh {
                name: mesh.name().unwrap_or("Mesh").to_string(),
                vertices: positions,
                normals,
                uvs,
                indices,
                material_id,
            });
        }
    }
    
    if meshes.is_empty() {
        warnings.push("No meshes found in glTF file".to_string());
    }
    
    Ok(MeshImportResult {
        meshes,
        materials,
        warnings,
    })
}

#[cfg(not(feature = "model-import"))]
fn import_gltf(_path: &Path) -> Result<MeshImportResult, String> {
    Err("glTF import requires 'model-import' feature".to_string())
}

/// Simple base64 decoder for embedded glTF data
fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    
    for byte in input.bytes() {
        if byte == b'=' { break; }
        if byte == b'\n' || byte == b'\r' || byte == b' ' { continue; }
        
        let value = CHARS.iter().position(|&c| c == byte)
            .ok_or_else(|| format!("Invalid base64 character: {}", byte as char))? as u32;
        
        buffer = (buffer << 6) | value;
        bits += 6;
        
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    
    Ok(output)
}

/// Export mesh to glTF format
#[cfg(feature = "model-import")]
pub fn export_gltf(mesh: &ReconstructedMesh, path: &Path, binary: bool) -> Result<(), String> {
    // For now, export as OBJ - full glTF export is complex
    // A proper implementation would use gltf-json crate
    if binary {
        Err("GLB export not yet implemented - use OBJ or STL".to_string())
    } else {
        // Export as simple glTF with embedded buffer
        let mut file = File::create(path).map_err(|e| e.to_string())?;
        
        // Build buffer data
        let mut buffer = Vec::new();
        
        // Positions
        for v in &mesh.vertices {
            buffer.extend_from_slice(&v.x.to_le_bytes());
            buffer.extend_from_slice(&v.y.to_le_bytes());
            buffer.extend_from_slice(&v.z.to_le_bytes());
        }
        let positions_len = buffer.len();
        
        // Normals
        for n in &mesh.normals {
            buffer.extend_from_slice(&n.x.to_le_bytes());
            buffer.extend_from_slice(&n.y.to_le_bytes());
            buffer.extend_from_slice(&n.z.to_le_bytes());
        }
        let normals_len = buffer.len() - positions_len;
        
        // Indices
        let indices_offset = buffer.len();
        for &i in &mesh.indices {
            buffer.extend_from_slice(&i.to_le_bytes());
        }
        let indices_len = buffer.len() - indices_offset;
        
        // Base64 encode buffer
        let encoded = base64_encode(&buffer);
        
        // Calculate bounds
        let (min, max) = mesh.calculate_bounds();
        
        // Write glTF JSON
        let gltf_json = format!(r#"{{
  "asset": {{ "version": "2.0", "generator": "Eustress Engine" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0] }}],
  "nodes": [{{ "mesh": 0 }}],
  "meshes": [{{
    "primitives": [{{
      "attributes": {{ "POSITION": 0, "NORMAL": 1 }},
      "indices": 2
    }}]
  }}],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": {}, "type": "VEC3", "min": [{}, {}, {}], "max": [{}, {}, {}] }},
    {{ "bufferView": 1, "componentType": 5126, "count": {}, "type": "VEC3" }},
    {{ "bufferView": 2, "componentType": 5125, "count": {}, "type": "SCALAR" }}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": {} }},
    {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }},
    {{ "buffer": 0, "byteOffset": {}, "byteLength": {} }}
  ],
  "buffers": [{{ "uri": "data:application/octet-stream;base64,{}", "byteLength": {} }}]
}}"#,
            mesh.vertices.len(), min.x, min.y, min.z, max.x, max.y, max.z,
            mesh.normals.len(),
            mesh.indices.len(),
            positions_len, positions_len, normals_len, indices_offset, indices_len,
            encoded, buffer.len()
        );
        
        file.write_all(gltf_json.as_bytes()).map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(not(feature = "model-import"))]
pub fn export_gltf(_mesh: &ReconstructedMesh, _path: &Path, _binary: bool) -> Result<(), String> {
    Err("glTF export requires 'model-import' feature".to_string())
}

/// Simple base64 encoder
fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    
    let mut output = String::with_capacity((input.len() + 2) / 3 * 4);
    
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        
        let n = (b0 << 16) | (b1 << 8) | b2;
        
        output.push(CHARS[(n >> 18) as usize & 0x3F] as char);
        output.push(CHARS[(n >> 12) as usize & 0x3F] as char);
        
        if chunk.len() > 1 {
            output.push(CHARS[(n >> 6) as usize & 0x3F] as char);
        } else {
            output.push('=');
        }
        
        if chunk.len() > 2 {
            output.push(CHARS[n as usize & 0x3F] as char);
        } else {
            output.push('=');
        }
    }
    
    output
}

/// Export mesh to OBJ format
pub fn export_obj(mesh: &ReconstructedMesh, path: &Path) -> Result<(), String> {
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(file);
    
    writeln!(writer, "# Exported by Eustress Engine").map_err(|e| e.to_string())?;
    writeln!(writer, "# Vertices: {}", mesh.vertices.len()).map_err(|e| e.to_string())?;
    writeln!(writer, "# Triangles: {}", mesh.indices.len() / 3).map_err(|e| e.to_string())?;
    writeln!(writer).map_err(|e| e.to_string())?;
    
    // Vertices
    for v in &mesh.vertices {
        writeln!(writer, "v {} {} {}", v.x, v.y, v.z).map_err(|e| e.to_string())?;
    }
    
    // Normals
    for n in &mesh.normals {
        writeln!(writer, "vn {} {} {}", n.x, n.y, n.z).map_err(|e| e.to_string())?;
    }
    
    // UVs
    if let Some(ref uvs) = mesh.uvs {
        for uv in uvs {
            writeln!(writer, "vt {} {}", uv.x, uv.y).map_err(|e| e.to_string())?;
        }
    }
    
    // Faces
    let has_uvs = mesh.uvs.is_some();
    for tri in mesh.indices.chunks(3) {
        if tri.len() < 3 { continue; }
        
        if has_uvs {
            writeln!(writer, "f {}/{}/{} {}/{}/{} {}/{}/{}",
                tri[0] + 1, tri[0] + 1, tri[0] + 1,
                tri[1] + 1, tri[1] + 1, tri[1] + 1,
                tri[2] + 1, tri[2] + 1, tri[2] + 1,
            ).map_err(|e| e.to_string())?;
        } else {
            writeln!(writer, "f {}//{} {}//{} {}//{}",
                tri[0] + 1, tri[0] + 1,
                tri[1] + 1, tri[1] + 1,
                tri[2] + 1, tri[2] + 1,
            ).map_err(|e| e.to_string())?;
        }
    }
    
    writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

/// Export mesh to STL format
pub fn export_stl(mesh: &ReconstructedMesh, path: &Path, binary: bool) -> Result<(), String> {
    if binary {
        export_stl_binary(mesh, path)
    } else {
        export_stl_ascii(mesh, path)
    }
}

fn export_stl_ascii(mesh: &ReconstructedMesh, path: &Path) -> Result<(), String> {
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(file);
    
    writeln!(writer, "solid eustress_export").map_err(|e| e.to_string())?;
    
    for tri in mesh.indices.chunks(3) {
        if tri.len() < 3 { continue; }
        
        let v0 = mesh.vertices[tri[0] as usize];
        let v1 = mesh.vertices[tri[1] as usize];
        let v2 = mesh.vertices[tri[2] as usize];
        
        let normal = (v1 - v0).cross(v2 - v0).normalize_or_zero();
        
        writeln!(writer, "  facet normal {} {} {}", normal.x, normal.y, normal.z)
            .map_err(|e| e.to_string())?;
        writeln!(writer, "    outer loop").map_err(|e| e.to_string())?;
        writeln!(writer, "      vertex {} {} {}", v0.x, v0.y, v0.z).map_err(|e| e.to_string())?;
        writeln!(writer, "      vertex {} {} {}", v1.x, v1.y, v1.z).map_err(|e| e.to_string())?;
        writeln!(writer, "      vertex {} {} {}", v2.x, v2.y, v2.z).map_err(|e| e.to_string())?;
        writeln!(writer, "    endloop").map_err(|e| e.to_string())?;
        writeln!(writer, "  endfacet").map_err(|e| e.to_string())?;
    }
    
    writeln!(writer, "endsolid eustress_export").map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn export_stl_binary(mesh: &ReconstructedMesh, path: &Path) -> Result<(), String> {
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(file);
    
    // 80-byte header
    let header = b"Eustress Engine STL Export                                                      ";
    writer.write_all(header).map_err(|e| e.to_string())?;
    
    // Triangle count
    let tri_count = (mesh.indices.len() / 3) as u32;
    writer.write_all(&tri_count.to_le_bytes()).map_err(|e| e.to_string())?;
    
    for tri in mesh.indices.chunks(3) {
        if tri.len() < 3 { continue; }
        
        let v0 = mesh.vertices[tri[0] as usize];
        let v1 = mesh.vertices[tri[1] as usize];
        let v2 = mesh.vertices[tri[2] as usize];
        
        let normal = (v1 - v0).cross(v2 - v0).normalize_or_zero();
        
        // Normal
        writer.write_all(&normal.x.to_le_bytes()).map_err(|e| e.to_string())?;
        writer.write_all(&normal.y.to_le_bytes()).map_err(|e| e.to_string())?;
        writer.write_all(&normal.z.to_le_bytes()).map_err(|e| e.to_string())?;
        
        // Vertices
        for v in [v0, v1, v2] {
            writer.write_all(&v.x.to_le_bytes()).map_err(|e| e.to_string())?;
            writer.write_all(&v.y.to_le_bytes()).map_err(|e| e.to_string())?;
            writer.write_all(&v.z.to_le_bytes()).map_err(|e| e.to_string())?;
        }
        
        // Attribute byte count
        writer.write_all(&[0u8, 0u8]).map_err(|e| e.to_string())?;
    }
    
    writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

// ============================================================================
// 3. Point Cloud Import/Export
// ============================================================================

/// Import point cloud from PTS format (Leica)
pub fn import_pts(path: &Path) -> Result<Vec<PointCloudPoint>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    
    // First line is point count
    let count_line = lines.next()
        .ok_or("Empty file")?
        .map_err(|e| e.to_string())?;
    let _count: usize = count_line.trim().parse().unwrap_or(0);
    
    let mut points = Vec::new();
    
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        
        if parts.len() < 3 { continue; }
        
        let x: f32 = parts[0].parse().unwrap_or(0.0);
        let y: f32 = parts[1].parse().unwrap_or(0.0);
        let z: f32 = parts[2].parse().unwrap_or(0.0);
        
        // PTS can have intensity (4th) and RGB (5th-7th)
        let color = if parts.len() >= 7 {
            let r: u8 = parts[4].parse().unwrap_or(255);
            let g: u8 = parts[5].parse().unwrap_or(255);
            let b: u8 = parts[6].parse().unwrap_or(255);
            ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | 255
        } else if parts.len() >= 4 {
            // Intensity only - grayscale
            let i: u8 = parts[3].parse().unwrap_or(128);
            ((i as u32) << 24) | ((i as u32) << 16) | ((i as u32) << 8) | 255
        } else {
            0xFFFFFFFF
        };
        
        points.push(PointCloudPoint { x, y, z, color });
    }
    
    Ok(points)
}

/// Import point cloud from DXF (points only)
pub fn import_dxf_points(path: &Path) -> Result<Vec<PointCloudPoint>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    
    let mut points = Vec::new();
    let mut lines = reader.lines().peekable();
    
    while let Some(line_result) = lines.next() {
        let line = line_result.map_err(|e| e.to_string())?;
        let code = line.trim();
        
        // Look for POINT entity
        if code == "POINT" {
            let mut x = 0.0f32;
            let mut y = 0.0f32;
            let mut z = 0.0f32;
            
            // Read point coordinates
            while let Some(next_result) = lines.next() {
                let next = next_result.map_err(|e| e.to_string())?;
                let group_code: i32 = next.trim().parse().unwrap_or(0);
                
                if let Some(value_result) = lines.next() {
                    let value = value_result.map_err(|e| e.to_string())?;
                    
                    match group_code {
                        10 => x = value.trim().parse().unwrap_or(0.0),
                        20 => y = value.trim().parse().unwrap_or(0.0),
                        30 => z = value.trim().parse().unwrap_or(0.0),
                        0 => break,  // Next entity
                        _ => {}
                    }
                }
            }
            
            points.push(PointCloudPoint { x, y, z, color: 0xFFFFFFFF });
        }
    }
    
    Ok(points)
}

/// Export point cloud to PLY
pub fn export_ply(points: &[PointCloudPoint], path: &Path, binary: bool) -> Result<(), String> {
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(file);
    
    // Header
    writeln!(writer, "ply").map_err(|e| e.to_string())?;
    if binary {
        writeln!(writer, "format binary_little_endian 1.0").map_err(|e| e.to_string())?;
    } else {
        writeln!(writer, "format ascii 1.0").map_err(|e| e.to_string())?;
    }
    writeln!(writer, "comment Exported by Eustress Engine").map_err(|e| e.to_string())?;
    writeln!(writer, "element vertex {}", points.len()).map_err(|e| e.to_string())?;
    writeln!(writer, "property float x").map_err(|e| e.to_string())?;
    writeln!(writer, "property float y").map_err(|e| e.to_string())?;
    writeln!(writer, "property float z").map_err(|e| e.to_string())?;
    writeln!(writer, "property uchar red").map_err(|e| e.to_string())?;
    writeln!(writer, "property uchar green").map_err(|e| e.to_string())?;
    writeln!(writer, "property uchar blue").map_err(|e| e.to_string())?;
    writeln!(writer, "end_header").map_err(|e| e.to_string())?;
    
    if binary {
        for point in points {
            // Copy values to avoid packed field reference issues
            let (x, y, z, color) = { (point.x, point.y, point.z, point.color) };
            writer.write_all(&x.to_le_bytes()).map_err(|e| e.to_string())?;
            writer.write_all(&y.to_le_bytes()).map_err(|e| e.to_string())?;
            writer.write_all(&z.to_le_bytes()).map_err(|e| e.to_string())?;
            writer.write_all(&[(color >> 24) as u8]).map_err(|e| e.to_string())?;
            writer.write_all(&[(color >> 16) as u8]).map_err(|e| e.to_string())?;
            writer.write_all(&[(color >> 8) as u8]).map_err(|e| e.to_string())?;
        }
    } else {
        for point in points {
            // Copy values to avoid packed field reference issues
            let (x, y, z, color) = { (point.x, point.y, point.z, point.color) };
            writeln!(writer, "{} {} {} {} {} {}",
                x, y, z,
                (color >> 24) as u8,
                (color >> 16) as u8,
                (color >> 8) as u8,
            ).map_err(|e| e.to_string())?;
        }
    }
    
    writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

// ============================================================================
// 4. Elevation Data Import
// ============================================================================

/// Elevation data result
#[derive(Debug, Clone)]
pub struct ElevationData {
    /// Width in samples
    pub width: usize,
    /// Height in samples
    pub height: usize,
    /// Height values (row-major)
    pub heights: Vec<f32>,
    /// Cell size in meters
    pub cell_size: f32,
    /// Lower-left corner X
    pub origin_x: f64,
    /// Lower-left corner Y
    pub origin_y: f64,
    /// No-data value
    pub nodata: f32,
    /// Data type (DEM, DSM, DTM)
    pub data_type: ElevationFormat,
}

impl ElevationData {
    /// Convert to point cloud
    pub fn to_point_cloud(&self) -> Vec<PointCloudPoint> {
        let mut points = Vec::with_capacity(self.width * self.height);
        
        for row in 0..self.height {
            for col in 0..self.width {
                let idx = row * self.width + col;
                let h = self.heights[idx];
                
                if (h - self.nodata).abs() < 0.001 { continue; }
                
                let x = self.origin_x as f32 + col as f32 * self.cell_size;
                let z = self.origin_y as f32 + row as f32 * self.cell_size;
                
                // Color by height (gradient)
                let normalized = ((h - self.min_height()) / (self.max_height() - self.min_height() + 0.001)).clamp(0.0, 1.0);
                let color = height_to_color(normalized);
                
                points.push(PointCloudPoint { x, y: h, z, color });
            }
        }
        
        points
    }
    
    /// Convert to mesh (heightmap triangulation)
    pub fn to_mesh(&self) -> ReconstructedMesh {
        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        
        // Create vertices
        for row in 0..self.height {
            for col in 0..self.width {
                let idx = row * self.width + col;
                let h = self.heights[idx];
                
                let x = self.origin_x as f32 + col as f32 * self.cell_size;
                let z = self.origin_y as f32 + row as f32 * self.cell_size;
                
                vertices.push(Vec3::new(x, h, z));
            }
        }
        
        // Create triangles
        for row in 0..self.height - 1 {
            for col in 0..self.width - 1 {
                let i0 = (row * self.width + col) as u32;
                let i1 = i0 + 1;
                let i2 = i0 + self.width as u32;
                let i3 = i2 + 1;
                
                // Two triangles per cell
                indices.extend_from_slice(&[i0, i2, i1]);
                indices.extend_from_slice(&[i1, i2, i3]);
            }
        }
        
        // Calculate normals
        normals = vec![Vec3::ZERO; vertices.len()];
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
        
        ReconstructedMesh {
            vertices,
            normals,
            indices,
            uvs: None,
            colors: None,
        }
    }
    
    pub fn min_height(&self) -> f32 {
        self.heights.iter()
            .filter(|&&h| (h - self.nodata).abs() > 0.001)
            .copied()
            .fold(f32::INFINITY, f32::min)
    }
    
    pub fn max_height(&self) -> f32 {
        self.heights.iter()
            .filter(|&&h| (h - self.nodata).abs() > 0.001)
            .copied()
            .fold(f32::NEG_INFINITY, f32::max)
    }
}

/// Import ESRI ASCII Grid (.asc)
pub fn import_asc(path: &Path) -> Result<ElevationData, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    
    let mut width = 0usize;
    let mut height = 0usize;
    let mut origin_x = 0.0f64;
    let mut origin_y = 0.0f64;
    let mut cell_size = 1.0f32;
    let mut nodata = -9999.0f32;
    
    // Parse header
    for _ in 0..6 {
        if let Some(line_result) = lines.next() {
            let line = line_result.map_err(|e| e.to_string())?;
            let parts: Vec<&str> = line.split_whitespace().collect();
            
            if parts.len() >= 2 {
                match parts[0].to_lowercase().as_str() {
                    "ncols" => width = parts[1].parse().unwrap_or(0),
                    "nrows" => height = parts[1].parse().unwrap_or(0),
                    "xllcorner" | "xllcenter" => origin_x = parts[1].parse().unwrap_or(0.0),
                    "yllcorner" | "yllcenter" => origin_y = parts[1].parse().unwrap_or(0.0),
                    "cellsize" => cell_size = parts[1].parse().unwrap_or(1.0),
                    "nodata_value" => nodata = parts[1].parse().unwrap_or(-9999.0),
                    _ => {}
                }
            }
        }
    }
    
    // Parse height data
    let mut heights = Vec::with_capacity(width * height);
    
    for line_result in lines {
        let line = line_result.map_err(|e| e.to_string())?;
        for value in line.split_whitespace() {
            let h: f32 = value.parse().unwrap_or(nodata);
            heights.push(h);
        }
    }
    
    // Determine data type from filename
    let data_type = path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| {
            let lower = s.to_lowercase();
            if lower.contains("dsm") { ElevationFormat::DSM }
            else if lower.contains("dtm") { ElevationFormat::DTM }
            else { ElevationFormat::DEM }
        })
        .unwrap_or(ElevationFormat::DEM);
    
    Ok(ElevationData {
        width,
        height,
        heights,
        cell_size,
        origin_x,
        origin_y,
        nodata,
        data_type,
    })
}

/// Import SRTM HGT file
pub fn import_hgt(path: &Path) -> Result<ElevationData, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    
    // Determine resolution from file size
    let file_size = file.metadata().map_err(|e| e.to_string())?.len();
    let (width, height, cell_size) = match file_size {
        25934402 => (3601, 3601, 1.0 / 3600.0),  // 1 arc-second
        2884802 => (1201, 1201, 3.0 / 3600.0),   // 3 arc-second
        _ => return Err("Unknown HGT resolution".to_string()),
    };
    
    // Parse coordinates from filename (e.g., N47W122.hgt)
    let (origin_x, origin_y) = parse_hgt_filename(path)?;
    
    // Read height data (big-endian 16-bit signed integers)
    let mut heights = Vec::with_capacity(width * height);
    let mut buf = [0u8; 2];
    
    for _ in 0..(width * height) {
        file.read_exact(&mut buf).map_err(|e| e.to_string())?;
        let h = i16::from_be_bytes(buf);
        heights.push(if h == -32768 { -9999.0 } else { h as f32 });
    }
    
    Ok(ElevationData {
        width,
        height,
        heights,
        cell_size: cell_size * 111320.0,  // Convert degrees to meters (approximate)
        origin_x,
        origin_y,
        nodata: -9999.0,
        data_type: ElevationFormat::HGT,
    })
}

fn parse_hgt_filename(path: &Path) -> Result<(f64, f64), String> {
    let stem = path.file_stem()
        .and_then(|s| s.to_str())
        .ok_or("Invalid filename")?;
    
    if stem.len() < 7 { return Err("Invalid HGT filename".to_string()); }
    
    let ns = &stem[0..1];
    let lat: f64 = stem[1..3].parse().map_err(|_| "Invalid latitude")?;
    let ew = &stem[3..4];
    let lon: f64 = stem[4..7].parse().map_err(|_| "Invalid longitude")?;
    
    let lat = if ns == "S" { -lat } else { lat };
    let lon = if ew == "W" { -lon } else { lon };
    
    Ok((lon, lat))
}

/// Import GeoTIFF elevation data
#[cfg(feature = "geotiff")]
pub fn import_geotiff(path: &Path) -> Result<ElevationData, String> {
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::ColorType;
    
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = Decoder::new(file).map_err(|e| format!("Failed to open TIFF: {}", e))?;
    
    let (width, height) = decoder.dimensions().map_err(|e| e.to_string())?;
    let width = width as usize;
    let height = height as usize;
    
    // Read image data
    let result = decoder.read_image().map_err(|e| format!("Failed to read TIFF: {}", e))?;
    
    let heights: Vec<f32> = match result {
        DecodingResult::U8(data) => {
            data.iter().map(|&v| v as f32).collect()
        }
        DecodingResult::U16(data) => {
            data.iter().map(|&v| v as f32).collect()
        }
        DecodingResult::U32(data) => {
            data.iter().map(|&v| v as f32).collect()
        }
        DecodingResult::U64(data) => {
            data.iter().map(|&v| v as f32).collect()
        }
        DecodingResult::I8(data) => {
            data.iter().map(|&v| v as f32).collect()
        }
        DecodingResult::I16(data) => {
            data.iter().map(|&v| v as f32).collect()
        }
        DecodingResult::I32(data) => {
            data.iter().map(|&v| v as f32).collect()
        }
        DecodingResult::I64(data) => {
            data.iter().map(|&v| v as f32).collect()
        }
        DecodingResult::F32(data) => data,
        DecodingResult::F64(data) => {
            data.iter().map(|&v| v as f32).collect()
        }
    };
    
    // For grayscale, heights is already correct
    // For RGB, we need to extract just one channel or average
    let color_type = decoder.colortype().map_err(|e| e.to_string())?;
    let final_heights = match color_type {
        ColorType::Gray(_) => heights,
        ColorType::RGB(_) | ColorType::RGBA(_) => {
            // Take first channel (red) as height
            let channels = if matches!(color_type, ColorType::RGBA(_)) { 4 } else { 3 };
            heights.iter()
                .step_by(channels)
                .copied()
                .collect()
        }
        _ => heights,
    };
    
    // Try to read GeoTIFF tags for georeferencing
    // Default to 1 meter cell size if no geotags
    let cell_size = 1.0f32;
    let origin_x = 0.0f64;
    let origin_y = 0.0f64;
    
    // Determine data type from filename
    let data_type = path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| {
            let lower = s.to_lowercase();
            if lower.contains("dsm") { ElevationFormat::DSM }
            else if lower.contains("dtm") { ElevationFormat::DTM }
            else { ElevationFormat::GeoTIFF }
        })
        .unwrap_or(ElevationFormat::GeoTIFF);
    
    Ok(ElevationData {
        width,
        height,
        heights: final_heights,
        cell_size,
        origin_x,
        origin_y,
        nodata: -9999.0,
        data_type,
    })
}

#[cfg(not(feature = "geotiff"))]
pub fn import_geotiff(_path: &Path) -> Result<ElevationData, String> {
    Err("GeoTIFF import requires 'geotiff' feature".to_string())
}

/// Import elevation data from any supported format
pub fn import_elevation(path: &Path) -> Result<ElevationData, String> {
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    
    match ext.to_lowercase().as_str() {
        "asc" | "grd" => import_asc(path),
        "hgt" => import_hgt(path),
        "tif" | "tiff" | "geotiff" => import_geotiff(path),
        "dem" | "dsm" | "dtm" => {
            // Try GeoTIFF first, then ASC, then HGT
            import_geotiff(path)
                .or_else(|_| import_asc(path))
                .or_else(|_| import_hgt(path))
        }
        _ => Err(format!("Unsupported elevation format: {}", ext)),
    }
}

// ============================================================================
// 5. Utility Functions
// ============================================================================

/// Convert normalized height (0-1) to terrain color
fn height_to_color(normalized: f32) -> u32 {
    let (r, g, b) = if normalized < 0.2 {
        // Deep water to shallow water
        let t = normalized / 0.2;
        (
            (20.0 + t * 30.0) as u8,
            (50.0 + t * 100.0) as u8,
            (100.0 + t * 100.0) as u8,
        )
    } else if normalized < 0.3 {
        // Beach/sand
        let t = (normalized - 0.2) / 0.1;
        (
            (194.0 + t * 20.0) as u8,
            (178.0 + t * 10.0) as u8,
            (128.0 + t * 20.0) as u8,
        )
    } else if normalized < 0.6 {
        // Grass/forest
        let t = (normalized - 0.3) / 0.3;
        (
            (34.0 + t * 50.0) as u8,
            (139.0 - t * 40.0) as u8,
            (34.0 + t * 20.0) as u8,
        )
    } else if normalized < 0.8 {
        // Mountain/rock
        let t = (normalized - 0.6) / 0.2;
        (
            (139.0 - t * 20.0) as u8,
            (119.0 - t * 20.0) as u8,
            (101.0 - t * 20.0) as u8,
        )
    } else {
        // Snow
        let t = (normalized - 0.8) / 0.2;
        (
            (200.0 + t * 55.0) as u8,
            (200.0 + t * 55.0) as u8,
            (200.0 + t * 55.0) as u8,
        )
    };
    
    ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | 255
}

/// Detect format from file path
pub fn detect_format(path: &Path) -> Option<ImportFormat> {
    ImportFormat::from_path(path)
}

/// Get all supported extensions
pub fn supported_extensions() -> Vec<&'static str> {
    vec![
        // Mesh
        "obj", "gltf", "glb", "fbx", "dae", "stl", "usdz", "usda", "usdc",
        // Point cloud
        "ply", "las", "laz", "pts", "xyz", "txt", "dxf", "pcd", "e57", "splat",
        // Elevation
        "dem", "dsm", "dtm", "tif", "tiff", "asc", "grd", "hgt", "bil", "ter",
    ]
}

/// Check if file is supported
pub fn is_supported(path: &Path) -> bool {
    detect_format(path).is_some()
}
