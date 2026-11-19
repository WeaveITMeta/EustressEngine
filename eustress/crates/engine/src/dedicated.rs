//! The dedicated server Studio launches, and the process `--server` runs.
//!
//! Studio stays in Edit. It mints a server credential for the open Space's
//! staging listing and starts `eustress-engine --server`, which downloads
//! that published world, hosts the Player's WebTransport session on UDP
//! 7777, and heartbeats `POST /api/simulations/{id}/live`. Stop writes a
//! file the child notices; the child then tells the API the session ended.

use std::net::{SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use parking_lot::Mutex;

use eustress_common::api_base::STAGING_API;

/// Asked by Ctrl+C / SIGTERM, and by the stop file.
static STOP_ASKED: AtomicBool = AtomicBool::new(false);

/// UDP port the Player and the browser join on.
pub const HOST_PORT: u16 = 7777;

/// Staging is the default API for a dedicated server.
pub const DEFAULT_ENV: &str = "staging";

pub fn stop_asked() -> bool {
    STOP_ASKED.load(Ordering::SeqCst)
}

/// Arm Ctrl+C and SIGTERM. The handler only stores a flag.
pub fn arm_stop_signals() {
    #[cfg(unix)]
    {
        unsafe extern "C" fn handle(_: i32) {
            STOP_ASKED.store(true, Ordering::SeqCst);
        }
        unsafe extern "C" {
            fn signal(sig: i32, handler: unsafe extern "C" fn(i32)) -> usize;
        }
        unsafe {
            signal(2, handle);
            signal(15, handle);
        }
    }
    #[cfg(windows)]
    {
        unsafe extern "system" fn handle(ctrl: u32) -> i32 {
            // CTRL_C_EVENT, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT.
            if ctrl <= 2 {
                STOP_ASKED.store(true, Ordering::SeqCst);
                1
            } else {
                0
            }
        }
        unsafe extern "system" {
            fn SetConsoleCtrlHandler(handler: unsafe extern "system" fn(u32) -> i32, add: i32) -> i32;
        }
        unsafe {
            SetConsoleCtrlHandler(handle, 1);
        }
    }
}

/// Where the child writes its status, next to the open Universe.
pub fn status_path_for_space(space_root: &Path) -> PathBuf {
    let universe = crate::space::universe_root_for_path(space_root).unwrap_or_else(|| space_root.to_path_buf());
    universe.join(".eustress").join("dedicated-server.json")
}

pub fn stop_path(status_file: &Path) -> PathBuf {
    status_file.with_file_name("dedicated.stop")
}

pub fn log_path(status_file: &Path) -> PathBuf {
    status_file.with_file_name("dedicated-server.log")
}

/// Ask the child to leave. It deletes the live marker, then exits.
pub fn request_stop(status_file: &Path) {
    if let Some(dir) = status_file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(stop_path(status_file), b"stop\n");
}

pub fn stop_file_present(status_file: &Path) -> bool {
    stop_path(status_file).is_file()
}

/// The status file's `phase` when the server left because nobody was in it
/// for `EUSTRESS_SERVER_EMPTY_EXIT_SECS`. A fleet reads it to tell an empty
/// exit from a crash or a stop.
pub const EMPTY_EXIT_PHASE: &str = "exited-empty";

/// The status file's `phase` when the server left because it could not host:
/// the world bake, the bind or the listener failed, so nobody could join.
/// The process ends with a non-zero code. A fleet frees the slot and starts
/// a fresh server.
pub const HOST_FAILED_PHASE: &str = "host-failed";

/// The shortest empty wait `EUSTRESS_SERVER_EMPTY_EXIT_SECS` may ask for:
/// two gallery heartbeats (30 s apart), so a session the gallery has just
/// listed is not gone before a player can click Play.
pub const MIN_EMPTY_EXIT_SECS: f64 = 60.0;

/// Seconds an empty server waits before it leaves, from
/// `EUSTRESS_SERVER_EMPTY_EXIT_SECS`. Unset, empty, not a number, not
/// finite, zero or negative means never; anything shorter than
/// [`MIN_EMPTY_EXIT_SECS`] reads as that.
pub fn empty_exit_secs_from(raw: Option<&str>) -> Option<f64> {
    raw.and_then(|text| text.trim().parse::<f64>().ok())
        .filter(|secs| secs.is_finite() && *secs > 0.0)
        .map(|secs| secs.max(MIN_EMPTY_EXIT_SECS))
}

pub fn empty_exit_secs_from_env() -> Option<f64> {
    empty_exit_secs_from(std::env::var("EUSTRESS_SERVER_EMPTY_EXIT_SECS").ok().as_deref())
}

/// The open Universe's listing on the staging API.
pub fn staging_listing_id(space_root: &Path) -> Result<String, String> {
    let universe = crate::space::universe_root_for_path(space_root).unwrap_or_else(|| space_root.to_path_buf());
    let path = universe.join(".eustress").join("sync.toml");
    let sync = eustress_common::load_toml_file::<eustress_common::SyncManifest>(&path)
        .map_err(|_| format!("this Universe has no listing record at {}", path.display()))?;
    let origin = api_origin_of(STAGING_API);
    sync.remote
        .listing_id_for(&origin)
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "this Universe has no listing on {STAGING_API}. Publish it to staging first. The id is kept in sync.toml under [{origin}]."
            )
        })
}

