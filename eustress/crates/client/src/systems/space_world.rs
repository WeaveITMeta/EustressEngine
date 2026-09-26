//! Opens a Eustress Space in the Player instead of the hardcoded demo scene.
//!
//! Before this, the Client's world was `spawn_baseplate` + `spawn_welcome_cube`
//! — two hardcoded primitives — and `space_fetch` unpacked archives that
//! nothing consumed. There was no concept of a *current Space* anywhere in the
//! shell, which is why published content could never be played.
//!
//! Which Space, in order:
//! 1. `EUSTRESS_SPACE` — an absolute path to a Space root.
//! 2. `<workspace>/Movement/Spaces/Climbing` — the movement test course.
//! 3. Nothing; the shell keeps its demo scene.
//!
//! The workspace root resolution mirrors the engine's: `EUSTRESS_WORKSPACE`,
//! else `~/Documents/Eustress`. It deliberately does **not** use
//! `dirs::document_dir()`, because on Windows that follows OneDrive's Known
//! Folder Move and lands somewhere the engine is not looking.
//!
//! ## Parts
//!
//! A Space opened on this computer spawns its parts straight from its files
//! (`space_read`). A world that arrived from a host, or as a published
//! simulation, is read into a DataModel tree instead (`tree_read`), set up
//! by `play_session::begin_session` as Studio's Play sets up its own, and
//! inserted as the session's `PlayDataModel` with its [`SceneKeys`]; the
//! Play runtime Studio shares draws it as a replica. The tree is where
//! replication writes, so the host's changes reach the screen, and each
//! part is drawn once.
//!
//! ## Terrain
//!
//! A Space's terrain opens with it and replaces the terrain of the Space
//! before, from its `Workspace/Terrain` directory:
//!
//! - With a `_terrain.toml`, its chunk heightmaps, material maps and volume
//!   bricks are read and spawned while the Space opens
//!   (`hydrate_terrain_from_disk`, the reader Studio uses).
//! - Otherwise, when the Roblox importer left voxel chunk files in
//!   `voxel_chunks/` and the Terrain instance (`_instance.toml`) is not
//!   cleared, the imported voxel terrain is read and built on the
//!   `AsyncComputeTaskPool` (`voxel_import`, the build Studio runs over its
//!   world database), and its root spawns when the build lands, provided its
//!   Space is still the open one. [`SpaceTerrainLoad`] says whether a build
//!   is still on its way. The load window stays where the Space opened it:
//!   the Player does not move the window of an import wider than it to follow
//!   the camera, as Studio does. Its cells are laid out in metres, converted
//!   from the unit the Terrain instance declares, as the Space's parts are,
//!   so the two line up as they do in Studio.
//!
//! Either way the terrain material slots load from the Space's
//! `Workspace/Terrain`, so its custom materials and imported Roblox colours
//! apply.

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};

use bevy::ecs::system::SystemParam;
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};
use eustress_common::datamodel::{is_base_part, DataModel};
use eustress_common::space_read::{read_space_parts, spawn_space_parts, SpawnedFromSpace};
use eustress_common::play_session::{
    begin_session, space_character_auto_loads, FromTree, PlayAssetRoot, PlayDataModel, SessionCamera, SessionStart,
};
use eustress_play_runtime::material_registry::{register_space_materials, MaterialRegistry};

use super::net_replica::SceneKeys;
use eustress_common::terrain::disk::hydrate_terrain_from_disk;
use eustress_common::terrain::layer_instances::{
    read_layer_instances, spawn_layer_instances, TerrainFlattenPad, TerrainMaterialFill, TerrainNoise, TerrainScatter,
    TerrainSpline, TerrainStamp, TerrainWaterBody,
};
use eustress_common::terrain::voxel_import::{self, BuiltVoxelTerrain};
use eustress_common::terrain::{
    process_terrain_generation_queue, reload_terrain_material_slots, ChunkSpawnThrottle, TerrainMaterialSource,
    TerrainRoot,
};

