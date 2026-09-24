//! # Hosting a session from Studio
//!
//! **Start Server** (F9, Network > Start Local Server, or Start in the Test
//! tab's Server group) hosts the Space being played so Players can join it:
//!
//! 1. Save, when editing, so players get what the host sees.
//! 2. Bake the Space and the Universe's shared `assets/` into `.echk` chunks
//!    off the main thread: the same export publishing uploads
//!    ([`crate::space::echk_export`]).
//! 3. Listen for WebTransport sessions ([`eustress_networking::native`]):
//!    on this machine only by default, on the local network when asked.
//! 4. Enter Play with a character, if not already playing.
//! 5. Show the join link, and leave it where a Player on this machine finds
//!    it (`<workspace>/.eustress/hosts/<port>.link`).
//!
//! **Stop Server**, or returning to Edit, ends the session. Pausing does not.
//!
//! The host is authoritative: its scripts and physics run here, and players
//! see the host's world plus every avatar in real time.
//!
//! ## Settings
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `EUSTRESS_HOST_PORT` | 7777 | Port to listen on (0 picks any free port) |
//! | `EUSTRESS_HOST_LAN` | off | `1` also accepts players on the local network |
//! | `EUSTRESS_HOST_MAX_PLAYERS` | 8 | Players besides the host |
//! | `EUSTRESS_HOST_ON_PLAY` | off | `1` hosts whenever Play starts; with `eustress-headless` that is a dedicated server |

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};

use bevy::ecs::message::{MessageCursor, Messages};
use bevy::prelude::*;

use eustress_echk::WorldManifest;
use eustress_networking::native::{self, HostOptions};
use eustress_networking::{begin_host, EndSession, HostConfig, HostSession, JoinLink, NetNotice, NetPlugin};

use crate::keybindings::Action;
use crate::notifications::NotificationManager;
use crate::play_mode::{PlayModeState, PlayModeType, StartPlayEvent};
use crate::ui::MenuActionEvent;
use crate::ui::slint_ui::OutputConsole;

type ExportResult = Result<(WorldManifest, HashMap<String, Arc<Vec<u8>>>), String>;

struct PendingExport {
    rx: Mutex<mpsc::Receiver<ExportResult>>,
    space: String,
    host_name: String,
    spawn: Vec3,
    start_play: bool,
}

struct ActiveHost {
    join_key: String,
    lan: bool,
    port: u16,
    saw_playing: bool,
}

/// Where hosting is: asked for, baking, or running.
#[derive(Resource, Default)]
pub struct HostRequest {
    wanted: bool,
    pending: Option<PendingExport>,
    active: Option<ActiveHost>,
}

impl HostRequest {
    pub fn is_hosting(&self) -> bool {
        self.active.is_some()
    }
}

/// Adds hosting. Registered in `app_core`, so the headless engine can host
/// too (see `EUSTRESS_HOST_ON_PLAY`).
pub struct MultiplayerPlugin;

impl Plugin for MultiplayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(NetPlugin).init_resource::<HostRequest>().add_systems(
            Update,
            (
                read_server_actions,
                host_on_play,
                start_host_export,
                poll_host_export,
                stop_when_play_ends,
                report_net_notices,
            )
                .chain(),
        );
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes"))
        .unwrap_or(false)
}

fn env_number<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

#[derive(Clone, Copy)]
enum Say {
    Info,
    Error,
}

fn notify(world: &mut World, say: Say, message: String) {
    match say {
        Say::Error => error!("multiplayer: {message}"),
        Say::Info => info!("multiplayer: {message}"),
    }
    if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
        match say {
            Say::Error => n.error(message),
            Say::Info => n.info(message),
        }
    }
}

/// The link players use, as this host would print it.
fn join_link(active: &ActiveHost, pin: Option<[u8; 32]>) -> JoinLink {
    let host = if active.lan {
        native::local_network_ip().map(|ip| ip.to_string()).unwrap_or_else(|| "127.0.0.1".into())
    } else {
        "127.0.0.1".into()
    };
    JoinLink { host, port: active.port, key: Some(active.join_key.clone()), pin }
}

