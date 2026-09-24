//! # Published-content fetch
//!
//! Downloads a published simulation so the Player can open it.
//!
//! ## What a published simulation is
//!
//! Studio publishes a world as `.echk` chunks: each Space baked into
//! content-addressed spatial chunks, plus the Universe's `assets/`, named by
//! a [`WorldManifest`]. Simulations published before that are one `.pak`
//! (tar + zstd of the Universe folder), and still open.
//!
//! ## Flow
//!
//! ```text
//! FetchSpace(sim_id)
//!   → GET {API}/api/simulations/{id}/world/manifest
//!       → each chunk not already cached:
//!           GET {API}/api/simulations/{id}/world/chunks/{blake3}, verified by hash
//!       → decode → WorldDownloaded
//!   (409 {"format":"pak"}: an older publish)
//!   → GET {API}/api/simulations/{id}/download → zstd → untar → WorldDownloaded
//! ```
//!
//! A downloaded world then opens exactly like one a host sent
//! (`systems::net_play`). Work happens on a worker thread; Bevy only sees
//! messages. Native only: a browser build fetches with the platform's
//! `fetch` instead of blocking HTTP on a thread.

use bevy::prelude::*;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use eustress_echk::{content_hash, is_content_hash, WorldManifest};
use eustress_networking::session::{assemble_world, ChunkCache, DownloadedWorld, WorldDownloaded};

use super::live_world::{find_spawn, world_from_universe_dir, DiskChunkCache};

/// Where published content is fetched from. Matches Studio's `PUBLISH_API`.
pub const PUBLISH_API: &str = "https://api.eustress.dev";

/// Where a published world's players stand when it names no SpawnLocation.
const DEFAULT_SPAWN: [f32; 3] = [0.0, 2.0, 8.0];

/// Ask the Player to download and open a published simulation.
#[derive(Message, Debug, Clone)]
pub struct FetchSpace {
    pub simulation_id: String,
    /// Bearer token, for a simulation that is not public.
    pub token: Option<String>,
}

#[derive(Message, Debug, Clone)]
pub struct SpaceFetchFailed {
    pub simulation_id: String,
    pub error: String,
}

enum FetchResult {
    Ok(DownloadedWorld),
    Err(SpaceFetchFailed),
}

#[derive(Resource)]
struct FetchChannel {
    tx: Sender<FetchResult>,
    rx: Mutex<Receiver<FetchResult>>,
}

pub struct SpaceFetchPlugin;

impl Plugin for SpaceFetchPlugin {
    fn build(&self, app: &mut App) {
        let (tx, rx) = channel();
        app.insert_resource(FetchChannel { tx, rx: Mutex::new(rx) })
            .add_message::<FetchSpace>()
            .add_message::<SpaceFetchFailed>()
            // Also registered by NetPlugin; idempotent.
            .add_message::<WorldDownloaded>()
            .add_systems(Update, (start_fetches, drain_results));
    }
}

fn start_fetches(mut events: MessageReader<FetchSpace>, chan: Res<FetchChannel>) {
    for req in events.read() {
        let id = req.simulation_id.clone();
        let token = req.token.clone();
        let tx = chan.tx.clone();

        info!("📥 fetching published simulation {id}");

        std::thread::Builder::new()
            .name(format!("space-fetch-{id}"))
            .spawn(move || {
                let result = match fetch_world(&id, token.as_deref()) {
                    Ok(world) => FetchResult::Ok(world),
                    Err(error) => FetchResult::Err(SpaceFetchFailed { simulation_id: id.clone(), error }),
                };
                let _ = tx.send(result);
            })
            .ok();
    }
}

fn drain_results(
    chan: Res<FetchChannel>,
    mut ok: MessageWriter<WorldDownloaded>,
    mut fail: MessageWriter<SpaceFetchFailed>,
) {
    let Ok(rx) = chan.rx.lock() else { return };
    while let Ok(r) = rx.try_recv() {
        match r {
            FetchResult::Ok(world) => {
                let files: usize = world.spaces.iter().map(|(_, r)| r.len()).sum::<usize>() + world.assets.len();
                info!("📦 fetched {} ({} Spaces, {} files)", world.universe, world.spaces.len(), files);
                ok.write(WorldDownloaded(Arc::new(world)));
            }
            FetchResult::Err(e) => {
                error!("📥 fetch {} failed: {}", e.simulation_id, e.error);
                fail.write(e);
            }
        }
    }
}

/// Download a published world. Blocking; runs off the Bevy thread.
fn fetch_world(sim_id: &str, token: Option<&str>) -> Result<DownloadedWorld, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let get = |url: &str| {
        let mut req = client.get(url);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        req.send().map_err(|e| format!("GET {url}: {e}"))
    };

    let manifest_url = format!("{PUBLISH_API}/api/simulations/{sim_id}/world/manifest");
    let resp = get(&manifest_url)?;
    let status = resp.status();
    if status.as_u16() == 409 {
        // Published before .echk: one .pak of the whole Universe.
        return fetch_legacy_pak(sim_id, &get);
    }
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("{status} from {manifest_url}: {}", body.chars().take(200).collect::<String>()));
    }
    let manifest: WorldManifest = resp.json().map_err(|e| format!("read the world manifest: {e}"))?;
    manifest.validate().map_err(|e| format!("the world manifest was refused: {e}"))?;

    let cache = DiskChunkCache::default();
    let mut chunks: HashMap<String, Vec<u8>> = HashMap::new();
    for hash in manifest.hashes() {
        if !is_content_hash(&hash) {
            return Err(format!("bad chunk name {hash:?}"));
        }
        if let Some(bytes) = cache.get(&hash).filter(|b| content_hash(b) == hash) {
            chunks.insert(hash, bytes);
            continue;
        }
        let url = format!("{PUBLISH_API}/api/simulations/{sim_id}/world/chunks/{hash}");
        let resp = get(&url)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("{status} from {url}"));
        }
        let bytes = resp.bytes().map_err(|e| format!("read chunk {hash}: {e}"))?.to_vec();
        if content_hash(&bytes) != hash {
            return Err(format!("chunk {hash} does not match its content hash"));
        }
        cache.put(&hash, &bytes);
        chunks.insert(hash, bytes);
    }

    let mut world = assemble_world(&manifest, |h| chunks.get(h).map(|b| b.as_slice()), DEFAULT_SPAWN)?;
    if let Some(at) = find_spawn(&world) {
        world.spawn = at.to_array();
    }
    Ok(world)
}

