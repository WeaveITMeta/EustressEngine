//! # Terrain System for Eustress Engine
//! 
//! High-performance terrain rendering with LOD, heightmaps, and per-cell
//! material identity. Shared between Engine Studio and Client.
//!
//! ## Features
//! - Chunk-based terrain with automatic LOD
//! - Procedural generation via Perlin noise
//! - Heightmap support (grayscale images)
//! - Material map: two of 256 material slots and their blend per cell
//! - Runtime editing (height painting, material painting)
//! - Camera-driven chunk streaming and view distance culling
//! - **Physics collisions** (static LOD-0 heightfield per chunk, requires `physics` feature)
//! - **Undo/Redo** of terrain edits through the host's undo stack
//! - **Advanced brushes** (noise stamps, erosion simulation)
//! - **GPU compute** mesh generation with CPU fallback
//! - **Greedy meshing** for optimized flat areas
//!
//! ## Architecture
//! - `TerrainConfig`: Configuration for terrain generation
//! - `TerrainData`: Runtime data (height raster, material map)
//! - `Chunk`: Individual terrain tile with LOD level
//! - `TerrainPlugin`: Main plugin for terrain systems
//! - `TerrainEditRecorder`: Records the raster tiles and volume bricks an
//!   edit touches, as before/after `TerrainTileDelta`s and
//!   `TerrainBrickDelta`s for the host's undo stack
//! - `NoiseBrush`: Advanced noise-based brushes
//! - `ErosionSimulation`: Hydraulic/thermal erosion
//!
//! ## Physics
//! Enable the `physics` feature to give every terrain chunk a static Avian3D
//! heightfield collider (see `collider`). It is always built at LOD 0, from
//! the same samples as the full-detail mesh, whatever LOD the chunk renders at.
//! Its friction follows the chunk's dominant material slot where that slot
//! names a realism material (`apply_terrain_chunk_friction`).
//!
//! ## Edits
//! Code that writes `TerrainData`'s height raster or material map marks the
//! touched chunks in `TerrainDirtyChunks`; `apply_terrain_dirty_chunks`
//! remeshes and re-collides them (see `dirty`). An undoable writer also
//! records the tiles it touches in a `TerrainEditRecorder` before writing
//! (see `history`).
//!
//! ## Materials on disk
//! Every writer of a Space's `Workspace/Terrain` directory (Save, the
//! worldgen and flat exporters, the heightmap importer, and any new one)
//! must emit `matmap/x{cx}_z{cz}.png` beside each `chunks/x{cx}_z{cz}.r16`:
//! RGBA8, one `[id_a, id_b, blend_b, 0]` material cell per raster cell (see
//! the `material` module docs and `toml_loader`). A chunk without a matmap
//! falls back to a legacy `splatmap/` PNG if one is there, converted on
//! load, and to all Grass if not.
//!
//! ## Material slots and textures
//! What each slot id looks like lives in the `TerrainMaterialSlots` resource
//! (see `material_slots`): the 23 built-ins on the bundled texture sets, plus
//! the custom slots of the Space the host points `TerrainMaterialSource` at,
//! from `_terrain.toml`'s palette and `materials/*.mat.toml`.
//! `texture_arrays` builds one albedo, normal and ORM array layer per
//! distinct texture set off the main thread. `surface_material` draws them:
//! once they are ready, every chunk of a terrain with a material layer moves
//! onto its `TerrainSurfaceMaterial`, which blends the strongest four slots
//! of the material map per pixel through Bevy's standard lighting. Until
//! then (or when the arrays cannot be built, and for procedural terrain)
//! chunks keep the vertex-colour material, which paints every slot in its
//! swatch colour from `TerrainData::slot_palette`. Chunks always spawn on
//! the vertex-colour material; the surface systems move them.
//!
//! ## Volumes
//! Caves, overhangs and tunnels live in a sparse `TerrainVolume` of CSG
//! bricks on the same root, layered over the heightfield as one signed field
//! (see `volume`). An empty volume leaves the terrain exactly its heightfield.
//! A chunk whose columns hold bricks renders by marching cubes at LOD 0 and
//! collides on a trimesh of that surface (see `marching`); every system that
//! builds a chunk mesh or collider goes through `generate_chunk_render_mesh`
//! (`generate_chunk_render_mesh_and_surface` when it spawns the chunk, so
//! mesh and collider share one march) and `attach_chunk_collider` /
//! `refresh_chunk_collider`, which choose. The
//! 3D brush modes (`BrushMode::VoxelAdd`, `VoxelRemove`, `VoxelSmooth`) write
//! the volume one CSG dab at a time (`apply_voxel_dab`), recording the bricks
//! each dab can write before it writes, so a stroke undoes exactly.
//!
//! ## Layers
//! A root's `TerrainData` is its editable base. Non-destructive layers
//! (splines, stamps, flatten pads, noise, material fills) are baked over it
//! into a `TerrainBaked` on the same root (see `layers`), present only while
//! the root has layers. Everything that turns terrain into pixels, physics or
//! gameplay answers reads through `surface_data`, which picks the bake when
//! there is one; every writer, and Save, keeps to the base. The regions the
//! writers mark dirty are re-baked before they are remeshed.
//!
//! The layers themselves are ordinary instances (`TerrainSpline` with its
//! `TerrainSplinePoint`s, `TerrainStamp`, `TerrainFlattenPad`,
//! `TerrainNoise`, `TerrainMaterialFill`) kept under
//! `Workspace/Terrain/Layers`; `TerrainLayersPlugin` hands them to the bake
//! whenever one changes (see `layer_instances`). A spline in Road mode is a
//! road: the same plugin lays a drivable ribbon and collider along the line
//! its corridor was baked to (see `road_surface`).
//!
//! ## Scatter
//! `TerrainScatter` instances, kept beside the layers, place grass, shrubs,
//! rocks, trees or a custom mesh over the finished ground by rules. They bake
//! nothing: `TerrainScatterPlugin` places each chunk deterministically from
//! the surface data, draws small objects as one merged mesh per chunk and
//! the rest as batched entities, streams them around the view, and places a
//! chunk again whenever its ground or the layer changes (see `scatter`, and
//! `scatter_meshes` for the procedural meshes).
//!
//! ## Water
//! Every water surface draws with one material that measures its depth
//! against the root's terrain height texture (see `water` and
//! `surface_material`): the ocean plane at sea level, the lakes
//! `TerrainWaterBody` instances flood over the finished ground, and the
//! ribbon each River spline with WaterSurface on lays along its channel (see
//! `water_bodies`). `TerrainWaterPlugin` builds them all; water has no
//! colliders.

