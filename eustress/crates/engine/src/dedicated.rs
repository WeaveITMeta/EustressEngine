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

/// GPU and CPU class for the heartbeat. Read once. An empty GPU is unknown,
/// not a guessed model.
pub fn hardware_class() -> serde_json::Value {
    static CACHED: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    CACHED.get_or_init(|| {
        let cpu = cpu_model();
        let gpu = String::new();
        let tier = part_tier(&gpu).max(part_tier(&cpu));
        serde_json::json!({ "gpu": gpu, "cpu": cpu, "tier": tier_name(tier) })
    }).clone()
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
    forward_guidance(lan.as_deref(), mapped.as_ref().map(|(ip, _)| ip.as_str()), mapped.map(|(_, p)| p))
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
}
