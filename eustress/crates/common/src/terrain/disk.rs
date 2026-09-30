//! # A Space's terrain read back from `Workspace/Terrain`
//!
//! The on-disk terrain format every writer emits (Save, the worldgen and flat
//! exporters, the heightmap importer): `_terrain.toml`, one
//! `chunks/x{cx}_z{cz}.r16` heightmap and `matmap/x{cx}_z{cz}.png` material
//! map per chunk, the volumetric edits in `volume/*.vbk`, and the water in
//! `water.bin`. Studio's Space-open loader, its Terrain class sync and
//! generate paths, and the Player's Space opener all read it through
//! [`hydrate_terrain_from_disk`] and spawn the result with
//! [`HydratedTerrain::spawn`], so every terrain spawned from disk carries its
//! volumetric edits and its water.
//!
//! [`TerrainSnapshot`] is the other direction without a disk: a copy of a
//! live terrain whose [`TerrainSnapshot::encode`] returns the files a save
//! would write, byte for byte, through the same encoders (`toml_loader`'s
//! R16 and matmap, `volume`'s brick, `voxel_water`'s water). The host's
//! world export takes it on the main thread and encodes it on a worker, so a
//! joining Player gets the terrain as it stands, unsaved edits included,
//! without a write to the Space.

use std::borrow::Cow;
use std::path::Path;

use bevy::prelude::*;

use super::volume::{brick_file_name, VOLUME_DIR_NAME};
use super::voxel_water::{encode_voxel_water, load_voxel_water, TerrainVoxelWater, UnreadWaterFile, WATER_FILE_NAME};
use super::{
    encode_brick, lattice_cell_size, load_volume_bricks, rebase_height_band, spawn_terrain_with_volume, toml_loader,
    TerrainConfig, TerrainData, TerrainVolume,
};

/// A terrain read back from a `Workspace/Terrain/` directory, ready for
/// [`spawn_terrain_with_volume`].
pub struct HydratedTerrain {
    pub config: TerrainConfig,
    pub data: TerrainData,
    /// The volumetric edits from `volume/*.vbk`, empty when there are none.
    pub volume: TerrainVolume,
    /// The water from `water.bin`, `None` when there is none.
    pub water: Option<TerrainVoxelWater>,
    /// Why a `water.bin` that was there could not be used (see
    /// [`UnreadWaterFile`]); `None` when it loaded or was not there.
    pub unread_water: Option<String>,
    /// Chunk heightmaps that were found and read.
    pub chunk_files: usize,
}