pub mod config;
pub mod chunk;
pub mod mesh;
pub mod lod;
pub mod editor;
pub mod material;
pub mod history;
pub mod brushes;
pub mod compute;
pub mod toml_loader;
pub mod water;
pub mod collider;
pub mod dirty;
/// Shared world-space height query/write helpers — the single place that
/// encodes the `height_cache` normalization + chunk-local index math, so a
/// third caller (the road tool) doesn't hand-roll it a third time. See its
/// module docs for the two existing hand-rolled call sites this factors out.
pub mod height_query;
/// Spline path, elevation profile and drivable surface geometry of a road
/// (Catmull-Rom path, smoothed elevation profile, ribbon mesh, collision
/// boxes), which the spline layers and `road_surface` build on. Pure math,
/// no ECS. Referenced via the explicit `road::` path (NOT glob-re-exported),
/// matching the `worldgen`/`voxel_extract` convention for newer, domain-
/// specific modules, avoiding name collisions with the editor/brush modules.
pub mod road;
/// The ribbon mesh and collider laid along every Road-mode `TerrainSpline`,
/// built from the stations its corridor was baked along. Added by
/// `TerrainLayersPlugin`; referenced via the explicit `road_surface::` path.
pub mod road_surface;
/// Wave 9.C — imported-terrain voxel decode + multi-span column extractor +
/// `TerrainData` cache fill. Engine-free (no worlddb dep); the engine-side
/// loader reads Fjall voxels and calls into this. Referenced via the
/// explicit `voxel_extract::` path (NOT glob-re-exported) so its names
/// (`CHUNK_EDGE`, `Span`, …) don't collide with the other terrain modules'.
pub mod voxel_extract;
/// Phase B — deterministic multi-agent terrain *generation* (the new
/// generation mechanic): seam-free base elevation, hydrology, erosion,
/// climate/biome and material passes producing a `worldgen::GeneratedRegion`
/// the engine seam lifts into `TerrainData`. Engine-free pure math.
/// Referenced via the explicit `worldgen::` path (NOT glob-re-exported) so
/// its names don't collide with the brush/edit `editor`/`brushes` modules.
pub mod worldgen;
/// Sparse volumetric edits (ADD and CARVE signed-distance bricks) over the
/// heightfield: the terrain field, CSG writes and the `.vbk` brick files.
/// The names callers need are re-exported below; the rest (quantization,
/// lattice and file-name helpers) stay behind the explicit `volume::` path.
pub mod volume;
/// Marching cubes over the terrain field for chunks that hold volumetric
/// edits, and the mesh dispatch every chunk-meshing system goes through.
/// The caller-facing names are re-exported below; the tables and the
/// lattice-box mesher stay behind the explicit `marching::` path.
pub mod marching;
/// The material slot table (built-in and Space-defined slots, their
/// textures, tint, tiling and physics material), its loader and the systems
/// that keep it and every root's palette current.
pub mod material_slots;
/// The albedo, normal and ORM texture arrays the textured terrain material
/// samples, built off the main thread from the slot table's texture sets.
pub mod texture_arrays;
/// The textured terrain material (`StandardMaterial` extended with the
/// texture arrays, slot records and material map, shader embedded), and the
/// systems that keep it current and move chunks onto it and back.
pub mod surface_material;
/// Non-destructive terrain layers: their plain-data descriptions, the
/// engine-free bake over the editable base, and the `TerrainBaked` component
/// every reader sees through `surface_data`. Names callers need are
/// re-exported below; the layer parameter types stay behind `layers::`.
pub mod layers;
/// The terrain layer classes as instances: their components and field
/// tables, the mapping onto `layers::LayerDesc`, the system that keeps each
/// root's `TerrainBaked` in step with them, and a reader for hosts without
/// the Studio loader. The plugin and components are re-exported below; the
/// rest stays behind `layer_instances::`.
pub mod layer_instances;
/// Grass, shrub, rock and tree meshes built from code, a few seeded variants
/// per family, for scatter. Referenced via the explicit `scatter_meshes::`
/// path.
pub mod scatter_meshes;
/// Deterministic placement of every `TerrainScatter` layer over the surface
/// data, its merged meshes and batched entities, and their streaming around
/// the view. The plugin is re-exported below; the rest stays behind
/// `scatter::`.
pub mod scatter;
/// Lakes flooded from every `TerrainWaterBody` over the surface data, and the
/// water ribbon of every River spline, meshed for the shared water material.
/// Added by `water::TerrainWaterPlugin`; referenced via the explicit
/// `water_bodies::` path.
pub mod water_bodies;

