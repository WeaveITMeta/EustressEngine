//! # In-App Updater
//!
//! Checks for new versions, downloads them, and installs them.
//!
//! Flow:
//! 1. Startup: fetch https://downloads.eustress.dev/latest.json, the unsigned
//!    pointer that names the newest version.
//! 2. Compare against compiled-in CARGO_PKG_VERSION.
//! 3. When this build trusts a release signing key (`update_trust::TRUSTED_KEYS`
//!    is not empty), fetch `v<version>/latest.json` and `v<version>/latest.json.sig`,
//!    verify the signature, and take the artifact URL, SHA-256 and exact size
//!    from that signed manifest. Anything that fails leaves the installed build
//!    untouched and shows a short error on the ribbon badge. When the list is
//!    empty, the pointer's own URL and SHA-256 drive an update the user clicks,
//!    and nothing downloads or installs without a click.
//! 4. If newer: the ribbon shows the badge. With Settings > Automatic updates on
//!    and a signed manifest, the download starts by itself, a re-check runs
//!    every 6 hours, and a verified download installs when Studio quits
//!    normally, without starting Studio again. The badge still offers an
//!    immediate restart. Without automatic updates the user clicks to download,
//!    then clicks again to restart into the new build.
//!
//! A development build (a debug build, or one run from a cargo target folder)
//! never installs over itself. `EUSTRESS_UPDATE_CHECK=0` (or `false`, `no`,
//! `off`) turns every check off.
//!
//! The install extracts the archive into a staging folder inside the install
//! folder, then moves each file into place keeping a `.old` copy of what it
//! replaces, and puts every `.old` back if any step fails.

use bevy::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::update_trust;

const MANIFEST_URL: &str = "https://downloads.eustress.dev/latest.json";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the ribbon says when this install cannot be updated in place. The reason is in the log.
const REFUSED_UI_TEXT: &str = "Reinstall to enable updates";
/// What the ribbon says when a release failed its signature or manifest check.
/// The reason is in the log.
const UNVERIFIED_UI_TEXT: &str = "Update could not be verified";
/// What the ribbon says when a development build is asked to install an update.
const DEV_BUILD_UI_TEXT: &str = "Development builds do not self-update";

/// Set to `0` (or `false`, `no`, `off`) to turn every update check off.
pub const UPDATE_CHECK_VAR: &str = "EUSTRESS_UPDATE_CHECK";
/// How often a running Studio checks again while automatic updates are on.
const RECHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// The folder inside the install folder that an archive is extracted into
/// before its files are moved into place.
const STAGING_DIR_NAME: &str = ".eustress-update-staging";
/// The file inside the install folder listing `.old` copies that could not be
/// deleted right after an install (the running executable on Windows). The
/// next start deletes them.
const OLD_FILES_JOURNAL: &str = ".eustress-update-old-files";

/// Bevy resource tracking update state
#[derive(Resource)]
pub struct UpdateState {
    /// Whether a newer version is available
    pub available: bool,
    /// The new version string (e.g., "0.3.3")
    pub new_version: String,
    /// Download URL for current platform
    pub download_url: String,
    /// Expected SHA-256 hash
    pub expected_hash: String,
    /// Download size in bytes
    pub size_bytes: u64,
    /// Whether the URL, hash and size come from a manifest signed by a trusted
    /// key. A signed offer must match its size exactly, and only a signed offer
    /// is downloaded without a click or installed on quit.
    pub signed: bool,
    /// When the last check started; the periodic re-check counts from here.
    /// `None` while checks are off.
    pub last_check: Option<Instant>,
    /// Current status: "", "downloading", "ready", "error"
    pub status: String,
    /// Download progress 0-100
    pub progress: u32,
    /// Error message if any
    pub error: Option<String>,
    /// Shared state for async download thread
    pub async_state: Arc<Mutex<AsyncUpdateState>>,
}

#[derive(Default)]
pub struct AsyncUpdateState {
    pub progress: u32,
    pub status: String,
    pub error: Option<String>,
    pub download_path: Option<std::path::PathBuf>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            available: false,
            new_version: String::new(),
            download_url: String::new(),
            expected_hash: String::new(),
            size_bytes: 0,
            signed: false,
            last_check: None,
            status: String::new(),
            progress: 0,
            error: None,
            async_state: Arc::new(Mutex::new(AsyncUpdateState::default())),
        }
    }
}

/// Plugin that checks for updates and handles the update flow.
pub struct UpdaterPlugin;

impl Plugin for UpdaterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UpdateState>()
            .add_systems(Startup, (clean_up_previous_update, check_for_updates).chain())
            .add_systems(Update, sync_update_state_to_slint)
            .add_systems(Update, recheck_for_updates)
            .add_systems(Update, exit_for_pending_update)
            .add_systems(Last, install_on_quit);
    }
}

/// Whether update checks run: `EUSTRESS_UPDATE_CHECK` set to `0`, `false`,
/// `no` or `off` (any case, surrounding spaces ignored) turns them off; unset
/// or anything else leaves them on.
pub fn update_check_enabled(var: Option<&str>) -> bool {
    !matches!(
        var.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("0" | "false" | "no" | "off")
    )
}

/// `update_check_enabled` for this process's environment.
fn checks_enabled_now() -> bool {
    update_check_enabled(std::env::var(UPDATE_CHECK_VAR).ok().as_deref())
}

/// Whether the folder holding the executable looks like cargo's output: it
/// holds `.fingerprint`, `deps` or `incremental`, or it or a folder above it is
/// named `target`.
pub fn looks_like_cargo_output(exe_dir: &Path) -> bool {
    [".fingerprint", "deps", "incremental"].iter().any(|name| exe_dir.join(name).exists())
        || exe_dir
            .ancestors()
            .any(|dir| dir.file_name().is_some_and(|name| name.eq_ignore_ascii_case("target")))
}

/// A development build never installs over itself: a debug build, or an
/// executable run from a cargo target folder. Worked out once per process.
fn is_dev_build() -> bool {
    static DEV_BUILD: OnceLock<bool> = OnceLock::new();
    *DEV_BUILD.get_or_init(|| {
        cfg!(debug_assertions)
            || std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(looks_like_cargo_output))
                .unwrap_or(false)
    })
}

/// Whether Studio may download and install by itself right now: the setting is
/// on, this build trusts a signing key, checks are not turned off, and this is
/// not a development build. Read at each decision, never cached.
fn automatic_updates_on(settings: Option<&crate::editor_settings::EditorSettings>) -> bool {
    settings.is_some_and(|s| s.automatic_updates)
        && !update_trust::TRUSTED_KEYS.is_empty()
        && checks_enabled_now()
        && !is_dev_build()
}

