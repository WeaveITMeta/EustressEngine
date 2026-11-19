//! # Editing a gallery listing in Studio
//!
//! Edit opens `eustress://edit/<simulation id>`, which the installer routes to
//! Studio. The website shows it on a listing whose author turned on Share
//! Source, and on every Space in its author's own list.
//!
//! Studio opens, in order:
//! 1. The Space on this computer already published as that listing (its
//!    `.eustress/sync.toml` names it), chosen at launch
//!    ([`local_space_for`]). Nothing is downloaded, and publishing it updates
//!    the listing as before.
//! 2. Otherwise a download: the listing's source world (the chunks players get
//!    plus the server-only services, each checked against its content hash),
//!    written into the workspace as a new Universe, opened at the Space players
//!    start in. Anyone may download a listing that shares its source; its
//!    author may download it either way. A listing with no source world gives
//!    only what players get, and Studio says the server-only services are
//!    missing.
//!
//! A downloaded copy is never linked to the listing: publishing it creates a
//! new listing, and the original is untouched. (The Publish dialog reads its
//! settings, Share Source among them, from a `publish.toml` a copy does not
//! have, so a linked copy could change the listing without asking.) Only the
//! Universe the listing was published from updates it.
//!
//! The download waits for the saved sign-in to finish restoring, so the
//! author's token goes with it.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use bevy::ecs::message::Messages;
use bevy::prelude::*;
use eustress_echk::{content_hash, is_content_hash, is_safe_record_path, Record, WorldManifest, MAX_CHUNK_BYTES};

use crate::notifications::NotificationManager;
use crate::ui::FileEvent;

const EDIT_SCHEME: &str = "eustress://edit/";
/// A manifest names at most 65,536 chunks at about 160 bytes each.
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
/// Where a copy goes when its listing has no usable name.
const FALLBACK_NAME: &str = "Gallery Simulation";
/// How long Edit waits for the saved sign-in to finish restoring before it
/// downloads signed out. The restore asks the API once.
const SIGN_IN_WAIT: Duration = Duration::from_secs(20);

/// The simulation id in `eustress://edit/<id>`. Ids are UUIDs, so anything
/// but hex digits and dashes is refused rather than put in a URL.
pub fn edit_target(link: &str) -> Option<String> {
    let id = link.strip_prefix(EDIT_SCHEME)?.trim_end_matches('/');
    let valid = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    valid.then(|| id.to_ascii_lowercase())
}

/// The Space on this computer already published as listing `sim_id` on the
/// API in use: a Space whose own `.eustress/sync.toml` names it, else the
/// first Space of a Universe whose `sync.toml` does. Publishing writes both,
/// so the Space it names is the one published from. When several name it,
/// the one published most recently wins.
pub fn local_space_for(workspace: &Path, sim_id: &str) -> Option<PathBuf> {
    let mut best: Option<(SystemTime, PathBuf)> = None;
    let universes = std::fs::read_dir(workspace).ok()?.flatten().map(|e| e.path()).filter(|p| super::is_universe_dir(p));
    for universe in universes {
        let mut found_space = false;
        for tier in ["Spaces", "spaces"] {
            let Ok(spaces) = std::fs::read_dir(universe.join(tier)) else { continue };
            for space in spaces.flatten().map(|e| e.path()).filter(|p| p.is_dir() && super::looks_like_space_root(p)) {
                if let Some(when) = published_as(&space, sim_id) {
                    found_space = true;
                    keep_newest(&mut best, when, space);
                }
            }
        }
        if !found_space {
            if let (Some(when), Some(space)) = (published_as(&universe, sim_id), super::first_space_root_in_universe(&universe)) {
                keep_newest(&mut best, when, space);
            }
        }
    }
    best.map(|(_, space)| space)
}