pub use config::*;
pub use chunk::*;
pub use mesh::*;
pub use lod::*;
pub use editor::*;
pub use material::*;
pub use history::*;
pub use brushes::*;
pub use compute::*;
pub use water::*;
pub use height_query::*;
pub use collider::*;
pub use dirty::*;
pub use volume::{
    apply_box, apply_cylinder, apply_shape, apply_smooth, apply_sphere, chunk_has_volume,
    chunk_volume_y_range, decode_brick, encode_brick, field_gradient, field_normal,
    heightfield_slope_factor, heightfield_term, lattice_cell_size, lattice_surface_height,
    load_volume_bricks, material_at, sample_field, sample_field_lattice, sample_field_parts,
    save_volume_bricks, CsgOp, CsgShape, FieldSample, FieldTerm, TerrainVolume, VolumeBrick,
    VolumeCell, VolumeEdit, VolumeSaveReport, BRICK_EDGE,
};
pub use marching::{
    build_volume_chunk_geometry, chunk_collider_cost, chunk_mesh_cost, chunk_spawn_cost,
    chunk_uses_marching_cubes, generate_chunk_render_mesh, generate_chunk_render_mesh_and_surface,
    volume_chunk_triangles, VolumeChunkGeometry, VOLUMETRIC_CHUNK_COST,
};
pub use material_slots::{
    builtin_slot, keep_paint_material_defined, reload_terrain_material_slots, sync_terrain_slot_palette,
    watch_terrain_material_files, write_custom_material_toml, MaterialSlot, TerrainMaterialHit,
    TerrainMaterialQuery, TerrainMaterialSlots, TerrainMaterialSlotsPlugin, TerrainMaterialSource,
    TerrainSlotPalette, TerrainTextureSet,
};
pub use texture_arrays::{
    drive_terrain_texture_arrays, TerrainSlotSurface, TerrainTextureArraySet, TerrainTextureArrays,
    TerrainTextureLayer, TerrainTextureResolution, TerrainTextureSettings, TerrainTextureStatus,
};
pub use surface_material::{
    settle_terrain_surface_bindings, swap_terrain_chunk_materials, sync_terrain_height_textures, sync_terrain_surfaces,
    TerrainHeightTexture, TerrainHeightTextureRequest, TerrainSlotRecord, TerrainSurface, TerrainSurfaceBindings,
    TerrainSurfaceExtension, TerrainSurfaceMaterial, TerrainSurfaceParams, TerrainSurfacePlugin,
};
pub use layers::{surface_data, RebakeOutcome, TerrainBaked, TerrainRebake};
pub use layer_instances::{
    sync_terrain_layers, TerrainFlattenPad, TerrainLayersPlugin, TerrainMaterialFill, TerrainNoise, TerrainScatter,
    TerrainSpline, TerrainSplinePoint, TerrainStamp, TerrainWaterBody,
};
pub use scatter::{update_terrain_scatter, ScatterBatch, TerrainScatterPlugin};