/// Check for updates on startup (async, non-blocking)
fn check_for_updates(
    mut state: ResMut<UpdateState>,
    settings: Option<Res<crate::editor_settings::EditorSettings>>,
) {
    if !checks_enabled_now() {
        info!("Update checks are off ({UPDATE_CHECK_VAR})");
        return;
    }
    if update_trust::TRUSTED_KEYS.is_empty() && settings.as_deref().is_some_and(|s| s.automatic_updates) {
        info!("Automatic updates wait for a signed release: this build trusts no update signing key");
    }
    info!("🔄 Checking for updates (current: v{})", CURRENT_VERSION);
    spawn_check(&mut state);
}

/// Checks again every `RECHECK_INTERVAL` while automatic updates are on. A
/// real-time timer read each frame; no thread waits between checks.
fn recheck_for_updates(
    mut state: ResMut<UpdateState>,
    settings: Option<Res<crate::editor_settings::EditorSettings>>,
) {
    let Some(last) = state.last_check else { return };
    if last.elapsed() < RECHECK_INTERVAL {
        return;
    }
    let busy = state.status == "downloading" || state.status == "ready";
    if busy || !automatic_updates_on(settings.as_deref()) {
        // Count the interval again from now, so the decision is made once per
        // interval and not every frame.
        state.last_check = Some(Instant::now());
        return;
    }
    info!("🔄 Checking for updates again (current: v{})", CURRENT_VERSION);
    spawn_check(&mut state);
}

/// What a check found, handed from the check thread to the main thread.
enum CheckOutcome {
    UpToDate,
    /// The pointer could not be fetched or read.
    Failed(String),
    /// A newer version this build may download.
    Offer(UpdateOffer),
    /// The pointer names a newer version, but its signed manifest was refused.
    Refused { version: String, reason: String },
}

struct UpdateOffer {
    version: String,
    url: String,
    sha256: String,
    size_bytes: u64,
    signed: bool,
}

/// Global for passing async check result to main thread
static UPDATE_CHECK_RESULT: Mutex<Option<CheckOutcome>> = Mutex::new(None);

/// True while a check thread runs, so a re-check never starts a second one.
static CHECK_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Start a check on a background thread. The thread ends when the check does.
fn spawn_check(state: &mut UpdateState) {
    if CHECK_IN_FLIGHT.swap(true, Ordering::AcqRel) {
        return;
    }
    state.last_check = Some(Instant::now());
    std::thread::spawn(|| {
        let outcome = run_check();
        if let Ok(mut slot) = UPDATE_CHECK_RESULT.lock() {
            *slot = Some(outcome);
        }
        CHECK_IN_FLIGHT.store(false, Ordering::Release);
    });
}

/// Fetch the pointer and, when this build trusts a signing key, the signed
/// manifest it names.
fn run_check() -> CheckOutcome {
    let (version, url, hash, size) = match check_manifest() {
        Ok(Some(found)) => found,
        Ok(None) => return CheckOutcome::UpToDate,
        Err(e) => return CheckOutcome::Failed(e),
    };
    if update_trust::TRUSTED_KEYS.is_empty() {
        return CheckOutcome::Offer(UpdateOffer { version, url, sha256: hash, size_bytes: size, signed: false });
    }
    match fetch_signed_target(&version) {
        Ok(target) => CheckOutcome::Offer(UpdateOffer {
            version: target.version_text,
            url: target.url,
            sha256: target.sha256,
            size_bytes: target.size_bytes,
            signed: true,
        }),
        Err(reason) => CheckOutcome::Refused { version, reason },
    }
}

/// Puts a check's outcome into the resource. Returns true when it installed a
/// new offer the user has not acted on yet.
fn apply_check_outcome(state: &mut UpdateState, outcome: CheckOutcome) -> bool {
    // A download in progress or ready to install is kept; a later check does
    // not replace it under the user.
    let busy = state.status == "downloading" || state.status == "ready";
    match outcome {
        CheckOutcome::UpToDate => {
            info!("✅ Up to date (v{})", CURRENT_VERSION);
            false
        }
        CheckOutcome::Failed(e) => {
            warn!("Update check failed: {e}");
            false
        }
        CheckOutcome::Offer(offer) if busy => {
            info!("Update to v{} found; the update already under way is kept", offer.version);
            false
        }
        CheckOutcome::Offer(offer) => {
            info!(
                "⬆ Update to v{} ready to download ({} bytes{})",
                offer.version,
                offer.size_bytes,
                if offer.signed { ", signed" } else { "" }
            );
            state.available = true;
            state.new_version = offer.version;
            state.download_url = offer.url;
            state.expected_hash = offer.sha256;
            state.size_bytes = offer.size_bytes;
            state.signed = offer.signed;
            state.status.clear();
            state.progress = 0;
            state.error = None;
            true
        }
        CheckOutcome::Refused { version, reason } => {
            warn!("Update to v{version} refused: {reason}. The installed build is unchanged.");
            if !busy {
                state.available = true;
                state.new_version = version;
                state.download_url.clear();
                state.expected_hash.clear();
                state.size_bytes = 0;
                state.signed = false;
                state.status = "error".to_string();
                state.error = Some(UNVERIFIED_UI_TEXT.to_string());
            }
            false
        }
    }
}

/// The downloaded update to install once the engine has shut down. Taken by
/// `finish_pending_update`.
struct PendingInstall {
    archive: std::path::PathBuf,
    /// Start the new build afterwards: true when the user asked to restart,
    /// false when an automatic update installs on a normal quit.
    relaunch: bool,
    /// The SHA-256 the signed manifest gave for `archive`, checked again just
    /// before the install. `None` for an update from an unsigned pointer.
    sha256: Option<String>,
}

/// The update to install after `App::run` returns.
static PENDING_UPDATE: Mutex<Option<PendingInstall>> = Mutex::new(None);

/// Set by `apply_update_and_restart`. The next frame closes the engine with
/// `AppExit`, which runs the normal shutdown.
static EXIT_FOR_UPDATE: AtomicBool = AtomicBool::new(false);

/// What the last sync pushed to Slint. The sync compares against it, so only a
/// change reaches the window.
#[derive(Default)]
struct PushedUpdate {
    /// False until the first push, so the first frame always sends.
    sent: bool,
    available: bool,
    version: String,
    error: String,
    status: String,
    progress: i32,
}

