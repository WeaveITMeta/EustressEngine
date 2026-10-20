//! # Opening the world that arrives
//!
//! The browser's side of what the desktop Player does through its world
//! folder (client `net_play.rs` `open_arrived_world`, and the tree branch of
//! `space_world.rs`), from the world's records, with no disk:
//! - the Space's tree, opened as every Player opens it
//!   ([`open_player_world`]: the local Player, then the session begun on the
//!   Space's camera, gravity and `Players.CharacterAutoLoads`);
//! - the tree, its record keys ([`SceneKeys`]) for replication to bind, its
//!   files for the Play runtime ([`PlaySpaceSource`] over a
//!   [`RecordsSource`]) and for `space://` (the in-memory asset source);
//! - the Space's Lighting service, its sky objects (Sun, Moon, Sky,
//!   Atmosphere, Clouds) and its Directional lights, read from the same
//!   records by the readers the desktop runs on its files (the tree's Point,
//!   Spot and Surface lights draw through the Play runtime);
//! - [`LocalWorldReady`] a few frames later, once the world has drawn, as on
//!   the desktop; the page then hears `in-world`.
//!
//! Not here yet: the Space's terrain and the Universe's shared `assets/`,
//! each waiting on a records-reading twin of its desktop reader. The local
//! character is: when `Players.CharacterAutoLoads` is on, its feet go on the
//! Space's SpawnLocation and the avatar runtime follows it. A joined session
//! uses the place the host sent. A Space that loads no character keeps the
//! fly camera.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::asset::io::memory::{Dir, Value};
use bevy::prelude::*;
use eustress_common::avatar::{AvatarDescriptor, SpawnAvatar};
use eustress_common::avatar::space_character::SpaceCharacterPolicy;
use eustress_common::play_session::{PlayAssetRoot, PlayDataModel, PlaySpaceSource};
use eustress_common::plugins::celestial_sections::{
    read_space_celestials_from, spawn_space_celestials,
};
use eustress_common::plugins::light_classes::{
    read_space_lights_from, spawn_space_lights, LightComponent,
};
use eustress_common::services::lighting::LightingService;
use eustress_common::services::lighting_properties::read_space_lighting_from;
use eustress_common::space_source::RecordsSource;
use eustress_common::tree_read::open_player_world;
use eustress_networking::player_replica::SceneKeys;
use eustress_networking::session::{EndSession, LocalWorldReady, NetNotice, WorldDownloaded};
use eustress_play_runtime::material_registry::{register_space_materials_from, MaterialRegistry};

use crate::page::PageState;
use crate::states;

/// Frames between the world opening and the session hearing it is ready,
/// as on the desktop (client `net_play.rs` `SETTLE_FRAMES`).
const SETTLE_FRAMES: u8 = 3;

/// The folder [`PlayAssetRoot`] names. The world has none: a path under it
/// is only ever text (common `space_path::contained_url`) or read through
/// [`PlaySpaceSource`].
const WORLD_ROOT: &str = "/space";

/// The opened world's files, as `space://` reads them.
#[derive(Resource)]
struct SpaceFiles(Dir);

/// The name this player joined as, for its Player in the tree (the desktop
/// Player's join name, as its tree branch gets it).
#[derive(Resource)]
struct LocalName(String);

/// The world opened this many frames ago, and is not yet ready.
#[derive(Resource)]
struct Settling(u8);

/// This page joined a host, so the character stands where the host placed it.
/// A published world played alone stands on the Space's SpawnLocation.
#[derive(Resource)]
struct JoinedSession;

/// Where the local character's feet go, once the world has settled. The
/// desktop Player waits the same few frames so the Space's colliders exist
/// before the body is placed (`net_play.rs` `SETTLE_FRAMES`).
#[derive(Resource)]
struct PendingCharacter {
    at: Vec3,
}