use bevy::prelude::*;
use tracing::info;

/// Shared terrain plugin, added by the Client.
///
/// The Studio engine does not add it: `EngineTerrainPlugin` registers the
/// same streaming chain and dirty-chunk pipeline next to its editor tooling.
/// Keep the two registrations in step.
pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<TerrainMode>()
            .init_resource::<TerrainBrush>()
            .init_resource::<TerrainGenerationQueue>()
            .init_resource::<LodUpdateState>()  // Throttled LOD updates for performance
            .init_resource::<TerrainDirtyChunks>()

            // Types for reflection/serialization
            .register_type::<TerrainConfig>()
            .register_type::<TerrainData>()
            .register_type::<Chunk>()

            .init_resource::<ChunkSpawnThrottle>()

            // Initial fill, then LOD, streaming and culling around the
            // scene camera. Chained so each sees the chunks the previous
            // one spawned or despawned this frame.
            .add_systems(Update, (
                process_terrain_generation_queue,
                update_lod_system,
                chunk_spawn_system,
                chunk_cull_system,
            ).chain())

            // Editor systems (only run in Editor mode)
            .add_systems(Update, (
                toggle_editor_system,
                terrain_paint_system,
            ).run_if(resource_equals(TerrainMode::Editor)))

            // Ungated: edits also arrive outside Editor mode (undo, layer
            // changes), and their chunks still need rebuilding.
            .add_systems(Update, apply_terrain_dirty_chunks
                .after(chunk_cull_system)
                .after(terrain_paint_system));

        // Material slots and their texture arrays, then the textured material
        // that draws them. The engine adds the same plugins from
        // `EngineTerrainPlugin`; the guards keep a host that adds both from
        // registering them twice.
        if !app.is_plugin_added::<TerrainMaterialSlotsPlugin>() {
            app.add_plugins(TerrainMaterialSlotsPlugin);
        }
        if !app.is_plugin_added::<TerrainSurfacePlugin>() {
            app.add_plugins(TerrainSurfacePlugin);
        }
        // Layer instances baked over the base, same guard.
        if !app.is_plugin_added::<TerrainLayersPlugin>() {
            app.add_plugins(TerrainLayersPlugin);
        }
        // Scatter placed over the finished ground, same guard.
        if !app.is_plugin_added::<TerrainScatterPlugin>() {
            app.add_plugins(TerrainScatterPlugin);
        }
        // The ocean, lakes and rivers and their material, same guard.
        if !app.is_plugin_added::<TerrainWaterPlugin>() {
            app.add_plugins(TerrainWaterPlugin);
        }
    }
}