/// When `dir`'s `.eustress/sync.toml` was written, if it names listing `sim_id`.
fn published_as(dir: &Path, sim_id: &str) -> Option<SystemTime> {
    let path = dir.join(".eustress").join("sync.toml");
    let sync = eustress_common::load_toml_file::<eustress_common::SyncManifest>(&path).ok()?;
    let id = sync.remote.listing_id()?;
    id.eq_ignore_ascii_case(sim_id)
        .then(|| std::fs::metadata(&path).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH))
}

fn keep_newest(best: &mut Option<(SystemTime, PathBuf)>, when: SystemTime, space: PathBuf) {
    if best.as_ref().map_or(true, |(seen, _)| when > *seen) {
        *best = Some((when, space));
    }
}

/// What a download wrote.
struct Downloaded {
    /// The Space to open.
    space: PathBuf,
    /// The listing's name, for the notice.
    name: String,
    /// The world had no source, so the server-only services are missing.
    player_world_only: bool,
    /// The viewer is the listing's author, whose original Universe is not on
    /// this computer.
    author: bool,
}

/// A download under way on its own thread.
#[derive(Resource)]
struct PendingEdit {
    sim_id: String,
    rx: Mutex<mpsc::Receiver<Result<Downloaded, String>>>,
}

pub struct GalleryEditPlugin;

impl Plugin for GalleryEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (start_edit, finish_edit).chain());
    }
}

/// Start the download the launch arguments asked for, once, after the saved
/// sign-in has finished restoring (or [`SIGN_IN_WAIT`] has passed).
fn start_edit(
    mut commands: Commands,
    args: Option<Res<crate::startup::StartupArgs>>,
    auth: Option<Res<crate::auth::AuthState>>,
    mut started: Local<bool>,
    mut waiting_since: Local<Option<Instant>>,
    mut notifications: Option<ResMut<NotificationManager>>,
) {
    if *started {
        return;
    }
    let Some(args) = args else {
        *started = true;
        return;
    };
    let Some(sim_id) = args.gallery_edit.clone() else {
        *started = true;
        return;
    };
    // Opened at launch: the user's own Space, already on this computer.
    if let Some(space) = args.gallery_edit_local.as_ref() {
        *started = true;
        info!("gallery edit: simulation {sim_id} is published from {}; opened it", space.display());
        if let Some(n) = notifications.as_mut() {
            n.success(format!("Opened your Space, the one published as this listing: {}", space.display()));
        }
        return;
    }
    // The saved sign-in restores on a thread at startup. Without waiting for
    // it, the author's token is never sent and their own listing reads as
    // missing.
    let restoring = auth.as_ref().is_some_and(|a| {
        matches!(a.status, crate::auth::AuthStatus::Validating | crate::auth::AuthStatus::LoggingIn)
    });
    if restoring && waiting_since.get_or_insert_with(Instant::now).elapsed() < SIGN_IN_WAIT {
        return;
    }
    *started = true;
    // A listing the viewer may play needs no sign-in; the token lets an
    // author open their own listing, shared or not, before review approves it.
    let token = auth.as_ref().and_then(|a| a.token.clone());
    let user_id = auth.as_ref().and_then(|a| a.user.as_ref().map(|u| u.id.clone()));
    let workspace = super::workspace_root();
    let (tx, rx) = mpsc::channel();
    let id = sim_id.clone();
    let spawned = std::thread::Builder::new()
        .name("gallery-edit".into())
        .spawn(move || {
            let _ = tx.send(download(&id, token.as_deref(), user_id.as_deref(), &workspace));
        });
    if let Err(e) = spawned {
        error!("gallery edit: could not start the download: {e}");
        return;
    }
    info!("gallery edit: downloading simulation {sim_id}");
    if let Some(n) = notifications.as_mut() {
        n.info("Downloading the simulation to open a copy for editing...");
    }
    commands.insert_resource(PendingEdit { sim_id, rx: Mutex::new(rx) });
}