// ─────────────────────────────────────────────────────────────────────────────
// Systems
// ─────────────────────────────────────────────────────────────────────────────

/// Start Server / Stop Server from the ribbon, the Test menu and F9. Read
/// through an optional cursor: the headless engine has no menu and no
/// `MenuActionEvent`, and this system must not fail there.
fn read_server_actions(
    messages: Option<Res<Messages<MenuActionEvent>>>,
    mut cursor: Local<MessageCursor<MenuActionEvent>>,
    mut request: ResMut<HostRequest>,
    host: Option<Res<HostSession>>,
    mut end: MessageWriter<EndSession>,
    mut notifications: Option<ResMut<NotificationManager>>,
) {
    let Some(messages) = messages else { return };
    for event in cursor.read(&messages) {
        match event.action {
            Action::StartServer => {
                if let (Some(active), Some(host)) = (&request.active, &host) {
                    let link = join_link(active, host.pin()).to_link();
                    info!("multiplayer: already hosting; join with {link}");
                    if let Some(n) = notifications.as_mut() {
                        n.info(format!("Already hosting. Players join with: {link}"));
                    }
                } else if request.wanted || request.pending.is_some() {
                    if let Some(n) = notifications.as_mut() {
                        n.info("The server is already starting.");
                    }
                } else {
                    request.wanted = true;
                }
            }
            Action::StopServer => {
                if host.is_some() {
                    end.write(EndSession { reason: "the host stopped the server".into() });
                } else if request.pending.take().is_some() || std::mem::take(&mut request.wanted) {
                    if let Some(n) = notifications.as_mut() {
                        n.info("Server start cancelled.");
                    }
                } else if let Some(n) = notifications.as_mut() {
                    n.info("No server is running.");
                }
            }
            _ => {}
        }
    }
}

/// `EUSTRESS_HOST_ON_PLAY=1`: host once each time Play starts.
fn host_on_play(
    state: Option<Res<State<PlayModeState>>>,
    host: Option<Res<HostSession>>,
    mut request: ResMut<HostRequest>,
    mut enabled: Local<Option<bool>>,
    mut asked_this_play: Local<bool>,
) {
    let enabled = *enabled.get_or_insert_with(|| env_flag("EUSTRESS_HOST_ON_PLAY"));
    let Some(state) = state else { return };
    if !enabled {
        return;
    }
    match state.get() {
        PlayModeState::Editing => *asked_this_play = false,
        PlayModeState::Playing => {
            if !*asked_this_play && host.is_none() && request.pending.is_none() && request.active.is_none() {
                *asked_this_play = true;
                request.wanted = true;
            }
        }
        PlayModeState::Paused => {}
    }
}

/// Save, then bake the Space on a worker thread.
fn start_host_export(world: &mut World) {
    if !world.resource::<HostRequest>().wanted {
        return;
    }
    world.resource_mut::<HostRequest>().wanted = false;
    match prepare_export(world) {
        Ok(pending) => {
            let message = format!("Preparing to host {}: baking the world for players...", pending.space);
            world.resource_mut::<HostRequest>().pending = Some(pending);
            notify(world, Say::Info, message);
        }
        Err(message) => notify(world, Say::Error, message),
    }
}