/// Sync update state to Slint UI each frame
fn sync_update_state_to_slint(
    mut state: ResMut<UpdateState>,
    slint_context: Option<NonSend<crate::ui::slint_ui::SlintUiState>>,
    settings: Option<Res<crate::editor_settings::EditorSettings>>,
    mut pushed: Local<PushedUpdate>,
) {
    // Check if async update check completed
    let outcome = UPDATE_CHECK_RESULT.lock().ok().and_then(|mut result| result.take());
    if let Some(outcome) = outcome {
        let offered = apply_check_outcome(&mut state, outcome);
        if offered && state.signed && automatic_updates_on(settings.as_deref()) {
            info!("⬇ Downloading v{} in the background (automatic updates)", state.new_version);
            start_download(&mut state);
        }
    }

    // Check async download progress
    if state.status == "downloading" {
        let (progress, new_status, error) = {
            if let Ok(async_s) = state.async_state.lock() {
                (
                    async_s.progress,
                    if !async_s.status.is_empty() { Some(async_s.status.clone()) } else { None },
                    async_s.error.clone(),
                )
            } else {
                (state.progress, None, None)
            }
        };
        state.progress = progress;
        if let Some(s) = new_status { state.status = s; }
        if let Some(e) = error { state.error = Some(e); state.status = "error".to_string(); }
    }

    // The apply step runs after the download and reports here too, so a refusal
    // (a folder this account cannot update) reaches the UI.
    if state.status == "ready" {
        let error = state.async_state.lock().ok().and_then(|s| s.error.clone());
        if let Some(e) = error { state.error = Some(e); state.status = "error".to_string(); }
    }

    // Sync to Slint. Only a change reaches Slint: an idle frame makes no setter
    // calls and no string copies.
    let Some(ref context) = slint_context else { return };
    let ui = &context.window;
    let error = state.error.as_deref().unwrap_or("");
    let progress = state.progress as i32;
    let unchanged = pushed.sent
        && pushed.available == state.available
        && (!state.available
            || (pushed.version == state.new_version
                && pushed.error == error
                && pushed.status == state.status
                && pushed.progress == progress));
    if unchanged {
        return;
    }
    ui.set_update_available(state.available);
    if state.available {
        ui.set_update_version(crate::ui::display_text::shown_line(&state.new_version));
        ui.set_update_error(crate::ui::display_text::shown_line(error));
        ui.set_update_status(state.status.clone().into());
        ui.set_update_progress(progress);
    }
    pushed.sent = true;
    pushed.available = state.available;
    pushed.version.clone_from(&state.new_version);
    pushed.error.clear();
    pushed.error.push_str(error);
    pushed.status.clone_from(&state.status);
    pushed.progress = progress;
}

/// The manifest key for this platform's artifact.
fn platform_key() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows-x64"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") { "macos-arm64" } else { "macos-x64" }
    } else {
        "linux-x64"
    }
}

/// Check manifest and compare versions. Returns Some((version, url, hash, size)) if update available.
fn check_manifest() -> Result<Option<(String, String, String, u64)>, String> {
    let resp = ureq::get(MANIFEST_URL)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| format!("Failed to fetch manifest: {}", e))?;

    let body: serde_json::Value = resp.into_json()
        .map_err(|e| format!("Failed to parse manifest: {}", e))?;

    let remote_version = body.get("version")
        .and_then(|v| v.as_str())
        .ok_or("No version field in manifest")?;

    // Compare versions
    if !is_newer(remote_version, CURRENT_VERSION) {
        return Ok(None);
    }

    // Get platform-specific download info
    let platform_key = platform_key();

    let platform = body.get("platforms")
        .and_then(|p| p.get(platform_key))
        .ok_or(format!("No platform entry for {}", platform_key))?;

    let url = platform.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let hash = platform.get("sha256").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let size = platform.get("size_bytes").and_then(|v| v.as_u64()).unwrap_or(0);

    Ok(Some((remote_version.to_string(), url, hash, size)))
}

/// Fetch, verify and read the signed manifest for the version the pointer
/// named, and choose this platform's artifact from it.
fn fetch_signed_target(pointer_version: &str) -> Result<update_trust::UpdateTarget, String> {
    let expected = update_trust::parse_version(pointer_version)
        .ok_or_else(|| format!("the pointer's version {pointer_version:?} is not X.Y.Z"))?;
    // `pointer_version` is digits and dots only, checked just above, so it is
    // safe in the URL path.
    let folder = format!("{}v{pointer_version}/", update_trust::RELEASE_ORIGIN);
    let manifest = fetch_capped(&format!("{folder}latest.json"), update_trust::MAX_MANIFEST_BYTES)?;
    let signature = fetch_capped(&format!("{folder}latest.json.sig"), update_trust::MAX_SIGNATURE_FILE_BYTES)?;
    let signature = String::from_utf8(signature).map_err(|_| "the signature file is not UTF-8 text".to_string())?;
    update_trust::verify_manifest(&manifest, &signature, update_trust::TRUSTED_KEYS).map_err(|e| e.to_string())?;
    let parsed = update_trust::parse_signed_manifest(&manifest).map_err(|e| e.to_string())?;
    update_trust::select_update(&parsed, expected, version_tuple(CURRENT_VERSION), platform_key())
        .map_err(|e| e.to_string())
}

/// GET a small file from the release host: HTTPS on that one host only, no
/// redirects, status 200, and at most `cap` bytes read.
fn fetch_capped(url: &str, cap: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;

    if !url.starts_with(update_trust::RELEASE_ORIGIN) {
        return Err(format!("{url:?} is not under {}", update_trust::RELEASE_ORIGIN));
    }
    let agent = ureq::AgentBuilder::new().redirects(0).build();
    let resp = agent
        .get(url)
        .timeout(Duration::from_secs(30))
        .call()
        .map_err(|e| format!("Failed to fetch {url}: {e}"))?;
    if resp.status() != 200 {
        return Err(format!("{url} answered HTTP {}", resp.status()));
    }
    let mut body = Vec::new();
    resp.into_reader()
        .take(cap as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|e| format!("Failed to read {url}: {e}"))?;
    if body.len() > cap {
        return Err(format!("{url} is larger than {cap} bytes"));
    }
    Ok(body)
}

/// `X.Y.Z` as numbers, leniently: a leading `v` is ignored and a missing or
/// unreadable part counts as 0.
fn version_tuple(s: &str) -> (u32, u32, u32) {
    let s = s.strip_prefix('v').unwrap_or(s);
    let parts: Vec<u32> = s.split('.').filter_map(|p| p.parse().ok()).collect();
    (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    )
}

/// Simple semver comparison: is `remote` newer than `current`?
fn is_newer(remote: &str, current: &str) -> bool {
    version_tuple(remote) > version_tuple(current)
}