/// Open the copy once it is written, or say why there is none.
fn finish_edit(
    mut commands: Commands,
    pending: Option<Res<PendingEdit>>,
    files: Option<ResMut<Messages<FileEvent>>>,
    mut notifications: Option<ResMut<NotificationManager>>,
) {
    let Some(pending) = pending else { return };
    let result = {
        let Ok(rx) = pending.rx.lock() else { return };
        match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("the download stopped without a result".into()),
        }
    };
    commands.remove_resource::<PendingEdit>();
    match result {
        Ok(done) => {
            info!(
                "gallery edit: simulation {} copied to {} (player world only: {}, author: {})",
                pending.sim_id,
                done.space.display(),
                done.player_world_only,
                done.author
            );
            if let Some(n) = notifications.as_mut() {
                // The author's Universe was not found on this computer, so
                // this is a copy, and publishing it makes a new listing.
                let whose = if done.author {
                    "Your original Universe is not on this computer, so this is a copy; publishing it creates a new listing."
                } else {
                    "Publishing it creates a listing of your own."
                };
                if done.player_world_only {
                    n.warning(format!(
                        "Opened a copy of {} at {}. It was published without Share Source, so the server-only services \
                         (ServerScriptService, ServerStorage) are not in it. {whose}",
                        done.name,
                        done.space.display()
                    ));
                } else {
                    n.success(format!("Opened a copy of {} at {}. {whose}", done.name, done.space.display()));
                }
            }
            if let Some(mut files) = files {
                files.write(FileEvent::OpenRecent(done.space));
            }
        }
        Err(e) => {
            error!("gallery edit: simulation {}: {e}", pending.sim_id);
            if let Some(n) = notifications.as_mut() {
                n.error(format!("Could not open the simulation for editing: {e}"));
            }
        }
    }
}

/// Download a listing's world and write it as a new Universe in `workspace`.
/// `user_id` is the signed-in account, which may download its own listing
/// whether or not it shares its source. Blocking; runs off the Bevy thread.
fn download(sim_id: &str, token: Option<&str>, user_id: Option<&str>, workspace: &Path) -> Result<Downloaded, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(300))
        .build();
    let get = |path: &str| {
        let mut request = agent.get(&eustress_common::api_base::api_url(path));
        if let Some(token) = token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        request.call()
    };

    let listing: serde_json::Value = match get(&format!("/api/simulations/{sim_id}")) {
        Ok(resp) => resp.into_json().map_err(|e| format!("read the listing: {e}"))?,
        Err(ureq::Error::Status(404, _)) => return Err("it does not exist, or it is not available to you".into()),
        Err(e) => return Err(format!("read the listing: {e}")),
    };
    let author = is_author(&listing, user_id);
    if listing["is_open_source"].as_bool() != Some(true) && !author {
        return Err("its author has not shared its source".into());
    }

    // The source world: what players get plus the server-only services. A
    // listing published without Share Source, or before sources existed, has
    // only the player world.
    let (manifest_bytes, player_world_only) = match get(&format!("/api/simulations/{sim_id}/world/source")) {
        Ok(resp) => (read_limited(resp, MAX_MANIFEST_BYTES, "the source manifest")?, false),
        Err(ureq::Error::Status(status, _)) if status == 404 || status == 403 => {
            warn!("gallery edit: simulation {sim_id} has no source world ({status}); copying what players get");
            match get(&format!("/api/simulations/{sim_id}/world/manifest")) {
                Ok(resp) => (read_limited(resp, MAX_MANIFEST_BYTES, "the world manifest")?, true),
                Err(ureq::Error::Status(409, _)) => {
                    return Err("it was published before worlds were chunked; its author can publish it again".into())
                }
                Err(ureq::Error::Status(404, _)) => return Err("nothing has been published to it yet".into()),
                Err(e) => return Err(format!("read the world manifest: {e}")),
            }
        }
        Err(e) => return Err(format!("read the source manifest: {e}")),
    };
    let manifest: WorldManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| format!("the world manifest is unreadable: {e}"))?;
    manifest.validate().map_err(|e| format!("the world manifest was refused: {e}"))?;

    let mut chunks: HashMap<String, Vec<u8>> = HashMap::new();
    for hash in manifest.hashes() {
        if !is_content_hash(&hash) {
            return Err(format!("bad chunk name {hash:?}"));
        }
        let resp = get(&format!("/api/simulations/{sim_id}/world/chunks/{hash}"))
            .map_err(|e| format!("download chunk {hash}: {e}"))?;
        let bytes = read_limited(resp, MAX_CHUNK_BYTES, "a chunk")?;
        if content_hash(&bytes) != hash {
            return Err(format!("chunk {hash} does not match its content hash"));
        }
        chunks.insert(hash, bytes);
    }
    let world = eustress_networking::session::assemble_world(&manifest, |h| chunks.get(h).map(|b| b.as_slice()), [0.0; 3])?;

    let name = listing["name"].as_str().map(str::to_owned).unwrap_or_else(|| world.universe.clone());
    let universe = unique_dir(workspace, &folder_name(&name));
    for (space, records) in &world.spaces {
        write_records(&universe.join("Spaces").join(space), records)?;
    }
    write_records(&universe, &world.assets)?;
    let start = if world.start_space.is_empty() {
        world.spaces.first().map(|(n, _)| n.clone()).unwrap_or_default()
    } else {
        world.start_space.clone()
    };
    Ok(Downloaded { space: universe.join("Spaces").join(start), name, player_world_only, author })
}

