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
//! The listing is created once. Its id is kept in the Universe's
//! `.eustress/sync.toml` (`remote.experience_id`) the moment the API returns
//! it, so every later publish, a Space-only one included, and a retry after a
//! failed upload, all land in the same listing. The Worker side of each call
//! is `infrastructure/cloudflare/api/src/world.mjs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use eustress_echk::WorldManifest;
use eustress_worlddb::WorldDb;

use super::echk_export::{export_world, Audience, ExportedWorld, SpaceInput, SpaceSource, SAVE_SETTLE};

/// Largest chunk the API takes in one request (`MAX_UPLOAD_CHUNK_BYTES` in
/// `world.mjs`, under the Worker's 100 MB request limit).
pub const MAX_UPLOAD_CHUNK_BYTES: u64 = 95 * 1024 * 1024;
/// What an unchanged publish reports instead of uploading.
pub const NO_CHANGES: &str = "No changes since the last publish";

/// Name, description, genre and visibility, as the Publish dialog set them.
#[derive(Debug, Clone)]
pub struct Listing {
    pub name: String,
    pub description: String,
    pub genre: String,
    pub is_public: bool,
    /// "Share Source": the gallery offers Edit (open in Studio) on the listing.
    pub open_source: bool,
}

impl Listing {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "genre": self.genre,
            "is_public": self.is_public,
            "open_source": self.open_source,
        })
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
/// receive (with the Universe's assets), or the server-only services.
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
    export_world(&plan.universe, inputs, start, assets, &out_root, plan.big_space_threshold, audience)
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
}

/// Where a Universe keeps its publish state.
fn state_dir(universe_root: &Path) -> PathBuf {
    universe_root.join(".eustress")
}

/// The listing a Universe publishes into, if it has one.
pub fn known_listing(universe_root: &Path, space_root: &Path) -> Option<String> {
    [universe_root, space_root].iter().find_map(|root| {
        eustress_common::load_toml_file::<eustress_common::SyncManifest>(&state_dir(root).join("sync.toml"))
            .ok()
            .and_then(|s| s.remote.experience_id)
            .filter(|id| !id.trim().is_empty())
    })
}

/// Keep the listing id in the Universe's `sync.toml`, and the open Space's,
/// which the Publish dialog reads.
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
        sync.remote.experience_id = Some(sim_id.to_string());
        eustress_common::save_toml_file(&sync, &path).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(())
}

fn read_state(universe_root: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(state_dir(universe_root).join(name)).ok().map(|s| s.trim().to_string())
}

fn write_state(universe_root: &Path, name: &str, value: &str) {
    let dir = state_dir(universe_root);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(name), value);
}

fn create_listing(api: &Api, listing: &Listing, spaces: &[String]) -> Result<String, String> {
    let body = serde_json::json!({
        "name": listing.name,
        "description": listing.description,
        "genre": listing.genre,
        "max_players": 10,
        // The author's intent; the gallery lists it only once review approves.
        "is_public": listing.is_public,
        "open_source": listing.open_source,
        "spaces": spaces,
    });
    let answer = api
        .post_json("/api/simulations/publish", &body)
        .map_err(|e| format!("Creating the listing failed: {e}"))?;
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
    let api = Api::new(token);
    let mut sim_id = known_listing(&plan.universe_root, &plan.space_root);
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

    let (manifest, source) = if space_only {
        let id = sim_id.as_deref().unwrap_or_default();
        let published = fetch_manifest(&api, id, "manifest")?.ok_or_else(|| {
            "This listing was published before .echk. Publish the whole Universe once, then Spaces can be updated one at a time."
                .to_string()
        })?;
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
        (manifest, source)
    } else {
        let source = server.as_ref().map(|s| source_of(&play.manifest, &s.manifest)).transpose()?;
        (play.manifest.clone(), source)
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
    let listing_json = if space_only {
        serde_json::json!({ "open_source": listing.open_source })
    } else {
        listing.to_json()
    };

    // Nothing to do when this exact world, source and listing text went up
    // last time.
    let source_hash = source_json.as_deref().map(|s| blake3::hash(s.as_bytes()).to_hex().to_string()).unwrap_or_default();
    let state = |id: &str| {
        blake3::hash(format!("{id}\n{publish_hash}\n{source_hash}\n{listing_json}").as_bytes()).to_hex().to_string()
    };
    if let Some(id) = &sim_id {
        if read_state(&plan.universe_root, ".last_publish_state").as_deref() == Some(state(id).as_str()) {
            return Err(NO_CHANGES.into());
        }
    }

    let space_names: Vec<String> = manifest.spaces.iter().map(|s| s.name.clone()).collect();
    let mut created = false;
    let id = match sim_id.take() {
        Some(id) => id,
        None => {
            progress("Creating the listing...", 12.0);
            let id = create_listing(&api, listing, &space_names)?;
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
        Ok(begin) => (id, begin),
        Err(e) if e.listing_gone() && !created && !space_only => {
            warn!("publish: listing {id} is gone or not this account's ({e}); creating a new one");
            let fresh = create_listing(&api, listing, &space_names)?;
            remember_listing(&plan.universe_root, &plan.space_root, &fresh)?;
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

    progress("Committing...", 86.0);
    let commit_body = serde_json::json!({
        "manifest_json": manifest_json,
        "publish_hash": publish_hash,
        "listing": listing_json,
    });
    api.post_json(&format!("/api/simulations/{id}/world/commit"), &commit_body)
        .map_err(|e| format!("Committing the world failed: {e}"))?;
    if let Some(source_json) = &source_json {
        progress("Committing the source...", 88.0);
        let source_body = serde_json::json!({ "manifest_json": source_json, "kind": "source" });
        api.post_json(&format!("/api/simulations/{id}/world/commit"), &source_body)
            .map_err(|e| format!("Committing the source failed: {e}"))?;
    }

    write_state(&plan.universe_root, ".last_publish_hash", &publish_hash);
    write_state(&plan.universe_root, ".last_publish_state", &state(&id));
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
        assert_eq!(from_space.remote.experience_id.as_deref(), Some("11111111-2222-3333-4444-555555555555"));
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
}
