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
//!
//! ## Whose sign-in
//!
//! A solo link from the website's Play button carries a one-time ticket
//! (`eustress-player://play/<id>?api=<name>&ticket=<t>`,
//! `docs/networking/PLAYER_JOIN_TICKET.md`, "Solo play pass"). The ticket is
//! redeemed once, `POST {api}/api/simulations/{id}/play-pass`, for a pass that
//! reads this one simulation as the account signed in on the website, and
//! every request then carries `Authorization: PlayPass <pass>`. Studio's
//! sign-in is not sent with the world on such a launch, not even on the
//! fallback below. (The avatar is still fetched with Studio's sign-in, origin
//! checked by `systems::credential`, so the body shown is Studio's account's.)
//! The redeem follows no redirect and, except to a test server on this
//! machine, speaks https only, so the ticket goes nowhere but the API. Without
//! a ticket, Studio's sign-in goes only to the API origin that issued it
//! (`systems::credential`).
//! Neither the ticket nor the pass is ever logged, shown or printed.
//!
//! ## A link that names no server
//!
//! A `play/<id>` link that names no `api`, opened by a Player set up for
//! staging, whose manifest staging answers 404, is tried once more on
//! production ([`fallback_api`]): production is the moderated server, so
//! opening it is always allowed. Never the other way, and never for a link
//! that named its server.

use bevy::prelude::*;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use eustress_common::api_base::{self, PRODUCTION_API, STAGING_API};
use eustress_echk::{content_hash, is_content_hash, WorldManifest};
use eustress_networking::join_link::JoinTicket;
use eustress_networking::session::{assemble_world, ChunkCache, DownloadedWorld, WorldDownloaded};

use super::credential::{self, Credential, CredentialKind};
use super::join_notice::PLAY_TICKET_REFUSED;
use super::live_world::{find_spawn, world_from_universe_dir, DiskChunkCache};

/// Where a published world's players stand when it names no SpawnLocation.
const DEFAULT_SPAWN: [f32; 3] = [0.0, 2.0, 8.0];

/// The ticket a gallery's Play page put in a solo link, for the one fetch
/// that redeems it. Cloning shares it: whichever holder takes it first spends
/// it, and after that it is gone. Its `Debug` prints no ticket text.
#[derive(Clone)]
pub struct PlayTicket(Arc<Mutex<Option<JoinTicket>>>);

impl PlayTicket {
    pub fn new(ticket: JoinTicket) -> Self {
        Self(Arc::new(Mutex::new(Some(ticket))))
    }

    /// The ticket, once.
    fn take(&self) -> Option<JoinTicket> {
        self.0.lock().ok().and_then(|mut held| held.take())
    }
}

impl std::fmt::Debug for PlayTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PlayTicket(..)")
    }
}

/// Ask the Player to download and open a published simulation. Its `Debug`
/// prints no secret: [`Credential`] and [`PlayTicket`] print none.
#[derive(Message, Debug, Clone)]
pub struct FetchSpace {
    pub simulation_id: String,
    /// Studio's sign-in, for a simulation not open to everyone. Sent only to
    /// the API origin that issued it, and not at all when `ticket` is set.
    pub credential: Option<Credential>,
    /// The gallery's one-time ticket from the launching link.
    pub ticket: Option<PlayTicket>,
    /// The API the launching link named (`api=`), if any. Only a link that
    /// named none may fall back to production ([`fallback_api`]).
    pub link_api: Option<&'static str>,
}

#[derive(Message, Debug, Clone)]
pub struct SpaceFetchFailed {
    pub simulation_id: String,
    pub error: String,
    /// The launch carried a gallery's one-time ticket: the way back in is
    /// the gallery, never the same link.
    pub gallery_ticket: bool,
}

/// Why a solo launch fetches from the API it does, for the log: `link names
/// production`, `link names staging`, or `link names none, using <x>`. Never
/// the link itself, which may carry a ticket.
pub fn api_choice(link_api: Option<&str>, run_api: &str) -> String {
    match link_api {
        Some(api) => format!("link names {}", api_base::server_name(api)),
        None => format!("link names none, using {}", api_base::server_name(run_api)),
    }
}