fn fetch_legacy_pak(
    sim_id: &str,
    get: &dyn Fn(&str) -> Result<reqwest::blocking::Response, String>,
) -> Result<DownloadedWorld, String> {
    let url = format!("{PUBLISH_API}/api/simulations/{sim_id}/download");
    let resp = get(&url)?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("{status} from {url}: {}", body.chars().take(200).collect::<String>()));
    }
    let bytes = resp.bytes().map_err(|e| format!("read body: {e}"))?;
    if bytes.is_empty() {
        return Err("empty .pak".into());
    }
    let dest = super::live_world::data_root().join("pak").join(sim_id);
    unpack_pak(&bytes, &dest)?;
    world_from_universe_dir(&dest)
}

/// zstd → tar → directory. Public so a local `.pak` can be opened without a
/// round trip, which is also how this gets tested offline.
pub fn unpack_pak(pak: &[u8], dest: &Path) -> Result<(), String> {
    let mut tar_bytes = Vec::new();
    zstd::stream::Decoder::new(std::io::Cursor::new(pak))
        .map_err(|e| format!("zstd init: {e}"))?
        .read_to_end(&mut tar_bytes)
        .map_err(|e| format!("zstd decode: {e}"))?;

    if dest.exists() {
        // Replace wholesale — a partial overlay of two different publishes is
        // worse than a clean refetch.
        std::fs::remove_dir_all(dest).map_err(|e| format!("clear {dest:?}: {e}"))?;
    }
    std::fs::create_dir_all(dest).map_err(|e| format!("create {dest:?}: {e}"))?;

    let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
    for entry in archive.entries().map_err(|e| format!("tar entries: {e}"))? {
        let mut entry = entry.map_err(|e| format!("tar entry: {e}"))?;
        let path = entry.path().map_err(|e| format!("tar path: {e}"))?.into_owned();

        if is_unsafe_archive_path(&path) {
            return Err(format!("refusing unsafe archive path {path:?}"));
        }

        let out = dest.join(&path);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
        }
        entry.unpack(&out).map_err(|e| format!("unpack {path:?}: {e}"))?;
    }

    Ok(())
}

/// Would extracting this entry escape the destination directory?
///
/// A `.pak` is untrusted user content — an entry named
/// `../../.ssh/authorized_keys` would otherwise be written outside the cache.
///
/// Note the `tar` crate refuses to *create* archives containing `..`, so this
/// cannot be exercised by round-tripping through its writer; archives produced
/// by other tools carry no such guarantee, which is exactly why the check lives
/// on the read side.
pub fn is_unsafe_archive_path(path: &Path) -> bool {
    // `has_root()` as well as `is_absolute()`: on Windows a leading `/` is
    // root-relative, NOT absolute, so `Path::new("/etc/passwd").is_absolute()`
    // is false there and an absolute-looking entry would slip through a check
    // that only asked `is_absolute`.
    path.is_absolute()
        || path.has_root()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::Prefix(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip the legacy published format: tar + zstd of a Universe
    /// folder, the way Studio built a `.pak` before `.echk`.
    #[test]
    fn unpacks_a_legacy_pak_into_a_world() {
        let tmp = std::env::temp_dir().join(format!("eustress_pak_rt_{}", std::process::id()));
        let src = tmp.join("src");
        let out = tmp.join("out");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(src.join("Spaces/MySpace/Workspace/Floor")).unwrap();
        std::fs::write(src.join("Spaces/MySpace/Workspace/Floor/_instance.toml"), "[metadata]\nclass_name = \"Part\"\n")
            .unwrap();
        std::fs::write(src.join("readme.txt"), "hello").unwrap();

        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            b.append_dir_all(".", &src).unwrap();
            b.finish().unwrap();
        }
        let pak = zstd::encode_all(std::io::Cursor::new(&tar_bytes), 3).unwrap();

        unpack_pak(&pak, &out).expect("unpack");
        let world = world_from_universe_dir(&out).expect("world");
        assert_eq!(world.start_space, "MySpace");
        assert_eq!(world.spaces[0].1.len(), 1);
        assert_eq!(world.spaces[0].1[0].0, "Workspace/Floor/_instance.toml");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A `.pak` is untrusted user content, so the extractor must reject any
    /// entry that would land outside the destination.
    #[test]
    fn refuses_archive_paths_that_escape_the_destination() {
        for evil in [
            "../escaped.txt",
            "../../.ssh/authorized_keys",
            "MySpace/../../out.txt",
            "/etc/passwd",
        ] {
            assert!(
                is_unsafe_archive_path(Path::new(evil)),
                "should have refused {evil}"
            );
        }
        for ok in ["MySpace/_instance.toml", "readme.txt", "a/b/c.glb"] {
            assert!(!is_unsafe_archive_path(Path::new(ok)), "should have allowed {ok}");
        }
    }
}
