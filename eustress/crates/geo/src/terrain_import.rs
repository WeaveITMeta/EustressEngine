//! # Terrain Import — Multi-Tile WGS84 Grid
//!
//! Loads SRTM HGT and GeoTIFF elevation files, positions them on a WGS84
//! orbital grid, and generates Bevy terrain mesh chunks with LOD.
//!
//! ## WGS84 Tile Grid
//! SRTM tiles are 1°×1° cells named by their SW corner (e.g., `N25E081.hgt`).
//! Multiple tiles are loaded and positioned relative to the GeoOrigin,
//! forming a combinatorial grid that covers the project area.
//!
//! ## Table of Contents
//! 1. HGT file parsing (SRTM 1-arc-second and 3-arc-second)
//! 2. Tile grid positioning (WGS84 → Bevy local)
//! 3. Mesh chunk generation from heightmap data
//! 4. Multi-tile terrain spawning system

use bevy::prelude::*;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::asset::RenderAssetUsages;
use std::path::Path;

use crate::coords::GeoOrigin;
use crate::layers::GeoTerrainChunk;

// ============================================================================
// 1. HGT file parsing (SRTM 1-arc-second and 3-arc-second)
// ============================================================================

/// SRTM resolution variants
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SrtmResolution {
    /// 1 arc-second (~30m) — 3601×3601 samples per tile
    OneArcSecond,
    /// 3 arc-second (~90m) — 1201×1201 samples per tile
    ThreeArcSecond,
}

impl SrtmResolution {
    /// Samples per side for this resolution
    pub fn samples_per_side(&self) -> usize {
        match self {
            SrtmResolution::OneArcSecond => 3601,
            SrtmResolution::ThreeArcSecond => 1201,
        }
    }
}

/// Parsed SRTM HGT tile with elevation data and geographic bounds
#[derive(Debug, Clone)]
pub struct HgtTile {
    /// SW corner latitude (integer degrees)
    pub lat: i32,
    /// SW corner longitude (integer degrees)
    pub lon: i32,
    /// Resolution (1" or 3")
    pub resolution: SrtmResolution,
    /// Elevation samples (row-major, NW corner first, big-endian i16)
    /// Values in meters above WGS84 ellipsoid. -32768 = void/no-data.
    pub elevations: Vec<i16>,
}

impl HgtTile {
    /// Parse an SRTM HGT file from disk.
    /// Filename encodes the SW corner: `N25E081.hgt` → lat=25, lon=81
    pub fn load(path: &Path) -> Result<Self, TerrainImportError> {
        let filename = path.file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| TerrainImportError::Parse(
                path.to_path_buf(),
                "Invalid HGT filename".to_string(),
            ))?;

        let (lat, lon) = parse_hgt_filename(filename)
            .ok_or_else(|| TerrainImportError::Parse(
                path.to_path_buf(),
                format!("Cannot parse lat/lon from filename: {}", filename),
            ))?;

        let data = std::fs::read(path)
            .map_err(|e| TerrainImportError::Io(path.to_path_buf(), e))?;

        // Detect resolution from file size
        let resolution = match data.len() {
            // 3601 * 3601 * 2 bytes = 25,934,402
            25_934_402 => SrtmResolution::OneArcSecond,
            // 1201 * 1201 * 2 bytes = 2,884,802
            2_884_802 => SrtmResolution::ThreeArcSecond,
            other => {
                return Err(TerrainImportError::Parse(
                    path.to_path_buf(),
                    format!("Unexpected HGT file size: {} bytes (expected 25934402 or 2884802)", other),
                ));
            }
        };

        // Parse big-endian i16 elevation samples
        let sample_count = resolution.samples_per_side() * resolution.samples_per_side();
        let mut elevations = Vec::with_capacity(sample_count);
        for i in 0..sample_count {
            let offset = i * 2;
            if offset + 1 >= data.len() { break; }
            let value = i16::from_be_bytes([data[offset], data[offset + 1]]);
            elevations.push(value);
        }

        tracing::info!(
            "Loaded HGT tile: {}°{} {}°{} ({:?}, {} samples)",
            lat.abs(), if lat >= 0 { "N" } else { "S" },
            lon.abs(), if lon >= 0 { "E" } else { "W" },
            resolution, elevations.len()
        );