fn api_origin_of(base: &str) -> String {
    // The same origin `RemoteState::listing_id_for` stores. `api_origin()`
    // follows the process's own API, which may be production while this
    // lookup is for staging, so the staging origin is computed on its own.
    eustress_common::api_base::origin_of(base)
}

/// A public join address: IPv4 or a name, optional `:port`. Loopback and
/// link-local are refused because the gallery refuses them too.
pub fn check_public_host(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("Enter the public address players will use. Forward UDP 7777 to this computer, then put that address here.".into());
    }
    if raw.contains(char::is_whitespace) || raw.contains('/') || raw.starts_with('[') {
        return Err(format!(
            "{raw} is not an IPv4 address or a name. The host listens on IPv4 only."
        ));
    }
    let host = match raw.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !host.contains(':') => {
            let port: u16 = port.parse().map_err(|_| format!("{port} is not a UDP port"))?;
            if port == 0 {
                return Err("The port must be 1 to 65535.".into());
            }
            host
        }
        Some(_) => {
            return Err(format!(
                "{raw} is an IPv6 address. The host listens on IPv4 only. Use this computer's public IPv4 address."
            ));
        }
        None => raw,
    };
    let lower = host.trim_end_matches('.').to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") {
        return Err("localhost is this computer. Players on the internet cannot join it. Use the public IPv4 address.".into());
    }
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        let [a, b, _, _] = ip.octets();
        if a == 127 || a == 0 || (a == 169 && b == 254) {
            return Err(format!(
                "{ip} is loopback or link-local. Players outside this computer cannot join it. Use the public IPv4 address, or a LAN address your players can route to."
            ));
        }
    }
    Ok(raw.to_string())
}

/// Cache directory for one downloaded listing. Not the Space Studio has open.
pub fn download_dir(sim_id: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".eustress_engine")
        .join("dedicated")
        .join(sim_id)
}

/// The directory one server downloads its listing into.
///
/// A start clears this directory before it writes the world
/// (`gallery_edit::download_for_host`), so it is one server's own and never
/// holds anything else. A server a launcher named (`EUSTRESS_SERVER_ID`) and
/// gave a status file downloads into `world`, beside that status file: the
/// launcher gives each server its own status directory, so two servers of
/// one listing on one machine never clear each other's world, status file or
/// stop file. Any other server downloads into
/// `<listing cache>/<server id>/world`.
pub fn server_download_dir(sim_id: &str, server_id: &str, status_file: Option<&Path>) -> PathBuf {
    match status_file.and_then(Path::parent) {
        Some(dir) if server_id != "default" && !dir.as_os_str().is_empty() => dir.join("world"),
        _ => download_dir(sim_id).join(server_id).join("world"),
    }
}

/// What the child writes, and what the status window reads.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct DedicatedStatus {
    pub pid: u32,
    pub phase: String,
    pub port: u16,
    pub listening: bool,
    pub pin: String,
    pub join_link: String,
    pub public_host: String,
    pub players: usize,
    /// Seats taken: the players in the world and the ones still joining
    /// (`HostSession::seated_count`). A fleet places by this, because a
    /// player downloading the world is not in `players` yet.
    #[serde(default)]
    pub seated: usize,
    pub max_players: u16,
    pub sim_id: String,
    pub server_scripts: bool,
    pub script_note: String,
    pub probe: String,
    pub error: String,
    /// Stable job id (`EUSTRESS_SERVER_ID`), not a pid or a port.
    #[serde(default)]
    pub server_id: String,
    #[serde(default)]
    pub region: String,
    /// Round trip to the API, once measured. Absent until then. Never 0.
    #[serde(default)]
    pub latency_ms: Option<u32>,
    #[serde(default)]
    pub gpu: String,
    #[serde(default)]
    pub cpu: String,
    #[serde(default)]
    pub hardware_tier: String,
}