/// Whether the signed-in account (`user_id`) wrote this listing.
fn is_author(listing: &serde_json::Value, user_id: Option<&str>) -> bool {
    match (listing["author_id"].as_str(), user_id) {
        (Some(author), Some(user)) => !author.is_empty() && author == user,
        _ => false,
    }
}

fn read_limited(resp: ureq::Response, limit: u64, what: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    resp.into_reader()
        .take(limit + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("read {what}: {e}"))?;
    if out.len() as u64 > limit {
        return Err(format!("{what} is over the {limit} byte limit"));
    }
    Ok(out)
}

/// A listing name made safe as one folder name on every platform.
fn folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { ' ' } else { c })
        .collect();
    let trimmed: String = cleaned.trim().trim_end_matches(['.', ' ']).chars().take(80).collect();
    let reserved = matches!(
        trimmed.to_ascii_uppercase().as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "LPT1" | "LPT2" | "LPT3"
    );
    if trimmed.is_empty() || trimmed.starts_with('.') || reserved {
        FALLBACK_NAME.to_string()
    } else {
        trimmed
    }
}

/// `workspace/name`, or `workspace/name (2)`, `(3)`... when that is taken:
/// a copy never lands on top of an existing Universe.
fn unique_dir(workspace: &Path, name: &str) -> PathBuf {
    let first = workspace.join(name);
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| workspace.join(format!("{name} ({n})")))
        .find(|p| !p.exists())
        .expect("an unbounded range always finds a free name")
}

/// What a dedicated server downloaded for one listing.
pub struct DownloadedHostWorld {
    /// The Space folder to open.
    pub space_root: PathBuf,
    /// The source world (server services included) was what was written.
    pub from_source: bool,
    /// `.server.luau` files in the written Universe.
    pub server_script_count: usize,
    /// One sentence for the status window and the log.
    pub note: String,
}

/// Why a hosted session does not start on a listing without a source world.
pub const NO_SOURCE_FOR_SESSION: &str = "this listing has no source world this hosted session can read, so the session does not start: it would run with no server scripts. The author publishes the listing again from Studio, which commits its source world.";

/// Whether a server holding `token` may run the player world when it cannot
/// read the listing's source world. A hosted session (`eshs_`, started by the
/// fleet) may not: a session with no server scripts is half a simulation, so
/// it does not start. The author's own server (their session or their
/// `esrv_`) may, and its status says that server scripts will not run.
pub fn player_world_fallback_allowed(token: Option<&str>) -> bool {
    !token.is_some_and(|t| t.trim().starts_with("eshs_"))
}