        Ok(HgtTile { lat, lon, resolution, elevations })
    }

    /// Get elevation at a specific row/col (row 0 = north edge)
    pub fn elevation_at(&self, row: usize, col: usize) -> f32 {
        let side = self.resolution.samples_per_side();
        if row >= side || col >= side { return 0.0; }
        let idx = row * side + col;
        let val = self.elevations.get(idx).copied().unwrap_or(-32768);
        if val == -32768 { 0.0 } else { val as f32 }
    }

    /// Get elevation at geographic coordinates (bilinear interpolation)
    pub fn elevation_at_latlon(&self, lat: f64, lon: f64) -> f32 {
        let side = self.resolution.samples_per_side() as f64;
        // Fractional position within tile (0,0 = SW corner)
        let frac_lon = (lon - self.lon as f64) * (side - 1.0);
        let frac_lat = (lat - self.lat as f64) * (side - 1.0);

        // HGT rows go from north to south, so invert lat
        let row_f = (side - 1.0) - frac_lat;
        let col_f = frac_lon;

        // Bilinear interpolation
        let r0 = row_f.floor() as usize;
        let c0 = col_f.floor() as usize;
        let r1 = (r0 + 1).min(self.resolution.samples_per_side() - 1);
        let c1 = (c0 + 1).min(self.resolution.samples_per_side() - 1);

        let fr = row_f.fract() as f32;
        let fc = col_f.fract() as f32;

        let e00 = self.elevation_at(r0, c0);
        let e01 = self.elevation_at(r0, c1);
        let e10 = self.elevation_at(r1, c0);
        let e11 = self.elevation_at(r1, c1);

        let top = e00 * (1.0 - fc) + e01 * fc;
        let bot = e10 * (1.0 - fc) + e11 * fc;
        top * (1.0 - fr) + bot * fr
    }
}

/// Parse SRTM filename like "N25E081" → (25, 81)
fn parse_hgt_filename(name: &str) -> Option<(i32, i32)> {
    if name.len() < 7 { return None; }
    let name = name.to_uppercase();

    let lat_sign = match name.as_bytes()[0] {
        b'N' => 1,
        b'S' => -1,
        _ => return None,
    };
    let lat: i32 = name[1..3].parse().ok()?;

    let lon_sign = match name.as_bytes()[3] {
        b'E' => 1,
        b'W' => -1,
        _ => return None,
    };
    let lon: i32 = name[4..7].parse().ok()?;

    Some((lat * lat_sign, lon * lon_sign))
}

// ============================================================================
// 2. Tile grid positioning (WGS84 → Bevy local)
// ============================================================================

/// A terrain tile positioned in Bevy world space
#[derive(Debug, Clone)]
pub struct PositionedTile {
    /// The parsed HGT tile data
    pub tile: HgtTile,
    /// SW corner position in Bevy world space
    pub world_sw: Vec3,
    /// NE corner position in Bevy world space
    pub world_ne: Vec3,
    /// Width in Bevy world units (meters)
    pub world_width: f32,
    /// Height in Bevy world units (meters)
    pub world_height: f32,
}

/// Position multiple HGT tiles on the WGS84 orbital grid relative to a GeoOrigin.
/// Each 1°×1° tile maps to its correct geographic position in Bevy space.
pub fn position_tiles(tiles: Vec<HgtTile>, origin: &GeoOrigin) -> Vec<PositionedTile> {
    tiles.into_iter().map(|tile| {
        // SW corner
        let sw = crate::coords::geo_to_world(
            tile.lat as f64,
            tile.lon as f64,
            0.0,
            origin,
        );
        // NE corner (1° north and east)
        let ne = crate::coords::geo_to_world(
            (tile.lat + 1) as f64,
            (tile.lon + 1) as f64,
            0.0,
            origin,
        );

        let world_width = (ne.x - sw.x).abs();
        let world_height = (ne.z - sw.z).abs();

        PositionedTile {
            tile,
            world_sw: sw,
            world_ne: ne,
            world_width,
            world_height,
        }
    }).collect()
}

// ============================================================================
// 3. Mesh chunk generation from heightmap data
// ============================================================================