pub fn write_status(path: &Path, status: &DedicatedStatus) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(text) = serde_json::to_string_pretty(status) else { return };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

pub fn read_status(path: &Path) -> Option<DedicatedStatus> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Environment for `--server`, before anything reads `EUSTRESS_API_URL`.
///
/// `--api-url` wins. Otherwise `--env` selects staging or production.
/// Otherwise a URL already in the environment is kept. Otherwise the
/// dedicated server uses the staging API.
pub fn apply_server_env(args: &crate::startup::StartupArgs) {
    if std::env::var_os("EUSTRESS_HOST_ON_PLAY").is_none() {
        std::env::set_var("EUSTRESS_HOST_ON_PLAY", "1");
    }
    if std::env::var_os("EUSTRESS_HOST_LAN").is_none() {
        std::env::set_var("EUSTRESS_HOST_LAN", "1");
    }
    if let Some(port) = args.port {
        std::env::set_var("EUSTRESS_HOST_PORT", port.to_string());
    } else if std::env::var_os("EUSTRESS_HOST_PORT").is_none() {
        std::env::set_var("EUSTRESS_HOST_PORT", HOST_PORT.to_string());
    }
    if let Some(public) = args.public_host.as_ref().filter(|h| !h.trim().is_empty()) {
        std::env::set_var("EUSTRESS_HOST_PUBLIC", public);
    }
    if let Some(url) = args.api_url.as_ref().filter(|u| !u.trim().is_empty()) {
        std::env::set_var("EUSTRESS_API_URL", url);
    } else if let Some(name) = args.api_env.as_deref() {
        if let Some(base) = eustress_common::api_base::api_for_name(name) {
            std::env::set_var("EUSTRESS_API_URL", base);
        }
    } else if std::env::var_os("EUSTRESS_API_URL").is_none() {
        std::env::set_var("EUSTRESS_API_URL", STAGING_API);
    }
}

fn token_shape(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Job id from the launcher. `default` when the process was not given one.
pub fn server_id_from_env() -> String {
    std::env::var("EUSTRESS_SERVER_ID").ok().filter(|s| token_shape(s)).unwrap_or_else(|| "default".into())
}

/// Region name the host reports. Empty when it was not set.
pub fn region_from_env() -> Option<String> {
    std::env::var("EUSTRESS_HOST_REGION").ok().filter(|s| token_shape(s))
}

/// Put a server credential in `EUSTRESS_SERVER_TOKEN` when the launcher
/// passed the author's session. A failure is printed. The author's token
/// is never placed on the command line.
pub fn ensure_server_token(sim_id: Option<&str>) {
    if std::env::var("EUSTRESS_SERVER_TOKEN").ok().is_some_and(|t| !t.trim().is_empty()) {
        return;
    }
    let Some(author) = std::env::var("EUSTRESS_AUTHOR_TOKEN").ok().filter(|t| !t.trim().is_empty()) else {
        eprintln!("eustress-engine --server: no server credential. The heartbeat will fail until one exists.");
        return;
    };
    if author.starts_with("esrv_") {
        std::env::set_var("EUSTRESS_SERVER_TOKEN", author);
        return;
    }
    let Some(sim_id) = sim_id else {
        eprintln!("eustress-engine --server: no listing id, so a server credential was not minted. The heartbeat will fail until one exists.");
        return;
    };
    let api = std::env::var("EUSTRESS_API_URL").unwrap_or_else(|_| STAGING_API.to_string());
    match mint_server_credential(&api, sim_id, &author) {
        Ok(token) => {
            std::env::set_var("EUSTRESS_SERVER_TOKEN", token);
        }
        Err(error) => {
            eprintln!("eustress-engine --server: {error}");
            if error.contains("401") {
                eprintln!("A production sign-in is refused by the staging API. Sign in to staging ({STAGING_API}) and allocate again.");
            }
            eprintln!("The heartbeat will fail until a server credential exists.");
        }
    }
}

fn part_tier(text: &str) -> u8 {
    let t = text.to_ascii_lowercase();
    if t.trim().is_empty() {
        return 0;
    }
    const HIGH: &[&str] = &["4090", "4080", "3090", "3080", "a100", "h100", "7900", "6900", "threadripper", "xeon", "epyc", "i9", "ryzen 9", "ryzen9"];
    const MID: &[&str] = &["4070", "4060", "3070", "3060", "2080", "2070", "6800", "6700", "arc ", "i7", "i5", "ryzen 7", "ryzen 5", "ryzen7", "ryzen5"];
    const LOW: &[&str] = &["uhd", "iris", "vega", "celeron", "atom", "i3", "ryzen 3"];
    if HIGH.iter().any(|k| t.contains(k)) {
        return 3;
    }
    if MID.iter().any(|k| t.contains(k)) {
        return 2;
    }
    if LOW.iter().any(|k| t.contains(k)) {
        return 1;
    }
    1
}

fn tier_name(tier: u8) -> &'static str {
    match tier {
        3 => "high",
        2 => "mid",
        1 => "low",
        _ => "unknown",
    }
}