impl HydratedTerrain {
    /// Spawn the terrain root with its config, raster, volume and water in
    /// one spawn, so no chunk meshes before its caves are there.
    pub fn spawn(
        self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Entity {
        let root = spawn_terrain_with_volume(commands, meshes, materials, self.config, self.data, self.volume);
        if let Some(water) = self.water {
            commands.entity(root).insert(water);
        }
        if self.unread_water.is_some() {
            commands.entity(root).insert(UnreadWaterFile);
        }
        root
    }
}

/// Hydrate a terrain from a `Workspace/Terrain/` directory, the exact recipe
/// the exporters write for (see `worldgen/export.rs`): `_terrain.toml`, then
/// `to_terrain_config()`, `resize_cache` and `load_chunks_from_disk` (SIGNED
/// centered `[-N, +N]` chunk coords, never the importer's unsigned math),
/// then the `volume/*.vbk` bricks and `water.bin`.
///
/// Returns `Err` when `_terrain.toml` is missing/unparseable; a toml with
/// zero readable chunks still returns `Ok` (flat terrain: the config is
/// valid, chunks may stream in later or simply not exist yet). A brick file
/// that cannot be read is skipped with a warning (see `load_volume_bricks`),
/// and so is a water file (see `load_voxel_water`).
pub fn hydrate_terrain_from_disk(terrain_dir: &Path) -> Result<HydratedTerrain, String> {
    let toml = toml_loader::load_terrain_toml(&terrain_dir.join("_terrain.toml"))?;
    let config = toml.to_terrain_config();
    let mut data = TerrainData::procedural();
    data.resize_cache(&config);
    let loaded = toml_loader::load_chunks_from_disk(terrain_dir, &config, &mut data);
    let volume = load_volume_bricks(terrain_dir, &config);
    let (water, unread_water) = match load_voxel_water(terrain_dir, &data) {
        Ok(water) => (water, None),
        Err(error) => {
            tracing::warn!("terrain water: {error}; loading without water, and Save keeps the file");
            (None, Some(error))
        }
    };
    Ok(HydratedTerrain { config, data, volume, water, unread_water, chunk_files: loaded.len() })
}

/// Where a Space keeps its terrain, relative to the Space folder.
pub const TERRAIN_DIR_IN_SPACE: &str = "Workspace/Terrain";

/// A copy of a live terrain, taken cheaply (clones, no encoding) so its
/// files can be encoded off the main thread (see the module docs).
#[derive(Clone, Debug)]
pub struct TerrainSnapshot {
    pub config: TerrainConfig,
    pub data: TerrainData,
    pub volume: TerrainVolume,
    pub water: Option<TerrainVoxelWater>,
    /// The Space's `_terrain.toml` text: it carries what the config does not
    /// (materials, water style), and [`Self::encode`] only moves its band.
    pub toml_text: String,
}

impl TerrainSnapshot {
    /// The `Workspace/Terrain` files a save of this terrain would write, as
    /// Space-relative paths with `/` separators and their bytes:
    /// `_terrain.toml` (its height band widened first when a height lies
    /// outside it, exactly as Save widens it, with the raster rebased on a
    /// copy), `chunks/x{cx}_z{cz}.r16` for every chunk of the grid,
    /// `matmap/x{cx}_z{cz}.png` for every chunk when the terrain has a
    /// material layer, `volume/b{x}_{y}_{z}.vbk` for every brick, and
    /// `water.bin` when there is water to keep. Fails where a save would: a
    /// band the toml cannot take, a material layer of the wrong size, a
    /// material layer without the `image` feature's PNG encoder.
    pub fn encode(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        let mut config = self.config.clone();
        let (toml_text, data): (Cow<str>, Cow<TerrainData>) = match config.band_covering(&self.data) {
            None => (Cow::Borrowed(self.toml_text.as_str()), Cow::Borrowed(&self.data)),
            Some(wanted) => {
                let (text, band) = toml_loader::rewrite_height_band(&self.toml_text, wanted)?;
                let mut data = self.data.clone();
                rebase_height_band(&mut config, &mut data, band);
                (Cow::Owned(text), Cow::Owned(data))
            }
        };
        let dir = Path::new(TERRAIN_DIR_IN_SPACE);
        let relative = |path: std::path::PathBuf| path.to_string_lossy().replace('\\', "/");
        let mut files = vec![(relative(dir.join("_terrain.toml")), toml_text.as_bytes().to_vec())];

        for chunk in config.grid_chunks() {
            let heights = toml_loader::chunk_heights(&config, &data, chunk);
            let bytes = toml_loader::encode_r16(&heights, config.chunk_resolution)?;
            files.push((relative(toml_loader::chunk_r16_path(dir, chunk.x, chunk.y)), bytes));
        }
        if !data.material_cache.is_empty() {
            files.extend(encode_material_maps(&config, &data)?);
        }
        let cell = lattice_cell_size(&config);
        for (coord, brick) in self.volume.bricks() {
            let path = dir.join(VOLUME_DIR_NAME).join(brick_file_name(coord));
            files.push((relative(path), encode_brick(brick, cell)));
        }
        if let Some(water) = self.water.as_ref().filter(|water| !water.is_empty()) {
            files.push((relative(dir.join(WATER_FILE_NAME)), encode_voxel_water(water)?));
        }
        Ok(files)
    }
}

/// Every chunk's matmap PNG, as [`TerrainSnapshot::encode`] lists them.
#[cfg(feature = "image")]
fn encode_material_maps(config: &TerrainConfig, data: &TerrainData) -> Result<Vec<(String, Vec<u8>)>, String> {
    if !data.has_material_layer() {
        return Err(format!(
            "material map holds {} cells, expected {} ({}x{})",
            data.material_cache.len(),
            data.cache_width as usize * data.cache_height as usize,
            data.cache_width,
            data.cache_height
        ));
    }
    let dir = Path::new(TERRAIN_DIR_IN_SPACE);
    let mut files = Vec::new();
    for chunk in config.grid_chunks() {
        let cells = toml_loader::chunk_material_cells(config, data, chunk)?;
        let png = toml_loader::encode_material_tile_png(&cells, config.chunk_resolution)
            .map_err(|error| format!("matmap x{}_z{}: {}", chunk.x, chunk.y, error))?;
        let path = toml_loader::chunk_matmap_path(dir, chunk.x, chunk.y);
        files.push((path.to_string_lossy().replace('\\', "/"), png));
    }
    Ok(files)
}

/// Without the `image` feature there is no PNG encoder, so a terrain with a
/// material layer cannot be encoded, as it cannot be saved.
#[cfg(not(feature = "image"))]
fn encode_material_maps(_config: &TerrainConfig, _data: &TerrainData) -> Result<Vec<(String, Vec<u8>)>, String> {
    Err("eustress-common was built without the `image` feature, so matmap PNGs cannot be encoded".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::worldgen::export::{export_flat_to_space, FlatSpec};
    use crate::terrain::voxel_water::{save_voxel_water, water_file_path};
    use crate::terrain::{apply_sphere, height_at_world, save_volume_bricks, CsgOp};

    /// A flat plate written by the flat exporter, with a cave carved into it
    /// and saved as volume bricks, reads back whole: its config, every chunk
    /// heightmap at the authored height, and every brick.
    #[test]
    fn a_saved_terrain_hydrates_with_its_chunks_and_volume() {
        let root = std::env::temp_dir().join(format!("eustress_terrain_disk_hydrate_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let spec = FlatSpec {
            half_extent: 1,
            chunk_size: 64.0,
            chunk_resolution: 16,
            height_m: 10.0,
            height_offset: 0.0,
            height_scale: 100.0,
            ..FlatSpec::default()
        };
        let summary = export_flat_to_space(&spec, &root).expect("the flat plate exports");
        assert_eq!(summary.chunks_written, 9);

        let terrain_dir = root.join("Workspace").join("Terrain");
        let config = toml_loader::load_terrain_toml(&terrain_dir.join("_terrain.toml"))
            .expect("the exported toml loads")
            .to_terrain_config();
        let mut volume = TerrainVolume::new();
        apply_sphere(&config, &mut volume, Vec3::new(5.0, 8.0, -3.0), 6.0, CsgOp::Carve, None);
        assert!(volume.brick_count() > 0, "the carve wrote bricks");
        save_volume_bricks(&terrain_dir, &config, &volume).expect("the bricks save");

        let hydrated = hydrate_terrain_from_disk(&terrain_dir).expect("the terrain hydrates");
        assert_eq!(hydrated.chunk_files, 9);
        assert_eq!(hydrated.config.chunk_resolution, 16);
        assert_eq!((hydrated.config.chunks_x, hydrated.config.chunks_z), (config.chunks_x, config.chunks_z));
        assert_eq!(hydrated.volume.brick_count(), volume.brick_count());
        for (coord, brick) in volume.bricks() {
            assert!(hydrated.volume.brick(coord) == Some(brick), "brick {coord} differs after hydrating");
        }
        let tolerance = 0.5 * spec.height_scale / 65535.0 + 1e-4;
        let height = height_at_world(&hydrated.config, &hydrated.data, 20.0, -30.0);
        assert!((height - spec.height_m).abs() <= tolerance, "the plate hydrated at Y={height}");
        assert!(hydrated.water.is_none() && hydrated.unread_water.is_none(), "no water file, no water");

        // Water saved beside it comes back with it; a file for another
        // raster is reported and left out.
        let (width, depth) = (hydrated.data.cache_width, hydrated.data.cache_height);
        let cells = (width * depth) as usize;
        let water = TerrainVoxelWater {
            levels: (0..cells).map(|i| if i % 3 == 0 { 12.0 } else { f32::NAN }).collect(),
            width,
            height: depth,
            color: Some([0.2, 0.5, 0.7]),
            transparency: None,
        };
        save_voxel_water(&terrain_dir, Some(&water), false).expect("the water saves");
        let hydrated = hydrate_terrain_from_disk(&terrain_dir).expect("the terrain hydrates with water");
        let back = hydrated.water.expect("the water came back");
        assert_eq!((back.width, back.height, back.color), (width, depth, water.color));
        assert_eq!(back.levels.iter().filter(|level| !level.is_nan()).count(), cells.div_ceil(3));
        let other = TerrainVoxelWater { levels: vec![1.0; 4], width: 2, height: 2, ..water.clone() };
        save_voxel_water(&terrain_dir, Some(&other), false).expect("the other water saves");
        let hydrated = hydrate_terrain_from_disk(&terrain_dir).expect("the terrain still hydrates");
        assert!(hydrated.water.is_none() && hydrated.unread_water.is_some());

        // A fresh export takes the old terrain's water with it.
        export_flat_to_space(&spec, &root).expect("the plate exports again");
        assert!(!water_file_path(&terrain_dir).exists(), "the old water went with the old terrain");

        // Without `_terrain.toml` there is no terrain to hydrate.
        std::fs::remove_file(terrain_dir.join("_terrain.toml")).expect("the toml is removable");
        assert!(hydrate_terrain_from_disk(&terrain_dir).is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    /// A snapshot's files, written into an empty Space, hydrate back to the
    /// terrain it was taken from, a height past the saved band included.
    #[test]
    fn a_snapshot_encodes_the_files_a_save_would_write() {
        let root = std::env::temp_dir().join(format!("eustress_terrain_snapshot_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let spec = FlatSpec {
            half_extent: 1,
            chunk_size: 64.0,
            chunk_resolution: 16,
            height_m: 10.0,
            height_offset: 0.0,
            height_scale: 100.0,
            ..FlatSpec::default()
        };
        export_flat_to_space(&spec, &root).expect("the flat plate exports");
        let terrain_dir = root.join("Workspace").join("Terrain");
        let mut live = hydrate_terrain_from_disk(&terrain_dir).expect("the plate hydrates");
        // A peak far above the 0..100 m band, a cave and some water.
        live.data.height_cache[5] = live.config.normalized_height(250.0);
        apply_sphere(&live.config, &mut live.volume, Vec3::new(5.0, 8.0, -3.0), 6.0, CsgOp::Carve, None);
        let cells = (live.data.cache_width * live.data.cache_height) as usize;
        let water = TerrainVoxelWater {
            levels: (0..cells).map(|i| if i % 7 == 0 { 11.0 } else { f32::NAN }).collect(),
            width: live.data.cache_width,
            height: live.data.cache_height,
            ..TerrainVoxelWater::default()
        };
        let snapshot = TerrainSnapshot {
            config: live.config.clone(),
            data: live.data.clone(),
            volume: live.volume.clone(),
            water: Some(water.clone()),
            toml_text: std::fs::read_to_string(terrain_dir.join("_terrain.toml")).expect("the toml reads"),
        };
        let files = snapshot.encode().expect("the snapshot encodes");
        assert!(files.iter().all(|(path, _)| path.starts_with("Workspace/Terrain/") && !path.contains('\\')));
        let count = |prefix: &str| files.iter().filter(|(path, _)| path.starts_with(prefix)).count();
        assert_eq!(count("Workspace/Terrain/chunks/"), 9, "every chunk of the 3 x 3 grid");
        assert_eq!(count("Workspace/Terrain/volume/"), live.volume.brick_count());
        assert_eq!(count("Workspace/Terrain/water.bin"), 1);

        let copy = root.join("copy");
        for (path, bytes) in &files {
            let target = copy.join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(&target, bytes).unwrap();
        }
        let back = hydrate_terrain_from_disk(&copy.join("Workspace").join("Terrain")).expect("the copy hydrates");
        assert!(back.config.height_scale > 200.0, "the band widened to hold the peak: {:?}", back.config.height_scale);
        let tolerance = back.config.height_scale / 65535.0 + 1e-3;
        let world_height = |config: &TerrainConfig, data: &TerrainData, i: usize| config.world_height(data.height_cache[i]);
        for i in [0usize, 5, 17, cells - 1] {
            let (a, b) = (world_height(&live.config, &live.data, i), world_height(&back.config, &back.data, i));
            assert!((a - b).abs() <= tolerance, "cell {i}: {a} against {b}");
        }
        assert_eq!(back.volume.brick_count(), live.volume.brick_count());
        let back_water = back.water.expect("the water came back");
        assert_eq!(back_water.levels.iter().filter(|l| !l.is_nan()).count(), cells.div_ceil(7));
        // The snapshot itself was left as it was taken.
        assert_eq!(snapshot.config.height_scale, 100.0);
        std::fs::remove_dir_all(&root).ok();
    }
}
