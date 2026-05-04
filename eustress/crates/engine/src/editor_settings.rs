//! # Editor Settings Module
//!
//! Manages persistent editor settings for Eustress Engine.
//!
//! ## Features
//! - **Automatic Loading**: Settings are loaded from `~/.eustress_engine/settings.json` on startup
//! - **Auto-Save**: Settings are automatically saved when modified via Bevy's change detection
//! - **Default Fallback**: If loading fails or no file exists, default settings are used
//! - **Pretty JSON**: Settings are saved in human-readable JSON format
//!
//! ## Settings Persistence
//! - **Location**: `~/.eustress_engine/settings.json`
//! - **Format**: JSON with pretty formatting
//! - **Auto-creation**: Directory is created automatically if it doesn't exist
//!
//! ## Usage
//! Settings are automatically loaded and saved by the `EditorSettingsPlugin`.
//! Modify settings via `ResMut<EditorSettings>` and they will auto-save.

#![allow(dead_code)]

use bevy::prelude::*;
use bevy::gizmos::config::{GizmoConfigStore, DefaultGizmoConfigGroup};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use bevy::log::{info, warn};

/// Global editor settings resource
/// 
/// Automatically persisted to `~/.eustress_engine/settings.json`
#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct EditorSettings {
    /// Grid snap size in world units
    pub snap_size: f32,
    
    /// Enable snapping to grid
    pub snap_enabled: bool,
    
    /// Enable collision-based snapping
    pub collision_snap: bool,
    
    /// Enable surface snapping (raycast to place parts on other parts)
    #[serde(default = "default_surface_snap")]
    pub surface_snap_enabled: bool,

    /// Align-to-normal on surface drop — when free-dragging a part onto
    /// another surface with `surface_snap_enabled`, also rotate the
    /// part so its local +Y aligns with the hit surface normal. Default
    /// off (preserves pre-Phase-1 behaviour).
    #[serde(default)]
    pub align_to_normal_on_drop: bool,

    /// Scale Lock — when true, dragging any Scale face handle scales
    /// uniformly (preserves axis ratios). For CAD features where
    /// proportional scaling is the common case; disable for free-form
    /// box-shape edits. Default off — Phase 2 opt-in.
    #[serde(default)]
    pub scale_lock_proportional: bool,
    
    /// Angle snap increment in degrees
    pub angle_snap: f32,
    
    /// Show grid in viewport
    pub show_grid: bool,
    
    /// Grid size
    pub grid_size: f32,
    
    /// Auto-save interval in seconds (0 = disabled)
    pub auto_save_interval: f32,
    
    /// Enable auto-save for scenes
    pub auto_save_enabled: bool,

    /// Saved identity file paths for quick-switch login.
    /// Each entry is (username, absolute path to eustress-username.toml).
    /// The first entry is the active identity (auto-login on startup).
    #[serde(default)]
    pub saved_identities: Vec<SavedIdentity>,

    /// Last opened space path — restored on next launch instead of defaulting
    /// to the first alphabetical space.
    #[serde(default)]
    pub last_space_path: Option<String>,
}

/// A saved identity for quick-switch login.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedIdentity {
    /// Username extracted from the identity file.
    pub username: String,
    /// Absolute path to the identity TOML file.
    pub path: String,
    /// Public key (first 8 chars for display).
    pub public_key_short: String,
}

fn default_surface_snap() -> bool {
    true
}

/// Resource to track auto-save state
#[derive(Resource)]
pub struct AutoSaveState {
    /// Timer for auto-save
    pub timer: f32,
    /// Last save time
    pub last_save: Option<std::time::Instant>,
    /// Current scene path (if any)
    pub current_scene_path: Option<PathBuf>,
    /// Has unsaved changes
    pub has_changes: bool,
}

impl Default for AutoSaveState {
    fn default() -> Self {
        Self {
            timer: 0.0,
            last_save: None,
            current_scene_path: None,
            has_changes: false,
        }
    }
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            snap_size: 1.0, // 1m default (press 1/2/3 to change)
            snap_enabled: true,
            collision_snap: false,
            surface_snap_enabled: true,
            align_to_normal_on_drop: false,
            scale_lock_proportional: false,
            angle_snap: 15.0,
            show_grid: true,
            grid_size: 1.0, // 1m grid lines

            auto_save_interval: 300.0, // 5 minutes
            auto_save_enabled: true,
            saved_identities: Vec::new(),
            last_space_path: None,
        }
    }
}

