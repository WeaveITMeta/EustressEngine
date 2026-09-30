//! # Hosting a session from Studio
//!
//! **Start Server** (F9, Network > Start Local Server, or Start in the Test
//! tab's Server group) hosts the Space being played so Players can join it:
//!
//! 1. Save edits not yet saved, when editing, so players get what the host
//!    sees.
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
//! | `EUSTRESS_HOST_PUBLIC` | unset | The address players reach this host at, for the gallery's link (`host` or `host:port`; IPv6 in brackets): this computer's local network address when players come in through the tunnel's private route, a public name later |
//!
//! ## The gallery
//!
//! Hosting a published simulation, signed in as the listing's author, with
//! `EUSTRESS_HOST_LAN=1` and `EUSTRESS_HOST_PUBLIC` set, tells the gallery
//! every 30 seconds that the session is live, with its join link, and tells it
//! again when the session ends. The address is only ever the one configured:
//! guessing this computer's own can pick a VPN or tunnel adapter instead.

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

/// The baked world, each streamed core's record key by stored id, and, when
/// hosting begins in a Play session already running, what players load.
type ExportResult = Result<
    (
        WorldManifest,
        HashMap<String, Arc<Vec<u8>>>,
        HashMap<u64, String>,
        Option<eustress_networking::repl::WorldLayout>,
    ),
    String,
>;

struct PendingExport {
    rx: Mutex<mpsc::Receiver<ExportResult>>,
    space: String,
    /// The Space folder the world's record keys are relative to.
    space_root: std::path::PathBuf,
    host_name: String,
    spawn: Vec3,
    start_play: bool,
    /// The gallery listing this Universe is published as, if any.
    sim_id: Option<String>,
}

struct ActiveHost {
    join_key: String,
    lan: bool,
    port: u16,
    /// The certificate pin, once listening.
    pin: Option<[u8; 32]>,
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
        // The API in use, resolved at startup, so a test API
        // (`EUSTRESS_API_URL`) is announced before any call goes to it.
        let _ = eustress_common::api_base::api_base();
        app.add_plugins((NetPlugin, crate::net_replicate::ReplicationPlugin))
            .init_resource::<HostRequest>()
            .init_resource::<GalleryBeat>()
            .add_systems(
                Update,
                (
                    read_server_actions,
                    host_on_play,
                    start_host_export,
                    poll_host_export,
                    stop_when_play_ends,
                    report_net_notices,
                    refresh_host_file,
                    gallery_heartbeat,
                )
                    .chain(),
            );
    }
}

/// Seconds between the beats that keep a session live in the gallery; it
/// shows the session offline 90 s after the last one.
const GALLERY_BEAT_SECS: f64 = 30.0;

/// What the gallery was told about this host's session.
#[derive(Resource, Default)]
struct GalleryBeat {
    next_at: f64,
    /// The listing, and the link it shows.
    told: Option<(String, String)>,
    /// The last request's outcome, written by its thread.
    outcome: Arc<Mutex<Option<String>>>,
    /// The outcome last logged, so a repeated failure is logged once.
    reported: Option<String>,
}

/// `EUSTRESS_HOST_PUBLIC`: where players on the internet reach this host,
/// as a host and, when a port forward or tunnel maps a different one, a port.
fn public_address() -> Option<(String, Option<u16>)> {
    let raw = std::env::var("EUSTRESS_HOST_PUBLIC").ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(end) = raw.find(']').filter(|_| raw.starts_with('[')) {
        let port = raw[end + 1..].strip_prefix(':').and_then(|p| p.parse().ok());
        return Some((raw[..=end].to_string(), port));
    }
    match raw.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => match port.parse::<u16>() {
            Ok(p) => Some((host.to_string(), Some(p))),
            Err(_) => Some((raw.to_string(), None)),
        },
        _ => Some((raw.to_string(), None)),
    }
}

