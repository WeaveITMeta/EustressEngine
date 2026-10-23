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
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use eustress_networking::session::{DownloadedWorld, WorldDownloaded};

use super::live_world::data_root;

pub use eustress_networking::published::{is_unsafe_archive_path, unpack_pak, world_from_universe_dir};

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
/// The requests live in [`eustress_networking::published`] so the mobile
/// Player calls the same function.
fn fetch_world(sim_id: &str, token: Option<&str>) -> Result<DownloadedWorld, String> {
    eustress_networking::published::fetch_published(eustress_common::api_base::api_base(), sim_id, token, &data_root())
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