/// Download a listing's world into `universe_dir` for a dedicated server.
///
/// The source world is requested first: it is the player world plus the
/// server-only services, and it is what carries server Luau scripts. Studio
/// commits it on every publish; it is private unless the author shares it.
/// The author's session, their server credential (`esrv_…`), and a session
/// credential the fleet minted for this listing (`eshs_…`) can read it. When
/// the listing has no source world this server can read, an author's server
/// writes the player world instead and `note` says that server scripts are not
/// in it; a hosted session refuses ([`NO_SOURCE_FOR_SESSION`]).
///
/// Blocking. `EUSTRESS_API_URL` must already name the API this listing is on.
/// Writes `universe_dir/.eustress/sync.toml` so the host's heartbeat names
/// this listing.
pub fn download_for_host(sim_id: &str, token: Option<&str>, universe_dir: &Path) -> Result<DownloadedHostWorld, String> {
    if !sim_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') || sim_id.is_empty() || sim_id.len() > 64 {
        return Err("the simulation id is not a listing id".into());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(300))
        .build();
    let get = |path: &str| {
        let mut request = agent.get(&eustress_common::api_base::api_url(path));
        if let Some(token) = token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        request.call()
    };

    let (manifest_bytes, from_source) = match get(&format!("/api/simulations/{sim_id}/world/source")) {
        Ok(resp) => (read_limited(resp, MAX_MANIFEST_BYTES, "the source manifest")?, true),
        Err(ureq::Error::Status(status, _)) if status == 404 || status == 403 => {
            if !player_world_fallback_allowed(token) {
                return Err(NO_SOURCE_FOR_SESSION.into());
            }
            match get(&format!("/api/simulations/{sim_id}/world/manifest")) {
                Ok(resp) => (read_limited(resp, MAX_MANIFEST_BYTES, "the world manifest")?, false),
                Err(ureq::Error::Status(409, _)) => {
                    return Err("this listing was published as a .pak, before worlds were chunked. Publish it again from Studio.".into());
                }
                Err(ureq::Error::Status(404, _)) => {
                    return Err("nothing has been published to this listing yet".into());
                }
                Err(e) => return Err(format!("read the world manifest: {e}")),
            }
        }
        Err(e) => return Err(format!("read the source manifest: {e}")),
    };
    let manifest: WorldManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| format!("the world manifest is unreadable: {e}"))?;
    manifest.validate().map_err(|e| format!("the world manifest was refused: {e}"))?;

    if universe_dir.exists() {
        std::fs::remove_dir_all(universe_dir).map_err(|e| format!("clear {}: {e}", universe_dir.display()))?;
    }
    std::fs::create_dir_all(universe_dir).map_err(|e| format!("create {}: {e}", universe_dir.display()))?;

    let mut chunks: HashMap<String, Vec<u8>> = HashMap::new();
    for hash in manifest.hashes() {
        if !is_content_hash(&hash) {
            return Err(format!("bad chunk name {hash:?}"));
        }
        let resp = get(&format!("/api/simulations/{sim_id}/world/chunks/{hash}"))
            .map_err(|e| format!("download chunk {hash}: {e}"))?;
        let bytes = read_limited(resp, MAX_CHUNK_BYTES, "a chunk")?;
        if content_hash(&bytes) != hash {
            return Err(format!("chunk {hash} does not match its content hash"));
        }
        chunks.insert(hash, bytes);
    }
    let world = eustress_networking::session::assemble_world(&manifest, |h| chunks.get(h).map(|b| b.as_slice()), [0.0; 3])?;
    for (space, records) in &world.spaces {
        write_records(&universe_dir.join("Spaces").join(space), records)?;
    }
    write_records(universe_dir, &world.assets)?;
    remember_listing(universe_dir, sim_id)?;

    let server_script_count = count_server_scripts(universe_dir);
    let note = if from_source {
        format!(
            "Loaded the published source world. {server_script_count} server Luau script{} on disk.",
            if server_script_count == 1 { "" } else { "s" }
        )
    } else {
        "The source world is not available for this listing, so this is the player world: server scripts it does not carry will not run. Publish the listing again from Studio, which commits its source world, and start again.".to_string()
    };
    let start = if world.start_space.is_empty() {
        world.spaces.first().map(|(n, _)| n.clone()).unwrap_or_default()
    } else {
        world.start_space.clone()
    };
    Ok(DownloadedHostWorld {
        space_root: universe_dir.join("Spaces").join(start),
        from_source,
        server_script_count,
        note,
    })
}

