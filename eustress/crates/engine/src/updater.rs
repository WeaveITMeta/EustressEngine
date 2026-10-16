//! # In-App Updater
//!
//! Checks for new versions on startup, downloads updates, and restarts.
//!
//! Flow:
//! 1. Startup: fetch https://downloads.eustress.dev/latest.json
//! 2. Compare against compiled-in CARGO_PKG_VERSION
//! 3. If newer: set update_available flag → Slint shows button
//! 4. User clicks → download platform binary → verify SHA-256 → replace → restart

use bevy::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const MANIFEST_URL: &str = "https://downloads.eustress.dev/latest.json";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the ribbon says when this install cannot be updated in place. The reason is in the log.
const REFUSED_UI_TEXT: &str = "Reinstall to enable updates";

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
            .add_systems(Startup, check_for_updates)
            .add_systems(Update, sync_update_state_to_slint)
            .add_systems(Update, exit_for_pending_update);
    }
}

/// Check for updates on startup (async, non-blocking)
fn check_for_updates(state: ResMut<UpdateState>) {
    info!("🔄 Checking for updates (current: v{})", CURRENT_VERSION);

    let async_state = state.async_state.clone();

    // Spawn background thread for HTTP request
    std::thread::spawn(move || {
        let result = check_manifest();
        if let Ok(Some((version, url, hash, size))) = result {
            if let Ok(mut s) = async_state.lock() {
                s.status = "available".to_string();
                // Store in a way the main thread can read
                // We'll use the status field to communicate
            }
            info!("⬆ Update available: v{} ({} bytes)", version, size);
            // Store result for main thread via a different mechanism
            // since we need to update the Bevy resource
            UPDATE_CHECK_RESULT.lock().unwrap().replace((version, url, hash, size));
        } else {
            info!("✅ Up to date (v{})", CURRENT_VERSION);
        }
    });
}

/// Global for passing async check result to main thread
static UPDATE_CHECK_RESULT: Mutex<Option<(String, String, String, u64)>> = Mutex::new(None);