/// The terrain layer, scatter and water body instances a Space spawns
/// (a spline's points are its children).
type TerrainLayerFilter = Or<(
    With<TerrainSpline>,
    With<TerrainStamp>,
    With<TerrainFlattenPad>,
    With<TerrainNoise>,
    With<TerrainMaterialFill>,
    With<TerrainScatter>,
    With<TerrainWaterBody>,
)>;

/// Where the Space came from, so the rest of the shell can tell whether it is
/// showing authored content or the fallback demo.
#[derive(Resource, Debug, Clone, Default)]
pub struct OpenSpace {
    pub root: Option<PathBuf>,
    pub parts: usize,
    pub skipped: usize,
    /// Where a character's feet go: on the Space's SpawnLocation, when it
    /// has one.
    pub spawn: Option<Vec3>,
}

/// Open a Space folder now. How a world that arrived at runtime (from a host,
/// or a published simulation) gets opened.
#[derive(Message, Debug, Clone)]
pub struct OpenSpaceRequest {
    pub root: PathBuf,
}

/// A Space finished opening: its parts, terrain layers and on-disk terrain
/// are spawned. Imported voxel terrain may still be building
/// ([`SpaceTerrainLoad`]).
#[derive(Message, Debug, Clone)]
pub struct SpaceOpened {
    pub root: PathBuf,
    pub parts: usize,
}

/// Whether the open Space's terrain is in the world yet. A disk terrain is
/// spawned while the Space opens; imported voxel terrain builds in the
/// background, and a character placed before it lands would fall through.
#[derive(Resource, Debug, Default)]
pub struct SpaceTerrainLoad {
    /// The Space root a background terrain build is running for.
    pending_for: Option<PathBuf>,
}

impl SpaceTerrainLoad {
    /// No terrain is still on its way for the open Space: none was needed, it
    /// failed, or its root has spawned.
    pub fn ready(&self) -> bool {
        self.pending_for.is_none()
    }
}

/// The imported voxel terrain building in the background, at most one.
#[derive(Resource, Default)]
struct VoxelTerrainBuild {
    in_flight: Option<InFlightVoxelBuild>,
}

/// A voxel terrain build running on the `AsyncComputeTaskPool`.
struct InFlightVoxelBuild {
    /// The Space root it was started for.
    space: PathBuf,
    /// Resolves to the built terrain, or to why there is none.
    task: Task<Result<BuiltVoxelTerrain, String>>,
}

/// Where an opening Space's parts come from.
enum PartSource<'a> {
    /// Spawned straight from its files: a Space on this computer.
    Files,
    /// Its DataModel tree, which replication writes into and the shared Play
    /// runtime draws: a world that arrived from a host or as a published
    /// simulation.
    Tree { player_name: &'a str },
}

/// What opening a Space needs to replace the world's terrain with its own.
#[derive(SystemParam)]
struct SpaceTerrainAccess<'w, 's> {
    roots: Query<'w, 's, Entity, With<TerrainRoot>>,
    material_source: Option<ResMut<'w, TerrainMaterialSource>>,
    build: ResMut<'w, VoxelTerrainBuild>,
    load: ResMut<'w, SpaceTerrainLoad>,
}

pub struct SpaceWorldPlugin {
    /// Open the local Space (`EUSTRESS_SPACE`, else the movement course) at
    /// startup. Off when the Player was launched to join a host or play a
    /// published simulation: that world arrives later, through
    /// [`OpenSpaceRequest`].
    pub open_local: bool,
}

impl Plugin for SpaceWorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenSpace>()
            .init_resource::<SpaceTerrainLoad>()
            .init_resource::<VoxelTerrainBuild>()
            .add_message::<OpenSpaceRequest>()
            .add_message::<SpaceOpened>()
            // Ahead of the material slot reload, so a Space's slots load in
            // the frame it opens, and of the terrain streaming chain, so a
            // root despawned or spawned here is settled before it streams
            // chunks, and no chunk is spawned under a root despawned the same
            // frame.
            .add_systems(
                Update,
                (open_requested_space, poll_space_voxel_terrain)
                    .chain()
                    .before(reload_terrain_material_slots)
                    .before(process_terrain_generation_queue),
            )
            .add_systems(Update, register_opened_space_materials);
        if self.open_local {
            app.add_systems(Startup, open_local_space);
        }
    }
}