/// Start downloading the update (called from Slint callback)
pub fn start_download(state: &mut UpdateState) {
    if state.download_url.is_empty() || state.status == "downloading" {
        return;
    }
    // With a trusted key, only a signed offer may download.
    if !update_trust::TRUSTED_KEYS.is_empty() && !state.signed {
        state.status = "error".to_string();
        state.error = Some(UNVERIFIED_UI_TEXT.to_string());
        return;
    }

    state.status = "downloading".to_string();
    state.progress = 0;

    let url = state.download_url.clone();
    let expected_hash = state.expected_hash.clone();
    let expected_size = state.size_bytes;
    let exact_size = state.signed;
    let async_state = state.async_state.clone();
    // A fresh download starts from a clean slate: no status, error or file left
    // by an earlier attempt.
    if let Ok(mut s) = async_state.lock() {
        *s = AsyncUpdateState::default();
    }

    std::thread::spawn(move || {
        match download_and_verify(&url, &expected_hash, expected_size, exact_size, async_state.clone()) {
            Ok(path) => {
                if let Ok(mut s) = async_state.lock() {
                    s.status = "ready".to_string();
                    s.progress = 100;
                    s.download_path = Some(path);
                }
            }
            Err(e) => {
                if let Ok(mut s) = async_state.lock() {
                    s.error = Some(e);
                    s.status = "error".to_string();
                }
            }
        }
    });
}

/// Download file, verify SHA-256, return path to downloaded file.
///
/// With `exact_size` (an offer from a signed manifest) the response must be
/// HTTP 200 and the body exactly `expected_size` bytes.
fn download_and_verify(
    url: &str,
    expected_hash: &str,
    expected_size: u64,
    exact_size: bool,
    progress: Arc<Mutex<AsyncUpdateState>>,
) -> Result<std::path::PathBuf, String> {
    use sha2::{Sha256, Digest};
    use std::io::Read;

    // Only ever fetch the update from the official release host over HTTPS, and
    // never follow redirects — otherwise a hostile manifest `url` could aim the
    // auto-installer at an arbitrary origin (or bounce through one).
    const RELEASE_PREFIX: &str = "https://downloads.eustress.dev/";
    if !url.starts_with(RELEASE_PREFIX) {
        return Err(format!(
            "Refusing update: artifact URL {url:?} is not under {RELEASE_PREFIX}"
        ));
    }
    if exact_size && expected_size == 0 {
        return Err("Refusing update: the signed manifest gives no size".to_string());
    }

    let agent = ureq::AgentBuilder::new().redirects(0).build();
    let resp = agent.get(url)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("Download failed: {}", e))?;
    if exact_size && resp.status() != 200 {
        return Err(format!("Download failed: HTTP {}", resp.status()));
    }

    let total = resp.header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    // Hard cap on bytes written so a hostile/oversized manifest cannot fill the
    // disk. A signed manifest's size is exact; otherwise prefer the manifest's
    // declared size (+1 MiB slack), else a 2 GiB absolute ceiling.
    let max_bytes: u64 = if exact_size {
        expected_size
    } else if expected_size > 0 {
        expected_size + (1u64 << 20)
    } else {
        2u64 << 30
    };
    if total > max_bytes {
        return Err(format!("Refusing update: advertised size {total} exceeds cap {max_bytes}"));
    }
    if exact_size && total > 0 && total != expected_size {
        return Err(format!("Refusing update: advertised size {total} is not the signed {expected_size}"));
    }

    let download_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("eustress-updates");

    std::fs::create_dir_all(&download_dir)
        .map_err(|e| format!("Failed to create download dir: {}", e))?;

    let filename = url.rsplit('/').next().unwrap_or("update.zip");
    let download_path = download_dir.join(filename);

    let mut file = std::fs::File::create(&download_path)
        .map_err(|e| format!("Failed to create file: {}", e))?;

    let mut reader = resp.into_reader();
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 65536];

    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Read error: {}", e))?;
        if n == 0 { break; }

        std::io::Write::write_all(&mut file, &buf[..n])
            .map_err(|e| format!("Write error: {}", e))?;
        hasher.update(&buf[..n]);

        downloaded += n as u64;
        if downloaded > max_bytes {
            std::fs::remove_file(&download_path).ok();
            return Err(format!("Refusing update: download exceeded {max_bytes} bytes"));
        }
        if total > 0 {
            let pct = (downloaded * 100 / total) as u32;
            if let Ok(mut s) = progress.lock() {
                s.progress = pct;
            }
        }
    }
    drop(file);

    if exact_size && downloaded != expected_size {
        std::fs::remove_file(&download_path).ok();
        return Err(format!("Refusing update: downloaded {downloaded} bytes, the signed size is {expected_size}"));
    }

    // Verify hash, failing CLOSED. A missing/short/non-hex hash means the manifest
    // cannot vouch for these bytes, so refuse rather than install something
    // unverified.
    let hash = format!("{:x}", hasher.finalize());
    let expected = expected_hash.trim().to_ascii_lowercase();
    let is_sha256 = expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit());
    if !is_sha256 {
        std::fs::remove_file(&download_path).ok();
        return Err(format!(
            "Refusing update: manifest has no valid SHA-256 for this artifact ({expected_hash:?})"
        ));
    }
    if hash != expected {
        std::fs::remove_file(&download_path).ok();
        return Err(format!("SHA-256 mismatch: expected {expected}, got {hash}"));
    }

    info!("✅ Update downloaded and verified: {:?}", download_path);
    Ok(download_path)
}

/// An archive entry's name as a relative path, or `None` when it is unsafe:
/// absolute (a leading `/` or `\`), a `..` component, a `:` (a Windows drive
/// or stream), a NUL, or a first component that names the updater's own
/// staging folder or journal. `.` and empty components are dropped, so `./a`
/// is `a` and `./` is the empty path.
fn archive_entry_path(name: &str) -> Option<PathBuf> {
    if name.contains('\0') || name.starts_with('/') || name.starts_with('\\') {
        return None;
    }
    let mut path = PathBuf::new();
    for part in name.split(|c| c == '/' || c == '\\') {
        match part {
            "" | "." => continue,
            ".." => return None,
            p if p.contains(':') => return None,
            p => path.push(p),
        }
    }
    let reserved = path.iter().next().is_some_and(|first| {
        first == std::ffi::OsStr::new(STAGING_DIR_NAME) || first == std::ffi::OsStr::new(OLD_FILES_JOURNAL)
    });
    if reserved {
        return None;
    }
    Some(path)
}