/// Render orders at and above this belong to UI overlays (the Studio's Slint
/// chrome camera sits at 300), never to a camera looking at the world.
const OVERLAY_CAMERA_ORDER: isize = 300;

/// World position the terrain streams, culls and picks LOD around: the camera
/// the user is looking through.
///
/// That is the active `Camera3d` at order 0 (the Studio scene camera, the
/// Client's avatar camera). In Studio Play mode the scene camera is
/// deactivated and the avatar camera renders at order 10, so the lowest
/// active order in `1..OVERLAY_CAMERA_ORDER` comes next; following the
/// parked editor camera there would cull the ground from under the player.
/// Last comes the order-0 camera even while inactive, which covers the frames
/// between pressing Play and the avatar camera spawning.
///
/// Never `single()`: the Studio engine runs several `Camera3d` entities at
/// once (the scene camera at order 0, the Slint overlay at order 300, the AI
/// camera at order -1), so `single()` always errors there and a system gated
/// on it silently never runs.
pub fn scene_camera_translation(
    cameras: &Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) -> Option<Vec3> {
    let active_scene_camera = cameras
        .iter()
        .find(|(camera, _)| camera.is_active && camera.order == 0);
    let active_play_camera = || {
        cameras
            .iter()
            .filter(|(camera, _)| camera.is_active && camera.order > 0 && camera.order < OVERLAY_CAMERA_ORDER)
            .min_by_key(|(camera, _)| camera.order)
    };
    let parked_scene_camera = || cameras.iter().find(|(camera, _)| camera.order == 0);
    active_scene_camera
        .or_else(active_play_camera)
        .or_else(parked_scene_camera)
        .map(|(_, transform)| transform.translation())
}

/// Terrain mode: Render-only or Editor (allows painting)
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TerrainMode {
    #[default]
    Render,
    Editor,
}

/// Marker component for terrain root entity
#[derive(Component, Default)]
pub struct TerrainRoot;

/// The vertex-colour material every chunk of a terrain spawns with, kept on
/// its [`TerrainRoot`] so chunks streamed in after the initial fill (or after
/// all chunks were culled) match the rest. Chunks wear it until the root's
/// textured `TerrainSurface` is drawable, and again whenever it stops being
/// so (see `surface_material`).
#[derive(Component, Clone, Debug)]
pub struct TerrainChunkMaterial(pub Handle<StandardMaterial>);

/// Resource to track async terrain generation progress
///
/// Chunks read `TerrainConfig` / `TerrainData` from their live
/// [`TerrainRoot`], not from a copy taken here, so an edit made while the
/// fill is still running reaches the chunks spawned after it.
#[derive(Resource, Default)]
pub struct TerrainGenerationQueue {
    /// Chunks waiting to be spawned (chunk_pos, terrain_entity)
    pub pending_chunks: Vec<(IVec2, Entity)>,
    /// Terrain material handle (shared across chunks)
    pub material: Option<Handle<StandardMaterial>>,
    /// Chunks spawned per frame (tune for performance)
    pub chunks_per_frame: usize,
    /// Total chunks to spawn
    pub total_chunks: usize,
    /// Chunks spawned so far
    pub spawned_count: usize,
}

impl TerrainGenerationQueue {
    /// Check if generation is in progress
    pub fn is_generating(&self) -> bool {
        !self.pending_chunks.is_empty()
    }
    
    /// Get progress as percentage (0.0 - 1.0)
    pub fn progress(&self) -> f32 {
        if self.total_chunks == 0 {
            1.0
        } else {
            self.spawned_count as f32 / self.total_chunks as f32
        }
    }
}