fn cpu_model() -> String {
    let Ok(text) = std::fs::read_to_string("/proc/cpuinfo") else { return String::new() };
    text.lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == "model name").then(|| value.trim().to_string())
        })
        .unwrap_or_default()
}

// The fleet host agent's GPU read (`eustress-forge`, `gpu_probe.rs`), compiled
// here as well so both report a GPU the same way: an operating-system query
// (`nvidia-smi`, `/sys/class/drm`, the registry), never a GPU device.
#[allow(dead_code)]
#[path = "../../forge/src/gpu_probe.rs"]
mod gpu_probe;

/// GPU and CPU class for the heartbeat. An empty GPU is unknown or none, not
/// a guessed model. The GPU is read on a background thread the first time
/// this is asked, so a frame never waits on it; until the read answers, the
/// class carries no GPU and is not kept.
pub fn hardware_class() -> serde_json::Value {
    static CACHED: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    if let Some(done) = CACHED.get() {
        return done.clone();
    }
    gpu_probe::start();
    let cpu = cpu_model();
    match gpu_probe::ready() {
        Some(gpu) => CACHED.get_or_init(|| hardware_json(&gpu.name, gpu.vram_mb, gpu.available, &cpu)).clone(),
        None => hardware_json("", 0, false, &cpu),
    }
}

/// The heartbeat's `hardware`: `gpu`, `cpu` and `tier` as every host sends
/// them, and `gpu_vram_mb` (0 where the platform does not expose it) and
/// `gpu_available` (a hardware GPU, not a software or display-only adapter),
/// which the API reads when present.
pub fn hardware_json(gpu: &str, gpu_vram_mb: u64, gpu_available: bool, cpu: &str) -> serde_json::Value {
    let tier = part_tier(gpu).max(part_tier(cpu));
    serde_json::json!({
        "gpu": gpu,
        "cpu": cpu,
        "tier": tier_name(tier),
        "gpu_vram_mb": gpu_vram_mb,
        "gpu_available": gpu_available,
    })
}

static LATENCY_MS: Mutex<Option<u32>> = Mutex::new(None);
static LATENCY_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Time one GET to the API in use. The result is omitted until it returns,
/// and a 0 ms result is not stored.
pub fn start_api_latency_probe() {
    if LATENCY_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        let url = eustress_common::api_base::api_url("/");
        let started = std::time::Instant::now();
        let _ = ureq::get(&url).timeout(Duration::from_secs(5)).call();
        let ms = started.elapsed().as_millis();
        if (1..=60_000).contains(&ms) {
            *LATENCY_MS.lock() = Some(ms as u32);
        }
    });
}

pub fn api_latency_ms() -> Option<u32> {
    *LATENCY_MS.lock()
}

/// Fields added to the gallery heartbeat. `cert_sha256` is the pin in the
/// join link. Latency is included only after it has been measured.
pub fn heartbeat_extra(pin: Option<[u8; 32]>) -> serde_json::Value {
    let mut body = serde_json::json!({
        "server_id": server_id_from_env(),
        "hardware": hardware_class(),
    });
    if let Some(pin) = pin {
        body["cert_sha256"] = serde_json::json!(eustress_networking::join_link::hex32(&pin));
    }
    if let Some(region) = region_from_env() {
        body["region"] = serde_json::json!(region);
    }
    if let Some(ms) = api_latency_ms() {
        body["latency_ms"] = serde_json::json!(ms);
    }
    body
}

/// Mint `esrv_…` for `sim_id`. The author's staging session is `author_token`.
pub fn mint_server_credential(api: &str, sim_id: &str, author_token: &str) -> Result<String, String> {
    let url = format!("{}/api/simulations/{sim_id}/server-credential", api.trim_end_matches('/'));
    let response = ureq::post(&url)
        .timeout(Duration::from_secs(20))
        .set("Authorization", &format!("Bearer {author_token}"))
        .send_json(serde_json::json!({}))
        .map_err(|e| match e {
            ureq::Error::Status(status, resp) => {
                let body = resp.into_string().unwrap_or_default();
                let message = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                    .unwrap_or(body);
                format!("the API refused the server credential ({status}): {message}")
            }
            ureq::Error::Transport(t) => format!("could not reach {api} to mint a server credential: {t}"),
        })?;
    let value: serde_json::Value = response.into_json().map_err(|e| format!("the credential reply was unreadable: {e}"))?;
    value
        .get("token")
        .and_then(|t| t.as_str())
        .filter(|t| t.starts_with("esrv_"))
        .map(str::to_string)
        .ok_or_else(|| "the API did not return a server credential".into())
}

