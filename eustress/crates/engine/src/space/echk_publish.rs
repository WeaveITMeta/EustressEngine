//! # Publishing a world as `.echk`
//!
//! Publish sends a Universe to the gallery as content-addressed chunks:
//!
//! 1. Export every Space with [`super::echk_export`]: the open Space from its
//!    live database, the others from their own databases (or their folders
//!    when they have none), plus the Universe's shared `assets/`.
//! 2. `POST /api/simulations/{id}/world/begin` with the manifest. The API
//!    answers with the chunks it does not already hold.
//! 3. `PUT /world/chunks/{hash}` for each of those, and nothing else, so a
//!    republish that changed one corner of a map uploads that corner.
//! 4. `POST /world/commit`: the listing now plays this manifest, and goes back
//!    to review when anything in it changed.
//!
//! A Space-only publish exports that one Space and swaps it into the manifest
//! the listing already plays, keeping every other Space as published.
//!
//! Players never receive the server-only services (ServerScriptService and
//! ServerStorage), as in Roblox. When the author turns on Share Source, a
//! second manifest, the source world, adds them for the gallery's Edit;
//! otherwise they never leave the machine. A publish refuses a webhook URL in
//! anything the public can download.
//!
//! The listing is created once per API. Its id is kept in the Universe's
//! `.eustress/sync.toml` (`RemoteState::listing_id`, which keeps production's
//! and each test API's apart) the moment the API returns it, so every later
//! publish, a Space-only one included, and a retry after a failed upload, all
//! land in the same listing. The Worker side of each call is
//! `infrastructure/cloudflare/api/src/world.mjs`.
//!
//! A publish counts as done, and an unchanged one is skipped, only once its
//! review was requested ([`record_reviewed`]): a publish whose review never
//! went out (Studio closed, or the request failed) goes up again next time and
//! asks again.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use eustress_common::project_manifest::{CoverChoice, PublishTarget};
use eustress_echk::{ChunkEntry, WorldManifest};
use eustress_worlddb::WorldDb;

use super::echk_export::{export_world, Audience, Destination, ExportedWorld, SpaceInput, SpaceSource, SAVE_SETTLE};

/// Largest chunk the API takes in one request (`MAX_UPLOAD_CHUNK_BYTES` in
/// `world.mjs`, under the Worker's 100 MB request limit).
pub const MAX_UPLOAD_CHUNK_BYTES: u64 = 95 * 1024 * 1024;
/// What an unchanged publish reports instead of uploading.
pub const NO_CHANGES: &str = "No changes since the last publish";

/// Name, description, tags and visibility, as the Publish dialog set them.
#[derive(Debug, Clone)]
pub struct Listing {
    pub name: String,
    pub description: String,
    pub is_public: bool,
    /// "Share Source": the gallery offers Edit (open in Studio) on the listing.
    pub open_source: bool,
    /// The listing's tags (`docs/networking/LISTING_TAGS.md`), normalized and
    /// checked by the dialog: `Some` sends them (an empty list asks the review
    /// to choose them), `None` leaves the listing's tags as they are.
    pub tags: Option<Vec<String>>,
    /// The cover: kept, replaced or removed with a whole-Universe publish,
    /// before its commit, so the review the publish requests covers it too.
    pub cover: CoverChoice,
}

impl Listing {
    fn to_json(&self) -> serde_json::Value {
        let mut json = serde_json::json!({
            "name": self.name,
            "description": self.description,
            "is_public": self.is_public,
            "open_source": self.open_source,
        });
        if let Some(tags) = &self.tags {
            json["tags"] = serde_json::json!(tags);
        }
        json
    }
}

/// What an export needs, gathered on the main thread where the World is.
pub struct ExportPlan {
    pub universe: String,
    pub universe_root: PathBuf,
    /// The Space open in Studio. Players start in it.
    pub start_space: String,
    /// The folder of the Space open in Studio.
    pub space_root: PathBuf,
    /// Every Space of the Universe, by folder name.
    pub spaces: Vec<(String, PathBuf)>,
    active_db: Option<Arc<dyn WorldDb>>,
    big_space_threshold: usize,
    /// A save just ran, so the export first waits for the tree to hold it.
    after_save: bool,
    /// The start Space's `Players.MaxPlayers`: the size the listing shows
    /// (`docs/networking/MAX_PLAYERS.md`).
    pub max_players: u16,
}

/// Gather an export of the Universe that holds `space_root`. Call after
/// saving, with `after_save` set.
pub fn plan_export(world: &World, space_root: &Path, after_save: bool) -> Result<ExportPlan, String> {
    let still_opening = world
        .get_resource::<super::world_db_plugin::PendingWorldDbOpen>()
        .is_some_and(|p| p.0.is_some());
    if still_opening {
        return Err("The Space is still opening. Publish again in a moment.".into());
    }
    let universe_root = super::universe_root_for_path(space_root).unwrap_or_else(|| space_root.to_path_buf());
    let start_space = folder_name(space_root);
    let mut spaces = list_spaces(&universe_root);
    if !spaces.iter().any(|(name, _)| *name == start_space) {
        spaces.push((start_space.clone(), space_root.to_path_buf()));
    }
    let big_space_threshold = world
        .get_resource::<super::residency::ResidencyConfig>()
        .map(|c| c.big_space_threshold)
        .unwrap_or(100_000);
    Ok(ExportPlan {
        universe: folder_name(&universe_root),
        universe_root,
        start_space,
        space_root: space_root.to_path_buf(),
        spaces,
        active_db: super::active_db::db_arc(),
        big_space_threshold,
        after_save,
        max_players: crate::multiplayer::space_max_players(world).players,
    })
}

/// The Space folders under `<universe>/Spaces/`, sorted by name.
fn list_spaces(universe_root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(universe_root.join("Spaces")) else { return Vec::new() };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && super::looks_like_space_root(p))
        .map(|p| (folder_name(&p), p))
        .filter(|(name, _)| !name.starts_with('.'))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn folder_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".to_string())
}

/// Export the planned Spaces, or only `only`, for `audience`: what players
/// receive (with the Universe's assets), or the server-only services. A
/// publish leaves this machine, so a key in it refuses the export
/// ([`Destination::Publish`]).
pub fn export(plan: &ExportPlan, only: Option<&str>, audience: Audience) -> Result<ExportedWorld, String> {
    let inputs: Vec<SpaceInput> = plan
        .spaces
        .iter()
        .filter(|(name, _)| only.map_or(true, |o| o == name.as_str()))
        .map(|(name, folder)| space_input(plan, name, folder))
        .collect();
    if inputs.is_empty() {
        return Err(format!("No Space to publish in {}", plan.universe_root.display()));
    }
    let start = only.unwrap_or(&plan.start_space);
    // A Space opened outside any Universe is its own root; its assets are
    // already among its files.
    let standalone = plan.spaces.iter().any(|(_, folder)| *folder == plan.universe_root);
    let assets = if standalone { None } else { Some(plan.universe_root.as_path()) };
    let out_root = plan.universe_root.join(".eustress").join("publish");
    export_world(&plan.universe, inputs, start, assets, &out_root, plan.big_space_threshold, audience, Destination::Publish)
}

/// Where one Space's content is read from: the open Space's live database;
/// another Space's own database when it has a seeded one; otherwise its folder.
fn space_input(plan: &ExportPlan, name: &str, folder: &Path) -> SpaceInput {
    let db_source = |db: Arc<dyn WorldDb>| SpaceInput {
        name: name.to_string(),
        source: SpaceSource::Db(db),
        folder: Some(folder.to_path_buf()),
        terrain: None,
    };
    if name == plan.start_space {
        if let Some(db) = &plan.active_db {
            return db_source(db.clone());
        }
    }
    let db_dir = folder.join("world.fjalldb");
    if db_dir.is_dir() {
        match eustress_worlddb::backend::open(&db_dir) {
            Ok(db) if !db.tree_is_empty().unwrap_or(true) => return db_source(db),
            Ok(_) => {}
            // Another window may hold it. The dual model keeps the TOML
            // hierarchy on disk, so the folder is the fallback.
            Err(e) => warn!("publish: the database of {name} could not be opened ({e}); publishing its folder"),
        }
    }
    SpaceInput { name: name.to_string(), source: SpaceSource::Disk(folder.to_path_buf()), folder: None, terrain: None }
}

// ─────────────────────────────────────────────────────────────────────────────
// The API
// ─────────────────────────────────────────────────────────────────────────────

/// An API call that failed: its HTTP status (0 when no response came back),
/// the error it gave, and its `code` when it named one.
#[derive(Debug)]
struct ApiError {
    status: u16,
    message: String,
    code: Option<String>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.status == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{} (HTTP {})", self.message, self.status)
        }
    }
}

impl ApiError {
    /// The kept listing id no longer names one of this author's listings.
    fn listing_gone(&self) -> bool {
        self.status == 404 || (self.status == 403 && self.code.is_none())
    }
}

struct Api {
    agent: ureq::Agent,
    auth: String,
}