/// Extract a zip archive's entries into `dest_dir`, preserving its internal
/// directory structure (the release zip's root already IS the install layout:
/// `eustress-engine.exe` + `assets/**` at the top level; see
/// `.github/workflows/release.yml`'s Windows Package step). An entry with an
/// absolute path, a `..` component or a symbolic link fails the whole update.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn extract_zip_into(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("Failed to open update zip: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Failed to read update zip: {e}"))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry {i}: {e}"))?;
        let name = entry.name().to_string();
        let unsafe_entry = || format!("Refusing update: unsafe archive entry {name:?}");
        let rel_path = archive_entry_path(&name).ok_or_else(unsafe_entry)?;
        if entry.enclosed_name().is_none() || entry.is_symlink() {
            return Err(unsafe_entry());
        }
        let dest = dest_dir.join(&rel_path);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| format!("Failed to create dir {dest:?}: {e}"))?;
            continue;
        }
        if rel_path.as_os_str().is_empty() {
            return Err(unsafe_entry());
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir {parent:?}: {e}"))?;
        }
        let mut out = std::fs::File::create(&dest).map_err(|e| format!("Failed to create {dest:?}: {e}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("Failed to write {dest:?}: {e}"))?;
    }
    Ok(())
}

/// Extract a `.tar.gz` archive's entries into `dest_dir`. Each path is checked
/// by `archive_entry_path` first (absolute or `..` fails the update), only
/// files and folders are accepted, and `unpack_in` (not the less-safe `unpack`)
/// checks the destination again and creates intermediate directories.
#[cfg(target_os = "linux")]
fn extract_targz_into(archive_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive_path).map_err(|e| format!("Failed to open update archive: {e}"))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    for entry in archive
        .entries()
        .map_err(|e| format!("Failed to read update archive: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("Failed to read archive entry: {e}"))?;
        let name = entry
            .path()
            .map_err(|e| format!("Failed to read archive entry path: {e}"))?
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| "Refusing update: an archive entry path is not UTF-8".to_string())?;
        let unsafe_entry = || format!("Refusing update: unsafe archive entry {name:?}");
        let rel_path = archive_entry_path(&name).ok_or_else(unsafe_entry)?;
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            let dest = dest_dir.join(&rel_path);
            std::fs::create_dir_all(&dest).map_err(|e| format!("Failed to create dir {dest:?}: {e}"))?;
            continue;
        }
        if !kind.is_file() || rel_path.as_os_str().is_empty() {
            return Err(unsafe_entry());
        }
        let unpacked = entry
            .unpack_in(dest_dir)
            .map_err(|e| format!("Failed to extract archive entry: {e}"))?;
        if !unpacked {
            return Err(unsafe_entry());
        }
    }
    Ok(())
}

/// Ask to apply the downloaded update and restart. Called when the user clicks
/// "Restart to update".
///
/// Nothing is replaced here. The engine is asked to close the normal way: the
/// `AppExit` runs the shutdown systems, which write pending edits and flush the
/// open Space database. Once `main` has returned, `finish_pending_update` swaps
/// the files and starts the new build. So no Space is open in two engines at
/// once, and no running system reads a file while it is being replaced.
///
/// The folder is checked before anything closes. An install under Program Files
/// (what an administrator's install produced before this version) cannot be
/// changed by a standard account, so the update stops here with a message
/// instead of closing the engine and failing after.
pub fn apply_update_and_restart(state: &UpdateState) {
    if cfg!(target_os = "macos") {
        // The download is a `.dmg`, an Apple compressed disk image with no
        // lightweight pure-Rust reader, and replacing a running `.app` from a
        // mounted image has Gatekeeper and notarization implications. Writing the
        // raw bytes over the executable is what this path used to do, so it stays
        // off. The verified download stays on disk for a manual reinstall.
        info!("Update downloaded and verified. macOS auto-apply is not supported: open the .dmg and reinstall.");
        return;
    }
    if is_dev_build() {
        error!("Update not applied: this is a development build (a debug build or one run from a cargo target folder), which never installs over itself");
        if let Ok(mut s) = state.async_state.lock() {
            s.error = Some(DEV_BUILD_UI_TEXT.to_string());
        }
        return;
    }
    // With a trusted key, only a download from a signed manifest may install.
    if !update_trust::TRUSTED_KEYS.is_empty() && !state.signed {
        error!("Update not applied: it does not come from a signed manifest");
        if let Ok(mut s) = state.async_state.lock() {
            s.error = Some(UNVERIFIED_UI_TEXT.to_string());
        }
        return;
    }

    let download_path = state.async_state.lock().ok().and_then(|s| s.download_path.clone());
    let Some(download_path) = download_path else {
        error!("No downloaded update to apply");
        return;
    };
    let Some(install_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
    else {
        error!("Could not determine the install directory");
        return;
    };
    if !folder_is_writable(&install_dir) {
        let message = format!(
            "Eustress can't update itself from {}: this account can't change that folder. \
             Install the current version from eustress.dev. New installs go to your user \
             folder, where updates apply without administrator rights.",
            install_dir.display()
        );
        error!("{message}");
        if let Ok(mut s) = state.async_state.lock() {
            s.error = Some(REFUSED_UI_TEXT.to_string());
        }
        return;
    }

    info!("🔄 Closing to apply the update from {:?}", download_path);
    if let Ok(mut pending) = PENDING_UPDATE.lock() {
        *pending = Some(PendingInstall {
            archive: download_path,
            relaunch: true,
            sha256: state.signed.then(|| state.expected_hash.clone()),
        });
    }
    EXIT_FOR_UPDATE.store(true, Ordering::Release);
}

/// Whether this account can create files in `dir`. The update renames the
/// running executable and writes the new files beside it, so a folder that
/// refuses a new file cannot be updated.
fn folder_is_writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(format!(".update-probe-{}", std::process::id()));
    let writable = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    writable
}

/// Closes the engine once the user has asked for an update. The shutdown
/// systems in `Last` see the `AppExit` written here, as they do for a normal quit.
fn exit_for_pending_update(mut exit: MessageWriter<bevy::app::AppExit>) {
    if EXIT_FOR_UPDATE.swap(false, Ordering::AcqRel) {
        exit.write(bevy::app::AppExit::Success);
    }
}