/// A tree's Space materials (`MaterialService/*.mat.toml`) into the shared
/// registry, as Studio's loader fills it, each time a tree opens.
fn register_opened_space_materials(
    root: Option<Res<PlayAssetRoot>>,
    registry: Option<ResMut<MaterialRegistry>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    let (Some(root), Some(mut registry)) = (root, registry) else { return };
    if !root.is_changed() {
        return;
    }
    *registry = MaterialRegistry::default();
    let (count, failed) = register_space_materials(&root.0, &mut registry, &mut materials, &asset_server);
    info!("space: {count} material(s) from {}", root.0.display());
    for f in failed {
        warn!("space: material {f}");
    }
}

pub(crate) fn workspace_root() -> Option<PathBuf> {
    if let Some(w) = std::env::var_os("EUSTRESS_WORKSPACE") {
        return Some(PathBuf::from(w));
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join("Documents").join("Eustress"))
}

/// True when a real Space will be opened, so the shell can skip its built-in
/// demo scene. Without this the hardcoded 512 m baseplate and welcome cube
/// spawn UNDERNEATH the authored course — overlapping ground planes and a
/// cube sitting at the origin of the level.
pub fn space_is_available() -> bool {
    resolve_space().is_some()
}

pub(crate) fn resolve_space() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EUSTRESS_SPACE") {
        let p = PathBuf::from(p);
        return p.join("Workspace").is_dir().then_some(p);
    }
    let candidate = workspace_root()?
        .join("Movement")
        .join("Spaces")
        .join("Climbing");
    candidate.join("Workspace").is_dir().then_some(candidate)
}

fn open_local_space(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut open: ResMut<OpenSpace>,
    mut terrain: SpaceTerrainAccess,
    mut opened: MessageWriter<SpaceOpened>,
) {
    let Some(root) = resolve_space() else {
        info!("space: none found — keeping the built-in demo scene");
        return;
    };
    let parts = open_space_at(
        &root,
        PartSource::Files,
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut open,
        &mut terrain,
    );
    if let Some(parts) = parts {
        opened.write(SpaceOpened { root, parts });
    }
}

/// Open each requested Space, replacing whatever Space parts and terrain are
/// already in the world. A Player launched to join a host or play a published
/// simulation has a world folder ([`super::live_world::LiveWorld`]), and every
/// world it opens goes through the tree.
#[allow(clippy::too_many_arguments)]
fn open_requested_space(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut open: ResMut<OpenSpace>,
    mut terrain: SpaceTerrainAccess,
    mut requests: MessageReader<OpenSpaceRequest>,
    mut opened: MessageWriter<SpaceOpened>,
    existing: Query<Entity, Or<(With<SpawnedFromSpace>, With<FromTree>)>>,
    old_layers: Query<Entity, TerrainLayerFilter>,
    live: Option<Res<super::live_world::LiveWorld>>,
    join: Option<Res<super::net_play::JoinTarget>>,
) {
    let Some(request) = requests.read().last().cloned() else { return };
    for e in &existing {
        commands.entity(e).try_despawn();
    }
    // The previous Space's terrain layers, scatter and water bodies go too,
    // or they would bake onto the next Space's terrain.
    for e in &old_layers {
        commands.entity(e).try_despawn();
    }
    let source = match live {
        Some(_) => PartSource::Tree { player_name: join.as_deref().map_or("Player", |j| j.name.as_str()) },
        None => PartSource::Files,
    };
    let parts =
        open_space_at(&request.root, source, &mut commands, &mut meshes, &mut materials, &mut open, &mut terrain);
    if let Some(parts) = parts {
        opened.write(SpaceOpened { root: request.root, parts });
    }
}