pub struct LaunchSpec {
    pub sim_id: String,
    pub public_host: String,
    pub token: String,
    pub status_file: PathBuf,
    pub api: String,
}

/// Start the dedicated server. The token stays in the environment, not argv.
pub fn spawn_dedicated(spec: &LaunchSpec) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| format!("could not find this program to launch the server: {e}"))?;
    if let Some(dir) = spec.status_file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    let _ = std::fs::remove_file(stop_path(&spec.status_file));
    let log = std::fs::File::create(log_path(&spec.status_file)).map_err(|e| format!("open the server log: {e}"))?;
    let err = log.try_clone().map_err(|e| format!("open the server log: {e}"))?;
    Command::new(exe)
        .args([
            "--server",
            "--sim",
            &spec.sim_id,
            "--env",
            "staging",
            "--api-url",
            &spec.api,
            "--public",
            &spec.public_host,
            "--port",
            &HOST_PORT.to_string(),
            "--status-file",
            &spec.status_file.display().to_string(),
        ])
        .env("EUSTRESS_SERVER_TOKEN", &spec.token)
        .env("EUSTRESS_API_URL", &spec.api)
        .env("EUSTRESS_HOST_LAN", "1")
        .env("EUSTRESS_HOST_PUBLIC", &spec.public_host)
        .env("EUSTRESS_HOST_PORT", HOST_PORT.to_string())
        .env("EUSTRESS_HOST_ON_PLAY", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| format!("could not start the dedicated server: {e}"))
}

pub fn log_tail(path: &Path, max_lines: usize) -> String {
    let Ok(text) = std::fs::read_to_string(path) else { return String::new() };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

/// Studio's handle on the child it started.
#[derive(Resource, Clone)]
pub struct LocalDedicated {
    inner: Arc<Mutex<LocalInner>>,
}

struct LocalInner {
    child: Option<Child>,
    status_file: Option<PathBuf>,
    error: Option<String>,
    phase: String,
}

impl Default for LocalDedicated {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(LocalInner {
                child: None,
                status_file: None,
                error: None,
                phase: "stopped".into(),
            })),
        }
    }
}

impl LocalDedicated {
    pub fn phase(&self) -> String {
        self.inner.lock().phase.clone()
    }

    pub fn error(&self) -> Option<String> {
        self.inner.lock().error.clone()
    }

    pub fn status_file(&self) -> Option<PathBuf> {
        self.inner.lock().status_file.clone()
    }

    pub fn set_error(&self, error: impl Into<String>) {
        let mut inner = self.inner.lock();
        inner.error = Some(error.into());
        inner.phase = "error".into();
    }

    pub fn mark_starting(&self, status_file: PathBuf) {
        let mut inner = self.inner.lock();
        inner.status_file = Some(status_file);
        inner.error = None;
        inner.phase = "starting".into();
    }

    /// Mint a credential and start the child. Returns immediately.
    pub fn launch(&self, request: LaunchRequest) {
        self.mark_starting(request.status_file.clone());
        let slot = self.inner.clone();
        std::thread::spawn(move || match prepare_and_spawn(&request) {
            Ok(child) => store_child(&slot, child),
            Err(error) => store_launch_error(&slot, error),
        });
    }

    /// Write the stop file and wait. Kill the process if it is still there.
    pub fn stop(&self) {
        let path = self.inner.lock().status_file.clone();
        if let Some(path) = path {
            request_stop(&path);
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let mut inner = self.inner.lock();
            let still_running = match inner.child.as_mut() {
                None => false,
                Some(child) => matches!(child.try_wait(), Ok(None)) && std::time::Instant::now() < deadline,
            };
            if still_running {
                drop(inner);
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            if let Some(mut child) = inner.child.take() {
                if child.try_wait().ok().flatten().is_none() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
            inner.phase = "stopped".into();
            return;
        }
    }

    pub fn poll(&self) -> Option<DedicatedStatus> {
        let mut inner = self.inner.lock();
        if let Some(child) = inner.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let tail = inner.status_file.as_ref().map(|p| log_tail(&log_path(p), 30)).unwrap_or_default();
                    inner.error = Some(if status.success() {
                        "The dedicated server stopped.".into()
                    } else {
                        format!("The dedicated server exited ({status}).\n{tail}")
                    });
                    inner.phase = "stopped".into();
                    inner.child = None;
                }
                Ok(None) => {}
                Err(e) => {
                    inner.error = Some(format!("could not check the dedicated server: {e}"));
                    inner.phase = "error".into();
                }
            }
        }
        let path = inner.status_file.clone()?;
        read_status(&path)
    }
}