/// The `StandardMaterial` terrain is lit with: the vertex-colour chunk
/// material, and the base of the textured `TerrainSurfaceMaterial`, so both
/// paths light the ground alike. White, so the mesh's per-vertex colours show
/// through unmodified (StandardMaterial multiplies base_color by the vertex
/// colour); matte, with a low dielectric reflectance.
pub(crate) fn terrain_standard_material() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        metallic: 0.0,
        reflectance: 0.3,
        ..default()
    }
}

/// The vertex-colour chunk material for a terrain (see
/// [`TerrainChunkMaterial`]). It has no textures: the mesh's vertex colours
/// paint each cell in its material slots' swatches. The UV transform repeats
/// a chunk's `[0, 1]` UVs every 8 m, the density a texture bound to it would
/// tile at.
pub fn new_terrain_chunk_material(
    config: &TerrainConfig,
    materials: &mut Assets<StandardMaterial>,
) -> Handle<StandardMaterial> {
    const TERRAIN_TILE_WORLD_SIZE: f32 = 8.0;
    let tile_repeat = config.chunk_size / TERRAIN_TILE_WORLD_SIZE;
    materials.add(StandardMaterial {
        uv_transform: bevy::math::Affine2::from_scale(
            bevy::math::Vec2::new(tile_repeat, tile_repeat),
        ),
        ..terrain_standard_material()
    })
}

/// Spawns a complete terrain entity with ASYNC chunk generation
///
/// This queues chunks for generation over multiple frames to prevent UI freezing.
/// A raster terrain within `MAX_RESIDENT_RASTER_CHUNKS` is queued whole.
/// When `physics` feature is enabled, each chunk also gets a static LOD-0
/// heightfield collider as it spawns (see `collider`).
///
/// The root starts with an empty [`TerrainVolume`]; see
/// [`spawn_terrain_with_volume`] for a terrain that already has one.
pub fn spawn_terrain(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    config: TerrainConfig,
    data: TerrainData,
) -> Entity {
    spawn_terrain_with_volume(commands, meshes, materials, config, data, TerrainVolume::default())
}

/// [`spawn_terrain`] for a terrain that already has volumetric edits, such
/// as a Space whose `Workspace/Terrain/volume` holds `.vbk` bricks. The
/// volume goes on the root in the same spawn, so no chunk can be meshed
/// before its caves are there.
pub fn spawn_terrain_with_volume(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    config: TerrainConfig,
    data: TerrainData,
    volume: TerrainVolume,
) -> Entity {
    #[cfg(feature = "physics")]
    info!("🏔️ Spawning terrain with PHYSICS: {}x{} chunks, {} LOD levels (async)",
        config.chunks_x, config.chunks_z, config.lod_levels);

    #[cfg(not(feature = "physics"))]
    info!("🏔️ Spawning terrain: {}x{} chunks, {} LOD levels (async)",
        config.chunks_x, config.chunks_z, config.lod_levels);

    let terrain_material = new_terrain_chunk_material(&config, materials);

    // Build list of chunks to spawn. A raster small enough to stay resident
    // is queued whole: its chunks carry the colliders bodies rest on, and a
    // headless host (no camera, so no spawn scan) gets them nowhere else.
    // Anything else queues the middle and leaves the rest to streaming.
    // Decided before `data` moves into the root below.
    let (half_x, half_z) = if raster_fully_resident(&config, &data) {
        (config.chunks_x as i32, config.chunks_z as i32)
    } else {
        ((config.chunks_x / 2) as i32, (config.chunks_z / 2) as i32)
    };

    // Spawn terrain root (without chunks - they'll be added async)
    let terrain_entity = commands.spawn((
        TerrainRoot,
        config,
        data,
        volume,
        TerrainChunkMaterial(terrain_material.clone()),
        Transform::default(),
        Visibility::default(),
        Name::new("Terrain"),
    )).id();

    let mut pending_chunks = Vec::new();
    for cx in -half_x..=half_x {
        for cz in -half_z..=half_z {
            pending_chunks.push((IVec2::new(cx, cz), terrain_entity));
        }
    }
    // The queue pops from the back, so the fill grows outward from the
    // middle instead of starting in a far corner.
    pending_chunks.sort_by_key(|(pos, _)| std::cmp::Reverse(pos.x * pos.x + pos.y * pos.y));

    let total_chunks = pending_chunks.len();

    // Queue for async generation
    commands.insert_resource(TerrainGenerationQueue {
        pending_chunks,
        material: Some(terrain_material),
        chunks_per_frame: 2,  // Spawn 2 chunks per frame to avoid frame spikes
        total_chunks,
        spawned_count: 0,
    });

    info!("📋 Queued {} chunks for async generation", total_chunks);

    terrain_entity
}