/// Read a Space folder and spawn its parts, terrain layers and terrain.
/// Returns the number of parts, or `None` when the Space could not be read.
fn open_space_at(
    root: &Path,
    source: PartSource,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    open: &mut OpenSpace,
    terrain: &mut SpaceTerrainAccess<'_, '_>,
) -> Option<usize> {
    let root = root.to_path_buf();
    let from_tree = matches!(source, PartSource::Tree { .. });
    // The Space's character policy (its replacement bodies and StarterPlayer
    // switches), in place before the avatar spawns; it replaces the last
    // Space's, so each world starts from its own.
    let (policy, policy_warnings) = eustress_common::avatar::space_character::SpaceCharacterPolicy::load(&root);
    for w in &policy_warnings {
        warn!("avatar: {w}");
    }
    commands.insert_resource(policy);
    // The Space's gravity, by Studio's rule. Workspace.gravity is its one
    // owner; `sync_workspace_gravity_to_avian` carries it into physics.
    let gravity = eustress_common::play_session::space_gravity(&root);
    commands.queue(move |world: &mut World| {
        world.get_resource_or_insert_with(eustress_common::services::workspace::Workspace::default).gravity = gravity;
    });
    let (spawned, skipped, spawn, errors) = match source {
        PartSource::Files => {
            let geo = match read_space_parts(&root) {
                Ok(g) => g,
                Err(e) => {
                    // Loud, not silent: a Space that fails to open is the
                    // difference between testing the course and testing an
                    // empty plane.
                    error!("space: {} failed to open — {e}", root.display());
                    return None;
                }
            };
            for err in &geo.errors {
                warn!("space: {err}");
            }
            if !geo.skipped.is_empty() {
                // Reported by class so "the course looks wrong" has an answer
                // that is not "read the loader".
                warn!("space: skipped non-Part content: {:?}", geo.skipped);
            }
            let boxed = geo.custom_mesh_parts();
            if boxed > 0 {
                warn!("space: {boxed} parts use custom meshes, drawn as blocks at their size");
            }
            let spawned = spawn_space_parts(commands, meshes, materials, &geo);
            (spawned, geo.skipped_total(), geo.spawn_point(), geo.errors.len())
        }
        PartSource::Tree { player_name } => {
            let scene = match eustress_common::tree_read::read_space_dir(&root) {
                Ok(scene) => scene,
                Err(e) => {
                    error!("space: {} failed to open — {e}", root.display());
                    return None;
                }
            };
            info!("space: {} read into its tree: {}", root.display(), scene.report.summary());
            for problem in &scene.report.problems {
                warn!("space: {problem}");
            }
            let parts = tree_parts(&scene.dm);
            // The session, set up as Studio's Play sets up its own: the local
            // Player (which replication maps the host's Player for this peer
            // onto), the Space's camera, gravity and CharacterAutoLoads.
            let mut dm = scene.dm;
            let me = match dm.local_player.filter(|p| dm.exists(*p)) {
                Some(me) => me,
                None => {
                    let players = dm.get_service("Players");
                    dm.create_virtual("Player", player_name, players)
                }
            };
            let start = SessionStart {
                camera: SessionCamera::SpaceOrFresh,
                gravity,
                character_auto_loads: space_character_auto_loads(&root),
            };
            begin_session(&mut dm, me, &start);
            let spawn = eustress_common::space_read::tree_spawn_point(&dm);
            let shared = eustress_common::datamodel::new_shared();
            *shared.lock() = dm;
            // Replication binds the tree by these keys; the draw step draws it
            // from the next frame, its MeshIds relative to this folder.
            commands.insert_resource(SceneKeys(scene.keys));
            commands.insert_resource(PlayDataModel { dm: shared });
            commands.insert_resource(PlayAssetRoot(root.clone()));
            (parts, 0, spawn, scene.report.problems.len())
        }
    };

    // PointLight / SpotLight / SurfaceLight / DirectionalLight instances,
    // placed as Studio places them; the shared `light_classes` plugin lights
    // them exactly as Studio does. They clear with the parts. A tree's draw
    // step spawns its Point, Spot and Surface lights itself, so from files a
    // tree takes only its Directional lights.
    let (mut lights, light_problems) = eustress_common::plugins::light_classes::read_space_lights(&root);
    if from_tree {
        lights.retain(|l| matches!(l.component, eustress_common::plugins::light_classes::LightComponent::Directional(_)));
    }
    for problem in &light_problems {
        warn!("space: light {problem}");
    }
    let light_count = eustress_common::plugins::light_classes::spawn_space_lights(commands, &lights);
    if light_count > 0 {
        info!("space: {light_count} lights");
    }

    // The Space's Lighting service (its clock, latitude, brightness, ambient
    // terms and fog) and its sky objects (the Sun, Moon, Sky, Atmosphere and
    // Clouds), read and lit by the code Studio uses, so the Player shows the
    // sky Studio shows. The tree and the files hold the same values at open;
    // later script and replicated writes reach Lighting through
    // play-runtime's `apply_lighting_prop`. With no Clouds object the sky has
    // no clouds (Roblox's rule).
    let lighting = match eustress_common::services::lighting_properties::read_space_lighting(&root) {
        Ok(found) => found.unwrap_or_default(),
        Err(e) => {
            warn!("space: lighting {e}");
            eustress_common::services::lighting::LightingService::default()
        }
    };
    let (sky_objects, sky_problems) = eustress_common::plugins::celestial_sections::read_space_celestials(&root);
    for problem in &sky_problems {
        warn!("space: sky {problem}");
    }
    let sky_count =
        eustress_common::plugins::celestial_sections::spawn_space_celestials(commands, &sky_objects, &lighting);
    if sky_count > 0 {
        info!("space: {sky_count} sky object(s), Lighting at {}", lighting.clock_time);
    }
    commands.insert_resource(lighting);

    // Terrain layers (roads, stamps, pads, noise, material fills), scatter
    // layers and water bodies are instances too; the shared terrain plugin
    // bakes the layers over whatever terrain the Space spawns, and places
    // the scatter and floods the water bodies on the result.
    let (layers, layer_problems) = read_layer_instances(&root);
    for problem in &layer_problems {
        warn!("space: terrain layer {problem}");
    }
    let layer_entities = spawn_layer_instances(commands, &layers);
    if layer_entities > 0 {
        info!("space: {layer_entities} terrain layer instances");
    }

    open_space_terrain(&root, commands, meshes, materials, terrain);

    open.root = Some(root.clone());
    open.parts = spawned;
    open.skipped = skipped;
    open.spawn = spawn;

    info!("space: opened {} — {spawned} parts ({skipped} skipped, {errors} errors)", root.display());
    Some(spawned)
}