impl EditorSettings {
    /// Get the settings file path (~/.eustress_engine/settings.json).
    ///
    /// Performs a one-shot migration of the legacy `~/.eustress_studio/`
    /// directory the very first time the new location is requested.
    /// Renaming in-place is atomic on all three platforms and keeps the
    /// user's `settings.json`, `autosave/`, and any other sidecar
    /// artefacts together — no per-file copy needed.
    fn settings_path() -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        let new_dir = home.join(".eustress_engine");
        let legacy_dir = home.join(".eustress_studio");
        if !new_dir.exists() && legacy_dir.exists() {
            if let Err(e) = fs::rename(&legacy_dir, &new_dir) {
                // Rename can fail if the target volume differs or a
                // concurrent process holds a handle — fall back silently
                // and let the caller re-create the default settings.
                warn!("Could not migrate {:?} → {:?}: {}", legacy_dir, new_dir, e);
            }
        }
        Some(new_dir.join("settings.json"))
    }
    
    /// Load settings from file or create default
    pub fn load() -> Self {
        if let Some(path) = Self::settings_path() {
            if path.exists() {
                // Try to load from file
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        match serde_json::from_str::<EditorSettings>(&content) {
                            Ok(settings) => {
                                println!("✅ Loaded editor settings from {:?}", path);
                                return settings;
                            }
                            Err(e) => {
                                eprintln!("⚠ Failed to parse settings file: {}. Using defaults.", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("⚠ Failed to read settings file: {}. Using defaults.", e);
                    }
                }
            } else {
                println!("ℹ No settings file found. Creating default settings.");
            }
        } else {
            eprintln!("⚠ Could not determine home directory. Using default settings.");
        }
        
        // Return default settings if loading failed
        Self::default()
    }
    
    /// Save settings to file
    pub fn save(&self) -> Result<(), String> {
        let path = Self::settings_path()
            .ok_or_else(|| "Could not determine home directory".to_string())?;
        
        // Create directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create settings directory: {}", e))?;
        }
        
        // Serialize settings to JSON with pretty formatting
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize settings: {}", e))?;
        
        // Write to file
        fs::write(&path, json)
            .map_err(|e| format!("Failed to write settings file: {}", e))?;
        
        println!("✅ Saved editor settings to {:?}", path);
        Ok(())
    }
    
    /// Apply snap to a value
    pub fn apply_snap(&self, value: f32) -> f32 {
        if self.snap_enabled && self.snap_size > 0.0 {
            (value / self.snap_size).round() * self.snap_size
        } else {
            value
        }
    }
    
    /// Apply snap to a vector
    pub fn apply_snap_vec3(&self, value: Vec3) -> Vec3 {
        if self.snap_enabled {
            Vec3::new(
                self.apply_snap(value.x),
                self.apply_snap(value.y),
                self.apply_snap(value.z),
            )
        } else {
            value
        }
    }
    
    /// Apply angle snap
    pub fn apply_angle_snap(&self, angle_degrees: f32) -> f32 {
        if self.snap_enabled && self.angle_snap > 0.0 {
            (angle_degrees / self.angle_snap).round() * self.angle_snap
        } else {
            angle_degrees
        }
    }
}

/// Plugin to manage editor settings
pub struct EditorSettingsPlugin;

impl Plugin for EditorSettingsPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(EditorSettings::load())
            .init_resource::<AutoSaveState>()
            .add_systems(Startup, setup_grid_gizmo_config)
            .add_systems(Update, draw_grid_overlay)
            .add_systems(Update, auto_save_settings)
            .add_systems(Update, auto_save_scene_system);
    }
}

/// Auto-save settings when they change
fn auto_save_settings(
    settings: Res<EditorSettings>,
) {
    // Save when settings are modified
    if settings.is_changed() && !settings.is_added() {
        if let Err(e) = settings.save() {
            eprintln!("❌ Failed to save editor settings: {}", e);
        }
    }
}

/// Configure grid gizmos — normal depth testing so grid renders at ground level
fn setup_grid_gizmo_config(
    mut config_store: ResMut<GizmoConfigStore>,
) {
    let (config, _) = config_store.config_mut::<DefaultGizmoConfigGroup>();
    // depth_bias = 0.0: normal depth testing. Grid lines at y=0.01 render
    // on the ground plane. Positive = towards camera in Bevy 0.18 reversed-Z.
    config.depth_bias = 0.0;
}