/// Where a solo fetch tries next after `run_api` answered `manifest_status`
/// for the world manifest: production, when the link named no API
/// (`link_named_api` false), this run's API is staging, and staging answered
/// 404. Production is the moderated server, so opening it is always allowed.
/// `None` otherwise: never from production to staging, and never when the
/// link named its server.
pub fn fallback_api(link_named_api: bool, run_api: &str, manifest_status: u16) -> Option<&'static str> {
    if link_named_api || manifest_status != 404 {
        return None;
    }
    (api_base::origin_of(run_api) == api_base::origin_of(STAGING_API)).then_some(PRODUCTION_API)
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
        let ticket = req.ticket.as_ref().and_then(PlayTicket::take);
        let gallery_ticket = ticket.is_some();
        // With the website's ticket, Studio's sign-in is not sent at all.
        let credential = if gallery_ticket { None } else { req.credential.clone() };
        let link_api = req.link_api;
        let tx = chan.tx.clone();

        info!(
            "📥 fetching published simulation {id} ({}; {})",
            api_choice(link_api, api_base::api_base()),
            if gallery_ticket { "as the website's account" } else if credential.is_some() { "with Studio's sign-in" } else { "signed out" }
        );

        std::thread::Builder::new()
            .name(format!("space-fetch-{id}"))
            .spawn(move || {
                let result = match fetch_world(&id, credential, ticket, link_api.is_some()) {
                    Ok(world) => FetchResult::Ok(world),
                    Err(error) => FetchResult::Err(SpaceFetchFailed { simulation_id: id.clone(), error, gallery_ticket }),
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
///
/// `ticket`: the gallery's one-time ticket, redeemed here once for a pass
/// that is then the only credential sent. `credential`: Studio's sign-in,
/// used only without a ticket, and sent only to its own origin.
/// `link_named_api`: the launching link named its server, so no fallback.
fn fetch_world(
    sim_id: &str,
    credential: Option<Credential>,
    ticket: Option<JoinTicket>,
    link_named_api: bool,
) -> Result<DownloadedWorld, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let base = api_base::api_base();
    let gallery_ticket = ticket.is_some();
    let credential = match ticket {
        Some(ticket) => Some(redeem_play_ticket(base, sim_id, ticket)?),
        None => credential,
    };
    match fetch_world_from(&client, base, sim_id, credential.as_ref()) {
        Err(FetchError::Manifest { status, error }) => match fallback_api(link_named_api, base, status) {
            Some(next) => {
                info!(
                    "📥 {} has no simulation {sim_id} and the link names no server; trying {}",
                    api_base::server_name(base),
                    api_base::server_name(next)
                );
                let login = fallback_credential(gallery_ticket, next, credential::studio_login_for);
                fetch_world_from(&client, next, sim_id, login.as_ref()).map_err(FetchError::into_message)
            }
            None => Err(error),
        },
        other => other.map_err(FetchError::into_message),
    }
}

/// The credential a fallback fetch from `next` (production) sends: none after
/// a launch with the gallery's ticket (its pass belongs to the API that issued
/// it), otherwise Studio's own login for `next` (`login_for`, which is
/// [`credential::studio_login_for`]), and only when `next` issued it. A login
/// issued by any other API is never sent there.
fn fallback_credential(
    gallery_ticket: bool,
    next: &str,
    login_for: impl Fn(&str) -> Option<Credential>,
) -> Option<Credential> {
    if gallery_ticket {
        return None;
    }
    login_for(next).filter(|login| login.secret_for(next).is_some())
}

/// Why a fetch from one API failed: its world manifest answered `status`, or
/// anything else.
enum FetchError {
    Manifest { status: u16, error: String },
    Other(String),
}

impl FetchError {
    fn into_message(self) -> String {
        match self {
            FetchError::Manifest { error, .. } | FetchError::Other(error) => error,
        }
    }
}

impl From<String> for FetchError {
    fn from(error: String) -> Self {
        FetchError::Other(error)
    }
}

/// Trade the gallery's one-time ticket for a pass to this simulation at the
/// API `base`. The ticket goes in the request body, never in a URL, and no
/// error quotes it or the answer's pass.
fn redeem_play_ticket(base: &str, sim_id: &str, ticket: JoinTicket) -> Result<Credential, String> {
    let url = format!("{base}/api/simulations/{sim_id}/play-pass");
    let client = redeem_client(base).map_err(|e| format!("{PLAY_TICKET_REFUSED}: http client: {e}"))?;
    let body = serde_json::json!({ "ticket": ticket.into_secret() }).to_string();
    let resp = client
        .post(&url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .map_err(|e| format!("{PLAY_TICKET_REFUSED}: could not reach {url}: {}", e.without_url()))?;
    let status = resp.status().as_u16();
    let text = resp.text().unwrap_or_default();
    pass_from_answer(status, &text, &url)
}

/// The client the ticket is sent with. It follows no redirect: reqwest re-sends
/// a body on a 307 or 308, to any host, so a redirect could carry the ticket
/// away. The API never redirects this route, so a redirect is a refusal.
/// Https only, except to a test server on this machine ([`redeem_https_only`]).
fn redeem_client(base: &str) -> reqwest::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .https_only(redeem_https_only(base))
        .build()
}

/// Whether a redeem to the API at `base` must be https: always, except to
/// `http://localhost` or a loopback address, a test server on this machine.
fn redeem_https_only(base: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(base.trim()) else { return true };
    let host = url.host_str().unwrap_or("").trim_start_matches('[').trim_end_matches(']');
    let local = host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false);
    !(local && url.scheme() == "http")
}

/// The pass in the play-pass route's answer, as a credential for the API at
/// `issuer`, or why there is none: [`PLAY_TICKET_REFUSED`], then the status
/// and the answer's `code` when it is a plain word. Nothing else from the
/// answer is quoted, so no message can carry a ticket or a pass.
fn pass_from_answer(status: u16, answer: &str, issuer: &str) -> Result<Credential, String> {
    let parsed: Option<serde_json::Value> = serde_json::from_str(answer).ok();
    if (200..300).contains(&status) {
        return parsed
            .as_ref()
            .and_then(|v| v.get("pass"))
            .and_then(|p| p.as_str())
            .filter(|p| !p.is_empty() && p.len() <= 2048 && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')))
            .and_then(|p| Credential::issued_by(CredentialKind::PlayPass, p, issuer))
            .ok_or_else(|| format!("{PLAY_TICKET_REFUSED}: {status} with no pass in the answer"));
    }
    let code = parsed
        .as_ref()
        .and_then(|v| v.get("code"))
        .and_then(|c| c.as_str())
        .filter(|c| !c.is_empty() && c.len() <= 32 && c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
        .unwrap_or("unknown");
    Err(format!("{PLAY_TICKET_REFUSED}: {status} {code}"))
}

/// The header a world manifest answer carries its play token in, and the one
/// chunk GETs send it back in (the Worker's `src/world_play_token.mjs`, and
/// docs/architecture/WORLD_PLAY_TOKEN.md).
const PLAY_TOKEN_HEADER: &str = "X-Eustress-Play-Token";
/// The longest token the Worker accepts.
const MAX_PLAY_TOKEN_CHARS: usize = 1024;

/// The play token in a manifest answer's headers, or `None` when there is
/// none or it is not shaped like one (`wpt1.` then base64url, at most
/// [`MAX_PLAY_TOKEN_CHARS`]). It is sent back only to the API that gave it,
/// on that listing's chunk GETs.
fn play_token_of(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get(PLAY_TOKEN_HEADER)?
        .to_str()
        .ok()
        .filter(|t| {
            t.starts_with("wpt1.")
                && t.len() <= MAX_PLAY_TOKEN_CHARS
                && t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        })
        .map(str::to_string)
}

/// Download a published world from the API at `base`, with `credential`
/// sent only where it belongs ([`credential::request`]).
fn fetch_world_from(
    client: &reqwest::blocking::Client,
    base: &str,
    sim_id: &str,
    credential: Option<&Credential>,
) -> Result<DownloadedWorld, FetchError> {
    let get = |url: &str| {
        credential::request(client, reqwest::Method::GET, url, credential)
            .send()
            .map_err(|e| format!("GET {url}: {}", e.without_url()))
    };

    let manifest_url = format!("{base}/api/simulations/{sim_id}/world/manifest");
    let resp = get(&manifest_url)?;
    let status = resp.status();
    if status.as_u16() == 409 {
        // Published before .echk: one .pak of the whole Universe.
        return fetch_legacy_pak(base, sim_id, &get).map_err(FetchError::Other);
    }
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(FetchError::Manifest {
            status: status.as_u16(),
            error: format!("{status} from {manifest_url}: {}", body.chars().take(200).collect::<String>()),
        });
    }
    // The Worker's short-lived pass for this manifest's chunks, when it gave
    // one: a chunk GET that shows it skips the listing lookup. Without it, or
    // once it lapses, every chunk still passes the full gate on `credential`.
    let play_token = play_token_of(resp.headers());
    let manifest: WorldManifest = resp.json().map_err(|e| format!("read the world manifest: {e}"))?;
    manifest.validate().map_err(|e| format!("the world manifest was refused: {e}"))?;

    let cache = DiskChunkCache::default();
    let mut chunks: HashMap<String, Vec<u8>> = HashMap::new();
    for hash in manifest.hashes() {
        if !is_content_hash(&hash) {
            return Err(format!("bad chunk name {hash:?}").into());
        }
        if let Some(bytes) = cache.get(&hash).filter(|b| content_hash(b) == hash) {
            chunks.insert(hash, bytes);
            continue;
        }
        let url = format!("{base}/api/simulations/{sim_id}/world/chunks/{hash}");
        let mut request = credential::request(client, reqwest::Method::GET, &url, credential);
        if let Some(token) = play_token.as_deref() {
            request = request.header(PLAY_TOKEN_HEADER, token);
        }
        let resp = request.send().map_err(|e| format!("GET {url}: {}", e.without_url()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("{status} from {url}").into());
        }
        let bytes = resp.bytes().map_err(|e| format!("read chunk {hash}: {e}"))?.to_vec();
        if content_hash(&bytes) != hash {
            return Err(format!("chunk {hash} does not match its content hash").into());
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
    base: &str,
    sim_id: &str,
    get: &dyn Fn(&str) -> Result<reqwest::blocking::Response, String>,
) -> Result<DownloadedWorld, String> {
    let url = format!("{base}/api/simulations/{sim_id}/download");
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

    /// A ticket's shape. No real ticket or pass is ever written into a test.
    const TICKET: &str = "eit1.c2VjcmV0LXRpY2tldA.c2ln-_x";
    const PASS: &str = "epp1.c2VjcmV0LXBhc3M.c2ln-_y";

    /// Only a link that named no server, opened on staging, whose manifest
    /// staging answered 404, is tried again, and only on production.
    #[test]
    fn solo_fallback_is_staging_to_production_for_a_link_with_no_server() {
        assert_eq!(fallback_api(false, STAGING_API, 404), Some(PRODUCTION_API));
        assert_eq!(fallback_api(false, "https://api-staging.eustress.dev/", 404), Some(PRODUCTION_API), "a trailing slash");
        assert_eq!(fallback_api(true, STAGING_API, 404), None, "the link named its server");
        assert_eq!(fallback_api(false, PRODUCTION_API, 404), None, "never from production");
        assert_eq!(fallback_api(false, "http://localhost:8787", 404), None, "never from a test server");
        for status in [200, 401, 403, 409, 500, 503] {
            assert_eq!(fallback_api(false, STAGING_API, status), None, "{status}");
        }
    }

    /// The log says which API a launch used and why, and nothing from the link.
    #[test]
    fn the_log_names_the_api_and_why() {
        assert_eq!(api_choice(Some(PRODUCTION_API), STAGING_API), "link names production");
        assert_eq!(api_choice(Some(STAGING_API), STAGING_API), "link names staging");
        assert_eq!(api_choice(None, STAGING_API), "link names none, using staging");
        assert_eq!(api_choice(None, PRODUCTION_API), "link names none, using production");
    }

    /// A manifest answer's play token is taken only when it is shaped like
    /// one the Worker mints; anything else leaves the chunks on the full gate.
    #[test]
    fn a_play_token_is_taken_only_in_the_workers_shape() {
        let with = |value: &str| {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(PLAY_TOKEN_HEADER, reqwest::header::HeaderValue::from_str(value).unwrap());
            play_token_of(&headers)
        };
        let good = "wpt1.eyJ2IjoxfQ.AbC-_09";
        assert_eq!(with(good).as_deref(), Some(good));
        assert_eq!(play_token_of(&reqwest::header::HeaderMap::new()), None);
        assert_eq!(with("Bearer eit1.session"), None, "not a play token");
        assert_eq!(with("wpt1.a b.c"), None, "a space is not base64url");
        assert_eq!(with("wpt1.a/b.c"), None, "neither is a slash");
        assert_eq!(with(&format!("wpt1.{}", "a".repeat(MAX_PLAY_TOKEN_CHARS))), None, "too long");
    }

    /// The play-pass answer becomes a pass bound to the API that issued it,
    /// and a refusal names its status and code, never the ticket or a pass,
    /// even when the answer quotes them.
    #[test]
    fn a_play_pass_answer_never_quotes_a_ticket_or_a_pass() {
        let issuer = "https://api-staging.eustress.dev/api/simulations/07d0/play-pass";
        let pass = pass_from_answer(200, &format!(r#"{{"pass":"{PASS}","expires":1}}"#), issuer).expect("a pass");
        assert_eq!(pass.kind(), CredentialKind::PlayPass);
        assert_eq!(pass.origin(), STAGING_API);
        assert_eq!(pass.secret_for("https://api-staging.eustress.dev/api/simulations/07d0/world/manifest"), Some(PASS));
        assert_eq!(pass.secret_for("https://api.eustress.dev/api/simulations/07d0/world/manifest"), None);
        assert!(!format!("{pass:?}").contains(PASS), "Debug printed the pass");

        let used = pass_from_answer(403, r#"{"error":"This Play link was already used.","code":"used"}"#, issuer).unwrap_err();
        assert_eq!(used, format!("{PLAY_TICKET_REFUSED}: 403 used"));
        for echoed in [
            format!(r#"{{"error":"{TICKET}","code":"{TICKET}"}}"#),
            format!(r#"{{"error":"bad","code":"invalid","ticket":"{TICKET}","pass":"{PASS}"}}"#),
            TICKET.to_string(),
        ] {
            let e = pass_from_answer(401, &echoed, issuer).unwrap_err();
            assert!(e.starts_with(PLAY_TICKET_REFUSED), "{e}");
            assert!(!e.contains(TICKET) && !e.contains(PASS), "{e}");
        }
        for bad in [format!(r#"{{"pass":"{PASS} x"}}"#), format!(r#"{{"pass":"{PASS}\r\nX: 1"}}"#), r#"{"pass":""}"#.into(), "{}".into(), TICKET.into()] {
            let e = pass_from_answer(200, &bad, issuer).unwrap_err();
            assert!(!e.contains(PASS) && !e.contains(TICKET), "{e}");
        }
    }

    /// The ticket goes only to https, except to a test server on this machine.
    #[test]
    fn a_redeem_is_https_only_except_on_this_machine() {
        for remote in [STAGING_API, PRODUCTION_API, "http://api-staging.eustress.dev", "http://203.0.113.7:8787", "http://localhost.evil.com", "not a url"] {
            assert!(redeem_https_only(remote), "{remote}");
        }
        for local in ["http://localhost:8787", "http://127.0.0.1:8787", "http://[::1]:8787", "http://LOCALHOST"] {
            assert!(!redeem_https_only(local), "{local}");
        }
        assert!(redeem_https_only("https://localhost:8787"), "https stays https");
    }

    /// A 307 or 308 from the redeem route is a refusal: the ticket in the body
    /// is never sent on to where it points (reqwest re-sends a body on both).
    #[test]
    fn a_redeem_follows_no_redirect() {
        use std::io::{Read as _, Write as _};
        use std::sync::atomic::{AtomicBool, Ordering};
        let api = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let api_port = api.local_addr().unwrap().port();
        let elsewhere = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let elsewhere_port = elsewhere.local_addr().unwrap().port();
        let reached = Arc::new(AtomicBool::new(false));
        let seen = reached.clone();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = elsewhere.accept() {
                seen.store(true, Ordering::SeqCst);
                let _ = s.read(&mut [0u8; 4096]);
                let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        });
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = api.accept() {
                let _ = s.read(&mut [0u8; 4096]);
                let answer = format!(
                    "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:{elsewhere_port}/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = s.write_all(answer.as_bytes());
            }
        });
        let split = eustress_networking::join_link::split_link_extras(&format!("eustress-player://play/07d0?ticket={TICKET}"))
            .expect("the link parses");
        let e = redeem_play_ticket(&format!("http://127.0.0.1:{api_port}"), "07d0", split.1.ticket.expect("a ticket"))
            .expect_err("a redirect is no pass");
        assert_eq!(e, format!("{PLAY_TICKET_REFUSED}: 307 unknown"));
        assert!(!reached.load(Ordering::SeqCst), "the ticket was sent on to the redirect's target");
    }

    /// The staging-to-production fallback sends Studio's production login
    /// only, and none at all after a launch with the gallery's ticket.
    #[test]
    fn the_fallback_sends_only_productions_own_login() {
        let production = || Credential::issued_by(CredentialKind::StudioToken, "eit1.production-session", PRODUCTION_API);
        let staging = || Credential::issued_by(CredentialKind::StudioToken, "eit1.staging-session", STAGING_API);
        let asked = std::cell::RefCell::new(Vec::new());
        let login_for = |base: &str| {
            asked.borrow_mut().push(base.to_string());
            if base == PRODUCTION_API { production() } else { staging() }
        };
        let sent = fallback_credential(false, PRODUCTION_API, &login_for).expect("production's own login");
        assert_eq!(sent.origin(), PRODUCTION_API);
        assert_eq!(sent.secret_for("https://api.eustress.dev/api/simulations/x/world/manifest"), Some("eit1.production-session"));
        assert_eq!(asked.borrow().as_slice(), [PRODUCTION_API.to_string()]);
        // A loader that hands back another API's login sends nothing.
        assert!(fallback_credential(false, PRODUCTION_API, |_: &str| staging()).is_none(), "staging's login never goes to production");
        // A ticket launch sends no Studio login on the fallback, and does not even read one.
        asked.borrow_mut().clear();
        assert!(fallback_credential(true, PRODUCTION_API, &login_for).is_none());
        assert!(asked.borrow().is_empty());
    }

    /// The fetch request prints no secret, and a gallery ticket is spent by
    /// whichever holder takes it first.
    #[test]
    fn a_fetch_request_prints_no_secret_and_its_ticket_is_taken_once() {
        let split = eustress_networking::join_link::split_link_extras(&format!("eustress-player://play/07d0?api=staging&ticket={TICKET}"))
            .expect("the link parses");
        let ticket = PlayTicket::new(split.1.ticket.expect("a ticket"));
        let studio = Credential::issued_by(CredentialKind::StudioToken, "eit1.studio-session", STAGING_API).unwrap();
        let req = FetchSpace { simulation_id: "07d0".into(), credential: Some(studio), ticket: Some(ticket.clone()), link_api: Some(STAGING_API) };
        let shown = format!("{req:?}");
        assert!(!shown.contains(TICKET) && !shown.contains("eit1.studio-session"), "{shown}");
        let copy = req.clone();
        assert_eq!(copy.ticket.as_ref().and_then(PlayTicket::take).map(JoinTicket::into_secret).as_deref(), Some(TICKET));
        assert!(req.ticket.as_ref().and_then(PlayTicket::take).is_none(), "spent once, by the first holder");
        assert!(ticket.take().is_none());
    }
}