impl Api {
    fn new(token: &str) -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(30))
                .timeout_read(Duration::from_secs(300))
                .build(),
            auth: format!("Bearer {token}"),
        }
    }

    /// `path` on the API in use (`eustress_common::api_base`).
    fn url(path: &str) -> String {
        eustress_common::api_base::api_url(path)
    }

    fn post_json(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value, ApiError> {
        let resp = answer(self.agent.post(&Self::url(path)).set("Authorization", &self.auth).send_json(body))?;
        resp.into_json().map_err(|e| ApiError { status: 0, message: format!("read the answer to {path}: {e}"), code: None })
    }

    fn get_text(&self, path: &str) -> Result<String, ApiError> {
        let resp = answer(self.agent.get(&Self::url(path)).set("Authorization", &self.auth).call())?;
        resp.into_string().map_err(|e| ApiError { status: 0, message: format!("read {path}: {e}"), code: None })
    }

    fn put_bytes(&self, path: &str, bytes: &[u8]) -> Result<(), ApiError> {
        answer(
            self.agent
                .put(&Self::url(path))
                .set("Authorization", &self.auth)
                .set("Content-Type", "application/octet-stream")
                .send_bytes(bytes),
        )
        .map(|_| ())
    }

    fn delete(&self, path: &str) -> Result<(), ApiError> {
        answer(self.agent.delete(&Self::url(path)).set("Authorization", &self.auth).call()).map(|_| ())
    }
}