/// Draw grid overlay in viewport - follows camera on X/Z plane
/// Origin axes (red X, blue Z) stay fixed at world origin
fn draw_grid_overlay(
    mut gizmos: Gizmos,
    settings: Res<EditorSettings>,
    camera_query: Query<&Transform, With<Camera3d>>,
) {
    if !settings.show_grid {
        return;
    }
    
    // Get camera position to center grid around it
    let camera_pos = camera_query.iter().next()
        .map(|t| t.translation)
        .unwrap_or(Vec3::ZERO);
    
    let grid_size = settings.grid_size;  // Spacing between grid lines
    let grid_half_extent = 100.0;  // How far the grid extends from camera
    let grid_divisions = (grid_half_extent * 2.0 / grid_size) as i32;
    
    // Snap grid center to grid increments so lines don't jitter when camera moves
    let grid_center_x = (camera_pos.x / grid_size).round() * grid_size;
    let grid_center_z = (camera_pos.z / grid_size).round() * grid_size;
    
    // Render grid slightly above ground (y = 0.01) to prevent z-fighting
    let grid_y = 0.01;
    
    // Grid line color
    let grid_color = Color::srgba(0.4, 0.4, 0.4, 0.5);
    
    // Draw grid lines centered around camera position
    for i in 0..=grid_divisions {
        let offset = (i as f32 * grid_size) - grid_half_extent;
        
        // Lines parallel to X axis (running along X, at different Z positions)
        let z_pos = grid_center_z + offset;
        gizmos.line(
            Vec3::new(grid_center_x - grid_half_extent, grid_y, z_pos),
            Vec3::new(grid_center_x + grid_half_extent, grid_y, z_pos),
            grid_color,
        );
        
        // Lines parallel to Z axis (running along Z, at different X positions)
        let x_pos = grid_center_x + offset;
        gizmos.line(
            Vec3::new(x_pos, grid_y, grid_center_z - grid_half_extent),
            Vec3::new(x_pos, grid_y, grid_center_z + grid_half_extent),
            grid_color,
        );
    }
    
    // ════════════════════════════════════════════════════════════════════════
    // Origin axes - ALWAYS at world origin (0, 0, 0)
    // These extend far enough to be visible from anywhere
    // ════════════════════════════════════════════════════════════════════════
    
    let axis_extent = 10000.0;  // Very long so always visible
    let origin_y = 0.02;  // Slightly above grid to render on top
    
    // X-axis (RED) - runs along X at Z=0
    gizmos.line(
        Vec3::new(-axis_extent, origin_y, 0.0),
        Vec3::new(axis_extent, origin_y, 0.0),
        Color::srgba(1.0, 0.2, 0.2, 0.9),
    );
    
    // Z-axis (BLUE) - runs along Z at X=0
    gizmos.line(
        Vec3::new(0.0, origin_y, -axis_extent),
        Vec3::new(0.0, origin_y, axis_extent),
        Color::srgba(0.2, 0.2, 1.0, 0.9),
    );
    
    // Small Y-axis indicator at origin (GREEN)
    gizmos.line(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 5.0, 0.0),
        Color::srgba(0.2, 1.0, 0.2, 0.9),
    );
}

// ============================================================================
// Auto-Save Scene System
// ============================================================================

/// System to auto-save the current scene at regular intervals.
///
/// The modern autosave commits the Space directory itself via git,
/// instead of writing an opaque binary snapshot to a user-profile
/// sidecar. This keeps every autosave recoverable with the same
/// `git checkout` / `git reflog` tools the user (and any external
/// editor) already knows, and piggybacks on git's delta compression
/// so autosaves cost effectively nothing on disk past the first one.
///
/// The Space's live on-disk state is already authoritative (every tool
/// edits TOML files directly via `write_instance_changes_system`) —
/// autosave only needs to capture a commit boundary, not re-derive the
/// scene from the ECS. That also makes autosave a no-op when nothing's
/// changed since the last tick, which is the common case while the
/// user is just looking around.
fn auto_save_scene_system(
    time: Res<Time>,
    settings: Res<EditorSettings>,
    mut auto_save: ResMut<AutoSaveState>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    auth: Option<Res<crate::auth::AuthState>>,
) {
    // Skip if auto-save is disabled
    if !settings.auto_save_enabled || settings.auto_save_interval <= 0.0 {
        return;
    }

    // Update timer
    auto_save.timer += time.delta_secs();

    // Check if it's time to auto-save
    if auto_save.timer < settings.auto_save_interval {
        return;
    }
    auto_save.timer = 0.0;

    let Some(space_root) = space_root else { return };
    let space_path = space_root.0.clone();
    if !space_path.exists() {
        return;
    }

    // Snapshot the current Eustress identity on the main thread so the
    // background commit can author under the logged-in user. Offline /
    // signed-out sessions get `None` and fall through to the anonymous
    // repo-local fallback. Captured once per tick so a late-breaking
    // logout during the commit doesn't change the author mid-write.
    let identity = auth
        .as_deref()
        .and_then(git_identity_from_auth);

    // Dispatch the git work to a background thread. `git add -A` +
    // commit can hit the filesystem harder than we want to pay for on
    // the main frame, and blocking the render loop for autosave would
    // make the editor hitch every interval.
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let message = format!("autosave {}", timestamp);
    std::thread::spawn(move || {
        match git_autosave_commit(&space_path, &message, identity.as_ref()) {
            Ok(GitAutosave::Committed(sha)) => {
                info!("✅ Autosave committed to git @ {} ({})", sha, space_path.display());
            }
            Ok(GitAutosave::NoChanges) => {
                // Quiet no-op — nothing changed since the last autosave.
            }
            Err(e) => {
                warn!("git autosave failed at {:?}: {}", space_path, e);
            }
        }
    });

    auto_save.last_save = Some(std::time::Instant::now());
    notifications.info("Auto-saved (git)".to_string());
}