/// Open each world that arrives, for the player joining as `name`. `space`
/// is the directory the app's `space://` source reads. `joined` is a live
/// session: the host named where this player stands.
pub(crate) fn add(app: &mut App, space: Dir, name: String, joined: bool) {
    if joined {
        app.insert_resource(JoinedSession);
    }
    app.insert_resource(SpaceFiles(space))
        .insert_resource(LocalName(name))
        .add_systems(
            Update,
            (
                open_arrived_world,
                register_opened_space_materials,
                ready_after_settling,
                spawn_local_character.before(eustress_common::avatar::AvatarSystems::Lifecycle),
                note_local_motion,
                report_notices,
            )
                .chain(),
        );
}

fn open_arrived_world(
    mut commands: Commands,
    mut arrivals: MessageReader<WorldDownloaded>,
    files: Res<SpaceFiles>,
    name: Res<LocalName>,
    joined: Option<Res<JoinedSession>>,
    mut end: MessageWriter<EndSession>,
    mut page: NonSendMut<PageState>,
) {
    for WorldDownloaded(world) in arrivals.read() {
        let Some((_, records)) = world
            .spaces
            .iter()
            .find(|(name, _)| *name == world.start_space)
        else {
            error!(
                "net: the world names {} to open and does not carry it",
                world.start_space
            );
            page.report(states::JOIN_FAILED);
            end.write(EndSession {
                reason: "the world could not be opened".into(),
            });
            continue;
        };
        let opened = open_player_world(records, &name.0);
        info!(
            "space: {} read into its tree: {}",
            world.start_space,
            opened.report.summary()
        );
        for problem in &opened.report.problems {
            warn!("space: {problem}");
        }
        let source = Arc::new(RecordsSource::new(records.iter().cloned()));
        // The Space's character policy (its replacement bodies and
        // StarterPlayer switches), in place before the avatar spawns, as the
        // desktop Player loads it from the folder (`space_world.rs`).
        let (policy, policy_warnings) =
            SpaceCharacterPolicy::load_source(source.as_ref(), Path::new(WORLD_ROOT));
        for warning in &policy_warnings {
            warn!("avatar: {warning}");
        }
        commands.insert_resource(policy);
        if opened.character_auto_loads {
            // Solo: the SpawnLocation, in metres. A host already placed a
            // joined player, and that position stands (desktop `net_play.rs`).
            // The published download's own spawn is the unset default, not a
            // converted SpawnLocation, so it is only the fallback.
            let at = if joined.is_some() {
                Vec3::from(world.spawn)
            } else {
                opened.spawn.unwrap_or(Vec3::from(world.spawn))
            };
            info!("space: the local character spawns at {at:?}");
            commands.insert_resource(PendingCharacter { at });
        } else {
            info!("space: this Space gives players no avatar (Players.CharacterAutoLoads is off)");
        }
        for (key, bytes) in source.files() {
            files
                .0
                .insert_asset(Path::new(key), Value::Vec(bytes.clone()));
        }
        let shared = eustress_common::datamodel::new_shared();
        *shared.lock() = opened.dm;
        // Replication binds the tree by these keys; the draw step draws it
        // from the next frame.
        commands.insert_resource(SceneKeys(opened.keys));
        commands.insert_resource(PlayDataModel { dm: shared });
        commands.insert_resource(PlayAssetRoot(PathBuf::from(WORLD_ROOT)));

        // Lit and skied as the desktop Player does it (client space_world.rs
        // after the tree branch), from the records: the tree draws its own
        // Point, Spot and Surface lights, so only Directional ones come
        // from here.
        let (mut lights, light_problems) = read_space_lights_from(&*source);
        lights.retain(|l| matches!(l.component, LightComponent::Directional(_)));
        for problem in &light_problems {
            warn!("space: light {problem}");
        }
        spawn_space_lights(&mut commands, &lights);
        let lighting = match read_space_lighting_from(&*source) {
            Ok(found) => found.unwrap_or_default(),
            Err(e) => {
                warn!("space: lighting {e}");
                LightingService::default()
            }
        };
        let (sky_objects, sky_problems) = read_space_celestials_from(&*source);
        for problem in &sky_problems {
            warn!("space: sky {problem}");
        }
        let sky_count = spawn_space_celestials(&mut commands, &sky_objects, &lighting);
        info!(
            "space: {sky_count} sky object(s), Lighting at {}",
            lighting.clock_time
        );
        commands.insert_resource(lighting);

        commands.insert_resource(PlaySpaceSource(source));
        commands.insert_resource(Settling(0));
    }
}