#[cfg(feature = "world-db")]
fn prepare_export(world: &mut World) -> Result<PendingExport, String> {
    use crate::space::echk_export::{export_world, SpaceInput, SpaceSource, SAVE_SETTLE};

    let space_root = world
        .get_resource::<crate::space::SpaceRoot>()
        .map(|r| r.0.clone())
        .ok_or("Open a Space before starting a server.")?;
    let still_opening = world
        .get_resource::<crate::space::world_db_plugin::PendingWorldDbOpen>()
        .is_some_and(|p| p.0.is_some());
    if still_opening {
        return Err("The Space is still opening. Start the server again in a moment.".into());
    }
    let db = crate::space::active_db::db_arc()
        .ok_or("This Space has no database open, so it cannot be served. Reopen the Space and try again.")?;

    let editing = world
        .get_resource::<State<PlayModeState>>()
        .map(|s| *s.get() == PlayModeState::Editing)
        .unwrap_or(true);
    if editing {
        crate::ui::file_event_handler::do_save_space(world);
    }

    let spawn = host_spawn_point(world);
    let host_name = world
        .get_resource::<crate::auth::AuthState>()
        .and_then(|a| a.user.as_ref().map(|u| u.username.clone()))
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Host".to_string());
    let universe_root = crate::space::universe_root_for_path(&space_root).unwrap_or_else(|| space_root.clone());
    let universe = folder_name(&universe_root);
    let space = folder_name(&space_root);
    let threshold = world
        .get_resource::<crate::space::residency::ResidencyConfig>()
        .map(|c| c.big_space_threshold)
        .unwrap_or(100_000);
    let out_root = universe_root.join(".eustress").join("host");

    let (tx, rx) = mpsc::channel();
    let space_name = space.clone();
    std::thread::Builder::new()
        .name("eustress-host-bake".into())
        .spawn(move || {
            if editing {
                std::thread::sleep(SAVE_SETTLE);
            }
            let inputs = vec![SpaceInput {
                name: space_name.clone(),
                source: SpaceSource::Db(db),
                folder: Some(space_root),
            }];
            // The Universe's shared assets travel too: a Space's meshes and
            // textures can live there.
            let result = export_world(&universe, inputs, &space_name, Some(universe_root.as_path()), &out_root, threshold).and_then(|exported| {
                let chunks = exported.load_all()?;
                Ok((exported.manifest, chunks))
            });
            let _ = tx.send(result);
        })
        .map_err(|e| format!("Could not start the world bake: {e}"))?;

    Ok(PendingExport { rx: Mutex::new(rx), space, host_name, spawn, start_play: editing })
}

#[cfg(not(feature = "world-db"))]
fn prepare_export(_world: &mut World) -> Result<PendingExport, String> {
    Err("Hosting serves the Space's database, and this build has no world-db feature.".into())
}

/// Where players appear: the Space's SpawnLocation, else the PlayerService
/// default. The same rule Play uses for the host's own character.
fn host_spawn_point(world: &mut World) -> Vec3 {
    let default = world
        .get_resource::<eustress_common::services::PlayerService>()
        .map(|p| p.spawn_position)
        .unwrap_or(Vec3::ZERO);
    let mut spawns = world.query::<(&Transform, &eustress_common::classes::SpawnLocation)>();
    eustress_common::services::get_spawn_position_or_default(spawns.iter(world), None, default).0
}

fn folder_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".to_string())
}

/// When the bake finishes, start listening and enter Play.
fn poll_host_export(
    mut commands: Commands,
    mut request: ResMut<HostRequest>,
    mut start_play: MessageWriter<StartPlayEvent>,
    mut notifications: Option<ResMut<NotificationManager>>,
) {
    let result = {
        let Some(pending) = &request.pending else { return };
        let Ok(rx) = pending.rx.lock() else { return };
        match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("the world bake stopped without a result".into()),
        }
    };
    let pending = request.pending.take().expect("checked above");

    let (manifest, chunks) = match result {
        Ok(baked) => baked,
        Err(e) => {
            error!("multiplayer: could not bake {} for hosting: {e}", pending.space);
            if let Some(n) = notifications.as_mut() {
                n.error(format!("Could not start the server: {e}"));
            }
            return;
        }
    };

    let options = HostOptions {
        port: env_number("EUSTRESS_HOST_PORT", native::DEFAULT_PORT),
        lan: env_flag("EUSTRESS_HOST_LAN"),
        join_key: native::new_join_key(),
    };
    let link = match native::start_host(options.clone()) {
        Ok(link) => link,
        Err(e) => {
            error!("multiplayer: {e}");
            if let Some(n) = notifications.as_mut() {
                n.error(format!("Could not start the server: {e}"));
            }
            return;
        }
    };
    let bytes: u64 = manifest.download_bytes();
    info!(
        "multiplayer: serving {} ({} chunks, {} bytes) as {}",
        pending.space,
        chunks.len(),
        bytes,
        pending.host_name
    );
    begin_host(
        &mut commands,
        link,
        HostConfig {
            host_name: pending.host_name,
            max_players: env_number("EUSTRESS_HOST_MAX_PLAYERS", 8u16),
            world: manifest,
            chunks,
            spawn: pending.spawn.to_array(),
        },
    );
    request.active = Some(ActiveHost { join_key: options.join_key, lan: options.lan, port: 0, saw_playing: false });
    if pending.start_play {
        start_play.write(StartPlayEvent { play_type: PlayModeType::WithCharacter });
    }
}