/// Remember `sim_id` as this Universe's listing on the API in use, so the
/// host heartbeat posts to that listing.
fn remember_listing(universe: &Path, sim_id: &str) -> Result<(), String> {
    let dir = universe.join(".eustress");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let mut remote = eustress_common::RemoteState {
        provider: "eustress".into(),
        project_id: None,
        experience_id: None,
        bucket: None,
        editable: false,
        open_source: false,
        listings: std::collections::BTreeMap::new(),
    };
    remote.set_listing_id(Some(sim_id.to_string()));
    let manifest = eustress_common::SyncManifest {
        sync: eustress_common::SyncState {
            mode: "local".into(),
            auto_sync: false,
            last_sync: None,
            pending_uploads: 0,
            pending_downloads: 0,
            offline_changes: 0,
        },
        remote,
    };
    eustress_common::save_toml_file(&manifest, &dir.join("sync.toml")).map_err(|e| format!("write the listing id: {e}"))
}

fn count_server_scripts(root: &Path) -> usize {
    fn walk(dir: &Path, count: &mut usize) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, count);
            } else if path.extension().and_then(|e| e.to_str()) == Some("luau")
                && path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".server.luau"))
            {
                *count += 1;
            }
        }
    }
    let mut count = 0;
    walk(root, &mut count);
    count
}