/// What Studio asks the launch thread to start.
pub struct LaunchRequest {
    pub sim_id: String,
    pub public_host: String,
    pub author_token: String,
    pub status_file: PathBuf,
    pub api: String,
}

fn prepare_and_spawn(request: &LaunchRequest) -> Result<Child, String> {
    let token = mint_server_credential(&request.api, &request.sim_id, &request.author_token)?;
    spawn_dedicated(&LaunchSpec {
        sim_id: request.sim_id.clone(),
        public_host: request.public_host.clone(),
        token,
        status_file: request.status_file.clone(),
        api: request.api.clone(),
    })
}

fn store_child(slot: &Mutex<LocalInner>, child: Child) {
    let mut inner = slot.lock();
    inner.child = Some(child);
    inner.phase = "running".into();
    inner.error = None;
}

fn store_launch_error(slot: &Mutex<LocalInner>, error: String) {
    let mut inner = slot.lock();
    inner.error = Some(error);
    inner.phase = "error".into();
}

// -----------------------------------------------------------------------------
// UDP 7777 self-test
// -----------------------------------------------------------------------------

const STUN_COOKIE: u32 = 0x2112_A442;
const STUN_SERVERS: &[&str] = &["stun.l.google.com:19302", "stun.cloudflare.com:3478"];

/// A Binding request, 20 bytes, with `transaction` as its id.
pub fn stun_binding_request(transaction: &[u8; 12]) -> [u8; 20] {
    let mut packet = [0u8; 20];
    packet[0] = 0x00;
    packet[1] = 0x01;
    packet[4..8].copy_from_slice(&STUN_COOKIE.to_be_bytes());
    packet[8..20].copy_from_slice(transaction);
    packet
}

/// The XOR-MAPPED-ADDRESS or MAPPED-ADDRESS in a Binding success, if the
/// transaction id matches.
pub fn parse_stun_mapped(packet: &[u8], transaction: &[u8; 12]) -> Option<(String, u16)> {
    if packet.len() < 20 || packet[0..2] != [0x01, 0x01] {
        return None;
    }
    if packet[4..8] != STUN_COOKIE.to_be_bytes() || &packet[8..20] != transaction {
        return None;
    }
    let length = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    let end = (20 + length).min(packet.len());
    let mut offset = 20;
    while offset + 4 <= end {
        let kind = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
        let attr_len = u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]) as usize;
        let value = offset + 4;
        let next = value + attr_len;
        if next > packet.len() {
            return None;
        }
        if (kind == 0x0020 || kind == 0x0001) && attr_len >= 8 && packet[value + 1] == 0x01 {
            let mut port = u16::from_be_bytes([packet[value + 2], packet[value + 3]]);
            let mut addr = [packet[value + 4], packet[value + 5], packet[value + 6], packet[value + 7]];
            if kind == 0x0020 {
                port ^= (STUN_COOKIE >> 16) as u16;
                let cookie = STUN_COOKIE.to_be_bytes();
                for (byte, mask) in addr.iter_mut().zip(cookie) {
                    *byte ^= mask;
                }
            }
            return Some((std::net::Ipv4Addr::from(addr).to_string(), port));
        }
        // Attributes are padded to 4 bytes.
        offset = next + (4 - attr_len % 4) % 4;
    }
    None
}

/// This computer's IPv4 address on the way to the internet. No packet is sent.
pub fn lan_ipv4() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip.to_string()),
        _ => None,
    }
}

/// Bind UDP `port`, ask a public STUN server where that socket appears, and
/// release the port. The words are what the status window shows.
pub fn probe_udp_port(port: u16) -> String {
    let lan = lan_ipv4();
    let socket = match UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], port))) {
        Ok(socket) => socket,
        Err(e) => {
            return format!(
                "UDP {port} is not free on this computer ({e}). Stop the other program that holds it, then start again.\n{}",
                forward_guidance(lan.as_deref(), None, None)
            );
        }
    };
    let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
    let transaction = {
        let mut id = [0u8; 12];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(1);
        id.copy_from_slice(&now.to_le_bytes()[..12]);
        id
    };
    let request = stun_binding_request(&transaction);
    let mut mapped = None;
    for server in STUN_SERVERS {
        if socket.send_to(&request, *server).is_err() {
            continue;
        }
        let mut buf = [0u8; 512];
        if let Ok((n, _)) = socket.recv_from(&mut buf) {
            if let Some(found) = parse_stun_mapped(&buf[..n], &transaction) {
                mapped = Some(found);
                break;
            }
        }
    }
    drop(socket);
    forward_guidance(lan.as_deref(), mapped.as_ref().map(|(ip, _)| ip.as_str()), mapped.as_ref().map(|(_, p)| *p))
}