/// Returning to Edit ends the session. Pausing does not.
fn stop_when_play_ends(
    state: Option<Res<State<PlayModeState>>>,
    host: Option<Res<HostSession>>,
    mut request: ResMut<HostRequest>,
    mut end: MessageWriter<EndSession>,
) {
    let (Some(state), Some(_host)) = (state, host) else { return };
    let Some(active) = request.active.as_mut() else { return };
    match state.get() {
        PlayModeState::Playing | PlayModeState::Paused => active.saw_playing = true,
        PlayModeState::Editing => {
            if active.saw_playing {
                active.saw_playing = false;
                end.write(EndSession { reason: "the host stopped playing".into() });
            }
        }
    }
}

/// Tell the person at the keyboard what the session is doing.
fn report_net_notices(
    mut notices: MessageReader<NetNotice>,
    mut request: ResMut<HostRequest>,
    mut notifications: Option<ResMut<NotificationManager>>,
    mut output: Option<ResMut<OutputConsole>>,
) {
    for notice in notices.read() {
        match notice {
            NetNotice::Hosting { port, pin } => {
                let Some(active) = request.active.as_mut() else { continue };
                active.port = *port;
                let link = join_link(active, Some(*pin));
                let text = link.to_link();
                native::write_host_file(&eustress_bridge_client::default_workspace_root(), &link);
                let reach = if active.lan { "on your local network" } else { "on this computer" };
                info!("multiplayer: hosting {reach}; join with {text}");
                if let Some(n) = notifications.as_mut() {
                    n.success(format!(
                        "Hosting {reach} on port {port}. A Player on this computer joins with: eustress-client --connect 127.0.0.1:{port}"
                    ));
                }
                if let Some(o) = output.as_mut() {
                    o.info(format!("Join link: {text}"));
                    o.info(format!("On this computer: eustress-client --connect 127.0.0.1:{port}"));
                }
            }
            NetNotice::HostEnded { reason } => {
                if let Some(active) = request.active.take() {
                    if active.port != 0 {
                        native::remove_host_file(&eustress_bridge_client::default_workspace_root(), active.port);
                    }
                }
                if let Some(n) = notifications.as_mut() {
                    n.info(format!("Server stopped: {reason}."));
                }
            }
            NetNotice::PeerJoined { name, .. } => {
                if let Some(n) = notifications.as_mut() {
                    n.info(format!("{name} joined."));
                }
                if let Some(o) = output.as_mut() {
                    o.info(format!("{name} joined the session."));
                }
            }
            NetNotice::PeerLeft { name, .. } => {
                if let Some(n) = notifications.as_mut() {
                    n.info(format!("{name} left."));
                }
                if let Some(o) = output.as_mut() {
                    o.info(format!("{name} left the session."));
                }
            }
            NetNotice::Chat { name, text, .. } => {
                if let Some(o) = output.as_mut() {
                    o.info(format!("[{name}] {text}"));
                }
            }
            _ => {}
        }
    }
}