/// Keep the gallery's live marker for this session current, and clear it
/// when the session ends. A failed beat is logged and hosting goes on.
fn gallery_heartbeat(
    time: Res<Time>,
    request: Res<HostRequest>,
    host: Option<Res<HostSession>>,
    auth: Option<Res<crate::auth::AuthState>>,
    mut beat: ResMut<GalleryBeat>,
) {
    let finished = beat.outcome.lock().ok().and_then(|mut o| o.take());
    if let Some(outcome) = finished {
        if beat.reported.as_deref() != Some(outcome.as_str()) {
            if outcome == "ok" {
                info!("multiplayer: the gallery shows this session as live");
            } else {
                warn!("multiplayer: gallery not told: {outcome}");
            }
            beat.reported = Some(outcome);
        }
    }

    let token = auth.as_deref().and_then(|a| a.get_token()).map(str::to_string);
    let mut hint: Option<&str> = None;
    let live = host.as_deref().zip(request.active.as_ref()).and_then(|(host, active)| {
        let sim = host.sim_id()?.to_string();
        active.pin?;
        // Only an address someone chose: guessing this machine's network
        // address can pick a VPN or tunnel adapter's instead.
        let Some((address, port)) = public_address() else {
            hint = Some(
                "set EUSTRESS_HOST_PUBLIC to the address players reach (this computer's local network \
                 address when they come in through the tunnel) to list this session",
            );
            return None;
        };
        if !active.lan {
            hint = Some("this session listens on this computer only; set EUSTRESS_HOST_LAN=1 to list it");
            return None;
        }
        let link = JoinLink { host: address, port: port.unwrap_or(active.port), key: Some(active.join_key.clone()), pin: active.pin }
            .to_link();
        Some((sim, link, host.player_count(), host.max_players()))
    });
    if let Some(hint) = hint.filter(|h| beat.reported.as_deref() != Some(*h)) {
        if let Ok(mut o) = beat.outcome.lock() {
            *o = Some(hint.to_string());
        }
    }
    let told = beat.told.take();
    let now = time.elapsed_secs_f64();
    match live {
        Some((sim, link, players, max_players)) => {
            if let (Some((old_sim, _)), Some(token)) = (&told, &token) {
                if *old_sim != sim {
                    gallery_request(&beat.outcome, "DELETE", old_sim, token, None);
                }
            }
            let changed = told.as_ref().map_or(true, |(s, l)| *s != sim || *l != link);
            if changed || now >= beat.next_at {
                beat.next_at = now + GALLERY_BEAT_SECS;
                match &token {
                    Some(token) => {
                        let body = serde_json::json!({
                            "link": link,
                            "players": players,
                            "max_players": max_players,
                            "protocol": eustress_networking::wire::PROTOCOL_VERSION,
                        });
                        gallery_request(&beat.outcome, "POST", &sim, token, Some(body));
                    }
                    None => {
                        if let Ok(mut o) = beat.outcome.lock() {
                            *o = Some("sign in as the listing's author to show it live".into());
                        }
                    }
                }
            }
            beat.told = Some((sim, link));
        }
        None => {
            if let (Some((sim, _)), Some(token)) = (told, token) {
                gallery_request(&beat.outcome, "DELETE", &sim, &token, None);
            }
        }
    }
}