/// The world's Space materials (`MaterialService/*.mat.toml`) into the shared
/// registry each time a world opens, as the desktop Player fills it (client
/// space_world.rs `register_opened_space_materials`), read from the records.
/// A map the Space does not hold loads from `bundled://`.
fn register_opened_space_materials(
    source: Option<Res<PlaySpaceSource>>,
    registry: Option<ResMut<MaterialRegistry>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    let (Some(source), Some(mut registry)) = (source, registry) else {
        return;
    };
    if !source.is_changed() {
        return;
    }
    *registry = MaterialRegistry::default();
    let (count, failed) = register_space_materials_from(
        &source,
        Path::new(WORLD_ROOT),
        &mut registry,
        &mut materials,
        &asset_server,
    );
    info!("space: {count} material(s) from the world's records");
    for f in failed {
        warn!("space: material {f}");
    }
}

fn ready_after_settling(
    mut commands: Commands,
    settling: Option<ResMut<Settling>>,
    mut ready: MessageWriter<LocalWorldReady>,
    mut page: NonSendMut<PageState>,
) {
    let Some(mut settling) = settling else { return };
    settling.0 = settling.0.saturating_add(1);
    if settling.0 < SETTLE_FRAMES {
        return;
    }
    ready.write(LocalWorldReady);
    page.report(states::IN_WORLD);
    commands.remove_resource::<Settling>();
}

/// The local character, once the world has had its settle frames. The
/// descriptor is the default body dressed by the Space's StarterPlayer, which
/// is what the desktop Player spawns when nobody is signed in.
fn spawn_local_character(
    mut commands: Commands,
    pending: Option<Res<PendingCharacter>>,
    settling: Option<Res<Settling>>,
    policy: Option<Res<SpaceCharacterPolicy>>,
    mut spawn: MessageWriter<SpawnAvatar>,
) {
    let Some(pending) = pending else { return };
    // Wait out the settle frames. `Settling` is removed by a command on the
    // frame the count is reached, so this also runs on the frame after, when
    // the resource is already gone: either frame places the body, and neither
    // misses it.
    if settling.is_some_and(|s| s.0 < SETTLE_FRAMES) {
        return;
    }
    let descriptor = match policy.as_deref() {
        Some(policy) => policy.apply(AvatarDescriptor::default()),
        None => AvatarDescriptor::default(),
    };
    let at = pending.at;
    info!(
        "space: spawning the local character at {at:?} ({})",
        descriptor.resolved_rig().body_asset
    );
    spawn.write(SpawnAvatar::new(descriptor, at));
    commands.remove_resource::<PendingCharacter>();
}

/// One line when the local body actually walks, so a playtest can see the
/// controller and not only the spawn.
fn note_local_motion(
    mut announced: Local<bool>,
    bodies: Query<&eustress_common::avatar::spawn::AvatarLocomotion, With<eustress_common::avatar::LocalAvatar>>,
) {
    if *announced {
        return;
    }
    let moving = bodies.iter().any(|body| body.planar_speed > 0.2);
    if !moving {
        return;
    }
    *announced = true;
    info!("space: the local character is moving");
}

/// The session's notices: to the log as the desktop Player logs them, and
/// to the page as a state name.
fn report_notices(mut notices: MessageReader<NetNotice>, mut page: NonSendMut<PageState>) {
    for notice in notices.read() {
        match notice {
            NetNotice::Joined { peer, host_name } => {
                info!("net: joined {host_name}'s session as player {peer}")
            }
            NetNotice::JoinFailed { reason } => error!("net: could not join: {reason}"),
            NetNotice::Disconnected { reason } => warn!("net: disconnected: {reason}"),
            _ => {}
        }
        if let Some(state) = states::state_for(notice, page.in_world()) {
            page.report(state);
        }
    }
}