/// System to process terrain generation queue over multiple frames
///
/// `chunks_per_frame` is a budget in heightfield chunks: a chunk holding
/// volumetric edits costs the cubes it marches per lattice cell, at least
/// [`VOLUMETRIC_CHUNK_COST`] (see [`chunk_spawn_cost`]), and the first chunk
/// of a frame always spawns.
pub fn process_terrain_generation_queue(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut queue: ResMut<TerrainGenerationQueue>,
    roots: Query<(&TerrainConfig, &TerrainData, Option<&TerrainVolume>, Option<&TerrainBaked>), With<TerrainRoot>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) {
    if queue.pending_chunks.is_empty() {
        return;
    }
    let Some(material) = queue.material.clone() else {
        return;
    };
    // Pick each chunk's first LOD from the scene camera when there is one,
    // so the LOD system does not have to remesh the whole fill afterwards.
    // Hosts without a camera (headless) measure from the world origin.
    let viewer = scene_camera_translation(&cameras).unwrap_or(Vec3::ZERO);

    let budget = queue.chunks_per_frame.max(1);
    let mut spent = 0;
    while spent < budget {
        let Some((chunk_pos, terrain_entity)) = queue.pending_chunks.pop() else {
            break;
        };
        queue.spawned_count += 1;

        // A root despawned mid-fill (Space switch, terrain cleared) takes
        // its pending chunks with it; parenting them to it would fail.
        let Ok((config, data, volume, baked)) = roots.get(terrain_entity) else {
            continue;
        };
        let volume = volume.unwrap_or(TerrainVolume::empty());
        let data = surface_data(data, baked);

        let lod = config.lod_for_distance(chunk_lod_distance(chunk_pos, config, data, viewer));
        spent += chunk_spawn_cost(chunk_pos, lod, config, data, volume);
        let (mesh_handle, surface) =
            generate_chunk_render_mesh_and_surface(chunk_pos, lod, config, data, volume, &mut meshes);

        let chunk_entity = commands.spawn((
            Chunk {
                position: chunk_pos,
                lod,
            },
            Mesh3d(mesh_handle),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(chunk_world_position(chunk_pos, config)),
            Visibility::default(),
            Name::new(format!("Chunk_{}_{}", chunk_pos.x, chunk_pos.y)),
            ChildOf(terrain_entity),
        )).id();
        attach_chunk_collider(&mut commands, chunk_entity, chunk_pos, config, data, volume, surface);
    }

    // Log progress periodically
    if queue.pending_chunks.is_empty() {
        info!("✅ Terrain generation complete: {} chunks spawned", queue.spawned_count);
        // Clear the queue
        queue.material = None;
        queue.total_chunks = 0;
        queue.spawned_count = 0;
    } else if queue.spawned_count % 20 == 0 {
        info!("🏔️ Terrain generation: {:.0}% ({}/{})", 
            queue.progress() * 100.0, 
            queue.spawned_count, 
            queue.total_chunks);
    }
}

/// Calculate world position for a chunk
pub fn chunk_world_position(chunk_pos: IVec2, config: &TerrainConfig) -> Vec3 {
    Vec3::new(
        chunk_pos.x as f32 * config.chunk_size,
        0.0,
        chunk_pos.y as f32 * config.chunk_size,
    )
}

/// Toggle editor mode with 'T' key
fn toggle_editor_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<TerrainMode>,
) {
    if keys.just_pressed(KeyCode::KeyT) {
        *mode = match *mode {
            TerrainMode::Render => {
                info!("🎨 Terrain Editor: ENABLED");
                TerrainMode::Editor
            }
            TerrainMode::Editor => {
                info!("🎨 Terrain Editor: DISABLED");
                TerrainMode::Render
            }
        };
    }
}