/// One live-marker request, on a thread of its own.
fn gallery_request(outcome: &Arc<Mutex<Option<String>>>, method: &'static str, sim: &str, token: &str, body: Option<serde_json::Value>) {
    let url = format!("{}/api/simulations/{sim}/live", crate::play_datamodel::commerce::api_base());
    let auth = format!("Bearer {token}");
    let outcome = outcome.clone();
    let started = std::thread::Builder::new().name("eustress-gallery-live".into()).spawn(move || {
        let request = ureq::request(method, &url).timeout(std::time::Duration::from_secs(10)).set("Authorization", &auth);
        let result = match body {
            Some(body) => request.send_json(body),
            None => request.call(),
        };
        let text = match result {
            Ok(_) => "ok".to_string(),
            // Two different 404s: an API without the live registry, and one
            // without this listing.
            Err(ureq::Error::Status(404, response)) => {
                if response.into_string().unwrap_or_default().contains("Simulation not found") {
                    "this API has no listing with this Universe's id (sync.toml experience_id) (404)".to_string()
                } else {
                    "this API has no live registry yet (404)".to_string()
                }
            }
            Err(ureq::Error::Status(403, _)) => "only the listing's author can show it live (403)".to_string(),
            Err(ureq::Error::Status(status, response)) => {
                let body = response.into_string().unwrap_or_default();
                format!("{method} answered {status}: {}", body.chars().take(200).collect::<String>())
            }
            Err(e) => format!("could not reach the API: {e}"),
        };
        if method == "POST" {
            if let Ok(mut o) = outcome.lock() {
                *o = Some(text);
            }
        } else if text != "ok" {
            warn!("multiplayer: the gallery may still show the session live: {text}");
        }
    });
    if let Err(e) = started {
        warn!("multiplayer: could not start the gallery request: {e}");
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

/// Flush unsaved edits, then bake the Space, with its live terrain, on a
/// worker thread.
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
    use crate::space::echk_export::{export_world, Audience, SpaceInput, SpaceSource, SAVE_SETTLE};

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
    // Players get what the host sees. The terrain comes from memory, so
    // hosting never writes the Space's terrain files; a Space whose terrain
    // cannot be taken that way (a migrated one) hosts its saved terrain. Some
    // edits (a rotate, an undo) reach disk, and so the tree the bake reads,
    // only through a save, so unsaved edits are flushed first: the parts whose
    // live values differ from their files. Hosting is not a save: no commit,
    // no toast. With nothing unsaved the bake already reads the host's world,
    // and flushing anyway held the frame for seconds on a large Space.
    let terrain = crate::ui::file_event_handler::terrain_snapshot(world);
    let stale_terrain =
        terrain.is_none() && editing && crate::ui::file_event_handler::terrain_changed_since_save(world, false);
    let saved = editing && has_unsaved_edits(world);
    if saved {
        // A refused flush wrote nothing (a snapshot revert is pending), and a
        // Space mid-revert is not served.
        crate::ui::file_event_handler::flush_space(world, crate::ui::file_event_handler::FlushTerrain::Skip)
            .map_err(|e| format!("Could not start the server: {e}"))?;
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
    // Where purchases in this session are made: the listing publishing keeps
    // in the Universe's sync.toml.
    let sim_id = eustress_common::load_toml_file::<eustress_common::SyncManifest>(
        &universe_root.join(".eustress").join("sync.toml"),
    )
    .ok()
    .and_then(|s| s.remote.experience_id)
    .filter(|id| !id.trim().is_empty());
    let threshold = world
        .get_resource::<crate::space::residency::ResidencyConfig>()
        .map(|c| c.big_space_threshold)
        .unwrap_or(100_000);
    let out_root = host_cache_dir(&universe_root);

    let (tx, rx) = mpsc::channel();
    let space_name = space.clone();
    let keys_root = space_root.clone();
    std::thread::Builder::new()
        .name("eustress-host-bake".into())
        .spawn(move || {
            if saved {
                std::thread::sleep(SAVE_SETTLE);
            }
            // Encoding a large terrain takes a while, so it happens here.
            let terrain = terrain.and_then(|snapshot| match snapshot.encode() {
                Ok(files) => Some(files),
                Err(e) => {
                    warn!("multiplayer: the live terrain could not be encoded ({e}); players get the terrain as last saved");
                    None
                }
            });
            let inputs = vec![SpaceInput {
                name: space_name.clone(),
                source: SpaceSource::Db(db),
                folder: Some(space_root),
                terrain,
            }];
            // The Universe's shared assets travel too: a Space's meshes and
            // textures can live there.
            // Players get what Roblox would replicate to them: the
            // server-only services stay on this machine.
            let result = export_world(
                &universe,
                inputs,
                &space_name,
                Some(universe_root.as_path()),
                &out_root,
                threshold,
                Audience::Players,
            )
            .and_then(|exported| {
                for (space, stats) in &exported.stats {
                    if !stats.webhook_paths.is_empty() {
                        warn!(
                            "multiplayer: {space}: players will receive {} file(s) holding a webhook URL: {}",
                            stats.webhook_paths.len(),
                            stats.webhook_paths.join(", ")
                        );
                    }
                }
                // Replication names a streamed core by the key players load
                // it from, which only the export knows.
                let cores: HashMap<u64, String> = exported
                    .stats
                    .iter()
                    .filter(|(s, _)| *s == space_name)
                    .flat_map(|(_, st)| st.core_keys.iter().cloned())
                    .collect();
                let chunks = exported.load_all()?;
                // Hosting a session already under way: what players load,
                // laid out as the Player reads it, so a late joiner gets
                // what the session changed before now.
                let layout = if editing { None } else { world_layout(&exported.manifest, &chunks, &space_name) };
                Ok((exported.manifest, chunks, cores, layout))
            });
            let _ = tx.send(result);
        })
        .map_err(|e| format!("Could not start the world bake: {e}"))?;

    if stale_terrain {
        notify(
            world,
            Say::Info,
            "Players get the terrain as it was last saved. Save to include this session's terrain edits.".into(),
        );
    }
    Ok(PendingExport { rx: Mutex::new(rx), space, space_root: keys_root, host_name, spawn, start_play: editing, sim_id })
}

/// Edits since the last save or autosave, as the title's asterisk counts
/// them, or terrain changed since it was last written.
#[cfg(feature = "world-db")]
fn has_unsaved_edits(world: &mut World) -> bool {
    let sequence = world.get_resource::<crate::undo::UndoStack>().map(|undo| undo.sequence());
    let unsaved = match (world.get_resource::<crate::ui::StudioState>(), sequence) {
        (Some(state), Some(sequence)) => state.has_unsaved_changes || state.saved_undo_sequence != sequence,
        _ => true,
    };
    unsaved || crate::ui::file_event_handler::terrain_changed_since_save(world, false)
}

#[cfg(not(feature = "world-db"))]
fn prepare_export(_world: &mut World) -> Result<PendingExport, String> {
    Err("Hosting serves the Space's database, and this build has no world-db feature.".into())
}

/// Where players' feet go: on top of the Space's SpawnLocation, else the
/// PlayerService default. The same rule Play uses for the host's own character.
fn host_spawn_point(world: &mut World) -> Vec3 {
    let default = world
        .get_resource::<eustress_common::services::PlayerService>()
        .map(|p| p.spawn_position)
        .unwrap_or(Vec3::ZERO);
    let mut spawns = world.query::<(&Transform, &eustress_common::classes::SpawnLocation)>();
    eustress_common::services::spawn_feet_position(spawns.iter(world), None, default).0
}

/// The start Space as a joining player reads it (see `WorldLayout`), from
/// the baked world. Runs on the bake thread, never the frame.
fn world_layout(
    manifest: &WorldManifest,
    chunks: &HashMap<String, Arc<Vec<u8>>>,
    space: &str,
) -> Option<eustress_networking::repl::WorldLayout> {
    let started = std::time::Instant::now();
    let world = eustress_networking::session::assemble_world(manifest, |h| chunks.get(h).map(|b| b.as_slice()), [0.0; 3])
        .map_err(|e| warn!("multiplayer: the world could not be laid out for late joiners ({e})"))
        .ok()?;
    let (_, records) = world.spaces.iter().find(|(name, _)| name == space)?;
    let layout = eustress_networking::repl::WorldLayout::of_records(records);
    info!(
        "multiplayer: laid out {} scene instances for late joiners in {} ms (bake thread)",
        layout.len(),
        started.elapsed().as_millis()
    );
    Some(layout)
}

/// Where hosting writes the world it serves: a cache in the Eustress
/// workspace's own `.eustress/host/`, one folder per Universe (its name and a
/// hash of its path), never inside a Space or Universe folder, so no Space's
/// autosave commits it, wherever the Universe lives.
fn host_cache_dir(universe_root: &std::path::Path) -> std::path::PathBuf {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in universe_root.to_string_lossy().bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let name = format!("{}-{hash:016x}", folder_name(universe_root));
    crate::space::workspace_root().join(".eustress").join("host").join(name)
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

    let (manifest, chunks, cores, layout) = match result {
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
    commands.insert_resource(crate::net_replicate::HostWorldKeys {
        space_root: pending.space_root.clone(),
        cores,
        world: layout.map(Arc::new),
    });
    begin_host(
        &mut commands,
        link,
        HostConfig {
            host_name: pending.host_name,
            max_players: env_number("EUSTRESS_HOST_MAX_PLAYERS", 8u16),
            world: manifest,
            chunks,
            spawn: pending.spawn.to_array(),
            sim_id: pending.sim_id,
        },
    );
    request.active = Some(ActiveHost { join_key: options.join_key, lan: options.lan, port: 0, pin: None, saw_playing: false });
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
                active.pin = Some(*pin);
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

/// Write this host's file again every [`native::HOST_FILE_REFRESH`] while it
/// hosts. A Player on this machine then tells a running host's file from one
/// a killed Studio left behind ([`native::HOST_FILE_FRESH`]). Real time, so a
/// paused session keeps its file current.
fn refresh_host_file(time: Res<Time<Real>>, request: Res<HostRequest>, mut next_at: Local<f64>) {
    let Some(active) = request.active.as_ref().filter(|a| a.port != 0 && a.pin.is_some()) else {
        return;
    };
    let now = time.elapsed_secs_f64();
    if now < *next_at {
        return;
    }
    *next_at = now + native::HOST_FILE_REFRESH.as_secs_f64();
    native::write_host_file(&eustress_bridge_client::default_workspace_root(), &join_link(active, active.pin));
}