pub(crate) enum GitAutosave {
    Committed(String),
    NoChanges,
}

/// Per-commit git author identity. Pulled from `AuthState` when the
/// user is logged in so autosave commits attribute to them; `None`
/// means "use the anonymous repo-local fallback".
pub(crate) struct GitIdentity {
    pub name: String,
    pub email: String,
}

/// Map the live `AuthState` to a git identity — only when the session
/// is actually online. The email is synthesised from the public key so
/// the author line is provably tied to the signing identity without
/// leaking the real user email (which Eustress doesn't store anyway).
pub(crate) fn git_identity_from_auth(auth: &crate::auth::AuthState) -> Option<GitIdentity> {
    if auth.status != crate::auth::AuthStatus::LoggedIn {
        return None;
    }
    let user = auth.user.as_ref()?;
    Some(GitIdentity {
        name: user.username.clone(),
        email: format!("{}@eustress.local", user.id),
    })
}

/// Init the space's git repo if missing, stage every change, and commit
/// with the supplied `message`. Returns the short SHA on commit, or
/// `NoChanges` when the working tree matches HEAD.
///
/// When `identity` is `Some(_)` the commit is authored under the
/// logged-in user via one-shot `-c user.name=… -c user.email=…` flags
/// — the repo's own `.git/config` is never rewritten per-commit, so a
/// logout (or a login as a different user) between ticks picks up the
/// new author automatically without stale config lying around.
pub(crate) fn git_autosave_commit(
    space_path: &std::path::Path,
    message: &str,
    identity: Option<&GitIdentity>,
) -> Result<GitAutosave, String> {
    use std::process::Command;

    // Wrapper so the one-off commands all share the same cwd + error
    // shape. Output is captured so nothing leaks to the engine stdout.
    let run = |args: &[&str]| -> Result<std::process::Output, String> {
        Command::new("git")
            .args(args)
            .current_dir(space_path)
            .output()
            .map_err(|e| format!("git {:?}: {}", args, e))
    };

    let git_dir = space_path.join(".git");
    if !git_dir.exists() {
        let init = run(&["init", "--quiet"])?;
        if !init.status.success() {
            return Err(format!("git init failed: {}", String::from_utf8_lossy(&init.stderr)));
        }
        // Fallback identity so a `git commit` run WITHOUT the per-call
        // identity override (e.g. from a terminal outside the engine)
        // doesn't fail with "please tell me who you are". Scoped to
        // this repo only — never touches the user's `~/.gitconfig`.
        let _ = run(&["config", "user.email", "autosave@eustress.local"]);
        let _ = run(&["config", "user.name", "Eustress Engine Autosave"]);
    }

    // Stage everything. `-A` picks up adds / modifies / deletes in one
    // pass without requiring the caller to enumerate paths.
    let add = run(&["add", "-A"])?;
    if !add.status.success() {
        return Err(format!("git add failed: {}", String::from_utf8_lossy(&add.stderr)));
    }

    // Fast no-op check — `git diff --cached --quiet` exits 0 when the
    // index matches HEAD (nothing to commit). Only on exit 1 do we
    // actually have changes to record.
    let diff = run(&["diff", "--cached", "--quiet"])?;
    if diff.status.success() {
        return Ok(GitAutosave::NoChanges);
    }

    // Build the commit args, prepending `-c` identity overrides when
    // we have a logged-in identity. The overrides are scoped to this
    // single invocation so they never persist in the repo config.
    let mut commit_args: Vec<String> = Vec::new();
    let name_override;
    let email_override;
    if let Some(id) = identity {
        name_override = format!("user.name={}", id.name);
        email_override = format!("user.email={}", id.email);
        commit_args.push("-c".into());
        commit_args.push(name_override.clone());
        commit_args.push("-c".into());
        commit_args.push(email_override.clone());
    }
    commit_args.push("commit".into());
    commit_args.push("-m".into());
    commit_args.push(message.to_string());
    commit_args.push("--quiet".into());
    let commit_args_ref: Vec<&str> = commit_args.iter().map(|s| s.as_str()).collect();
    let commit = run(&commit_args_ref)?;
    if !commit.status.success() {
        return Err(format!("git commit failed: {}", String::from_utf8_lossy(&commit.stderr)));
    }

    // Short SHA for the log line. Fall back to "HEAD" if rev-parse fails.
    let sha = run(&["rev-parse", "--short", "HEAD"])
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "HEAD".to_string());

    Ok(GitAutosave::Committed(sha))
}