fn answer(result: Result<ureq::Response, ureq::Error>) -> Result<ureq::Response, ApiError> {
    match result {
        Ok(resp) => Ok(resp),
        Err(ureq::Error::Status(status, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            let parsed: Option<serde_json::Value> = serde_json::from_str(&body).ok();
            let message = parsed
                .as_ref()
                .and_then(|v| v["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| body.chars().take(200).collect());
            let code = parsed.as_ref().and_then(|v| v["code"].as_str().map(str::to_owned));
            Err(ApiError { status, message, code })
        }
        Err(e) => Err(ApiError { status: 0, message: e.to_string(), code: None }),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Publishing
// ─────────────────────────────────────────────────────────────────────────────

/// What a publish did.
#[derive(Debug, Clone)]
pub struct Published {
    pub sim_id: String,
    /// BLAKE3 of the manifest JSON: the content id the website manifest and
    /// the moderation dossier cite.
    pub publish_hash: String,
    pub chunks: usize,
    pub uploaded: usize,
    pub bytes: u64,
    pub uploaded_bytes: u64,
    /// What this publish sent (listing, world, source and listing text), for
    /// [`record_reviewed`] once its review has been requested.
    pub state: String,
    /// The listing's version after this publish: the commit's answer, or the
    /// listing's own for a cover-only update.
    pub version: Option<u32>,
    /// What this publish changed, as `CHANGELOG.md`
    /// (`docs/architecture/PUBLISH_CHANGELOG.md`).
    pub changelog: Option<String>,
    /// The change log is stored with the listing on the API.
    pub changelog_stored: bool,
    /// What the person should read once the publish is done, already worded:
    /// the notice when SoulService's own key was left out of the package, then
    /// each key warning the export raised. None quotes a key.
    pub notes: Vec<String>,
}

/// Where a Universe keeps its publish state.
fn state_dir(universe_root: &Path) -> PathBuf {
    universe_root.join(".eustress")
}

/// The listing a Universe publishes into on the API in use, if it has one.
pub fn known_listing(universe_root: &Path, space_root: &Path) -> Option<String> {
    [universe_root, space_root].iter().find_map(|root| {
        eustress_common::load_toml_file::<eustress_common::SyncManifest>(&state_dir(root).join("sync.toml"))
            .ok()
            .and_then(|s| s.remote.listing_id().map(str::to_string))
    })
}

/// Keep the listing id for the API in use in the Universe's `sync.toml`, and
/// the open Space's, which the Publish dialog reads.
pub fn remember_listing(universe_root: &Path, space_root: &Path, sim_id: &str) -> Result<(), String> {
    let mut roots = vec![universe_root];
    if space_root != universe_root {
        roots.push(space_root);
    }
    for root in roots {
        let dir = state_dir(root);
        std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        let path = dir.join("sync.toml");
        let mut sync = eustress_common::load_toml_file::<eustress_common::SyncManifest>(&path).unwrap_or_default();
        sync.remote.set_listing_id(Some(sim_id.to_string()));
        eustress_common::save_toml_file(&sync, &path).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(())
}

/// The listing a publish starts from: this Universe's own on the API in use
/// (`remembered`) for `Remembered`, none (so a new one) for `New`, the
/// chosen one for `Existing`.
fn first_listing(target: &PublishTarget, remembered: Option<String>) -> Option<String> {
    match target {
        PublishTarget::Remembered => remembered,
        PublishTarget::New => None,
        PublishTarget::Existing(id) => Some(id.trim().to_string()).filter(|id| !id.is_empty()),
    }
}

/// Upload or remove the listing's cover (`docs/networking/LISTING_COVER.md`).
fn apply_cover(api: &Api, id: &str, cover: &CoverChoice) -> Result<(), String> {
    let path = format!("/api/simulations/{id}/cover");
    match cover {
        CoverChoice::Keep => Ok(()),
        CoverChoice::Set(bytes) => api.put_bytes(&path, bytes).map_err(|e| cover_refused(&e, "Uploading the cover failed")),
        CoverChoice::Remove => api.delete(&path).map_err(|e| cover_refused(&e, "Removing the cover failed")),
    }
}

/// Why the API refused a cover: its own words for a refusal it explains (a
/// bad image states the rules; a listing in review says so), else `what`
/// with the error.
fn cover_refused(e: &ApiError, what: &str) -> String {
    let explained = e.code.is_some() || matches!(e.status, 400 | 409 | 413 | 415 | 422);
    if explained && !e.message.trim().is_empty() {
        e.message.clone()
    } else {
        format!("{what}: {e}")
    }
}

/// The API's own words when it refused the listing's fields (a tag, or the
/// player count): its message is written to be shown as it is.
fn listing_refused(e: &ApiError) -> Option<String> {
    matches!(e.code.as_deref(), Some("bad_tags" | "bad_max_players"))
        .then(|| e.message.clone())
        .filter(|m| !m.trim().is_empty())
}

/// Why the API refused an upload into a listing the author chose.
fn chosen_listing_refused(e: &ApiError) -> String {
    let server = eustress_common::api_base::api_label().map_or_else(|| "Eustress".to_string(), |(name, _)| name);
    match (e.status, e.code.as_deref()) {
        (404, _) | (403, None) => {
            format!("That listing isn't one of yours on {server}, so it can't be replaced. Nothing was published.")
        }
        (409, _) => "That listing is being reviewed and can't be replaced right now. Nothing was published.".to_string(),
        _ => format!("Starting the upload failed: {e}"),
    }
}

/// What the last publish whose review went out sent.
const REVIEWED_STATE: &str = ".last_publish_state";

/// The publish is done: its review was requested. From now on a publish that
/// sends exactly this again is skipped as unchanged.
pub fn record_reviewed(universe_root: &Path, state: &str) {
    write_state(universe_root, REVIEWED_STATE, state);
}

/// Whether `state` is what the last publish whose review went out sent.
fn unchanged(universe_root: &Path, state: &str) -> bool {
    read_state(universe_root, REVIEWED_STATE).as_deref() == Some(state)
}

fn read_state(universe_root: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(state_dir(universe_root).join(name)).ok().map(|s| s.trim().to_string())
}

fn write_state(universe_root: &Path, name: &str, value: &str) {
    let dir = state_dir(universe_root);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(name), value);
}

fn create_listing(api: &Api, listing: &Listing, spaces: &[String], max_players: u16) -> Result<String, String> {
    let mut body = serde_json::json!({
        "name": listing.name,
        "description": listing.description,
        // The start Space's Players.MaxPlayers, everyone counted.
        "max_players": max_players,
        // The author's intent; the gallery lists it only once review approves.
        "is_public": listing.is_public,
        "open_source": listing.open_source,
        "spaces": spaces,
    });
    if let Some(tags) = &listing.tags {
        body["tags"] = serde_json::json!(tags);
    }
    let answer = api
        .post_json("/api/simulations/publish", &body)
        .map_err(|e| listing_refused(&e).unwrap_or_else(|| format!("Creating the listing failed: {e}")))?;
    answer["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "The API created a listing but returned no id".to_string())
}

/// Publish the whole Universe, or with `space_only` just the open Space, into
/// the Universe's listing, creating it the first time.
///
/// `progress` receives a stage and a percentage between 0 and 100. Returns
/// `Err(NO_CHANGES)` when nothing changed since the last publish.
pub fn publish(
    plan: &ExportPlan,
    token: &str,
    listing: &Listing,
    space_only: bool,
    progress: &dyn Fn(&str, f32),
) -> Result<Published, String> {
    publish_to(plan, token, listing, space_only, &PublishTarget::Remembered, progress)
}

/// [`publish`] into the listing `target` names (Publish As,
/// `docs/architecture/PUBLISH_AS.md`). A listing the author chose
/// (`Existing`) becomes this Universe's listing on the API in use only once
/// the API accepts the upload into it, which it does only for the listing's
/// author and never while it is under review; a refusal says why, and never
/// creates another listing. Publish As is for the whole Universe.
pub fn publish_to(
    plan: &ExportPlan,
    token: &str,
    listing: &Listing,
    space_only: bool,
    target: &PublishTarget,
    progress: &dyn Fn(&str, f32),
) -> Result<Published, String> {
    if space_only && *target != PublishTarget::Remembered {
        return Err("Publish As publishes the whole Universe. Choose the whole Universe to publish into another listing.".into());
    }
    let api = Api::new(token);
    let chosen = matches!(target, PublishTarget::Existing(_));
    let mut sim_id = first_listing(target, known_listing(&plan.universe_root, &plan.space_root));
    if space_only && sim_id.is_none() {
        return Err("Publish the Universe first, then its Spaces one at a time.".into());
    }

    if plan.after_save {
        std::thread::sleep(SAVE_SETTLE);
    }
    let only = space_only.then_some(plan.start_space.as_str());
    progress("Baking the world...", 5.0);
    let play = export(plan, only, Audience::Players)?;
    // The server-only services leave this machine only for a listing whose
    // author shares its source, and then only to Edit.
    let server = if listing.open_source {
        progress("Baking the source...", 9.0);
        Some(export(plan, only, Audience::Server)?)
    } else {
        None
    };
    refuse_webhooks(&play, server.as_ref())?;
    // What the screen asks the person to read: SoulService's notice and any
    // key warning, returned with the result and logged.
    let notes = publish_notes(&play, server.as_ref());
    for note in &notes {
        warn!("publish: {note}");
    }

    let (manifest, source, published_before) = if space_only {
        let id = sim_id.as_deref().unwrap_or_default();
        let published = fetch_manifest(&api, id, "manifest")?.ok_or_else(|| {
            "This listing was published before .echk. Publish the whole Universe once, then Spaces can be updated one at a time."
                .to_string()
        })?;
        let before = published.clone();
        let manifest = swap_space(published, &play.manifest, &plan.start_space)?;
        let source = match &server {
            Some(server) => {
                let base = fetch_manifest(&api, id, "source")?.ok_or_else(|| {
                    "Share Source is new for this listing. Publish the whole Universe once so every Space's source goes up."
                        .to_string()
                })?;
                Some(swap_space(base, &source_of(&play.manifest, &server.manifest)?, &plan.start_space)?)
            }
            None => None,
        };
        (manifest, source, Some(before))
    } else {
        let source = server.as_ref().map(|s| source_of(&play.manifest, &s.manifest)).transpose()?;
        (play.manifest.clone(), source, None)
    };
    // Everything that goes up: the source world names every player chunk too.
    let upload = source.as_ref().unwrap_or(&manifest);
    let read_chunk = |hash: &str| {
        play.read_chunk(hash).or_else(|e| server.as_ref().map_or(Err(e), |s| s.read_chunk(hash)))
    };
    refuse_oversized(upload, &read_chunk)?;

    let manifest_json = serde_json::to_string(&manifest).map_err(|e| format!("Serializing the manifest failed: {e}"))?;
    let publish_hash = blake3::hash(manifest_json.as_bytes()).to_hex().to_string();
    let source_json = source
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| format!("Serializing the source manifest failed: {e}"))?;
    let upload_json = source_json.as_ref().unwrap_or(&manifest_json);
    // A Space-only publish sends only Share Source, which its dialog shows too.
    // A whole publish also sends the start Space's Players.MaxPlayers, so a
    // change to it alone is a change to publish.
    let listing_json = if space_only {
        serde_json::json!({ "open_source": listing.open_source })
    } else {
        let mut json = listing.to_json();
        json["max_players"] = plan.max_players.into();
        json
    };

    // Nothing to do when this exact world, source and listing text went up
    // last time and its review was requested.
    let source_hash = source_json.as_deref().map(|s| blake3::hash(s.as_bytes()).to_hex().to_string()).unwrap_or_default();
    let state = |id: &str| {
        blake3::hash(format!("{id}\n{publish_hash}\n{source_hash}\n{listing_json}").as_bytes()).to_hex().to_string()
    };
    // The world and listing text are as last published: nothing to do,
    // unless the cover changes, which goes up by itself.
    let cover = if space_only { CoverChoice::Keep } else { listing.cover.clone() };
    if let Some(id) = &sim_id {
        if unchanged(&plan.universe_root, &state(id)) {
            if cover == CoverChoice::Keep {
                return Err(NO_CHANGES.into());
            }
            let before = ListingBefore::of(&api, id, false);
            progress("Updating the cover...", 50.0);
            apply_cover(&api, id, &cover)?;
            let version = before.version();
            let changelog = ChangeLog {
                title: listing.name.clone(),
                version,
                sim_id: id.clone(),
                when: chrono::Local::now(),
                server: eustress_common::api_base::api_label(),
                listing: listing_changes(&before, listing, plan.max_players, &cover, Sent::CoverOnly, false),
                world: Vec::new(),
                totals: vec!["Only the cover changed; the world was not uploaded again.".to_string()],
            }
            .markdown();
            let changelog_stored = store_changelog(&api, id, version, &publish_hash, &changelog);
            let done = state(id);
            return Ok(Published {
                sim_id: id.clone(),
                publish_hash,
                chunks: upload.all_chunks().count(),
                uploaded: 0,
                bytes: manifest.download_bytes(),
                uploaded_bytes: 0,
                state: done,
                version,
                changelog: Some(changelog),
                changelog_stored,
                notes,
            });
        }
    }

    let space_names: Vec<String> = manifest.spaces.iter().map(|s| s.name.clone()).collect();
    let mut created = false;
    let id = match sim_id.take() {
        Some(id) => id,
        None => {
            progress("Creating the listing...", 12.0);
            let id = create_listing(&api, listing, &space_names, plan.max_players)?;
            // Kept before any upload, so a failed upload retries into this
            // listing instead of creating another.
            remember_listing(&plan.universe_root, &plan.space_root, &id)?;
            created = true;
            id
        }
    };

    progress("Comparing with the published world...", 15.0);
    let begin_body = serde_json::json!({ "manifest_json": upload_json });
    let (id, begin) = match api.post_json(&format!("/api/simulations/{id}/world/begin"), &begin_body) {
        Ok(begin) => {
            // The API took it: the chosen listing is the author's and open,
            // so it becomes this Universe's listing on this API.
            if chosen {
                remember_listing(&plan.universe_root, &plan.space_root, &id)?;
            }
            (id, begin)
        }
        Err(e) if chosen => return Err(chosen_listing_refused(&e)),
        Err(e) if e.listing_gone() && !created && !space_only => {
            warn!("publish: listing {id} is gone or not this account's ({e}); creating a new one");
            let fresh = create_listing(&api, listing, &space_names, plan.max_players)?;
            remember_listing(&plan.universe_root, &plan.space_root, &fresh)?;
            created = true;
            let begin = api
                .post_json(&format!("/api/simulations/{fresh}/world/begin"), &begin_body)
                .map_err(|e| format!("Starting the upload failed: {e}"))?;
            (fresh, begin)
        }
        Err(e) => return Err(format!("Starting the upload failed: {e}")),
    };

    let missing: Vec<String> = begin["missing"]
        .as_array()
        .map(|a| a.iter().filter_map(|h| h.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    let sizes: std::collections::HashMap<&str, u64> =
        upload.all_chunks().map(|c| (c.blake3.as_str(), c.size)).collect();
    let upload_bytes: u64 = missing.iter().filter_map(|h| sizes.get(h.as_str())).sum();
    let mut sent_bytes = 0u64;
    for (i, hash) in missing.iter().enumerate() {
        if !sizes.contains_key(hash.as_str()) {
            return Err(format!("The API asked for chunk {hash}, which this world does not name"));
        }
        // A Space-only publish holds only its own Space's chunks and the
        // assets; everything else must already be up.
        let bytes = read_chunk(hash.as_str()).map_err(|_| {
            format!("The published world is missing chunk {hash}, which this machine does not have. Publish the whole Universe.")
        })?;
        let percent = 20.0 + 65.0 * (sent_bytes as f32 / upload_bytes.max(1) as f32);
        progress(
            &format!("Uploading {} of {} ({:.1} of {:.1} MB)...", i + 1, missing.len(), sent_bytes as f64 / 1_048_576.0, upload_bytes as f64 / 1_048_576.0),
            percent,
        );
        api.put_bytes(&format!("/api/simulations/{id}/world/chunks/{hash}"), &bytes)
            .map_err(|e| format!("Uploading chunk {} of {} failed: {e}", i + 1, missing.len()))?;
        sent_bytes += bytes.len() as u64;
    }

    // The listing and its world as they were, for the change log: read
    // before the cover and the commit replace them.
    let before = ListingBefore::of(&api, &id, created);
    let previous = match published_before {
        Some(published) => Ok(Some(published)),
        None if created => Ok(None),
        None => fetch_manifest(&api, &id, "manifest"),
    };

    // Before the commit, so a refused cover leaves the listing playing
    // what it played, and one review covers the world and the cover.
    if cover != CoverChoice::Keep {
        progress("Updating the cover...", 85.0);
        apply_cover(&api, &id, &cover)?;
    }
    progress("Committing...", 86.0);
    let commit_body = serde_json::json!({
        "manifest_json": manifest_json,
        "publish_hash": publish_hash,
        "listing": listing_json,
    });
    let committed = api
        .post_json(&format!("/api/simulations/{id}/world/commit"), &commit_body)
        .map_err(|e| listing_refused(&e).unwrap_or_else(|| format!("Committing the world failed: {e}")))?;
    let version = committed["version"].as_u64().and_then(|v| u32::try_from(v).ok());
    let in_review = committed["moderation"].as_str() == Some("in_review");
    if let Some(source_json) = &source_json {
        progress("Committing the source...", 88.0);
        let source_body = serde_json::json!({ "manifest_json": source_json, "kind": "source" });
        api.post_json(&format!("/api/simulations/{id}/world/commit"), &source_body)
            .map_err(|e| format!("Committing the source failed: {e}"))?;
    }

    // The change log. Folder names come only from the record paths of this
    // export's chunks, read again up to FOLDER_READ_LIMIT bytes.
    progress("Writing the change log...", 89.0);
    let budget = std::cell::Cell::new(FOLDER_READ_LIMIT);
    let unread = std::cell::Cell::new(false);
    let folders_of = |chunk: &ChunkEntry| {
        if chunk.size > budget.get() {
            unread.set(true);
            return BTreeSet::new();
        }
        budget.set(budget.get() - chunk.size);
        play.read_chunk(&chunk.blake3).map(|bytes| chunk_folders(&bytes)).unwrap_or_default()
    };
    let world = match &previous {
        Ok(old) => world_changes(old.as_ref(), &manifest, &folders_of),
        Err(_) => vec!["The world as it was published could not be read, so its changes are not listed.".to_string()],
    };
    let download_chunks = manifest.all_chunks().map(|c| c.blake3.as_str()).collect::<BTreeSet<_>>().len() as u64;
    let mut totals = vec![format!(
        "Players download {} in {}. This publish uploaded {} ({}).",
        size_text(manifest.download_bytes()),
        count_text(download_chunks, "chunk"),
        count_text(missing.len() as u64, "chunk"),
        size_text(sent_bytes)
    )];
    if unread.get() {
        totals.push(format!("Folder names come from the first {} of changed chunks.", size_text(FOLDER_READ_LIMIT)));
    }
    let sent = if space_only { Sent::OneSpace } else { Sent::Everything };
    // A Space-only publish sends no listing text: its title is the listing's.
    let title = match before.name() {
        Some(name) if space_only => name.to_string(),
        _ if !listing.name.trim().is_empty() => listing.name.clone(),
        _ => plan.universe.to_string(),
    };
    let changelog = ChangeLog {
        title,
        version,
        sim_id: id.clone(),
        when: chrono::Local::now(),
        server: eustress_common::api_base::api_label(),
        listing: listing_changes(&before, listing, plan.max_players, &cover, sent, in_review),
        world,
        totals,
    }
    .markdown();
    progress("Storing the change log...", 90.0);
    let changelog_stored = store_changelog(&api, &id, version, &publish_hash, &changelog);

    // The dossier binds to this hash; the publish counts as done only once
    // its review is requested (`record_reviewed`).
    write_state(&plan.universe_root, ".last_publish_hash", &publish_hash);
    let state = state(&id);
    info!(
        "publish: {} into {id}: {} chunks, {} uploaded ({:.1} MB of {:.1} MB)",
        plan.universe,
        sizes.len(),
        missing.len(),
        sent_bytes as f64 / 1_048_576.0,
        manifest.download_bytes() as f64 / 1_048_576.0
    );
    Ok(Published {
        sim_id: id,
        publish_hash,
        chunks: sizes.len(),
        uploaded: missing.len(),
        bytes: manifest.download_bytes(),
        uploaded_bytes: sent_bytes,
        state,
        version,
        changelog: Some(changelog),
        changelog_stored,
        notes,
    })
}

/// The published manifest with `space` replaced by its fresh export, and the
/// Universe's assets refreshed.
fn swap_space(mut published: WorldManifest, fresh: &WorldManifest, space: &str) -> Result<WorldManifest, String> {
    published.validate().map_err(|e| format!("The published world's manifest was refused: {e}"))?;
    if published.encoder_version != fresh.encoder_version || published.chunk_size != fresh.chunk_size {
        return Err("The published world was baked differently. Publish the whole Universe.".into());
    }
    let replacement = fresh
        .spaces
        .iter()
        .find(|s| s.name == space)
        .cloned()
        .ok_or_else(|| format!("The export has no Space named {space}"))?;
    published.spaces.retain(|s| s.name != space);
    published.spaces.push(replacement);
    published.assets = fresh.assets.clone();
    published.engine_version = fresh.engine_version.clone();
    published.canonicalize();
    published.validate().map_err(|e| format!("The updated world failed its own check: {e}"))?;
    Ok(published)
}

/// A manifest the listing already has: `which` is `manifest` (what players
/// get) or `source` (what Edit gets). `None` when the listing has none: a
/// `.pak` listing, or a listing whose source was never shared.
fn fetch_manifest(api: &Api, sim_id: &str, which: &str) -> Result<Option<WorldManifest>, String> {
    let text = match api.get_text(&format!("/api/simulations/{sim_id}/world/{which}")) {
        Ok(text) => text,
        Err(e) if e.status == 409 || e.status == 404 => return Ok(None),
        Err(e) => return Err(format!("Reading the published world failed: {e}")),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("The published world's manifest is unreadable: {e}"))
}

/// The world Edit downloads: every player chunk, plus each Space's
/// server-only chunks.
fn source_of(play: &WorldManifest, server: &WorldManifest) -> Result<WorldManifest, String> {
    let mut source = play.clone();
    for space in &mut source.spaces {
        if let Some(extra) = server.spaces.iter().find(|s| s.name == space.name) {
            space.chunks.extend(extra.chunks.iter().cloned());
        }
    }
    source.canonicalize();
    source.validate().map_err(|e| format!("The source world failed its own check: {e}"))?;
    Ok(source)
}

/// Refuse to publish a webhook URL in anything the public can download: the
/// player world always, and the server-only services when Share Source is on.
/// Names the files, never the URLs.
fn refuse_webhooks(play: &ExportedWorld, server: Option<&ExportedWorld>) -> Result<(), String> {
    let found: Vec<String> = std::iter::once(play)
        .chain(server)
        .flat_map(|exported| exported.stats.iter())
        .flat_map(|(space, stats)| stats.webhook_paths.iter().map(move |p| format!("{space}/{p}")))
        .collect();
    if found.is_empty() {
        return Ok(());
    }
    let shown = found.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
    let more = if found.len() > 8 { format!(" and {} more", found.len() - 8) } else { String::new() };
    Err(format!(
        "{} file(s) anyone could download hold a webhook URL: {shown}{more}. Whoever has the URL can post to that channel. Move the URL out of these files{}, then publish again.",
        found.len(),
        if server.is_some() { ", or turn off Share Source when they are in ServerScriptService or ServerStorage" } else { "" }
    ))
}

/// What the person should read once a publish is done: the notice when
/// SoulService's own key was left out of a package, then each key warning the
/// exports raised, each once.
fn publish_notes(play: &ExportedWorld, server: Option<&ExportedWorld>) -> Vec<String> {
    let mut notes: Vec<String> = Vec::new();
    for exported in std::iter::once(play).chain(server) {
        for line in exported.notice.iter().map(|notice| notice.to_string()).chain(exported.warnings.iter().cloned()) {
            if !notes.contains(&line) {
                notes.push(line);
            }
        }
    }
    notes
}

/// Refuse, before any upload, a chunk the API cannot take in one request,
/// naming the file that makes it that large.
fn refuse_oversized(manifest: &WorldManifest, read_chunk: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Result<(), String> {
    let Some(chunk) = manifest.all_chunks().find(|c| c.size > MAX_UPLOAD_CHUNK_BYTES) else { return Ok(()) };
    let biggest = read_chunk(&chunk.blake3)
        .ok()
        .and_then(|bytes| eustress_echk::decode_chunk(&bytes).ok())
        .and_then(|records| records.into_iter().max_by_key(|(_, data)| data.len()))
        .map(|(path, data)| format!("{path} ({} MB)", data.len() / (1024 * 1024)))
        .unwrap_or_else(|| format!("chunk {}", chunk.blake3));
    Err(format!(
        "{biggest} is larger than the {} MB a single upload carries. Shrink or split that file, then publish again.",
        MAX_UPLOAD_CHUNK_BYTES / (1024 * 1024)
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Change log (docs/architecture/PUBLISH_CHANGELOG.md)
// ─────────────────────────────────────────────────────────────────────────────

/// Most bytes a change log holds (the API's own cap).
const CHANGELOG_LIMIT: usize = 64 * 1024;
/// Folders one line names before "and N more".
const FOLDERS_NAMED: usize = 12;
/// Most chunk bytes a publish reads again for the folders its changes touch.
const FOLDER_READ_LIMIT: u64 = 256 * 1024 * 1024;

/// What a publish sent, for the listing lines of its change log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sent {
    /// The whole Universe and the listing's text.
    Everything,
    /// One Space; the listing's text stays as it was.
    OneSpace,
    /// Only the cover.
    CoverOnly,
}

/// The listing as it was before a publish, for its change log.
#[derive(Debug, Clone)]
enum ListingBefore {
    /// This publish created it.
    New,
    /// As `GET /api/simulations/{id}` answered.
    Read(serde_json::Value),
    /// The API did not answer.
    Unread,
}

impl ListingBefore {
    fn of(api: &Api, id: &str, created: bool) -> Self {
        if created {
            return Self::New;
        }
        api.get_text(&format!("/api/simulations/{id}"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .filter(serde_json::Value::is_object)
            .map_or(Self::Unread, Self::Read)
    }

    /// Its version, which a cover-only update keeps.
    fn version(&self) -> Option<u32> {
        match self {
            Self::Read(v) => v["version"].as_u64().and_then(|v| u32::try_from(v).ok()),
            _ => None,
        }
    }

    /// Its name, when it has one.
    fn name(&self) -> Option<&str> {
        match self {
            Self::Read(v) => v["name"].as_str().filter(|n| !n.trim().is_empty()),
            _ => None,
        }
    }
}

/// A publish's change log, `CHANGELOG.md`.
#[derive(Debug, Clone)]
struct ChangeLog {
    /// The listing's name.
    title: String,
    version: Option<u32>,
    sim_id: String,
    when: chrono::DateTime<chrono::Local>,
    /// The API in use, as [`eustress_common::api_base::api_label`] names it.
    server: Option<(String, String)>,
    listing: Vec<String>,
    world: Vec<String>,
    totals: Vec<String>,
}

impl ChangeLog {
    /// The log as markdown: a heading with the listing's name and version,
    /// when (12-hour, the author's local time) and to which server, then the
    /// listing, the world and the totals, at most [`CHANGELOG_LIMIT`] bytes.
    /// Every line is one line of text: no control characters but the line
    /// breaks between them.
    fn markdown(&self) -> String {
        let title = one_line(&self.title, 120);
        let heading = match self.version {
            Some(v) => format!("# {title}: version {v}"),
            None => format!("# {title}"),
        };
        let to = self
            .server
            .as_ref()
            .map_or_else(|| "Eustress".to_string(), |(name, host)| format!("{} ({})", one_line(name, 60), one_line(host, 120)));
        let mut out = format!(
            "{heading}\n\nPublished {} (author's local time) to {to}. Listing {}.\n",
            self.when.format("%b %-d, %Y at %-I:%M %p"),
            one_line(&self.sim_id, 80)
        );
        for (name, lines, none) in
            [("Listing", &self.listing, "Unchanged."), ("World", &self.world, "Unchanged."), ("Totals", &self.totals, "None.")]
        {
            out.push_str(&format!("\n## {name}\n\n"));
            if lines.is_empty() {
                out.push_str(&format!("- {none}\n"));
            }
            for line in lines {
                out.push_str(&format!("- {}\n", one_line(line, 2000)));
            }
        }
        if out.len() <= CHANGELOG_LIMIT {
            return out;
        }
        let note = "\n- The rest was left out to keep this log under 64 KB.\n";
        let mut end = CHANGELOG_LIMIT - note.len();
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        let cut = out[..end].rfind('\n').unwrap_or(0);
        out.truncate(cut);
        out.push_str(note);
        out
    }
}

/// Store the change log with the listing. A failure does not fail the
/// publish: the Output says so, and the Publish dialog still saves the text.
fn store_changelog(api: &Api, id: &str, version: Option<u32>, publish_hash: &str, markdown: &str) -> bool {
    let Some(version) = version else {
        warn!("publish: the API did not say which version listing {id} is at, so its change log was not stored; save it from the Publish dialog");
        return false;
    };
    let body = serde_json::json!({ "version": version, "publish_hash": publish_hash, "markdown": markdown });
    match api.post_json(&format!("/api/simulations/{id}/changelogs"), &body) {
        Ok(_) => true,
        Err(e) => {
            warn!("publish: the change log was not stored with listing {id} ({e}); save it from the Publish dialog");
            false
        }
    }
}

/// `text` on one line: control characters and runs of whitespace become one
/// space, cut at `max` characters.
fn one_line(text: &str, max: usize) -> String {
    let flat: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        format!("{}...", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

/// "340 KB", "1.2 MB", "512 bytes".
fn size_text(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} bytes")
    }
}

/// "1 chunk", "3 chunks".
fn count_text(n: u64, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// The words for a Gallery rating.
fn rating_word(code: &str) -> String {
    match code {
        "all_ages" => "All ages".to_string(),
        "teen_13" => "Teen 13+".to_string(),
        "mature_17" => "Mature 17+".to_string(),
        "adult_18" => "Adult 18+".to_string(),
        other => one_line(other, 40),
    }
}

/// Tags as the API stores them ([`eustress_common::listing_tags::normalize_tag`]),
/// each once, in order.
fn stored_tags<'a>(tags: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = eustress_common::listing_tags::normalize_tag(tag).unwrap_or_else(|_| one_line(tag, 40).to_lowercase());
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}

/// " (600 × 800)" for a cover image whose size can be read, else nothing.
fn cover_size(cover: &CoverChoice) -> String {
    let CoverChoice::Set(bytes) = cover else {
        return String::new();
    };
    image::ImageReader::new(std::io::Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .map(|(w, h)| format!(" ({w} × {h})"))
        .unwrap_or_default()
}

/// The listing's fields a publish changed, one line each, against the
/// listing as it was. Only a whole publish sends the listing's text, so only
/// it can change the name, description, max players and tags. The
/// description is told by its length and first line, never in full. Rating
/// is the review's: the line says what it was and, when the commit sent the
/// listing to review (`in_review`), that the review rates this version.
fn listing_changes(
    before: &ListingBefore,
    listing: &Listing,
    max_players: u16,
    cover: &CoverChoice,
    sent: Sent,
    in_review: bool,
) -> Vec<String> {
    let list = |tags: &[String]| if tags.is_empty() { "none".to_string() } else { tags.join(", ") };
    let mut lines = Vec::new();
    let before = match before {
        ListingBefore::New => {
            lines.push(format!("New listing: \"{}\"", one_line(&listing.name, 120)));
            lines.push(format!("Max players: {max_players}"));
            let tags = stored_tags(listing.tags.iter().flatten().map(String::as_str));
            if !tags.is_empty() {
                lines.push(format!("Tags: {}", list(&tags)));
            }
            if matches!(cover, CoverChoice::Set(_)) {
                lines.push(format!("Cover: added{}", cover_size(cover)));
            }
            lines.push("Rating: the review rates this first version.".to_string());
            return lines;
        }
        ListingBefore::Unread => {
            lines.push("The listing as it was could not be read, so its changes are not listed.".to_string());
            match cover {
                CoverChoice::Set(_) => lines.push(format!("Cover: set{}", cover_size(cover))),
                CoverChoice::Remove => lines.push("Cover: removed".to_string()),
                CoverChoice::Keep => {}
            }
            if sent != Sent::CoverOnly && in_review {
                lines.push("Rating: the review rates this version.".to_string());
            }
            return lines;
        }
        ListingBefore::Read(before) => before,
    };
    if sent == Sent::Everything {
        let old_name = before["name"].as_str().unwrap_or("");
        if old_name.trim() != listing.name.trim() {
            lines.push(format!("Name: \"{}\" → \"{}\"", one_line(old_name, 120), one_line(&listing.name, 120)));
        }
        let old_description = before["description"].as_str().unwrap_or("");
        if old_description.trim() != listing.description.trim() {
            let first = listing.description.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            lines.push(format!(
                "Description: changed ({}; first line: \"{}\")",
                count_text(listing.description.trim().chars().count() as u64, "character"),
                one_line(first, 120)
            ));
        }
        if let Some(old) = before["max_players"].as_u64().filter(|old| *old != u64::from(max_players)) {
            lines.push(format!("Max players: {old} → {max_players}"));
        }
        // Tags left out of the listing stay as they are; reordering them is
        // not a change.
        if let Some(tags) = &listing.tags {
            let old = stored_tags(before["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str()));
            let new = stored_tags(tags.iter().map(String::as_str));
            let (mut a, mut b) = (old.clone(), new.clone());
            a.sort();
            b.sort();
            if a != b {
                lines.push(format!("Tags: {} → {}", list(&old), list(&new)));
            }
        }
    }
    let had_cover = before["cover_url"].as_str().is_some();
    match cover {
        CoverChoice::Set(_) if had_cover => lines.push(format!("Cover: replaced{}", cover_size(cover))),
        CoverChoice::Set(_) => lines.push(format!("Cover: none → added{}", cover_size(cover))),
        CoverChoice::Remove if had_cover => lines.push("Cover: removed".to_string()),
        _ => {}
    }
    if sent != Sent::CoverOnly {
        let rating = before["rating"].as_str().map(rating_word);
        match (rating, in_review) {
            (Some(r), true) => lines.push(format!("Rating: {r} before this version; the review rates this version.")),
            (None, true) => lines.push("Rating: not rated yet; the review rates this version.".to_string()),
            (Some(r), false) => lines.push(format!("Rating: {r}, unchanged.")),
            (None, false) => {}
        }
    }
    lines
}

/// A record's Space-relative top-level folder: its first two segments, or
/// its first for a file one level down; none for a file at the Space's root,
/// and none for a path that is not a safe record path (absolute, `..`, a
/// drive or a backslash), so a log never names anything outside the Space.
fn top_folder(path: &str) -> Option<String> {
    if !eustress_echk::is_safe_record_path(path) {
        return None;
    }
    let parts: Vec<&str> = path.split('/').collect();
    match parts.len() {
        0 | 1 => None,
        2 => Some(parts[0].to_string()),
        _ => Some(format!("{}/{}", parts[0], parts[1])),
    }
}

/// The folders a chunk's records sit in ([`top_folder`]): their paths only,
/// never their contents.
fn chunk_folders(bytes: &[u8]) -> BTreeSet<String> {
    eustress_echk::decode_chunk(bytes)
        .map(|records| records.iter().filter_map(|(path, _)| top_folder(path)).collect())
        .unwrap_or_default()
}

/// What changed in the world between the published manifest (`old`; `None`
/// before the first `.echk` publish) and this one: one line per Space that
/// changed, and one for the shared assets. `folders_of` names the folders a
/// chunk touches ([`chunk_folders`]).
fn world_changes(
    old: Option<&WorldManifest>,
    new: &WorldManifest,
    folders_of: &dyn Fn(&ChunkEntry) -> BTreeSet<String>,
) -> Vec<String> {
    let folders = |chunks: &[&ChunkEntry]| -> String {
        let mut named = BTreeSet::new();
        for c in chunks {
            named.extend(folders_of(c));
        }
        if named.is_empty() {
            return String::new();
        }
        let more = named.len().saturating_sub(FOLDERS_NAMED);
        let shown: Vec<String> = named.into_iter().take(FOLDERS_NAMED).collect();
        if more > 0 {
            format!(" Folders touched: {}, and {more} more.", shown.join(", "))
        } else {
            format!(" Folders touched: {}.", shown.join(", "))
        }
    };
    let records = |chunks: &[&ChunkEntry]| chunks.iter().map(|c| u64::from(c.count)).sum::<u64>();
    let bytes = |chunks: &[&ChunkEntry]| chunks.iter().map(|c| c.size).sum::<u64>();
    let mut lines = Vec::new();
    for space in &new.spaces {
        let name = one_line(&space.name, 80);
        let Some(before) = old.and_then(|m| m.spaces.iter().find(|s| s.name == space.name)) else {
            let all: Vec<&ChunkEntry> = space.chunks.iter().collect();
            lines.push(format!(
                "Space \"{name}\": added ({}, {}, {}).{}",
                count_text(all.len() as u64, "chunk"),
                count_text(records(&all), "record"),
                size_text(bytes(&all)),
                folders(&all)
            ));
            continue;
        };
        let was: HashMap<(i32, i32), &str> = before.chunks.iter().map(|c| ((c.cx, c.cz), c.blake3.as_str())).collect();
        let now: BTreeSet<(i32, i32)> = space.chunks.iter().map(|c| (c.cx, c.cz)).collect();
        let added: Vec<&ChunkEntry> = space.chunks.iter().filter(|c| !was.contains_key(&(c.cx, c.cz))).collect();
        let changed: Vec<&ChunkEntry> =
            space.chunks.iter().filter(|c| was.get(&(c.cx, c.cz)).is_some_and(|h| *h != c.blake3)).collect();
        let removed = was.keys().filter(|k| !now.contains(k)).count() as u64;
        if added.is_empty() && changed.is_empty() && removed == 0 {
            continue;
        }
        let mut parts = Vec::new();
        if !changed.is_empty() {
            parts.push(format!("{} changed", count_text(changed.len() as u64, "chunk")));
        }
        if !added.is_empty() {
            parts.push(format!("{} added", count_text(added.len() as u64, "chunk")));
        }
        if removed > 0 {
            parts.push(format!("{} removed", count_text(removed, "chunk")));
        }
        let touched: Vec<&ChunkEntry> = changed.iter().chain(added.iter()).copied().collect();
        let weight = if touched.is_empty() {
            String::new()
        } else {
            format!(" ({}, {})", count_text(records(&touched), "record"), size_text(bytes(&touched)))
        };
        lines.push(format!("Space \"{name}\": {}{weight}.{}", parts.join(", "), folders(&touched)));
    }
    for space in old.iter().flat_map(|m| m.spaces.iter()).filter(|s| !new.spaces.iter().any(|n| n.name == s.name)) {
        let all: Vec<&ChunkEntry> = space.chunks.iter().collect();
        lines.push(format!(
            "Space \"{}\": removed ({}, {}).",
            one_line(&space.name, 80),
            count_text(all.len() as u64, "chunk"),
            size_text(bytes(&all))
        ));
    }
    // An asset is named by its content alone, so it is added or removed,
    // never changed.
    let had: BTreeSet<&str> = old.iter().flat_map(|m| m.assets.iter()).map(|c| c.blake3.as_str()).collect();
    let has: BTreeSet<&str> = new.assets.iter().map(|c| c.blake3.as_str()).collect();
    let added: Vec<&ChunkEntry> = new.assets.iter().filter(|c| !had.contains(c.blake3.as_str())).collect();
    let removed = had.iter().filter(|h| !has.contains(*h)).count() as u64;
    if !added.is_empty() || removed > 0 {
        let mut parts = Vec::new();
        if !added.is_empty() {
            parts.push(format!("{} added ({})", count_text(added.len() as u64, "chunk"), size_text(bytes(&added))));
        }
        if removed > 0 {
            parts.push(format!("{} removed", count_text(removed, "chunk")));
        }
        lines.push(format!("Shared assets: {}.{}", parts.join(", "), folders(&added)));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_echk::{ChunkEntry, SpaceManifest};

    fn entry(cx: i32, hash: char, size: u64) -> ChunkEntry {
        ChunkEntry { cx, cz: 0, file: format!("{cx}_0.echk"), size, count: 1, blake3: hash.to_string().repeat(64) }
    }

    #[test]
    fn a_space_update_keeps_every_other_space() {
        let mut published = WorldManifest::new("U", "0.3.5", 256.0);
        published.start_space = "City".into();
        published.spaces.push(SpaceManifest { name: "City".into(), chunks: vec![entry(0, 'a', 10)] });
        published.spaces.push(SpaceManifest { name: "Garage".into(), chunks: vec![entry(0, 'b', 10)] });
        published.assets.push(entry(0, 'c', 10));

        let mut fresh = WorldManifest::new("U", "0.3.6", 256.0);
        fresh.start_space = "Garage".into();
        fresh.spaces.push(SpaceManifest { name: "Garage".into(), chunks: vec![entry(0, 'd', 12)] });
        fresh.assets.push(entry(0, 'e', 10));

        let merged = swap_space(published, &fresh, "Garage").unwrap();
        assert_eq!(merged.start_space, "City", "players still start where the author chose");
        let city = merged.spaces.iter().find(|s| s.name == "City").unwrap();
        assert_eq!(city.chunks[0].blake3, "a".repeat(64), "an untouched Space changed");
        let garage = merged.spaces.iter().find(|s| s.name == "Garage").unwrap();
        assert_eq!(garage.chunks[0].blake3, "d".repeat(64), "the updated Space was not swapped in");
        assert_eq!(merged.assets[0].blake3, "e".repeat(64));
        assert_eq!(merged.engine_version, "0.3.6");
    }

    #[test]
    fn the_source_world_adds_each_spaces_server_chunks() {
        let mut play = WorldManifest::new("U", "0.3.6", 256.0);
        play.spaces.push(SpaceManifest { name: "City".into(), chunks: vec![entry(0, 'a', 10)] });
        play.spaces.push(SpaceManifest { name: "Garage".into(), chunks: vec![entry(0, 'b', 10)] });
        play.assets.push(entry(0, 'c', 10));
        let mut server = WorldManifest::new("U", "0.3.6", 256.0);
        server.spaces.push(SpaceManifest { name: "City".into(), chunks: vec![entry(0, 'd', 10)] });
        server.spaces.push(SpaceManifest { name: "Garage".into(), chunks: vec![] });

        let source = source_of(&play, &server).unwrap();
        let city = source.spaces.iter().find(|s| s.name == "City").unwrap();
        let hashes: Vec<&str> = city.chunks.iter().map(|c| &c.blake3[..1]).collect();
        assert_eq!(hashes, vec!["a", "d"], "the server chunk joins its own Space");
        assert_eq!(source.spaces.iter().find(|s| s.name == "Garage").unwrap().chunks.len(), 1);
        assert_eq!(source.assets, play.assets);
    }

    #[test]
    fn a_webhook_url_in_anything_public_stops_the_publish() {
        let exported = |space: &str, paths: &[&str]| {
            let stats = crate::space::echk_export::ExportStats {
                webhook_paths: paths.iter().map(|p| p.to_string()).collect(),
                ..Default::default()
            };
            ExportedWorld {
                manifest: WorldManifest::new("U", "0.3.6", 256.0),
                files: Default::default(),
                stats: vec![(space.to_string(), stats)],
                warnings: Vec::new(),
                notice: None,
            }
        };
        let clean = exported("City", &[]);
        let server = exported("City", &["ServerScriptService/Report/Report.luau"]);
        assert!(refuse_webhooks(&clean, None).is_ok());
        let err = refuse_webhooks(&clean, Some(&server)).unwrap_err();
        assert!(err.contains("City/ServerScriptService/Report/Report.luau"), "{err}");
        assert!(!err.contains("discord.com"), "the refusal must never echo a URL");
        assert!(refuse_webhooks(&exported("City", &["StarterGui/Menu/Menu.luau"]), None).is_err());
    }

    /// A publish's notes: SoulService's notice first, then each warning, each
    /// once, from the player world and the source alike.
    #[test]
    fn the_notice_and_warnings_reach_the_publish_result() {
        let exported = |notice: Option<&'static str>, warnings: &[&str]| ExportedWorld {
            manifest: WorldManifest::new("U", "0.3.6", 256.0),
            files: Default::default(),
            stats: Vec::new(),
            warnings: warnings.iter().map(|w| w.to_string()).collect(),
            notice,
        };
        let warning = "City/Shop/Pay.luau:3 contains what looks like a key for Stripe test mode.";
        let play = exported(Some(eustress_echk::SOUL_KEY_NOTICE), &[warning]);
        let server = exported(None, &[warning]);
        assert_eq!(
            publish_notes(&play, Some(&server)),
            vec![eustress_echk::SOUL_KEY_NOTICE.to_string(), warning.to_string()]
        );
        assert!(publish_notes(&exported(None, &[]), None).is_empty());
    }

    #[test]
    fn a_differently_baked_world_is_not_merged() {
        let mut published = WorldManifest::new("U", "0.3.5", 128.0);
        published.spaces.push(SpaceManifest { name: "City".into(), chunks: vec![entry(0, 'a', 10)] });
        let mut fresh = WorldManifest::new("U", "0.3.6", 256.0);
        fresh.spaces.push(SpaceManifest { name: "City".into(), chunks: vec![entry(0, 'b', 10)] });
        assert!(swap_space(published, &fresh, "City").is_err());
    }

    #[test]
    fn the_listing_id_is_kept_where_both_readers_look() {
        let root = std::env::temp_dir().join(format!("eustress_publish_state_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let universe = root.join("U");
        let space = universe.join("Spaces").join("S");
        std::fs::create_dir_all(&space).unwrap();

        assert_eq!(known_listing(&universe, &space), None);
        remember_listing(&universe, &space, "11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(known_listing(&universe, &space).as_deref(), Some("11111111-2222-3333-4444-555555555555"));
        let from_space = eustress_common::load_toml_file::<eustress_common::SyncManifest>(&space.join(".eustress/sync.toml")).unwrap();
        assert_eq!(from_space.remote.listing_id(), Some("11111111-2222-3333-4444-555555555555"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A publish whose review never went out (Studio closed, or the request
    /// failed) is not "unchanged": the next publish goes up again and asks
    /// for review again.
    #[test]
    fn a_publish_is_done_only_once_its_review_went_out() {
        let root = std::env::temp_dir().join(format!("eustress_publish_reviewed_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert!(!unchanged(&root, "state-a"));
        write_state(&root, ".last_publish_hash", "hash-a");
        assert!(!unchanged(&root, "state-a"), "uploaded, but its review was never requested");
        record_reviewed(&root, "state-a");
        assert!(unchanged(&root, "state-a"));
        assert!(!unchanged(&root, "state-b"), "anything else goes up");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_a_missing_or_foreign_listing_is_replaced() {
        let gone = |status, code: Option<&str>| ApiError { status, message: String::new(), code: code.map(str::to_owned) }.listing_gone();
        assert!(gone(404, None));
        assert!(gone(403, None), "Not your simulation");
        assert!(!gone(403, Some("publish_frozen")), "a frozen account must not mint listings");
        assert!(!gone(409, Some("in_review")), "a listing in review is kept, never replaced");
        assert!(!gone(0, None), "a network failure is not a missing listing");
    }

    /// Publish As: the listing a publish starts from follows the target; a
    /// chosen listing the API refuses is named, never replaced by a new one.
    #[test]
    fn publish_as_starts_from_the_chosen_listing() {
        let own = Some("own-listing".to_string());
        assert_eq!(first_listing(&PublishTarget::Remembered, own.clone()), own);
        assert_eq!(first_listing(&PublishTarget::Remembered, None), None, "a new one the first time");
        assert_eq!(first_listing(&PublishTarget::New, own.clone()), None, "a new listing whatever it had");
        assert_eq!(first_listing(&PublishTarget::Existing(" other ".into()), own), Some("other".to_string()));
        assert_eq!(first_listing(&PublishTarget::Existing("  ".into()), None), None);

        let refused = |status: u16, code: Option<&str>| {
            chosen_listing_refused(&ApiError { status, message: "x".into(), code: code.map(str::to_string) })
        };
        assert!(refused(403, None).contains("isn't one of yours"), "someone else's listing");
        assert!(refused(404, None).contains("isn't one of yours"), "no such listing");
        assert!(refused(409, Some("in_review")).contains("being reviewed"), "a locked listing");
        assert!(refused(403, Some("kyc_required")).starts_with("Starting the upload failed"), "another 403 says its own reason");
        assert!(refused(403, None).contains("Nothing was published"));
    }

    /// Choosing a listing rewrites the listing for the API in use only: the
    /// Universe's listings on other APIs stay.
    #[test]
    fn publish_as_rewrites_this_apis_listing_only() {
        let root = std::env::temp_dir().join(format!("eustress-publish-as-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let path = state_dir(&root).join("sync.toml");
        std::fs::create_dir_all(state_dir(&root)).unwrap();
        let mut sync = eustress_common::SyncManifest::default();
        sync.remote.set_listing_id_for("https://api-staging.eustress.dev", Some("staging-listing".into()));
        sync.remote.set_listing_id(Some("old-listing".into()));
        eustress_common::save_toml_file(&sync, &path).unwrap();

        remember_listing(&root, &root, "chosen-listing").unwrap();
        let after = eustress_common::load_toml_file::<eustress_common::SyncManifest>(&path).unwrap();
        assert_eq!(after.remote.listing_id(), Some("chosen-listing"));
        assert_eq!(after.remote.listing_id_for("https://api-staging.eustress.dev"), Some("staging-listing"));
        assert_eq!(known_listing(&root, &root).as_deref(), Some("chosen-listing"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Tags travel only when the dialog sends them; a refusal of a tag or
    /// the player count reads in the API's own words.
    #[test]
    fn listing_tags_travel_and_their_refusal_reads_as_given() {
        let listing = |tags: Option<Vec<String>>| Listing {
            name: "Box Head".into(),
            description: String::new(),
            is_public: true,
            open_source: false,
            tags,
            cover: CoverChoice::Keep,
        };
        assert!(listing(None).to_json().get("tags").is_none(), "None leaves the listing's tags alone");
        assert_eq!(listing(Some(vec![])).to_json()["tags"], serde_json::json!([]), "an empty list asks the review to choose");
        assert!(listing(None).to_json().get("genre").is_none(), "a listing's tags say what it is; no genre goes up");
        assert_eq!(
            listing(Some(vec!["zombies".into(), "co-op".into()])).to_json()["tags"],
            serde_json::json!(["zombies", "co-op"])
        );
        let said = "Tag \"sci_fi\" is not a valid tag. A tag is 2 to 24 characters.";
        let refused = |code: Option<&str>| {
            listing_refused(&ApiError { status: 400, message: said.into(), code: code.map(str::to_string) })
        };
        assert_eq!(refused(Some("bad_tags")).as_deref(), Some(said));
        assert!(refused(Some("bad_max_players")).is_some());
        assert_eq!(refused(Some("other")), None);
        assert_eq!(refused(None), None);
    }

    /// A cover the API refuses reads in its own words: a bad image states
    /// the rules, a listing in review says so; anything else says what
    /// failed.
    #[test]
    fn a_refused_cover_reads_in_the_apis_words() {
        let refused = |status: u16, code: Option<&str>, message: &str| {
            cover_refused(&ApiError { status, message: message.into(), code: code.map(str::to_string) }, "Uploading the cover failed")
        };
        let rules = "A cover is a PNG, JPEG or WebP image of at most 2 MB, at least 320 px on its short side, with a width to height ratio between 1:2 and 2:1.";
        for (status, code) in [(413, "cover_too_large"), (415, "cover_bad_type"), (422, "cover_too_small"), (422, "cover_bad_ratio"), (400, "cover_empty")] {
            assert_eq!(refused(status, Some(code), rules), rules, "{code}");
        }
        assert_eq!(refused(409, Some("in_review"), "This listing is being reviewed"), "This listing is being reviewed");
        assert!(refused(500, None, "boom").starts_with("Uploading the cover failed"));
        assert!(refused(0, None, "connection reset").starts_with("Uploading the cover failed"));
    }

    // ── change log ──

    fn at(cx: i32, hash: char, size: u64, count: u32) -> ChunkEntry {
        ChunkEntry { count, ..entry(cx, hash, size) }
    }

    fn listing_with(name: &str, tags: Option<&[&str]>, cover: CoverChoice) -> Listing {
        Listing {
            name: name.into(),
            description: "Hold the line.\nThen hold it again.".into(),
            is_public: true,
            open_source: false,
            tags: tags.map(|t| t.iter().map(|s| s.to_string()).collect()),
            cover,
        }
    }

    fn when() -> chrono::DateTime<chrono::Local> {
        use chrono::TimeZone;
        chrono::Local.with_ymd_and_hms(2026, 10, 2, 18, 26, 0).single().unwrap()
    }

    #[test]
    fn a_folder_is_one_or_two_segments_inside_the_space() {
        assert_eq!(top_folder("Workspace/Map/Wall.part.toml").as_deref(), Some("Workspace/Map"));
        assert_eq!(top_folder("Workspace/Map/Deep/Wall.part.toml").as_deref(), Some("Workspace/Map"));
        assert_eq!(top_folder("Lighting/_service.toml").as_deref(), Some("Lighting"));
        assert_eq!(top_folder("space.toml"), None, "a file at the Space's root names no folder");
        for hostile in [
            "C:/Users/someone/Desktop/a.toml",
            "/home/someone/a.toml",
            "../x/a.toml",
            "Workspace/../../etc/passwd",
            "Workspace\\Map\\a.toml",
            "Workspace/./a.toml",
            "Work\nspace/a.toml",
            "",
        ] {
            assert_eq!(top_folder(hostile), None, "{hostile:?} named a folder");
        }
    }

    #[test]
    fn a_chunks_folders_come_from_its_paths_never_its_contents() {
        let bytes = eustress_echk::encode_chunk(&[
            ("Workspace/Map/Wall.part.toml".to_string(), b"secret = \"Workspace/Hidden\"".to_vec()),
            ("Workspace/Zombies/Z1/_instance.toml".to_string(), b"x".to_vec()),
            ("space.toml".to_string(), b"y".to_vec()),
        ]);
        let folders: Vec<String> = chunk_folders(&bytes).into_iter().collect();
        assert_eq!(folders, vec!["Workspace/Map".to_string(), "Workspace/Zombies".to_string()]);
        assert!(chunk_folders(b"not a chunk").is_empty());
    }

    #[test]
    fn the_world_lines_say_what_each_space_and_the_assets_did() {
        let mut old = WorldManifest::new("U", "0.3.6", 256.0);
        old.spaces.push(SpaceManifest { name: "Main".into(), chunks: vec![at(0, 'a', 1000, 4), at(1, 'b', 1000, 4), at(2, 'c', 1000, 4)] });
        old.spaces.push(SpaceManifest { name: "Same".into(), chunks: vec![at(0, 'd', 1000, 1)] });
        old.spaces.push(SpaceManifest { name: "Old Test".into(), chunks: vec![at(0, 'e', 300 * 1024, 2), at(1, 'f', 10, 1)] });
        old.assets.push(at(0, 'g', 10, 1));

        let mut new = WorldManifest::new("U", "0.3.6", 256.0);
        new.spaces.push(SpaceManifest { name: "Main".into(), chunks: vec![at(0, 'h', 2048, 5), at(1, 'b', 1000, 4), at(3, 'i', 2048, 7)] });
        new.spaces.push(SpaceManifest { name: "Same".into(), chunks: vec![at(0, 'd', 1000, 1)] });
        new.spaces.push(SpaceManifest { name: "Lobby".into(), chunks: vec![at(0, 'j', 1024 * 1024, 88)] });
        new.assets.push(at(0, 'k', 2048, 1));

        let folders_of = |c: &ChunkEntry| -> BTreeSet<String> {
            match &c.blake3[..1] {
                "h" => ["Workspace/Map".to_string()].into(),
                "i" => ["Workspace/Zombies".to_string(), "Workspace/Map".to_string()].into(),
                _ => BTreeSet::new(),
            }
        };
        assert_eq!(
            world_changes(Some(&old), &new, &folders_of),
            vec![
                "Space \"Main\": 1 chunk changed, 1 chunk added, 1 chunk removed (12 records, 4 KB). Folders touched: Workspace/Map, Workspace/Zombies.",
                "Space \"Lobby\": added (1 chunk, 88 records, 1.0 MB).",
                "Space \"Old Test\": removed (2 chunks, 300 KB).",
                "Shared assets: 1 chunk added (2 KB), 1 chunk removed.",
            ]
        );
        assert!(world_changes(Some(&new), &new, &folders_of).is_empty(), "the same world changed nothing");
    }

    #[test]
    fn a_first_publish_lists_every_space_as_added() {
        let mut new = WorldManifest::new("U", "0.3.6", 256.0);
        new.spaces.push(SpaceManifest { name: "Main".into(), chunks: vec![at(0, 'a', 512, 3)] });
        new.assets.push(at(0, 'b', 10, 1));
        assert_eq!(
            world_changes(None, &new, &|_| BTreeSet::new()),
            vec!["Space \"Main\": added (1 chunk, 3 records, 512 bytes).", "Shared assets: 1 chunk added (10 bytes)."]
        );
    }

    #[test]
    fn the_listing_lines_compare_what_the_api_had() {
        let before = ListingBefore::Read(serde_json::json!({
            "name": "Box Head", "description": "Hold the line.\nThen hold it again.",
            "max_players": 10, "tags": ["zombies", "tycoon"], "cover_url": null, "rating": "teen_13", "version": 6,
        }));
        let listing = listing_with("Box Head", Some(&["Tycoon", "#Zombies", "co-op"]), CoverChoice::Set(vec![1, 2, 3]));
        assert_eq!(
            listing_changes(&before, &listing, 4, &listing.cover, Sent::Everything, true),
            vec![
                "Max players: 10 → 4",
                "Tags: zombies, tycoon → tycoon, zombies, co-op",
                "Cover: none → added",
                "Rating: Teen 13+ before this version; the review rates this version.",
            ]
        );
        assert_eq!(before.version(), Some(6));

        // Reordered tags, the same text, and no review: only the rating, unchanged.
        let same = listing_with("Box Head", Some(&["tycoon", "zombies"]), CoverChoice::Keep);
        assert_eq!(listing_changes(&before, &same, 10, &CoverChoice::Keep, Sent::Everything, false), vec!["Rating: Teen 13+, unchanged."]);

        // A renamed listing with a new description tells its length and first line only.
        let mut renamed = listing_with("Box Head 2", None, CoverChoice::Keep);
        renamed.description = "\n  Survive.\nSecret second line.".into();
        let lines = listing_changes(&before, &renamed, 10, &CoverChoice::Keep, Sent::Everything, true);
        assert_eq!(lines[0], "Name: \"Box Head\" → \"Box Head 2\"");
        assert_eq!(lines[1], "Description: changed (28 characters; first line: \"Survive.\")");
        assert!(!lines.iter().any(|l| l.contains("Secret")), "the description went in whole");
    }

    #[test]
    fn a_space_or_a_cover_update_never_reports_listing_text() {
        let before = ListingBefore::Read(serde_json::json!({ "name": "Old", "max_players": 10, "cover_url": "https://x/c", "rating": "all_ages" }));
        let listing = listing_with("New name", Some(&["co-op"]), CoverChoice::Keep);
        assert_eq!(
            listing_changes(&before, &listing, 4, &CoverChoice::Keep, Sent::OneSpace, true),
            vec!["Rating: All ages before this version; the review rates this version."]
        );
        assert_eq!(listing_changes(&before, &listing, 4, &CoverChoice::Remove, Sent::CoverOnly, true), vec!["Cover: removed"]);
        assert_eq!(
            listing_changes(&before, &listing, 4, &CoverChoice::Set(vec![0]), Sent::CoverOnly, false),
            vec!["Cover: replaced"]
        );
    }

    #[test]
    fn a_new_or_unread_listing_says_so() {
        let listing = listing_with("Box Head", Some(&["zombies"]), CoverChoice::Keep);
        assert_eq!(
            listing_changes(&ListingBefore::New, &listing, 10, &CoverChoice::Keep, Sent::Everything, true),
            vec!["New listing: \"Box Head\"", "Max players: 10", "Tags: zombies", "Rating: the review rates this first version."]
        );
        assert_eq!(
            listing_changes(&ListingBefore::Unread, &listing, 10, &CoverChoice::Keep, Sent::Everything, true),
            vec![
                "The listing as it was could not be read, so its changes are not listed.",
                "Rating: the review rates this version.",
            ]
        );
        assert_eq!(ListingBefore::Unread.version(), None);
    }

    #[test]
    fn the_markdown_has_a_heading_a_12_hour_time_and_three_sections() {
        let log = ChangeLog {
            title: "Box Head".into(),
            version: Some(7),
            sim_id: "07d084d8".into(),
            when: when(),
            server: Some(("staging".into(), "api-staging.eustress.dev".into())),
            listing: vec!["Max players: 10 → 4".into()],
            world: vec![],
            totals: vec!["Players download 2 KB in 1 chunk.".into()],
        };
        assert_eq!(
            log.markdown(),
            "# Box Head: version 7\n\n\
             Published Oct 2, 2026 at 6:26 PM (author's local time) to staging (api-staging.eustress.dev). Listing 07d084d8.\n\
             \n## Listing\n\n- Max players: 10 → 4\n\
             \n## World\n\n- Unchanged.\n\
             \n## Totals\n\n- Players download 2 KB in 1 chunk.\n"
        );
    }

    #[test]
    fn the_markdown_holds_no_control_characters_but_line_breaks() {
        let log = ChangeLog {
            title: "Box\r\nHead\u{7}".into(),
            version: None,
            sim_id: "id".into(),
            when: when(),
            server: None,
            listing: vec!["Name: \"a\u{0}b\"".into()],
            world: vec!["Space \"x\ny\": added.".into()],
            totals: vec![],
        };
        let text = log.markdown();
        assert!(text.starts_with("# Box Head\n"), "{text}");
        assert!(text.contains("- Space \"x y\": added.\n"), "{text}");
        assert!(text.chars().all(|c| c == '\n' || !c.is_control()), "{text:?}");
    }

    #[test]
    fn a_long_log_stops_under_64_kb_on_a_line() {
        let log = ChangeLog {
            title: "Big".into(),
            version: Some(1),
            sim_id: "id".into(),
            when: when(),
            server: None,
            listing: vec![],
            world: (0..2000).map(|i| format!("Space \"Плато {i}\": added (1 chunk, 1 record, 10 bytes). Folders touched: Карта/Стены.")).collect(),
            totals: vec!["Players download 1 MB.".into()],
        };
        let text = log.markdown();
        assert!(text.len() <= CHANGELOG_LIMIT, "{} bytes", text.len());
        assert!(text.ends_with("\n- The rest was left out to keep this log under 64 KB.\n"));
        let before_note = text.trim_end_matches("\n- The rest was left out to keep this log under 64 KB.\n");
        assert!(before_note.ends_with("Карта/Стены."), "cut inside a line: {:?}", before_note.lines().last());
    }
}
