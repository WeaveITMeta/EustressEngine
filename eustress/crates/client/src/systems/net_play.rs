//! # Joining a host, and opening a world that arrived
//!
//! Launched with `--connect <host:port>` (or a `eustress-player://join/...` link),
//! the Player joins a Studio host over WebTransport, downloads the host's
//! Space as `.echk` chunks, opens it, and appears at the spawn point the host
//! chose. Everyone in the session then sees everyone else move.
//!
//! The same opening path serves a published simulation downloaded by
//! [`super::space_fetch`]: both arrive as a [`WorldDownloaded`], are written
//! into this process's world folder ([`super::live_world`]), opened by
//! [`super::space_world`], and only then get the local avatar: a few frames
//! after the Space's colliders exist, and once its terrain has landed
//! ([`SpaceTerrainLoad::ready`]), so the character stands on them.

use bevy::prelude::*;
use eustress_common::avatar::profile::SpawnSavedAvatar;
use eustress_networking::native;
use eustress_networking::session::{begin_join, ChunkCacheRes, LocalWorldReady, NetNotice, NetPlugin, WorldDownloaded};
use eustress_networking::JoinLink;

use super::live_world::{materialize, DiskChunkCache, LiveWorld};
use super::space_world::{OpenSpaceRequest, SpaceOpened, SpaceTerrainLoad};

/// Frames between a Space opening and its avatar spawning.
const SETTLE_FRAMES: u8 = 3;

/// Set by `main` when the Player was launched to join a host.
#[derive(Resource, Clone, Debug)]
pub struct JoinTarget {
    pub link: JoinLink,
    pub name: String,
    /// An identity ticket bound to the host's pin, when the player is signed
    /// in: how the host learns which account joined.
    pub identity: Option<String>,
}

/// The avatar waiting for its world.
#[derive(Resource, Debug)]
struct PendingAvatar {
    at: Vec3,
    frames_since_open: Option<u8>,
    /// The Space that opened, once it has.
    root: Option<std::path::PathBuf>,
}

/// The Space's `Players.CharacterAutoLoads`, read as Studio reads it from
/// `Players/_service.toml` (`character_auto_loads`, in `[properties]`, at the
/// top level, or in `[service]`, the first of those winning): on unless the
/// Space turns it off. Off, nobody gets a body; a paddle game is played
/// through its camera and input alone.
fn character_auto_loads(space_root: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(space_root.join("Players").join("_service.toml")) else { return true };
    let Ok(doc) = text.parse::<toml::Table>() else { return true };
    let pick = |t: &toml::Table| t.get("character_auto_loads").or_else(|| t.get("CharacterAutoLoads")).and_then(toml::Value::as_bool);
    let section = |name: &str| doc.get(name).and_then(toml::Value::as_table).and_then(|t| pick(t));
    section("properties").or_else(|| pick(&doc)).or_else(|| section("service")).unwrap_or(true)
}

pub struct NetPlayPlugin;

impl Plugin for NetPlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((NetPlugin, super::net_replica::NetReplicaPlugin))
            // What the host's log says this Player runs.
            .insert_resource(eustress_networking::AppVersion(concat!("eustress-client ", env!("CARGO_PKG_VERSION")).into()))
            .add_systems(Startup, start_join.run_if(resource_exists::<JoinTarget>))
            .add_systems(Update, (open_arrived_world, spawn_avatar_in_opened_world, log_net_notices).chain());
    }
}

fn start_join(mut commands: Commands, target: Res<JoinTarget>) {
    info!(
        "net: joining {}:{} as {}{}",
        target.link.host,
        target.link.port,
        target.name,
        if target.link.pin.is_some() { "" } else { " (unpinned: this machine only)" }
    );
    match native::start_join(&target.link) {
        Ok(link) => {
            commands.insert_resource(ChunkCacheRes(Box::new(DiskChunkCache::default())));
            begin_join(&mut commands, link, &target.name, target.identity.clone());
        }
        Err(e) => error!("net: {e}"),
    }
}