/// Router and firewall words, plus what the STUN answer means.
pub fn forward_guidance(lan_ip: Option<&str>, mapped_ip: Option<&str>, mapped_port: Option<u16>) -> String {
    let lan = lan_ip.unwrap_or("this computer's LAN address");
    let mut text = format!(
        "Router: forward external UDP {HOST_PORT} to {lan} UDP {HOST_PORT}. Not TCP. The Player and the browser both speak QUIC on that UDP port.\n\
         Windows firewall: Windows Defender Firewall, Inbound Rules, New Rule, Port, UDP, {HOST_PORT}, Allow the connection. If that rule is missing, players time out even when the router is right.\n\
         Linux firewall: allow inbound udp/{HOST_PORT}.\n\
         This computer cannot see packets the internet sends in. A player on another network is the proof. A test from inside the same network often fails even when the forward is right (NAT hairpin)."
    );
    match (mapped_ip, mapped_port) {
        (Some(ip), Some(port)) if port == HOST_PORT => {
            text.push_str(&format!(
                "\nOutbound UDP from port {HOST_PORT} was seen on the internet as {ip}:{port}. The router kept the port for traffic this computer started. That is not yet proof it accepts a new connection."
            ));
        }
        (Some(ip), Some(port)) => {
            text.push_str(&format!(
                "\nOutbound UDP from port {HOST_PORT} was seen on the internet as {ip}:{port}. The router rewrote the port. Forward external UDP {HOST_PORT} to {lan} UDP {HOST_PORT}. A rewritten outbound port usually does not stay put for a player who connects first."
            ));
        }
        _ => {
            text.push_str(
                "\nNo STUN answer came back. Outbound UDP may be blocked, or the STUN servers were unreachable. That does not show whether an inbound forward is in place.",
            );
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hardware_class_keeps_its_shape_and_adds_the_gpu() {
        let with = hardware_json("NVIDIA GeForce RTX 2080 Ti", 11_264, true, "Intel(R) Core(TM) i9-10980XE");
        assert_eq!(
            with,
            serde_json::json!({
                "gpu": "NVIDIA GeForce RTX 2080 Ti",
                "cpu": "Intel(R) Core(TM) i9-10980XE",
                "tier": "high",
                "gpu_vram_mb": 11_264,
                "gpu_available": true,
            })
        );
        let none = hardware_json("", 0, false, "");
        assert_eq!(none["gpu"], "");
        assert_eq!(none["tier"], "unknown", "no GPU and no CPU name is unknown, not a guess");
        assert_eq!(none["gpu_available"], false);
    }

    /// A start clears its download directory. Two servers of one listing on
    /// one machine (a fleet's warm server beside a session with players)
    /// must never share that directory, and it must never hold a status file
    /// or a stop file: clearing it would take the world from under a running
    /// server and delete the file that stops it.
    #[test]
    fn each_server_downloads_into_its_own_directory() {
        let sim = "07d084d8-9098-4192-bb48-84d2c428db36";
        let listing = download_dir(sim);
        let status_a = listing.join("a").join("status.json");
        let status_b = listing.join("b").join("status.json");
        let world_a = server_download_dir(sim, "a", Some(&status_a));
        let world_b = server_download_dir(sim, "b", Some(&status_b));
        assert_eq!(world_a, listing.join("a").join("world"), "beside its own status file");
        assert_ne!(world_a, world_b, "two servers of one listing share a download directory");
        assert!(!world_a.starts_with(&world_b) && !world_b.starts_with(&world_a));
        let (stop_a, stop_b) = (stop_path(&status_a), stop_path(&status_b));
        for world in [&world_a, &world_b] {
            for kept in [&status_a, &status_b, &stop_a, &stop_b] {
                assert!(!kept.starts_with(world), "{} would be deleted with {}", kept.display(), world.display());
            }
        }

        // A server nobody named, and one with no status file, keep to the listing's cache, each under its own id.
        let default = server_download_dir(sim, "default", None);
        assert_eq!(default, listing.join("default").join("world"));
        assert_eq!(server_download_dir(sim, "a", None), listing.join("a").join("world"));
        assert!(!status_a.starts_with(&default) && !world_a.starts_with(&default));
        // Studio keeps its own server's status beside the open Universe; the download never goes there.
        let studio_status = Path::new("universe").join(".eustress").join("dedicated-server.json");
        assert_eq!(server_download_dir(sim, "default", Some(&studio_status)), default);
        // A status file with no directory names no place to download into.
        assert_eq!(server_download_dir(sim, "a", Some(Path::new("status.json"))), listing.join("a").join("world"));
    }

    /// The status file an older launcher reads has no `seated`; one an older
    /// engine wrote has none either and reads as 0.
    #[test]
    fn a_status_without_seated_reads_as_zero() {
        let old = r#"{"pid":1,"phase":"hosting","port":7777,"listening":true,"pin":"","join_link":"","public_host":"","players":2,"max_players":4,"sim_id":"","server_scripts":false,"script_note":"","probe":"","error":""}"#;
        let status: DedicatedStatus = serde_json::from_str(old).expect("an older status file still reads");
        assert_eq!((status.players, status.seated), (2, 0));
        let written = serde_json::to_value(DedicatedStatus { players: 1, seated: 3, ..Default::default() }).unwrap();
        assert_eq!(written["seated"], 3);
    }

    #[test]
    fn a_binding_request_is_twenty_bytes_and_its_answer_parses() {
        let id = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let request = stun_binding_request(&id);
        assert_eq!(request.len(), 20);
        assert_eq!(&request[0..2], &[0x00, 0x01]);
        assert_eq!(&request[8..20], &id);

        // XOR-MAPPED-ADDRESS for 203.0.113.7:7777.
        let port = 7777u16 ^ ((STUN_COOKIE >> 16) as u16);
        let cookie = STUN_COOKIE.to_be_bytes();
        let ip = [203, 0, 113, 7];
        let xored: Vec<u8> = ip.iter().zip(cookie).map(|(b, m)| b ^ m).collect();
        let mut packet = vec![0x01, 0x01, 0x00, 12, 0x21, 0x12, 0xA4, 0x42];
        packet.extend_from_slice(&id);
        packet.extend_from_slice(&[0x00, 0x20, 0x00, 8, 0x00, 0x01]);
        packet.extend_from_slice(&port.to_be_bytes());
        packet.extend_from_slice(&xored);
        assert_eq!(parse_stun_mapped(&packet, &id), Some(("203.0.113.7".into(), 7777)));
        assert_eq!(parse_stun_mapped(&packet, &[0; 12]), None);
        assert_eq!(parse_stun_mapped(&request, &id), None);
    }

    #[test]
    fn a_public_address_is_an_ipv4_host_or_a_name() {
        assert!(check_public_host("203.0.113.7").is_ok());
        assert!(check_public_host("203.0.113.7:7777").is_ok());
        assert!(check_public_host("play.example.com").is_ok());
        assert!(check_public_host("192.168.1.20").is_ok());
        for bad in ["", "127.0.0.1", "localhost", "0.0.0.0", "[2001:db8::1]", "169.254.1.1", "203.0.113.7:0"] {
            assert!(check_public_host(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_guidance_names_the_router_and_the_firewall() {
        let text = forward_guidance(Some("192.168.1.20"), Some("203.0.113.7"), Some(7777));
        assert!(text.contains("UDP 7777"));
        assert!(text.contains("192.168.1.20"));
        assert!(text.contains("Windows Defender Firewall"));
        assert!(text.contains("203.0.113.7:7777"));
        let rewritten = forward_guidance(Some("10.0.0.5"), Some("203.0.113.7"), Some(40000));
        assert!(text.contains("Not TCP"));
        assert!(rewritten.contains("rewrote the port"));
        let silent = forward_guidance(None, None, None);
        assert!(silent.contains("No STUN answer"));
    }

    #[test]
    fn a_stop_file_sits_beside_the_status_file() {
        let status = Path::new("/tmp/universe/.eustress/dedicated-server.json");
        assert_eq!(stop_path(status), Path::new("/tmp/universe/.eustress/dedicated.stop"));
        assert_eq!(log_path(status), Path::new("/tmp/universe/.eustress/dedicated-server.log"));
    }

    #[test]
    fn an_empty_exit_is_off_unless_asked_and_never_shorter_than_a_minute() {
        for never in [None, Some(""), Some("soon"), Some("0"), Some("-60"), Some("inf"), Some("NaN")] {
            assert_eq!(empty_exit_secs_from(never), None, "{never:?}");
        }
        assert_eq!(empty_exit_secs_from(Some(" 300 ")), Some(300.0));
        assert_eq!(empty_exit_secs_from(Some("5")), Some(MIN_EMPTY_EXIT_SECS));
    }
}