/// On a normal quit, queue a verified automatic update to install after the
/// engine has shut down, without starting Studio again. A restart the user
/// already asked for is left as it is.
fn install_on_quit(
    mut exits: MessageReader<bevy::app::AppExit>,
    state: Res<UpdateState>,
    settings: Option<Res<crate::editor_settings::EditorSettings>>,
) {
    if !exits.read().any(|exit| matches!(exit, bevy::app::AppExit::Success)) {
        return;
    }
    if !state.signed || state.status != "ready" || cfg!(target_os = "macos") {
        return;
    }
    if !automatic_updates_on(settings.as_deref()) {
        return;
    }
    let archive = state
        .async_state
        .lock()
        .ok()
        .filter(|s| s.error.is_none())
        .and_then(|s| s.download_path.clone());
    let Some(archive) = archive else { return };
    let Some(install_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
    else {
        return;
    };
    if !folder_is_writable(&install_dir) {
        warn!("Update v{} not installed on quit: this account can't change {}", state.new_version, install_dir.display());
        return;
    }
    if let Ok(mut pending) = PENDING_UPDATE.lock() {
        if pending.is_none() {
            info!("Installing v{} on the way out; the next start runs it", state.new_version);
            *pending = Some(PendingInstall { archive, relaunch: false, sha256: Some(state.expected_hash.clone()) });
        }
    }
}

/// Install an update that `apply_update_and_restart` or `install_on_quit`
/// queued. `main` calls this after `App::run` returns and the app has been
/// dropped. By then the shutdown systems have run and no system still reads
/// the files.
///
/// A restart the user asked for starts the engine again in every case, so a
/// failed update never leaves the user without one. An install on quit starts
/// nothing. A failure is logged and leaves the installed build as it was.
pub fn finish_pending_update() {
    let Some(pending) = PENDING_UPDATE.lock().ok().and_then(|mut p| p.take()) else {
        return;
    };
    let Ok(current_exe) = std::env::current_exe() else {
        error!("Update not applied: the current executable path is unknown");
        return;
    };

    // The open Space database lives in a static, so its Drop has not run on the
    // way out. Releasing the handle here runs that Drop, which checkpoints and
    // persists it. The new build must not open the database while this one holds it.
    crate::space::active_db::clear();

    // The archive was verified when it was downloaded and may have sat in the
    // cache for hours since. It is hashed again here, so what is installed is
    // exactly what the signed manifest named.
    let verified = match &pending.sha256 {
        Some(expected) => archive_sha256(&pending.archive).and_then(|found| {
            if found.eq_ignore_ascii_case(expected) {
                Ok(())
            } else {
                Err(format!("the downloaded update changed after it was verified (SHA-256 {found}, signed {expected})"))
            }
        }),
        None => Ok(()),
    };

    match verified.and_then(|()| replace_files(&pending.archive, &current_exe)) {
        Ok(()) if pending.relaunch => info!("✅ Update applied; starting the new build"),
        Ok(()) => info!("✅ Update installed; the next start runs the new build"),
        Err(e) if pending.relaunch => error!("Update apply failed; starting the current build: {e}"),
        Err(e) => error!("Update install failed; the installed build is unchanged: {e}"),
    }
    if !pending.relaunch {
        return;
    }
    if let Err(e) = std::process::Command::new(&current_exe).spawn() {
        error!("Failed to start the engine after the update: {e}");
    }
}

/// Lowercase hex SHA-256 of the file at `path`.
fn archive_sha256(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path).map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Puts the downloaded build in place of the running one, through a staging
/// folder (`install_staged`).
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn replace_files(download: &std::path::Path, current_exe: &std::path::Path) -> Result<(), String> {
    let install_dir = current_exe
        .parent()
        .ok_or_else(|| "Could not determine install directory".to_string())?;
    let exe_name = current_exe
        .file_name()
        .ok_or_else(|| "Could not determine the executable's file name".to_string())?
        .to_owned();
    let result = install_staged(install_dir, |staging| {
        if is_update_archive(download) {
            extract_update_archive(download, staging)
        } else {
            // Not an archive: a raw binary drop-in for the executable.
            std::fs::copy(download, staging.join(&exe_name))
                .map(|_| ())
                .map_err(|e| format!("Failed to stage the new binary: {e}"))
        }
    });
    if result.is_ok() {
        mark_executable(current_exe);
    }
    result
}

/// macOS never reaches this: `apply_update_and_restart` refuses before closing,
/// and `install_on_quit` never queues an install there.
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn replace_files(_download: &std::path::Path, _current_exe: &std::path::Path) -> Result<(), String> {
    Err("automatic update is not supported on this platform".to_string())
}

#[cfg(target_os = "windows")]
fn is_update_archive(download: &Path) -> bool {
    download.extension().is_some_and(|e| e == "zip")
}

#[cfg(target_os = "linux")]
fn is_update_archive(download: &Path) -> bool {
    download.to_string_lossy().ends_with(".tar.gz") || download.extension().is_some_and(|e| e == "gz")
}

#[cfg(target_os = "windows")]
fn extract_update_archive(download: &Path, staging: &Path) -> Result<(), String> {
    extract_zip_into(download, staging)
}

#[cfg(target_os = "linux")]
fn extract_update_archive(download: &Path, staging: &Path) -> Result<(), String> {
    extract_targz_into(download, staging)
}

/// Extraction does not keep the executable bit on every tar/gzip producer.
#[cfg(target_os = "linux")]
fn mark_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = std::fs::metadata(path) {
        let mut perms = metadata.permissions();
        perms.set_mode(0o755);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(target_os = "linux"))]
fn mark_executable(_path: &Path) {}

/// `path` with `.old` added to its file name: `eustress-engine.exe` becomes
/// `eustress-engine.exe.old`.
fn old_copy_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".old");
    PathBuf::from(name)
}

/// Install through a staging folder: `stage` fills `<install_dir>/<STAGING_DIR_NAME>`,
/// then each staged file moves into place, keeping a `.old` copy of the file it
/// replaces. If any move fails, every new file is removed and every `.old` is
/// put back, so the install folder holds exactly what it held before. The
/// staging folder is removed in every case. After a success the `.old` copies
/// are deleted, and any that cannot be (the running executable on Windows) are
/// listed in `OLD_FILES_JOURNAL` for the next start.
#[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
fn install_staged(
    install_dir: &Path,
    stage: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let staging = install_dir.join(STAGING_DIR_NAME);
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .map_err(|e| format!("Failed to clear the old staging folder {staging:?}: {e}"))?;
    }
    std::fs::create_dir_all(&staging).map_err(|e| format!("Failed to create the staging folder {staging:?}: {e}"))?;

    let result = stage(&staging)
        .and_then(|()| staged_files(&staging))
        .and_then(|files| {
            if files.is_empty() {
                return Err("Refusing update: the archive holds no files".to_string());
            }
            move_into_place(&staging, install_dir, &files)
        })
        .map(|old_copies| remove_old_copies(install_dir, &old_copies));

    if let Err(e) = std::fs::remove_dir_all(&staging) {
        warn!("Could not remove the update staging folder {staging:?}: {e}");
    }
    result
}