fn open_arrived_world(
    mut commands: Commands,
    mut arrivals: MessageReader<WorldDownloaded>,
    live: Option<Res<LiveWorld>>,
    mut open: MessageWriter<OpenSpaceRequest>,
) {
    for WorldDownloaded(world) in arrivals.read() {
        let Some(live) = live.as_deref() else {
            error!("net: a world arrived but this Player has no world folder (launch with --connect or --sim)");
            continue;
        };
        match materialize(world, live) {
            Ok(files) => {
                info!("net: opening {} from {} ({files} files)", world.start_space, world.universe);
                commands.insert_resource(PendingAvatar {
                    at: Vec3::from_array(world.spawn),
                    frames_since_open: None,
                    root: None,
                });
                open.write(OpenSpaceRequest { root: live.space_root.clone() });
            }
            Err(e) => error!("net: could not open the world: {e}"),
        }
    }
}

fn spawn_avatar_in_opened_world(
    mut commands: Commands,
    mut opened: MessageReader<SpaceOpened>,
    pending: Option<ResMut<PendingAvatar>>,
    terrain: Option<Res<SpaceTerrainLoad>>,
    mut spawn: MessageWriter<SpawnSavedAvatar>,
    mut ready: MessageWriter<LocalWorldReady>,
) {
    // Pending first: with no avatar waiting, SpaceOpened stays unread rather
    // than being consumed by a frame that could not use it.
    let Some(mut pending) = pending else { return };
    if let Some(space) = opened.read().last() {
        info!("net: opened {} ({} parts)", space.root.display(), space.parts);
        pending.frames_since_open = Some(0);
        pending.root = Some(space.root.clone());
    }
    let Some(frames) = pending.frames_since_open.as_mut() else { return };
    // Saturating: a large voxel terrain can take longer than 255 frames.
    *frames = frames.saturating_add(1);
    let terrain_ready = terrain.as_deref().map_or(true, SpaceTerrainLoad::ready);
    if *frames < SETTLE_FRAMES || !terrain_ready {
        return;
    }
    if pending.root.as_deref().map_or(true, character_auto_loads) {
        let token = dirs::data_local_dir().and_then(|d| std::fs::read_to_string(d.join("EustressEngine/auth_token")).ok());
        spawn.write(SpawnSavedAvatar { token, at: pending.at });
    } else {
        // No avatar, so none is announced or sent, and the host builds no
        // replica: the Space's own camera is the view.
        info!("net: this Space gives players no avatar (Players.CharacterAutoLoads is off)");
    }
    // Tells a host we are in the world. With no session (a published
    // simulation played solo) nothing reads it.
    ready.write(LocalWorldReady);
    commands.remove_resource::<PendingAvatar>();
}

fn log_net_notices(mut notices: MessageReader<NetNotice>, mut last_percent: Local<u64>) {
    for notice in notices.read() {
        match notice {
            NetNotice::Joined { peer, host_name } => info!("net: joined {host_name}'s session as player {peer}"),
            NetNotice::JoinFailed { reason } => error!("net: could not join: {reason}"),
            NetNotice::Disconnected { reason } => warn!("net: disconnected: {reason}"),
            NetNotice::Downloading { received, total } => {
                let percent = if *total == 0 { 100 } else { received.saturating_mul(100) / total };
                if percent >= *last_percent + 10 || percent == 100 {
                    *last_percent = percent;
                    info!("net: downloading the world: {percent}% of {} KB", total / 1024);
                }
            }
            NetNotice::PeerJoined { name, .. } => info!("net: {name} is here"),
            NetNotice::PeerLeft { name, .. } => info!("net: {name} left"),
            NetNotice::Chat { name, text, .. } => info!("[{name}] {text}"),
            // Answered by the commerce module, which asks the player.
            NetNotice::PurchasePrompt { prompt, product, .. } => info!("net: the host offers product {product} (prompt {prompt})"),
            NetNotice::Hosting { .. }
            | NetNotice::HostEnded { .. }
            | NetNotice::PeerIdentity { .. }
            | NetNotice::PurchaseClosed { .. }
            | NetNotice::Receipts { .. } => {}
        }
    }
}
