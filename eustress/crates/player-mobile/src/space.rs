//! Open a downloaded Space the way the desktop Player does.
//!
//! The records are written with [`eustress_networking::published::materialize_world`]
//! into this process's live folder (`space://` was registered there before
//! AssetPlugin froze the source table). The tree is
//! [`eustress_common::tree_read::open_player_world_dir`]. [`PlayRuntimePlugin`]
//! draws it. The gallery's stand-in floor and cube are removed once that
//! succeeds, so the downloaded Space is what the player sees.

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};
use eustress_common::datamodel::{is_base_part, DataModel};
use eustress_common::play_session::{
    space_character_auto_loads, space_gravity, PlayAssetRoot, PlayDataModel, PlayScriptSet, ScriptedPlayCamera,
};
use eustress_common::plugins::light_classes::LightComponent;
use eustress_common::terrain::disk::hydrate_terrain_from_disk;
use eustress_common::terrain::layer_instances::{read_layer_instances, spawn_layer_instances};
use eustress_common::terrain::voxel_import::{self, BuiltVoxelTerrain};
use eustress_common::terrain::{
    ChunkSpawnThrottle, TerrainFlattenPad, TerrainMaterialFill, TerrainMaterialSource, TerrainNoise, TerrainRoot,
    TerrainScatter, TerrainSpline, TerrainStamp, TerrainWaterBody,
};
use eustress_networking::player_replica::SceneKeys;
use eustress_networking::session::{EndSession, LocalWorldReady, WorldDownloaded};
use eustress_play_runtime::material_registry::{register_space_materials, MaterialRegistry};

use crate::session::{self, MobileApp};
use crate::{GalleryCamera, StandInYard};

/// Frames after the files are open before the avatar spawns, matching the
/// desktop Player (`net_play.rs` `SETTLE_FRAMES`), so part colliders exist.
const SETTLE_FRAMES: u8 = 3;

/// `<files>/eustress/live/<pid>/Universe/Spaces/Current`.
pub fn live_space_root() -> PathBuf {
    session::data_root()
        .join("live")
        .join(std::process::id().to_string())
        .join("Universe")
        .join("Spaces")
        .join("Current")
}

fn live_universe_root() -> PathBuf {
    live_space_root()
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(live_space_root)
}

pub struct SpaceOpenPlugin;

impl Plugin for SpaceOpenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<eustress_common::services::workspace::Workspace>()
            .init_resource::<TerrainMaterialSource>()
            .init_resource::<VoxelBuild>()
            // LightClassSyncPlugin orders itself after this set. The sky
            // plugin that normally configures it is not on mobile.
            .configure_sets(Update, eustress_common::plugins::sky_atmosphere::SkyLightSet)
            .add_systems(Update, eustress_common::services::workspace::sync_workspace_gravity_to_avian)
            .add_systems(
                Update,
                (
                    open_downloaded
                        .run_if(resource_exists::<Assets<Mesh>>)
                        .run_if(resource_exists::<Assets<StandardMaterial>>),
                    register_opened_materials.run_if(resource_exists::<Assets<StandardMaterial>>),
                    poll_voxel,
                    settle_avatar,
                    log_opened_view,
                )
                    .chain()
                    .after(session::drive),
            )
            // After the play camera copies the Space's CFrame, so a fallback
            // pose is replaced and a real one is left alone.
            .add_systems(Update, frame_spawn_if_fallback.after(PlayScriptSet::Apply));
    }
}

#[derive(Resource, Default)]
struct VoxelBuild {
    in_flight: Option<InFlightVoxel>,
    pending_for: Option<PathBuf>,
}

struct InFlightVoxel {
    space: PathBuf,
    task: Task<Result<BuiltVoxelTerrain, String>>,
}

/// Avatar spawn waiting on the open Space's colliders (and voxel terrain).
#[derive(Resource)]
struct PendingSpawn {
    at: Vec3,
    frames: u8,
    network: bool,
    auto_loads: bool,
}