/// The parts a tree draws: its BaseParts under Workspace.
fn tree_parts(dm: &DataModel) -> usize {
    let Some(ws) = dm.find_service("Workspace") else { return 0 };
    dm.descendants(ws)
        .into_iter()
        .filter(|&id| dm.class_of(id).is_some_and(|c| is_base_part(c) && c != "Terrain"))
        .count()
}

/// Replace the world's terrain with the terrain of the Space at `root` (see
/// "Terrain" in the module docs). Logs what it loads, and nothing for a Space
/// without terrain.
fn open_space_terrain(
    root: &Path,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    terrain: &mut SpaceTerrainAccess<'_, '_>,
) {
    // The previous Space's terrain goes, and so does a build still running
    // for it: dropping its task drops its result.
    for entity in terrain.roots.iter() {
        commands.entity(entity).try_despawn();
    }
    terrain.build.in_flight = None;
    terrain.load.pending_for = None;

    let terrain_dir = root.join("Workspace").join("Terrain");
    // Compared through `Deref`, since taking the source mutably reloads it.
    if let Some(source) = terrain.material_source.as_mut() {
        if source.terrain_dir() != Some(terrain_dir.as_path()) {
            source.set_terrain_dir(Some(terrain_dir.clone()));
        }
    }

    if terrain_dir.join("_terrain.toml").exists() {
        match hydrate_terrain_from_disk(&terrain_dir) {
            Ok(hydrated) => {
                let (chunk_files, volume_bricks) = (hydrated.chunk_files, hydrated.volume.brick_count());
                hydrated.spawn(commands, meshes, materials);
                info!("space: terrain from Workspace/Terrain: {chunk_files} chunk heightmaps, {volume_bricks} volume bricks");
            }
            Err(e) => warn!("space: Workspace/Terrain/_terrain.toml is there but unloadable, so no terrain: {e}"),
        }
        return;
    }

    if !voxel_import::has_voxel_chunk_files(&terrain_dir) {
        return;
    }
    let instance = match voxel_import::read_terrain_instance_file(&terrain_dir) {
        Ok(instance) => instance.unwrap_or_default(),
        Err(e) => {
            warn!("space: the Terrain instance could not be read, so the voxel terrain loads without a water style: {e}");
            Default::default()
        }
    };
    if instance.cleared {
        info!("space: the Terrain instance's source is \"none\"; its voxel chunks are not loaded");
        return;
    }

    // The chunk files are read inside the task too, so the main thread only
    // listed the folder.
    let task = AsyncComputeTaskPool::get().spawn(async move {
        // Caught here: a panic would close the task, and polling a closed
        // task panics the main thread.
        std::panic::catch_unwind(AssertUnwindSafe(move || -> Result<BuiltVoxelTerrain, String> {
            let started = Instant::now();
            let chunks = voxel_import::read_voxel_chunk_files(&terrain_dir)?;
            let mut built = voxel_import::build_from_chunks(chunks, instance, None)?;
            built.stats.elapsed = started.elapsed();
            Ok(built)
        }))
        .unwrap_or_else(|panic| Err(format!("the build panicked: {}", panic_text(&*panic))))
    });
    terrain.build.in_flight = Some(InFlightVoxelBuild { space: root.to_path_buf(), task });
    terrain.load.pending_for = Some(root.to_path_buf());
    info!("space: building the imported voxel terrain from Workspace/Terrain/voxel_chunks");
}