/// Every file under `staging`, relative to it, sorted. Anything that is
/// neither a file nor a folder (a symbolic link) fails the update.
fn staged_files(staging: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut folders = vec![PathBuf::new()];
    while let Some(rel_dir) = folders.pop() {
        let dir = staging.join(&rel_dir);
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("Failed to list {dir:?}: {e}"))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("Failed to list {dir:?}: {e}"))?;
            let rel = rel_dir.join(entry.file_name());
            let kind = entry.file_type().map_err(|e| format!("Failed to read {rel:?}: {e}"))?;
            if kind.is_dir() {
                folders.push(rel);
            } else if kind.is_file() {
                files.push(rel);
            } else {
                return Err(format!("Refusing update: staged entry {rel:?} is not a file or folder"));
            }
        }
    }
    files.sort();
    Ok(files)
}

/// One file the install has touched, for the rollback.
struct MovedFile {
    dest: PathBuf,
    /// Where the file it replaced was moved; `None` when `dest` was new.
    old: Option<PathBuf>,
    /// Whether the new file reached `dest`.
    placed: bool,
}

/// Move each staged file (`files`, relative to `staging`) to the same place in
/// `install_dir`. Returns the `.old` copies made. On a failure everything done
/// so far is undone before the error returns.
fn move_into_place(staging: &Path, install_dir: &Path, files: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut moved: Vec<MovedFile> = Vec::new();
    for rel in files {
        if let Err(e) = move_one(staging, install_dir, rel, &mut moved) {
            return Err(match roll_back(&moved) {
                Ok(()) => format!("{e}; every replaced file was put back"),
                Err(restore) => format!("{e}; putting the replaced files back also failed: {restore}"),
            });
        }
    }
    Ok(moved.into_iter().filter_map(|m| m.old).collect())
}

fn move_one(staging: &Path, install_dir: &Path, rel: &Path, moved: &mut Vec<MovedFile>) -> Result<(), String> {
    let src = staging.join(rel);
    let dest = install_dir.join(rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir {parent:?}: {e}"))?;
    }
    let old = match std::fs::symlink_metadata(&dest) {
        Ok(meta) if meta.is_dir() => return Err(format!("Cannot replace the folder {dest:?} with a file")),
        Ok(_) => {
            let old = old_copy_path(&dest);
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&dest, &old).map_err(|e| format!("Failed to move {dest:?} aside: {e}"))?;
            Some(old)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("Failed to read {dest:?}: {e}")),
    };
    moved.push(MovedFile { dest: dest.clone(), old, placed: false });
    std::fs::rename(&src, &dest).map_err(|e| format!("Failed to move the new {rel:?} into place: {e}"))?;
    if let Some(last) = moved.last_mut() {
        last.placed = true;
    }
    Ok(())
}

/// Undo `moved`, newest first: remove each new file and put its `.old` back.
fn roll_back(moved: &[MovedFile]) -> Result<(), String> {
    let mut failures = Vec::new();
    for m in moved.iter().rev() {
        if m.placed {
            if let Err(e) = std::fs::remove_file(&m.dest) {
                failures.push(format!("remove {:?}: {e}", m.dest));
            }
        }
        if let Some(old) = &m.old {
            if let Err(e) = std::fs::rename(old, &m.dest) {
                failures.push(format!("restore {:?}: {e}", m.dest));
            }
        }
    }
    if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
}

/// Delete the `.old` copies a successful install made. Any that cannot be
/// deleted now (the running executable on Windows) are added to
/// `OLD_FILES_JOURNAL`, which the next start reads.
fn remove_old_copies(install_dir: &Path, old_copies: &[PathBuf]) {
    let mut leftover = String::new();
    for old in old_copies {
        if std::fs::remove_file(old).is_err() {
            if let Ok(rel) = old.strip_prefix(install_dir) {
                leftover.push_str(&rel.to_string_lossy().replace('\\', "/"));
                leftover.push('\n');
            }
        }
    }
    if leftover.is_empty() {
        return;
    }
    let journal = install_dir.join(OLD_FILES_JOURNAL);
    let result = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&journal)
        .and_then(|mut file| std::io::Write::write_all(&mut file, leftover.as_bytes()));
    if let Err(e) = result {
        warn!("Could not record the leftover update files in {journal:?}: {e}");
    }
}

/// On start, remove what an earlier install left: the staging folder, the
/// `.old` copies listed in the journal, and the executable's own `.old` copy.
fn clean_up_previous_update() {
    if is_dev_build() {
        return;
    }
    let Ok(current_exe) = std::env::current_exe() else { return };
    let Some(install_dir) = current_exe.parent() else { return };
    clean_up_install_dir(install_dir);
    let _ = std::fs::remove_file(old_copy_path(&current_exe));
}