fn open_downloaded(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut arrivals: MessageReader<WorldDownloaded>,
    mut app: ResMut<MobileApp>,
    mut terrain_source: ResMut<TerrainMaterialSource>,
    mut voxels: ResMut<VoxelBuild>,
    existing: Query<Entity, Or<(With<eustress_common::play_session::FromTree>, With<eustress_common::space_read::SpawnedFromSpace>, With<StandInYard>)>>,
    layers: Query<
        Entity,
        Or<(
            With<TerrainSpline>,
            With<TerrainStamp>,
            With<TerrainFlattenPad>,
            With<TerrainNoise>,
            With<TerrainMaterialFill>,
            With<TerrainScatter>,
            With<TerrainWaterBody>,
            With<TerrainRoot>,
        )>,
    >,
    mut ends: MessageWriter<EndSession>,
    mut despawn_avatars: MessageWriter<eustress_common::avatar::DespawnAllAvatars>,
) {
    let world = arrivals.read().last().map(|WorldDownloaded(world)| std::sync::Arc::clone(world));
    let Some(world) = world else { return };

    let space_root = live_space_root();
    let universe_root = live_universe_root();
    let network = app.joined;
    match eustress_networking::published::materialize_world(&world, &universe_root, &space_root) {
        Ok(files) => {
            info!("space: wrote {files} files for {} into {}", world.start_space, space_root.display());
        }
        Err(err) => {
            app.status = format!("Could not open the Space: {err}");
            if network {
                ends.write(EndSession { reason: err });
            }
            return;
        }
    }

    let opened = match eustress_common::tree_read::open_player_world_dir(&space_root, &app.player_name) {
        Ok(opened) => opened,
        Err(err) => {
            app.status = format!("Could not read the Space: {err}");
            if network {
                ends.write(EndSession { reason: err });
            }
            return;
        }
    };
    info!("space: {} — {}", space_root.display(), opened.report.summary());
    for problem in &opened.report.problems {
        warn!("space: {problem}");
    }

    for entity in existing.iter().chain(layers.iter()) {
        commands.entity(entity).try_despawn();
    }
    despawn_avatars.write(eustress_common::avatar::DespawnAllAvatars);
    app.spawned = false;

    let (policy, policy_warnings) = eustress_common::avatar::space_character::SpaceCharacterPolicy::load(&space_root);
    for warning in &policy_warnings {
        warn!("avatar: {warning}");
    }
    commands.insert_resource(policy);
    let gravity = space_gravity(&space_root);
    commands.queue(move |world: &mut World| {
        world
            .get_resource_or_insert_with(eustress_common::services::workspace::Workspace::default)
            .gravity = gravity;
    });

    let parts = tree_parts(&opened.dm);
    let spawn = opened.spawn;
    let camera_at = opened.dm.current_camera().and_then(|id| {
        opened.dm.get(id).and_then(|inst| inst.cframe()).map(|cf| cf.to_transform().translation)
    });
    let shared = eustress_common::datamodel::new_shared();
    *shared.lock() = opened.dm;
    commands.insert_resource(SceneKeys(opened.keys));
    commands.insert_resource(PlayDataModel { dm: shared });
    commands.insert_resource(PlayAssetRoot(space_root.clone()));

    let (mut lights, light_problems) = eustress_common::plugins::light_classes::read_space_lights(&space_root);
    lights.retain(|light| matches!(light.component, LightComponent::Directional(_)));
    for problem in &light_problems {
        warn!("space: light {problem}");
    }
    let light_count = eustress_common::plugins::light_classes::spawn_space_lights(&mut commands, &lights);
    if light_count == 0 {
        commands.spawn((
            DirectionalLight {
                illuminance: 10_000.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(4.0, 12.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
            Name::new("Space fill light"),
        ));
    }

    let lighting = match eustress_common::services::lighting_properties::read_space_lighting(&space_root) {
        Ok(found) => found.unwrap_or_default(),
        Err(err) => {
            warn!("space: lighting {err}");
            eustress_common::services::lighting::LightingService::default()
        }
    };
    let (sky, sky_problems) = eustress_common::plugins::celestial_sections::read_space_celestials(&space_root);
    for problem in &sky_problems {
        warn!("space: sky {problem}");
    }
    eustress_common::plugins::celestial_sections::spawn_space_celestials(&mut commands, &sky, &lighting);
    commands.insert_resource(lighting);

    let (layers, layer_problems) = read_layer_instances(&space_root);
    for problem in &layer_problems {
        warn!("space: terrain layer {problem}");
    }
    spawn_layer_instances(&mut commands, &layers);

    let terrain_dir = space_root.join("Workspace").join("Terrain");
    if terrain_source.terrain_dir() != Some(terrain_dir.as_path()) {
        terrain_source.set_terrain_dir(Some(terrain_dir.clone()));
    }
    open_terrain(&space_root, &mut commands, &mut meshes, &mut materials, &mut voxels);

    let at = if network {
        Vec3::from_array(world.spawn)
    } else {
        spawn.unwrap_or_else(|| Vec3::from_array(world.spawn))
    };
    let auto_loads = space_character_auto_loads(&space_root);
    // The gallery yard (Bevy cuboids) is gone. Parts draw `parts/*.glb` from
    // the default asset source, which on Android is the APK. The gallery
    // directional light is not a yard entity, so it stays. The gallery camera
    // stays too; `gate_gallery_camera` turns it off once the play camera is
    // active. A published Space has no host, so this open does not join.
    eprintln!(
        "phase0: opened {} parts={parts} spawn=({:.1}, {:.1}, {:.1}) space_camera={} network={network} auto_loads={auto_loads} fill_lights={light_count}",
        world.start_space,
        at.x,
        at.y,
        at.z,
        camera_at
            .map(|p| format!("({:.1}, {:.1}, {:.1})", p.x, p.y, p.z))
            .unwrap_or_else(|| "none".into()),
    );
    commands.insert_resource(PendingSpawn {
        at,
        frames: 0,
        network,
        auto_loads,
    });
    app.status = format!("Opened {} — {parts} parts", world.start_space);
}

fn register_opened_materials(
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
    for failure in failed {
        warn!("space: material {failure}");
    }
}

fn tree_parts(dm: &DataModel) -> usize {
    let Some(workspace) = dm.find_service("Workspace") else { return 0 };
    dm.descendants(workspace)
        .into_iter()
        .filter(|&id| dm.class_of(id).is_some_and(|class| is_base_part(class) && class != "Terrain"))
        .count()
}

fn open_terrain(
    root: &Path,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    voxels: &mut VoxelBuild,
) {
    voxels.in_flight = None;
    voxels.pending_for = None;
    let terrain_dir = root.join("Workspace").join("Terrain");
    if terrain_dir.join("_terrain.toml").exists() {
        match hydrate_terrain_from_disk(&terrain_dir) {
            Ok(hydrated) => {
                let chunks = hydrated.chunk_files;
                hydrated.spawn(commands, meshes, materials);
                info!("space: terrain from Workspace/Terrain ({chunks} chunk heightmaps)");
            }
            Err(err) => warn!("space: Workspace/Terrain/_terrain.toml did not load: {err}"),
        }
        return;
    }
    if !voxel_import::has_voxel_chunk_files(&terrain_dir) {
        return;
    }
    let instance = match voxel_import::read_terrain_instance_file(&terrain_dir) {
        Ok(instance) => instance.unwrap_or_default(),
        Err(err) => {
            warn!("space: the Terrain instance could not be read: {err}");
            Default::default()
        }
    };
    if instance.cleared {
        info!("space: the Terrain instance's source is none; voxel chunks are not loaded");
        return;
    }
    let task = AsyncComputeTaskPool::get().spawn(async move {
        std::panic::catch_unwind(AssertUnwindSafe(move || -> Result<BuiltVoxelTerrain, String> {
            let started = std::time::Instant::now();
            let chunks = voxel_import::read_voxel_chunk_files(&terrain_dir)?;
            let mut built = voxel_import::build_from_chunks(chunks, instance, None)?;
            built.stats.elapsed = started.elapsed();
            Ok(built)
        }))
        .unwrap_or_else(|_| Err("the voxel terrain build panicked".into()))
    });
    voxels.pending_for = Some(root.to_path_buf());
    voxels.in_flight = Some(InFlightVoxel { space: root.to_path_buf(), task });
    info!("space: building imported voxel terrain");
}

fn poll_voxel(
    mut commands: Commands,
    mut voxels: ResMut<VoxelBuild>,
    roots: Query<Entity, With<TerrainRoot>>,
    throttle: Option<ResMut<ChunkSpawnThrottle>>,
) {
    if voxels.in_flight.is_none() {
        return;
    }
    let Some(in_flight) = voxels.in_flight.as_mut() else { return };
    let Some(result) = block_on(poll_once(&mut in_flight.task)) else { return };
    let Some(in_flight) = voxels.in_flight.take() else { return };
    if voxels.pending_for.as_deref() == Some(in_flight.space.as_path()) {
        voxels.pending_for = None;
    }
    let built = match result {
        Ok(built) => built,
        Err(err) => {
            warn!("space: imported voxel terrain did not build: {err}");
            return;
        }
    };
    for root in roots.iter() {
        commands.entity(root).try_despawn();
    }
    let BuiltVoxelTerrain { config, data, volume, water, stats, .. } = built;
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
    if let Some(mut throttle) = throttle {
        throttle.backlog = true;
    }
    info!(
        "space: imported voxel terrain, {} chunk files, {} filled",
        stats.chunks_read, stats.chunks_filled
    );
}

fn settle_avatar(
    mut commands: Commands,
    pending: Option<ResMut<PendingSpawn>>,
    voxels: Res<VoxelBuild>,
    mut app: ResMut<MobileApp>,
    mut spawn: MessageWriter<eustress_common::avatar::SpawnAvatar>,
    mut ready: MessageWriter<LocalWorldReady>,
) {
    let Some(mut pending) = pending else { return };
    pending.frames = pending.frames.saturating_add(1);
    let terrain_waiting = voxels.pending_for.is_some() && pending.frames < 180;
    if pending.frames < SETTLE_FRAMES || terrain_waiting {
        return;
    }
    if pending.auto_loads && !app.spawned {
        app.spawned = true;
        let body = eustress_common::avatar::AvatarDescriptor::default().resolved_rig().body_asset;
        let file = body.strip_prefix("bundled://").and_then(|rel| {
            std::env::var_os("EUSTRESS_BUNDLED_ASSETS").map(|root| std::path::PathBuf::from(root).join(rel))
        });
        let exists = file.as_ref().is_some_and(|path| path.is_file());
        eprintln!(
            "phase0: avatar spawn at ({:.1}, {:.1}, {:.1}) body={body} file={} exists={exists}",
            pending.at.x,
            pending.at.y,
            pending.at.z,
            file.as_ref().map(|path| path.display().to_string()).unwrap_or_else(|| "unset".into()),
        );
        spawn.write(eustress_common::avatar::SpawnAvatar::new(
            eustress_common::avatar::AvatarDescriptor::default(),
            pending.at,
        ));
    } else if !pending.auto_loads {
        eprintln!("phase0: Players.CharacterAutoLoads is off, so no avatar is spawned");
        info!("space: Players.CharacterAutoLoads is off, so no avatar is spawned");
        app.spawned = true;
    }
    // Tells a host this player is in the world. A published Space played
    // solo has no session reading it.
    let _ = pending.network;
    ready.write(LocalWorldReady);
    commands.remove_resource::<PendingSpawn>();
}

/// One line for the next logcat: where the cameras are, and whether any
/// part mesh exists yet. `tracing` on Android is easy to miss; this is
/// `eprintln` like the rest of the phase0 lines.
fn log_opened_view(
    pending: Option<Res<PendingSpawn>>,
    cameras: Query<
        (&Transform, &Camera, Option<&Name>),
        Or<(
            With<GalleryCamera>,
            With<ScriptedPlayCamera>,
            With<eustress_common::avatar::control::AvatarCamera>,
        )>,
    >,
    lights: Query<&DirectionalLight>,
    parts: Query<(), (With<Mesh3d>, With<eustress_common::play_session::FromTree>)>,
    mut logged: Local<bool>,
) {
    let Some(pending) = pending else {
        *logged = false;
        return;
    };
    if *logged || pending.frames < 2 {
        return;
    }
    *logged = true;
    eprintln!(
        "phase0: view spawn=({:.1}, {:.1}, {:.1}) directional_lights={} part_meshes={}",
        pending.at.x,
        pending.at.y,
        pending.at.z,
        lights.iter().count(),
        parts.iter().count(),
    );
    for (tf, camera, name) in &cameras {
        let name = name.map(|n| n.as_str()).unwrap_or("camera");
        eprintln!(
            "phase0: camera {name} active={} order={} at=({:.1}, {:.1}, {:.1})",
            camera.is_active,
            camera.order,
            tf.translation.x,
            tf.translation.y,
            tf.translation.z
        );
    }
}

/// The replica play camera starts at `(0, 12, 24)` looking at the origin when
/// the Space has no camera CFrame. A map whose spawn is elsewhere is then
/// outside the view, and the clear color is all that shows. A saved camera
/// is not this pose, so it is left where the Space put it.
fn frame_spawn_if_fallback(
    pending: Option<Res<PendingSpawn>>,
    mut scripted: Query<&mut Transform, (With<ScriptedPlayCamera>, Without<GalleryCamera>)>,
    mut gallery: Query<&mut Transform, (With<GalleryCamera>, Without<ScriptedPlayCamera>)>,
) {
    let Some(pending) = pending else { return };
    let spawn = pending.at;
    if spawn.distance(Vec3::ZERO) < 8.0 {
        return;
    }
    let eye = spawn + Vec3::new(0.0, 8.0, 16.0);
    let view = Transform::from_translation(eye).looking_at(spawn + Vec3::Y, Vec3::Y);
    let fallback = Vec3::new(0.0, 12.0, 24.0);
    for mut tf in &mut scripted {
        if tf.translation.distance(fallback) > 1.5 {
            continue;
        }
        *tf = view;
        eprintln!(
            "phase0: play camera was the fallback (0, 12, 24); now looks at the spawn ({:.1}, {:.1}, {:.1})",
            spawn.x, spawn.y, spawn.z
        );
    }
    // The yard camera looks at the origin. The yard is gone on this open.
    let yard = Vec3::new(0.0, 1.15, 6.2);
    for mut tf in &mut gallery {
        if tf.translation.distance(yard) > 1.5 {
            continue;
        }
        *tf = view;
    }
}