/// Poll the voxel terrain build without waiting on it, and when it lands
/// spawn its root in place of any other, unless the Space it was built for is
/// no longer the open one.
fn poll_space_voxel_terrain(
    mut commands: Commands,
    open: Res<OpenSpace>,
    mut build: ResMut<VoxelTerrainBuild>,
    mut load: ResMut<SpaceTerrainLoad>,
    roots: Query<Entity, With<TerrainRoot>>,
    throttle: Option<ResMut<ChunkSpawnThrottle>>,
) {
    if build.in_flight.is_none() {
        return;
    }
    let Some(in_flight) = build.in_flight.as_mut() else { return };
    let Some(result) = block_on(poll_once(&mut in_flight.task)) else { return };
    let Some(in_flight) = build.in_flight.take() else { return };
    let space = in_flight.space;
    if load.pending_for.as_deref() == Some(space.as_path()) {
        load.pending_for = None;
    }

    if open.root.as_deref() != Some(space.as_path()) {
        debug!("space: the voxel terrain of {} finished building after another Space opened; dropped", space.display());
        return;
    }
    let built = match result {
        Ok(built) => built,
        Err(e) => {
            warn!("space: the imported voxel terrain of {} did not build, so no terrain: {e}", space.display());
            return;
        }
    };

    // Despawned and spawned in one command flush, so no system sees two
    // roots: the streaming systems drive exactly one.
    for root in roots.iter() {
        commands.entity(root).try_despawn();
    }
    let BuiltVoxelTerrain { config, data, volume, water, window, stats } = built;
    // The components the engine's voxel loader spawns, `sparse_surface` as
    // the build set it; `chunk_spawn_system` meshes the root as the camera
    // moves.
    let mut root = commands.spawn((
        TerrainRoot,
        config,
        data,
        volume,
        Transform::default(),
        Visibility::default(),
        Name::new("Terrain (imported voxels)"),
    ));
    if let Some(water) = water {
        root.insert(water);
    }
    // The spawn scan looks for the new root's chunks on its next pass rather
    // than after its interval.
    if let Some(mut throttle) = throttle {
        throttle.backlog = true;
    }
    info!(
        "space: imported voxel terrain built from {} chunk files: {} filled, {} failed to decode, {} outside the \
         load window (centre {}, half {}, columns {} to {}); {} m cells ({}), {} water cells, {} cave bricks, {} ms",
        stats.chunks_read,
        stats.chunks_filled,
        stats.decode_errors,
        stats.skipped_off_grid,
        window.center,
        window.half,
        window.columns_min,
        window.columns_max,
        stats.cell_size,
        stats.unit.symbol(),
        stats.water_cells,
        stats.caves.bricks,
        stats.elapsed.as_millis()
    );
}

/// The message a panic carried, as far as it had one.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "no message".to_string()
    }
}
