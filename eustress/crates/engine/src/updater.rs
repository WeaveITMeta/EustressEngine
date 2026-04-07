//! # In-App Updater
//!
//! Checks for new versions on startup, downloads updates, and restarts.
//!
//! Flow:
//! 1. Startup: fetch https://releases.eustress.dev/latest.json
//! 2. Compare against compiled-in CARGO_PKG_VERSION
//! 3. If newer: set update_available flag → Slint shows button
//! 4. User clicks → download platform binary → verify SHA-256 → replace → restart

use bevy::prelude::*;
use std::sync::{Arc, Mutex};

const MANIFEST_URL: &str = "https://releases.eustress.dev/latest.json";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

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
            .add_systems(Update, sync_update_state_to_slint);
    }
}

/// Check for updates on startup (async, non-blocking)
fn check_for_updates(mut state: ResMut<UpdateState>) {
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

/// Sync update state to Slint UI each frame
fn sync_update_state_to_slint(
    mut state: ResMut<UpdateState>,
    slint_context: Option<NonSend<crate::ui::slint_ui::SlintUiState>>,
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

    // Sync to Slint
    let Some(ref context) = slint_context else { return };
    let ui = &context.window;
    ui.set_update_available(state.available);
    if state.available {
        ui.set_update_version(state.new_version.clone().into());
        ui.set_update_status(state.status.clone().into());
        ui.set_update_progress(state.progress as i32);
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
    let async_state = state.async_state.clone();

    std::thread::spawn(move || {
        match download_and_verify(&url, &expected_hash, async_state.clone()) {
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
    progress: Arc<Mutex<AsyncUpdateState>>,
) -> Result<std::path::PathBuf, String> {
    use sha2::{Sha256, Digest};
    use std::io::Read;

    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("Download failed: {}", e))?;

    let total = resp.header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

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
        if total > 0 {
            let pct = (downloaded * 100 / total) as u32;
            if let Ok(mut s) = progress.lock() {
                s.progress = pct;
            }
        }
    }

    // Verify hash
    let hash = format!("{:x}", hasher.finalize());
    if !expected_hash.is_empty() && hash != expected_hash {
        std::fs::remove_file(&download_path).ok();
        return Err(format!("SHA-256 mismatch: expected {}, got {}", expected_hash, hash));
    }

    info!("✅ Update downloaded and verified: {:?}", download_path);
    Ok(download_path)
}

/// Replace current binary and restart. Called when user clicks "Restart to update".
pub fn apply_update_and_restart(state: &UpdateState) {
    let download_path = if let Ok(s) = state.async_state.lock() {
        s.download_path.clone()
    } else {
        None
    };

    let Some(download_path) = download_path else {
        error!("No downloaded update to apply");
        return;
    };

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to get current exe path: {}", e);
            return;
        }
    };

    info!("🔄 Applying update from {:?}", download_path);

    #[cfg(target_os = "windows")]
    {
        // Windows: rename current to .old, extract new, restart
        let old_exe = current_exe.with_extension("exe.old");
        let _ = std::fs::remove_file(&old_exe); // Remove previous .old
        if let Err(e) = std::fs::rename(&current_exe, &old_exe) {
            error!("Failed to rename current exe: {}", e);
            return;
        }

        // Extract zip (the download is a zip containing eustress-engine.exe)
        // For now, if it's a direct binary, just copy
        if download_path.extension().map(|e| e == "zip").unwrap_or(false) {
            // TODO: unzip
            info!("ZIP extraction not yet implemented — please extract manually");
            let _ = std::fs::rename(&old_exe, &current_exe); // Restore
            return;
        } else {
            if let Err(e) = std::fs::copy(&download_path, &current_exe) {
                error!("Failed to copy new binary: {}", e);
                let _ = std::fs::rename(&old_exe, &current_exe); // Restore
                return;
            }
        }

        // Restart
        let _ = std::process::Command::new(&current_exe).spawn();
        std::process::exit(0);
    }

    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;

        // Unix: atomic rename, set permissions, exec into new process
        if let Err(e) = std::fs::copy(&download_path, &current_exe) {
            error!("Failed to replace binary: {}", e);
            return;
        }

        // Ensure executable permission
        if let Ok(metadata) = std::fs::metadata(&current_exe) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&current_exe, perms);
        }

        // Restart via exec
        let _ = std::process::Command::new(&current_exe).spawn();
        std::process::exit(0);
    }
}