/// Remove a leftover staging folder and the `.old` files the journal lists.
/// Only paths the journal names, that are relative, inside `install_dir` and
/// end in `.old`, are deleted. A file still in use stays listed for the next
/// start.
fn clean_up_install_dir(install_dir: &Path) {
    let staging = install_dir.join(STAGING_DIR_NAME);
    if staging.exists() {
        match std::fs::remove_dir_all(&staging) {
            Ok(()) => info!("Removed a leftover update staging folder"),
            Err(e) => warn!("Could not remove the leftover update staging folder {staging:?}: {e}"),
        }
    }
    let journal = install_dir.join(OLD_FILES_JOURNAL);
    let Ok(text) = std::fs::read_to_string(&journal) else { return };
    let mut kept = String::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rel) = archive_entry_path(line).filter(|p| p.extension().is_some_and(|e| e == "old")) else {
            continue;
        };
        match std::fs::remove_file(install_dir.join(&rel)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                kept.push_str(line);
                kept.push('\n');
            }
        }
    }
    let _ = if kept.is_empty() {
        std::fs::remove_file(&journal)
    } else {
        std::fs::write(&journal, kept)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty folder under the system temp folder for one test.
    fn temp_case(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress-updater-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn version_comparison() {
        assert!(is_newer("0.3.9", "0.3.8"));
        assert!(is_newer("0.4.0", "0.3.99"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(is_newer("v0.3.9", "0.3.8"));
        assert!(!is_newer("0.3.8", "0.3.8"));
        assert!(!is_newer("0.3.7", "0.3.8"));
        assert!(!is_newer("0.2.99", "0.3.0"));
        assert_eq!(version_tuple("1.2"), (1, 2, 0));
        assert_eq!(version_tuple("garbage"), (0, 0, 0));
    }

    #[test]
    fn environment_switch() {
        assert!(update_check_enabled(None));
        for off in ["0", "false", "no", "off", " OFF ", "False", "NO"] {
            assert!(!update_check_enabled(Some(off)), "{off:?} left checks on");
        }
        for on in ["1", "true", "yes", "on", "", "anything"] {
            assert!(update_check_enabled(Some(on)), "{on:?} turned checks off");
        }
    }

    #[test]
    fn dev_build_detection() {
        assert!(looks_like_cargo_output(Path::new("/home/dev/eustress/target/release")));
        assert!(looks_like_cargo_output(Path::new("/home/dev/eustress/Target/debug")));
        assert!(looks_like_cargo_output(Path::new("/home/dev/eustress/target")));
        assert!(!looks_like_cargo_output(Path::new("/opt/eustress-engine-no-such-folder")));
        assert!(!looks_like_cargo_output(Path::new("/opt/targets/eustress-no-such-folder")));

        for marker in [".fingerprint", "deps", "incremental"] {
            let dir = temp_case(&format!("dev-{marker}"));
            assert!(!looks_like_cargo_output(&dir), "a plain folder looked like cargo output: {dir:?}");
            std::fs::create_dir_all(dir.join(marker)).unwrap();
            assert!(looks_like_cargo_output(&dir), "{marker} was not detected");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn archive_entry_paths() {
        assert_eq!(archive_entry_path("a/b.txt"), Some(PathBuf::from("a").join("b.txt")));
        assert_eq!(archive_entry_path("./eustress-engine"), Some(PathBuf::from("eustress-engine")));
        assert_eq!(archive_entry_path("./"), Some(PathBuf::new()));
        for bad in [
            "../evil",
            "a/../../evil",
            "a/..",
            "/etc/passwd",
            "\\windows\\evil.dll",
            "C:/evil.exe",
            "a\\..\\..\\evil",
            "a\0b",
            ".eustress-update-staging/x",
            ".eustress-update-old-files",
        ] {
            assert_eq!(archive_entry_path(bad), None, "accepted {bad:?}");
        }
    }

    fn build_zip(path: &Path, entries: &[(&str, &str)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in entries {
            zip.start_file(*name, options).unwrap();
            std::io::Write::write_all(&mut zip, body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn staged_install_replaces_files_and_leaves_no_staging() {
        let case = temp_case("staged-ok");
        let install = case.join("install");
        write(&install.join("app.exe"), "old exe");
        write(&install.join("assets").join("a.txt"), "old a");
        write(&install.join("keep.txt"), "keep");
        let zip_path = case.join("update.zip");
        build_zip(&zip_path, &[("app.exe", "new exe"), ("assets/a.txt", "new a"), ("assets/b.txt", "new b")]);

        install_staged(&install, |staging| extract_zip_into(&zip_path, staging)).unwrap();

        assert_eq!(read(&install.join("app.exe")), "new exe");
        assert_eq!(read(&install.join("assets").join("a.txt")), "new a");
        assert_eq!(read(&install.join("assets").join("b.txt")), "new b");
        assert_eq!(read(&install.join("keep.txt")), "keep");
        assert!(!install.join(STAGING_DIR_NAME).exists(), "the staging folder was left behind");
        assert!(!install.join("app.exe.old").exists(), "an .old copy was left behind");
        assert!(!install.join(OLD_FILES_JOURNAL).exists(), "a journal was written with nothing to list");
        let _ = std::fs::remove_dir_all(&case);
    }

    #[test]
    fn staged_install_refuses_a_traversal_entry() {
        let case = temp_case("staged-traversal");
        let install = case.join("install");
        write(&install.join("app.exe"), "old exe");
        let result = install_staged(&install, |staging| {
            write(&staging.join("app.exe"), "new exe");
            archive_entry_path("../evil").map(|_| ()).ok_or_else(|| "unsafe entry".to_string())
        });
        assert!(result.is_err());
        assert_eq!(read(&install.join("app.exe")), "old exe");
        assert!(!install.join(STAGING_DIR_NAME).exists());
        let _ = std::fs::remove_dir_all(&case);
    }

    #[test]
    fn staged_install_failure_halfway_restores_every_original_byte() {
        let case = temp_case("staged-rollback");
        let install = case.join("install");
        write(&install.join("a.txt"), "original a");
        write(&install.join("b.txt"), "original b");
        // A non-empty folder where b.txt's `.old` copy must go: moving b.txt
        // aside fails, after 0new.txt and a.txt have already moved in.
        write(&install.join("b.txt.old").join("blocker"), "x");

        let result = install_staged(&install, |staging| {
            write(&staging.join("0new.txt"), "new file");
            write(&staging.join("a.txt"), "new a");
            write(&staging.join("b.txt"), "new b");
            Ok(())
        });

        let err = result.unwrap_err();
        assert!(err.contains("every replaced file was put back"), "{err}");
        assert_eq!(read(&install.join("a.txt")), "original a");
        assert_eq!(read(&install.join("b.txt")), "original b");
        assert!(!install.join("0new.txt").exists(), "a new file survived the rollback");
        assert!(!install.join("a.txt.old").exists(), "an .old copy survived the rollback");
        assert!(!install.join(STAGING_DIR_NAME).exists(), "the staging folder was left behind");
        let _ = std::fs::remove_dir_all(&case);
    }

    #[test]
    fn start_cleanup_removes_listed_old_files_and_staging() {
        let case = temp_case("cleanup");
        write(&case.join(STAGING_DIR_NAME).join("x"), "x");
        write(&case.join("app.exe.old"), "old");
        write(&case.join("assets").join("a.txt.old"), "old");
        write(&case.join("user.old"), "a file the journal does not list");
        write(&case.join("not-old.txt"), "listed, but not an .old file");
        write(
            &case.join(OLD_FILES_JOURNAL),
            "app.exe.old\nassets/a.txt.old\nnot-old.txt\n../outside.old\nmissing.old\n",
        );

        clean_up_install_dir(&case);

        assert!(!case.join(STAGING_DIR_NAME).exists());
        assert!(!case.join("app.exe.old").exists());
        assert!(!case.join("assets").join("a.txt.old").exists());
        assert!(case.join("user.old").exists(), "an unlisted file was deleted");
        assert!(case.join("not-old.txt").exists(), "a listed file without .old was deleted");
        assert!(!case.join(OLD_FILES_JOURNAL).exists(), "the journal outlived its entries");
        let _ = std::fs::remove_dir_all(&case);
    }
}