/// The downloaded update the user has asked to apply. Taken by
/// `finish_pending_update` once the engine has shut down.
static PENDING_UPDATE: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

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
    mut pushed: Local<PushedUpdate>,
) {
    // Check if async update check completed
    if !state.available {
        if let Ok(mut result) = UPDATE_CHECK_RESULT.lock() {
            if let Some((version, url, hash, size)) = result.take() {
                state.available = true;
                state.new_version = version;
                state.download_url = url;
                state.expected_hash = hash;
                state.size_bytes = size;
                info!("⬆ Update to v{} ready to download", state.new_version);
            }
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
    let platform_key = if cfg!(target_os = "windows") {
        "windows-x64"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") { "macos-arm64" } else { "macos-x64" }
    } else {
        "linux-x64"
    };

    let platform = body.get("platforms")
        .and_then(|p| p.get(platform_key))
        .ok_or(format!("No platform entry for {}", platform_key))?;

    let url = platform.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let hash = platform.get("sha256").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let size = platform.get("size_bytes").and_then(|v| v.as_u64()).unwrap_or(0);

    Ok(Some((remote_version.to_string(), url, hash, size)))
}

/// Simple semver comparison: is `remote` newer than `current`?
fn is_newer(remote: &str, current: &str) -> bool {
    let parse = |s: &str| -> (u32, u32, u32) {
        let s = s.strip_prefix('v').unwrap_or(s);
        let parts: Vec<u32> = s.split('.').filter_map(|p| p.parse().ok()).collect();
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
        )
    };

    let r = parse(remote);
    let c = parse(current);

    r > c
}

/// Start downloading the update (called from Slint callback)
pub fn start_download(state: &mut UpdateState) {
    if state.download_url.is_empty() || state.status == "downloading" {
        return;
    }

    state.status = "downloading".to_string();
    state.progress = 0;

    let url = state.download_url.clone();
    let expected_hash = state.expected_hash.clone();
    let expected_size = state.size_bytes;
    let async_state = state.async_state.clone();

    std::thread::spawn(move || {
        match download_and_verify(&url, &expected_hash, expected_size, async_state.clone()) {
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
fn download_and_verify(
    url: &str,
    expected_hash: &str,
    expected_size: u64,
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

    let agent = ureq::AgentBuilder::new().redirects(0).build();
    let resp = agent.get(url)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("Download failed: {}", e))?;

    let total = resp.header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    // Hard cap on bytes written so a hostile/oversized manifest cannot fill the
    // disk. Prefer the manifest's declared size (+1 MiB slack); otherwise fall
    // back to a 2 GiB absolute ceiling.
    let max_bytes: u64 = if expected_size > 0 { expected_size + (1u64 << 20) } else { 2u64 << 30 };
    if total > max_bytes {
        return Err(format!("Refusing update: advertised size {total} exceeds cap {max_bytes}"));
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

    // Verify hash — fail CLOSED. A missing/short/non-hex hash means the manifest
    // cannot vouch for these bytes, so refuse rather than install something
    // unverified. (Previously an empty `sha256` silently skipped verification.)
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

/// Extract a zip archive's entries into `install_dir`, preserving its
/// internal directory structure (the release zip's root already IS the
/// install layout: `eustress-engine.exe` + `assets/**` at the top level —
/// see `.github/workflows/release.yml`'s Windows Package step). Rejects
/// unsafe entries (path traversal / absolute paths) via `enclosed_name()`.
#[cfg(target_os = "windows")]
fn extract_zip_into(zip_path: &std::path::Path, install_dir: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("Failed to open update zip: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Failed to read update zip: {e}"))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry {i}: {e}"))?;
        let Some(rel_path) = entry.enclosed_name() else {
            continue; // unsafe entry (traversal/absolute) — skip rather than fail the whole update
        };
        let dest = install_dir.join(&rel_path);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| format!("Failed to create dir {dest:?}: {e}"))?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir {parent:?}: {e}"))?;
        }
        let mut out = std::fs::File::create(&dest).map_err(|e| format!("Failed to create {dest:?}: {e}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("Failed to write {dest:?}: {e}"))?;
    }
    Ok(())
}

/// Extract a `.tar.gz` archive's entries into `install_dir`. `unpack_in`
/// (not the less-safe `unpack`) rejects path-traversal entries per-entry
/// and creates intermediate directories as needed.
#[cfg(target_os = "linux")]
fn extract_targz_into(archive_path: &std::path::Path, install_dir: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::open(archive_path).map_err(|e| format!("Failed to open update archive: {e}"))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    for entry in archive
        .entries()
        .map_err(|e| format!("Failed to read update archive: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("Failed to read archive entry: {e}"))?;
        entry
            .unpack_in(install_dir)
            .map_err(|e| format!("Failed to extract archive entry: {e}"))?;
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
        *pending = Some(download_path);
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

/// Apply an update that `apply_update_and_restart` requested, then start the new
/// build. `main` calls this after `App::run` returns and the app has been dropped.
/// By then the shutdown systems have run and no system still reads the files.
///
/// The engine is started again in every case, so a failed update never leaves the
/// user without one. The failure is logged.
pub fn finish_pending_update() {
    let Some(download_path) = PENDING_UPDATE.lock().ok().and_then(|mut p| p.take()) else {
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

    match replace_files(&download_path, &current_exe) {
        Ok(()) => info!("✅ Update applied; starting the new build"),
        Err(e) => error!("Update apply failed; starting the current build: {e}"),
    }
    if let Err(e) = std::process::Command::new(&current_exe).spawn() {
        error!("Failed to start the engine after the update: {e}");
    }
}

/// Puts the downloaded build in place of the running one.
#[cfg(target_os = "windows")]
fn replace_files(download: &std::path::Path, current_exe: &std::path::Path) -> Result<(), String> {
    // Rename the running exe out of the way first, so the archive's own
    // eustress-engine.exe can be written to that exact path.
    let old_exe = current_exe.with_extension("exe.old");
    let _ = std::fs::remove_file(&old_exe);
    std::fs::rename(current_exe, &old_exe).map_err(|e| format!("Failed to rename current exe: {e}"))?;

    let result = match current_exe.parent() {
        None => Err("Could not determine install directory".to_string()),
        Some(install_dir) if download.extension().is_some_and(|e| e == "zip") => {
            extract_zip_into(download, install_dir)
        }
        // Not a zip: a raw binary drop-in.
        Some(_) => std::fs::copy(download, current_exe)
            .map(|_| ())
            .map_err(|e| format!("Failed to copy new binary: {e}")),
    };
    if result.is_err() {
        let _ = std::fs::rename(&old_exe, current_exe); // Restore
    }
    result
}

/// Puts the downloaded build in place of the running one.
#[cfg(target_os = "linux")]
fn replace_files(download: &std::path::Path, current_exe: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let old_exe = current_exe.with_extension("old");
    let _ = std::fs::remove_file(&old_exe);
    std::fs::rename(current_exe, &old_exe).map_err(|e| format!("Failed to rename current exe: {e}"))?;

    let is_archive = download.to_string_lossy().ends_with(".tar.gz")
        || download.extension().is_some_and(|e| e == "gz");
    let result = match current_exe.parent() {
        None => Err("Could not determine install directory".to_string()),
        Some(install_dir) if is_archive => extract_targz_into(download, install_dir),
        Some(_) => std::fs::copy(download, current_exe)
            .map(|_| ())
            .map_err(|e| format!("Failed to replace binary: {e}")),
    };
    match result {
        Err(e) => {
            let _ = std::fs::rename(&old_exe, current_exe); // Restore
            Err(e)
        }
        Ok(()) => {
            // Extraction does not keep the executable bit on every tar/gzip producer.
            if let Ok(metadata) = std::fs::metadata(current_exe) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(current_exe, perms);
            }
            Ok(())
        }
    }
}

/// macOS never reaches this: `apply_update_and_restart` refuses before closing.
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn replace_files(_download: &std::path::Path, _current_exe: &std::path::Path) -> Result<(), String> {
    Err("automatic update is not supported on this platform".to_string())
}