/// Generate a terrain mesh from an HGT tile at a given resolution.
///
/// - `tile` — Positioned tile with elevation data
/// - `resolution` — Vertices per side in the output mesh (e.g., 128 for LOD0)
/// - `vertical_exaggeration` — Multiplier for elevation (1.0 = real scale)
pub fn generate_terrain_mesh(
    tile: &PositionedTile,
    resolution: u32,
    vertical_exaggeration: f32,
) -> Mesh {
    let res = resolution.max(2) as usize;
    let num_verts = (res + 1) * (res + 1);
    let num_indices = res * res * 6;

    let mut positions = Vec::with_capacity(num_verts);
    let mut normals = Vec::with_capacity(num_verts);
    let mut uvs = Vec::with_capacity(num_verts);
    let mut indices = Vec::with_capacity(num_indices);

    let side = tile.tile.resolution.samples_per_side();

    // Generate vertex grid
    for row in 0..=res {
        for col in 0..=res {
            let u = col as f32 / res as f32;
            let v = row as f32 / res as f32;

            // Position in Bevy world space (interpolate between SW and NE)
            let x = tile.world_sw.x + u * tile.world_width;
            // Z axis: SW.z is south, NE.z is north (Bevy -Z = north)
            let z = tile.world_sw.z + (1.0 - v) * (tile.world_ne.z - tile.world_sw.z);

            // Sample elevation from HGT data
            let sample_col = ((u * (side - 1) as f32) as usize).min(side - 1);
            // HGT rows: 0 = north, so invert v
            let sample_row = ((v * (side - 1) as f32) as usize).min(side - 1);
            let elevation = tile.tile.elevation_at(sample_row, sample_col) * vertical_exaggeration;

            positions.push([x, elevation, z]);
            uvs.push([u, v]);
            // Placeholder normal (computed below)
            normals.push([0.0, 1.0, 0.0]);
        }
    }

    // Compute normals from neighboring vertices
    let stride = res + 1;
    for row in 0..=res {
        for col in 0..=res {
            let idx = row * stride + col;
            let pos = Vec3::from(positions[idx]);

            let left = if col > 0 { Vec3::from(positions[idx - 1]) } else { pos };
            let right = if col < res { Vec3::from(positions[idx + 1]) } else { pos };
            let up = if row > 0 { Vec3::from(positions[idx - stride]) } else { pos };
            let down = if row < res { Vec3::from(positions[idx + stride]) } else { pos };

            let dx = right - left;
            let dz = down - up;
            let normal = dz.cross(dx).normalize_or_zero();
            normals[idx] = normal.to_array();
        }
    }

    // Generate triangle indices
    for row in 0..res {
        for col in 0..res {
            let i = (row * stride + col) as u32;
            let stride_u32 = stride as u32;

            // Two triangles per quad (CCW winding)
            indices.push(i);
            indices.push(i + stride_u32);
            indices.push(i + 1);

            indices.push(i + 1);
            indices.push(i + stride_u32);
            indices.push(i + stride_u32 + 1);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ============================================================================
// 4. Multi-tile terrain spawning
// ============================================================================

/// Load all terrain sources from GeoConfig and spawn them as mesh entities.
///
/// Each HGT tile becomes a separate Bevy entity with a terrain mesh,
/// positioned correctly on the WGS84 orbital grid.
pub fn spawn_terrain_tiles(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    tiles: &[PositionedTile],
    vertical_exaggeration: f32,
    chunk_resolution: u32,
) -> Vec<Entity> {
    let terrain_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.55, 0.35),
        perceptual_roughness: 0.9,
        metallic: 0.0,
        double_sided: true,
        cull_mode: None,
        ..default()
    });

    let mut entities = Vec::with_capacity(tiles.len());

    for tile in tiles {
        let mesh = generate_terrain_mesh(tile, chunk_resolution, vertical_exaggeration);
        let mesh_handle = meshes.add(mesh);

        let tile_name = format!(
            "Terrain_{}{}{}{}",
            if tile.tile.lat >= 0 { "N" } else { "S" },
            tile.tile.lat.abs(),
            if tile.tile.lon >= 0 { "E" } else { "W" },
            tile.tile.lon.abs(),
        );

        let entity = commands.spawn((
            Mesh3d(mesh_handle),
            MeshMaterial3d(terrain_material.clone()),
            Transform::IDENTITY,
            GeoTerrainChunk {
                grid_x: tile.tile.lon,
                grid_z: tile.tile.lat,
                lod: 0,
                mesh_path: tile_name.clone(),
            },
            Name::new(tile_name),
        )).id();

        entities.push(entity);

        tracing::info!(
            "Spawned terrain tile: {}°{} {}°{} ({}×{} m)",
            tile.tile.lat.abs(),
            if tile.tile.lat >= 0 { "N" } else { "S" },
            tile.tile.lon.abs(),
            if tile.tile.lon >= 0 { "E" } else { "W" },
            tile.world_width as i32,
            tile.world_height as i32,
        );
    }

    entities
}

/// Scan a directory for all .hgt files and load them
pub fn load_hgt_directory(dir: &Path) -> Vec<HgtTile> {
    let mut tiles = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Cannot read terrain directory {}: {}", dir.display(), e);
            return tiles;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("hgt") {
            match HgtTile::load(&path) {
                Ok(tile) => tiles.push(tile),
                Err(e) => tracing::warn!("Failed to load HGT tile: {}", e),
            }
        }
    }

    tracing::info!("Loaded {} HGT tiles from {}", tiles.len(), dir.display());
    tiles
}

/// Errors from terrain import
#[derive(Debug)]
pub enum TerrainImportError {
    /// File I/O error
    Io(std::path::PathBuf, std::io::Error),
    /// Parse error
    Parse(std::path::PathBuf, String),
}

impl std::fmt::Display for TerrainImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TerrainImportError::Io(path, e) => write!(f, "Failed to read {}: {}", path.display(), e),
            TerrainImportError::Parse(path, e) => write!(f, "Failed to parse {}: {}", path.display(), e),
        }
    }
}

impl std::error::Error for TerrainImportError {}