fn write_records(root: &Path, records: &[Record]) -> Result<(), String> {
    for (path, bytes) in records {
        // Checked again at the one place bytes reach the disk.
        if !is_safe_record_path(path) {
            return Err(format!("refusing to write {path:?} outside the Universe folder"));
        }
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, bytes).map_err(|e| format!("write {}: {e}", target.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_simulation_id_is_taken_from_the_link() {
        let id = "11111111-2222-3333-4444-555555555555";
        assert_eq!(edit_target(&format!("eustress://edit/{id}")).as_deref(), Some(id));
        assert_eq!(edit_target(&format!("eustress://edit/{}/", id.to_uppercase())).as_deref(), Some(id));
        for bad in ["eustress://edit/", "eustress://edit/../x", "eustress://edit/abc?x=1", "eustress://play/abc", "https://x"] {
            assert_eq!(edit_target(bad), None, "accepted {bad}");
        }
    }

    #[test]
    fn a_hosted_session_never_falls_back_to_the_player_world() {
        let session = format!("eshs_{}", "ab".repeat(32));
        assert!(!player_world_fallback_allowed(Some(&session)), "a fleet session refuses to run without server scripts");
        assert!(!player_world_fallback_allowed(Some(&format!(" {session}"))), "whitespace does not hide the credential");
        // The author's own server keeps the fallback, and says so in its note.
        assert!(player_world_fallback_allowed(Some(&format!("esrv_{}", "cd".repeat(32)))));
        assert!(player_world_fallback_allowed(Some("author-session-jwt")));
        assert!(player_world_fallback_allowed(None));
        assert!(NO_SOURCE_FOR_SESSION.contains("source world"));
    }

    #[test]
    fn a_listing_name_becomes_one_safe_folder() {
        assert_eq!(folder_name("Vehicle Simulator"), "Vehicle Simulator");
        assert_eq!(folder_name("a/b\\c:d"), "a b c d");
        assert_eq!(folder_name("..."), FALLBACK_NAME);
        assert_eq!(folder_name("con"), FALLBACK_NAME);
        assert_eq!(folder_name("  Name. "), "Name");
    }

    /// A Universe folder under `workspace`, its Spaces, and the listing each
    /// one's `.eustress/sync.toml` names (`None`: no sync.toml).
    fn universe(workspace: &Path, name: &str, own: Option<&str>, spaces: &[(&str, Option<&str>)]) -> PathBuf {
        fn sync(dir: &Path, id: &str) {
            let mut remote = eustress_common::RemoteState {
                provider: "eustress".into(),
                project_id: None,
                experience_id: None,
                bucket: None,
                editable: false,
                open_source: false,
                listings: std::collections::BTreeMap::new(),
            };
            remote.set_listing_id(Some(id.to_string()));
            let manifest = eustress_common::SyncManifest {
                sync: eustress_common::SyncState {
                    mode: "local".into(),
                    auto_sync: false,
                    last_sync: None,
                    pending_uploads: 0,
                    pending_downloads: 0,
                    offline_changes: 0,
                },
                remote,
            };
            std::fs::create_dir_all(dir.join(".eustress")).unwrap();
            eustress_common::save_toml_file(&manifest, &dir.join(".eustress").join("sync.toml")).unwrap();
        }
        let root = workspace.join(name);
        for (space, id) in spaces {
            let dir = root.join("Spaces").join(space);
            std::fs::create_dir_all(dir.join("Workspace")).unwrap();
            if let Some(id) = id {
                sync(&dir, id);
            }
        }
        if let Some(id) = own {
            sync(&root, id);
        }
        root
    }

    #[test]
    fn edit_finds_the_space_its_listing_was_published_from() {
        let ws = std::env::temp_dir().join(format!("eustress_gallery_edit_local_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        let a = "aaaaaaaa-0000-0000-0000-000000000001";
        let b = "bbbbbbbb-0000-0000-0000-000000000002";
        let c = "cccccccc-0000-0000-0000-000000000003";
        // The Space that names the listing wins over the Universe's first Space.
        let one = universe(&ws, "One", Some(a), &[("Lobby", None), ("Main", Some(a))]);
        // Only the Universe names it: its first Space opens.
        let two = universe(&ws, "Two", Some(b), &[("Start", None)]);
        universe(&ws, "Three", None, &[("Other", Some("dddddddd-0000-0000-0000-000000000004"))]);

        assert_eq!(local_space_for(&ws, a), Some(one.join("Spaces").join("Main")));
        assert_eq!(local_space_for(&ws, &a.to_ascii_uppercase()), Some(one.join("Spaces").join("Main")));
        assert_eq!(local_space_for(&ws, b), Some(two.join("Spaces").join("Start")));
        assert_eq!(local_space_for(&ws, c), None, "no Universe names this listing");
        assert_eq!(local_space_for(&ws.join("missing"), a), None);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn only_the_author_may_download_a_listing_that_keeps_its_source() {
        let listing = serde_json::json!({ "author_id": "user-1", "is_open_source": false });
        assert!(is_author(&listing, Some("user-1")));
        assert!(!is_author(&listing, Some("user-2")));
        assert!(!is_author(&listing, None), "signed out is never the author");
        assert!(!is_author(&serde_json::json!({ "author_id": "" }), Some("")));
        assert!(!is_author(&serde_json::json!({}), Some("user-1")));
    }

    #[test]
    fn a_copy_never_lands_on_an_existing_universe() {
        let root = std::env::temp_dir().join(format!("eustress_gallery_edit_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("World")).unwrap();
        std::fs::create_dir_all(root.join("World (2)")).unwrap();
        assert_eq!(unique_dir(&root, "World"), root.join("World (3)"));
        assert_eq!(unique_dir(&root, "Fresh"), root.join("Fresh"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
