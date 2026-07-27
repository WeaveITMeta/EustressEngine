//! # Published-content fetch
//!
//! Downloads a published simulation and unpacks it so the Player can open it.
//! This is the half of the publish loop that never existed: Studio has uploaded
//! to R2 since the pipeline was built (`engine/src/ui/file_event_handler.rs:701`),
//! and nothing has ever read from it.
//!
//! ## What a published simulation actually is
//!
//! A `.pak` is **tar + zstd of the Universe directory**
//! (`file_event_handler.rs:912-963`) — the file-system-first workspace of
//! `.instance.toml` files, `.glb` meshes and `.soul` scripts. It is *not* a
//! binary scene blob, which is why "extract the binary scene parser" was the
//! wrong plan: unpacking gives a Space directory, and opening that directory is
//! the same job Studio already does.
//!
//! ## Flow
//!
//! ```text
//! FetchSpace(sim_id)
//!   → GET {API}/api/simulations/{id}/space      (bytes)
//!   → zstd decode → untar → content cache
//!   → SpaceFetched { root }
//! ```
//!
//! Work happens on a worker thread; Bevy only ever sees messages.
//!
//! ## Cache
//!
//! Unpacked under `dirs::data_local_dir()/eustress/spaces/<sim_id>/`, keyed by
//! the publish hash so an unchanged simulation is not refetched. Studio already
//! computes that hash to skip no-op uploads (`file_event_handler.rs:714`).

use bevy::prelude::*;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

/// Where published content is fetched from. Matches Studio's `PUBLISH_API`.
pub const PUBLISH_API: &str = "https://api.eustress.dev";

/// Ask the Player to download and unpack a published simulation.
#[derive(Message, Debug, Clone)]
pub struct FetchSpace {
    pub simulation_id: String,
    /// Bearer token. Published content is auth-gated today; the worker's
    /// download handler rejects anonymous reads.
    pub token: Option<String>,
}

/// A simulation is unpacked and ready to open.
#[derive(Message, Debug, Clone)]
pub struct SpaceFetched {
    pub simulation_id: String,
    pub root: PathBuf,
    /// Every `_instance.toml` found — the Spaces this Universe contains.
    pub spaces: Vec<PathBuf>,
    pub bytes: usize,
}

#[derive(Message, Debug, Clone)]
pub struct SpaceFetchFailed {
    pub simulation_id: String,
    pub error: String,
}

enum FetchResult {
    Ok(SpaceFetched),
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
            .add_message::<SpaceFetched>()
            .add_message::<SpaceFetchFailed>()
            .add_systems(Update, (start_fetches, drain_results));
    }
}

/// Local cache root for unpacked simulations.
pub fn cache_root() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("eustress")
        .join("spaces")
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
                let result = match fetch_and_unpack(&id, token.as_deref()) {
                    Ok(v) => FetchResult::Ok(v),
                    Err(e) => FetchResult::Err(SpaceFetchFailed {
                        simulation_id: id.clone(),
                        error: e,
                    }),
                };
                let _ = tx.send(result);
            })
            .ok();
    }
}

fn drain_results(
    chan: Res<FetchChannel>,
    mut ok: MessageWriter<SpaceFetched>,
    mut fail: MessageWriter<SpaceFetchFailed>,
) {
    let Ok(rx) = chan.rx.lock() else { return };
    while let Ok(r) = rx.try_recv() {
        match r {
            FetchResult::Ok(v) => {
                info!(
                    "📦 unpacked {} → {:?} ({} spaces, {} bytes)",
                    v.simulation_id,
                    v.root,
                    v.spaces.len(),
                    v.bytes
                );
                ok.write(v);
            }
            FetchResult::Err(e) => {
                error!("📥 fetch {} failed: {}", e.simulation_id, e.error);
                fail.write(e);
            }
        }
    }
}

/// Download, decompress, untar. Blocking; runs off the Bevy thread.
fn fetch_and_unpack(sim_id: &str, token: Option<&str>) -> Result<SpaceFetched, String> {
    let url = format!("{PUBLISH_API}/api/simulations/{sim_id}/space");

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let mut req = client.get(&url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }

    let resp = req.send().map_err(|e| format!("GET {url}: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        // Worth naming precisely: a 403 here is the historical failure mode —
        // the worker gates downloads on `sim.is_public`, which publish did not
        // set, so simulations listed publicly were denied on download.
        let body = resp.text().unwrap_or_default();
        return Err(format!("{status} from {url}: {}", body.chars().take(200).collect::<String>()));
    }

    let bytes = resp.bytes().map_err(|e| format!("read body: {e}"))?;
    if bytes.is_empty() {
        return Err("empty .pak".into());
    }

    let dest = cache_root().join(sim_id);
    unpack_pak(&bytes, &dest)?;

    let spaces = find_spaces(&dest);
    Ok(SpaceFetched {
        simulation_id: sim_id.to_string(),
        root: dest,
        spaces,
        bytes: bytes.len(),
    })
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

/// Every `_instance.toml` under `root` — one per authored instance; the Spaces
/// are the directories that contain them.
fn find_spaces(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut guard = 0;

    while let Some(dir) = stack.pop() {
        guard += 1;
        if guard > 20_000 {
            warn!("space scan hit its node budget; results are partial");
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().and_then(|n| n.to_str()) == Some("_instance.toml") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip the real published format: tar + zstd, exactly as
    /// `package_universe_to_pak` produces it.
    #[test]
    fn unpacks_a_pak_built_the_way_studio_builds_them() {
        let tmp = std::env::temp_dir().join("eustress_pak_rt");
        let src = tmp.join("src");
        let out = tmp.join("out");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(src.join("MySpace")).unwrap();
        std::fs::write(src.join("MySpace/_instance.toml"), "[metadata]\nclass_name = \"Space\"\n")
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

        let spaces = find_spaces(&out);
        assert_eq!(spaces.len(), 1, "expected one _instance.toml, got {spaces:?}");
        assert!(out.join("readme.txt").is_file());

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
