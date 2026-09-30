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
    /// Roblox's "Collisions": a dragged part stops instead of passing
    /// through another part. Off by default so free placement stays free.
    #[serde(default, alias = "collision_snap")]
    pub collisions_enabled: bool,
    
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
    /// Most recently opened Space paths, newest first, for File > Recent.
    #[serde(default)]
    pub recent_spaces: Vec<String>,
    /// Most recent inserts, newest first, as the id after `insert:`
    /// ("PointLight", "sphere"). The Insert menu's Recent section; kept by
    /// `ui::insert_classes::recent_after_insert`.
    #[serde(default)]
    pub recent_inserts: Vec<String>,

    /// Modern (dark/glass, high-tech) vs Classic (today's flat look)
    /// theme. Classic is the default for new/never-saved settings — see
    /// `default_theme_modern`. Pushed into the Slint `Theme.modern`
    /// global by `init_theme_to_slint` on startup and by the
    /// `SetThemeModern` drain handler on every live toggle.
    #[serde(default = "default_theme_modern")]
    pub theme_modern: bool,

    /// Active TOML theme id (e.g. "classic", "modern", or a user theme's id).
    /// `None` for settings written before the theme engine existed — the
    /// theme registry migrates from `theme_modern` in that case. `SelectTheme`
    /// writes it (and keeps `theme_modern` in sync for back-compat).
    #[serde(default)]
    pub active_theme_id: Option<String>,

    /// Active "Eustress Mode" manifest id (e.g. "engineering", "justice",
    /// or a user-submitted mode's id). Resolved against `ModeRegistry` at
    /// startup; falls back to "engineering" if the id no longer resolves
    /// (mode was renamed/removed from the Modes folder).
    #[serde(default = "default_active_mode")]
    pub active_mode_id: String,

    /// Active submode within `active_mode_id`, if that mode has any (e.g.
    /// Justice's Civil/Criminal/Judge, Military's six branches). Empty
    /// string = no specific submode selected.
    #[serde(default)]
    pub active_submode_id: String,

    /// Per-mode last-used Layout preset NAME (not index — names are
    /// stable across a future preset-list reorder). Keyed by mode id so
    /// switching Modes remembers each mode's own Layout choice
    /// independently.
    #[serde(default)]
    pub layout_preset_by_mode: std::collections::HashMap<String, String>,

    /// Anonymous usage telemetry — counts which ribbon tools get clicked so
    /// the (mostly aspirational) tool surface can be wired in demand order.
    /// ON by default; see `usage_telemetry.rs` for the privacy posture and
    /// Settings ▸ Notifications ▸ Privacy for the user-facing switch.
    #[serde(default = "default_usage_telemetry_enabled")]
    pub usage_telemetry_enabled: bool,

    /// Whether the first-run "usage stats are on" notice has been shown.
    /// Persisted so the notice appears exactly once per install, not once
    /// per launch.
    #[serde(default)]
    pub telemetry_notice_shown: bool,

    /// Bliss node mode — "Light" or "Full". Persisted because opting into a
    /// Full node is a deliberate resource commitment (~2GB RAM, stores chain
    /// data, +10% earning bonus); silently reverting to Light on every launch
    /// both loses the user's choice and quietly costs them the bonus.
    #[serde(default = "default_bliss_node_mode")]
    pub bliss_node_mode: String,

    /// Whether Bliss participation is enabled at all (the badge's on/off
    /// toggle). Same reasoning: an explicit opt-out must survive a restart.
    #[serde(default = "default_bliss_enabled")]
    pub bliss_enabled: bool,

    /// The Ribbon's tool rows are folded away (menu bar and tab row only).
    #[serde(default)]
    pub ribbon_collapsed: bool,

    // Settings > Graphics and Audio, applied by `crate::preferences`.

    /// Shadow quality, 0 Low to 3 Ultra. High (2) is the engine's defaults.
    #[serde(default = "default_render_quality")]
    pub render_quality: u8,
    /// Lights cast shadows.
    #[serde(default = "default_true")]
    pub shadows_enabled: bool,
    /// SMAA on the Studio cameras.
    #[serde(default = "default_true")]
    pub anti_aliasing: bool,
    /// Present in step with the monitor's refresh.
    #[serde(default)]
    pub vsync: bool,
    /// The most frames per second while editing; 0 is unlimited.
    #[serde(default = "default_max_fps")]
    pub max_fps: u32,
    /// Every sound, 0 to 1.
    #[serde(default = "default_volume")]
    pub master_volume: f32,
    /// SFX, Voice and UI sounds, 0 to 1.
    #[serde(default = "default_volume")]
    pub effects_volume: f32,
    /// Music and Ambient sounds (and looping sounds in Play), 0 to 1.
    #[serde(default = "default_volume")]
    pub music_volume: f32,
}

fn default_true() -> bool {
    true
}

fn default_render_quality() -> u8 {
    2
}

fn default_max_fps() -> u32 {
    120
}

fn default_volume() -> f32 {
    1.0
}

fn default_usage_telemetry_enabled() -> bool {
    true
}

fn default_bliss_node_mode() -> String {
    // Light is the zero-setup default; Full is always an explicit opt-in.
    "Light".to_string()
}

fn default_bliss_enabled() -> bool {
    true
}

fn default_theme_modern() -> bool {
    false
}

fn default_active_mode() -> String {
    "engineering".to_string()
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
            collisions_enabled: false,
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
            recent_spaces: Vec::new(),
            recent_inserts: Vec::new(),

            theme_modern: false,
            active_theme_id: Some("classic".to_string()),
            active_mode_id: "engineering".to_string(),
            active_submode_id: String::new(),
            layout_preset_by_mode: std::collections::HashMap::new(),
            usage_telemetry_enabled: default_usage_telemetry_enabled(),
            telemetry_notice_shown: false,
            bliss_node_mode: default_bliss_node_mode(),
            bliss_enabled: default_bliss_enabled(),
            ribbon_collapsed: false,
            render_quality: default_render_quality(),
            shadows_enabled: true,
            anti_aliasing: true,
            vsync: false,
            max_fps: default_max_fps(),
            master_volume: default_volume(),
            effects_volume: default_volume(),
            music_volume: default_volume(),
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
            .add_systems(Update, auto_save_settings)
            .add_systems(Update, auto_save_scene_system)
            .add_systems(Update, track_recent_spaces)
            .add_plugins(crate::preferences::PreferencesPlugin);
    }
}

/// How many Spaces File > Recent lists.
const RECENT_SPACES_CAP: usize = 8;

impl EditorSettings {
    /// Move `path` to the front of the recent list, dropping duplicates and
    /// anything past the cap.
    pub fn remember_space(&mut self, path: &std::path::Path) {
        let key = path.to_string_lossy().to_string();
        self.recent_spaces.retain(|p| p != &key);
        self.recent_spaces.insert(0, key);
        self.recent_spaces.truncate(RECENT_SPACES_CAP);
    }
}

/// Keep File > Recent current: every Space the editor opens goes to the
/// front of the list, and the settings file persists it.
fn track_recent_spaces(
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut settings: ResMut<EditorSettings>,
) {
    let Some(root) = space_root else { return };
    if !root.is_changed() {
        return;
    }
    let path = root.0.clone();
    if !path.is_dir() {
        return;
    }
    if settings.recent_spaces.first().map(|p| std::path::Path::new(p) == path).unwrap_or(false) {
        return;
    }
    settings.remember_space(&path);
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
/// Some entity edits reach disk as they happen (the move and scale tools, the
/// property panel), and some only through `save_space`: in the default build
/// `write_instance_changes_system` is not registered, so a rotate-tool drag
/// or an undo lives in the ECS until a save. So when anything is unsaved the
/// autosave runs `save_space` first, which writes only the parts that differ
/// from their files, then the terrain when it changed (brush, road, Part to
/// Terrain and volume edits live in memory until `save_terrain_to_disk`),
/// then commits. Nothing changed makes autosave a no-op, which is the common
/// case while the user is just looking around.
///
/// The work runs as a queued command, which gets the whole `World` the
/// terrain save needs.
fn auto_save_scene_system(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<EditorSettings>,
    mut auto_save: ResMut<AutoSaveState>,
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

    auto_save.last_save = Some(std::time::Instant::now());
    commands.queue(move |world: &mut World| autosave_space(world, space_path, identity));
}

/// One autosave: write the terrain when it changed since its last save,
/// commit the Space to git off the main thread, and record the snapshot for
/// the title asterisk and the exit prompt.
fn autosave_space(world: &mut World, space_path: PathBuf, identity: Option<GitIdentity>) {
    // Read before the snapshot below resets it. Terrain undo and redo push
    // the saved sequence back (`mark_terrain_unsaved`), so they count too.
    let sequence = world.get_resource::<crate::undo::UndoStack>().map(|undo| undo.sequence());
    let unsaved = match (world.get_resource::<crate::ui::StudioState>(), sequence) {
        (Some(state), Some(sequence)) => state.has_unsaved_changes || state.saved_undo_sequence != sequence,
        _ => true,
    };
    // Never during Play: physics and scripts move parts there and terrain
    // tools edit the terrain, and Stop restores all of it, so nothing Play
    // changes is an edit to persist or commit. The autosave waits for Edit.
    let editing = world
        .get_resource::<State<crate::play_mode::PlayModeState>>()
        .map_or(true, |s| *s.get() == crate::play_mode::PlayModeState::Editing);
    if !editing {
        retry_autosave_soon(world);
        return;
    }
    // A revert waits for this Space to reopen (`checkpoint::request_restore`),
    // and anything saved now would land over the files it restores.
    if crate::space::checkpoint::restore_pending(&space_path) {
        retry_autosave_soon(world);
        return;
    }
    // Both saves run under the commit lock, so no commit's `git add` stages a
    // half-written save. While a commit is still running, this autosave tries
    // again in a few seconds rather than hold the frame.
    let commit_guard = match GIT_COMMIT_LOCK.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => {
            retry_autosave_soon(world);
            return;
        }
    };
    // Edits that live only in the ECS until a save (a rotate, an undo).
    if unsaved {
        crate::space::space_ops::save_space(world);
    }
    // Only terrain edits need the whole-terrain rewrite.
    let terrain_unsaved = crate::ui::file_event_handler::terrain_changed_since_save(world, unsaved)
        && crate::ui::file_event_handler::save_terrain_to_disk(world);
    drop(commit_guard);

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
                crate::notifications::notify_from_background(
                    crate::notifications::NotificationLevel::Info,
                    "Auto-saved (git)".to_string(),
                );
            }
            Ok(GitAutosave::NoChanges) => {
                // Quiet no-op — nothing changed since the last autosave.
            }
            Err(e) => {
                warn!("git autosave failed at {:?}: {}", space_path, e);
                // Surface the failure — silent autosave failure is silent
                // data-loss risk. (The toast used to fire optimistically on
                // the main thread BEFORE the commit ran, so the user was told
                // "Auto-saved" even when git failed.)
                crate::notifications::notify_from_background(
                    crate::notifications::NotificationLevel::Error,
                    format!("Autosave git commit FAILED: {e}. Your on-disk files are current, but no recovery snapshot was made."),
                );
            }
        }
    });

    // The title asterisk and the exit prompt count edits since the last
    // snapshot; an autosave is one. Terrain edits that did not reach storage
    // keep the Space unsaved, the saved sequence left one behind as a
    // terrain undo leaves it, since the UI sync derives the marker from it.
    if let Some(mut state) = world.get_resource_mut::<crate::ui::StudioState>() {
        let sequence = sequence.unwrap_or(0);
        state.saved_undo_sequence = if terrain_unsaved { sequence.wrapping_sub(1) } else { sequence };
        state.has_unsaved_changes = terrain_unsaved;
        state.snapshot_status = format!("Autosaved {}", chrono::Local::now().format("%H:%M"));
    }
}

/// Try an autosave that could not run now again in a few seconds.
fn retry_autosave_soon(world: &mut World) {
    let interval = world
        .get_resource::<EditorSettings>()
        .map(|s| s.auto_save_interval)
        .unwrap_or(0.0);
    if let Some(mut auto_save) = world.get_resource_mut::<AutoSaveState>() {
        auto_save.timer = (interval - 5.0).max(0.0);
    }
}

/// One git commit of a Space at a time in this process
/// (`git_autosave_commit`), and no autosave writing files while a commit
/// stages them (`autosave_space`).
pub(crate) static GIT_COMMIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) enum GitAutosave {
    Committed(String),
    NoChanges,
}

/// Whether [`git_commit_locked`] records a commit when nothing changed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmptyCommit {
    /// Report [`GitAutosave::NoChanges`] and commit nothing.
    Skip,
    /// Commit anyway, so the message (a snapshot trailer) is recorded.
    Allow,
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
    // One commit at a time in this process. The periodic autosave and a
    // manual save each run this on their own background thread, and git
    // guards its index with `index.lock`: a second concurrent `git add`
    // fails outright instead of waiting, so a save that lands while the
    // autosave runs lost its commit and reported "git snapshot commit
    // failed". Held across add, commit and rev-parse, so the sha returned
    // is this call's commit and not a neighbour's. A second engine process
    // on the same Space is not covered; git's own lock still refuses it.
    let _commit_guard = GIT_COMMIT_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    git_commit_locked(space_path, message, identity, EmptyCommit::Skip)
}

/// [`git_autosave_commit`] for a caller that already holds
/// [`GIT_COMMIT_LOCK`], so it can write the Space and commit it with no
/// other commit in between. Called without the lock, a concurrent commit's
/// `git add` can fail on git's `index.lock`.
pub(crate) fn git_commit_locked(
    space_path: &std::path::Path,
    message: &str,
    identity: Option<&GitIdentity>,
    empty: EmptyCommit,
) -> Result<GitAutosave, String> {
    use std::process::Command;

    // Wrapper so the one-off commands all share the same cwd + error
    // shape. Output is captured so nothing leaks to the engine stdout.
    //
    // CREATE_NO_WINDOW (Windows): the autosave runs on a background
    // thread every ~5 minutes; without this flag every `git` spawn flashes
    // a console window over the engine. Mirrors `lsp_launcher.rs`. Routed
    // through this single closure so all 8 git calls (init/config/rm/add/
    // diff/commit/rev-parse) inherit it.
    let run = |args: &[&str]| -> Result<std::process::Output, String> {
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(space_path);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd.output()
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

    // Keep the binary Fjall DB, the `.eustress` sidecar/trash, and
    // recovery backups OUT of the autosave repo. Hashing the 32 MB
    // journals every interval was expensive, caused "unstable object
    // source data" errors (git reading journals mid-write), and churned
    // `.git/` (storming the file watcher → the ~5s editor stutter).
    // Raw-committing a live LSM also risks capturing a torn state.
    //
    // COVERAGE LIMIT — the autosave versions the on-disk TOML hierarchy,
    // which is NOT a complete record of the Space. `write_instance_definition`
    // (space/instance_loader.rs) skips the disk write whenever
    // `active_db::put_instance` succeeds, and that succeeds for LEAF
    // instances that are neither file-natured nor mesh-backed and have no
    // children — those persist only as a `<path>#bin` key in the DB's
    // `tree` partition. Parents, mesh instances and file-natured classes
    // still round-trip through disk TOML and ARE captured here.
    //
    // So an autosave commit restores the Space's structure but reverts
    // binary-collapsed leaf entities to their seed state. Closing that gap
    // needs a versioned export of the DB (`eustress_worlddb::bake` is the
    // intended vehicle) rather than committing the LSM directly.
    ensure_autosave_gitignore(space_path);
    // Drop any DB files an earlier build already committed so this commit
    // removes them from the index (no-op once gone; --ignore-unmatch
    // stays quiet when nothing matched).
    let _ = run(&["rm", "-r", "--cached", "--ignore-unmatch", "--quiet", "world.fjalldb"]);
    let _ = run(&["rm", "-r", "--cached", "--ignore-unmatch", "--quiet", ".eustress/host"]);
    // Each file stays on disk and leaves the index once; that one time is
    // logged (git names what it removed).
    for &runtime in RUNTIME_STATE {
        if let Ok(out) = run(&["rm", "--cached", "--ignore-unmatch", runtime]) {
            if out.status.success() && !out.stdout.is_empty() {
                info!(
                    "Stopped tracking {runtime} in {}: it is this machine's session state, not the Space's",
                    space_path.display()
                );
            }
        }
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
    if diff.status.success() && empty == EmptyCommit::Skip {
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
    if empty == EmptyCommit::Allow {
        commit_args.push("--allow-empty".into());
    }
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

/// Files in a Space that are this machine's state, not the Space's: what
/// the engine rewrites as it runs, and its caches of server state. Kept out
/// of git, so opening a Space changes no tracked file and snapshots do not
/// churn, and a revert never writes an old value back. Exact paths.
///
/// `last_reconcile`: when the database last matched the files; the next
/// open re-reads every file changed since. `lsp.port`: the port this
/// session's language server listens on. `output.log`: the Output panel,
/// saved into its Space on every switch and exit. `.last_name`: the folder
/// name the Space last opened under, so a rename is noticed.
/// `review.toml`: the Gallery's review of the Space's listing, as last
/// fetched from api.eustress.dev, and the temporary file it is written
/// through.
pub(crate) const RUNTIME_STATE: &[&str] = &[
    ".eustress/last_reconcile",
    ".eustress/lsp.port",
    ".eustress/output.log",
    ".eustress/.last_name",
    ".eustress/review.toml",
    ".eustress/review.toml.tmp",
];

/// Ensure the Space's autosave `.gitignore` excludes the binary Fjall DB,
/// the `.eustress` sidecar/trash, and recovery `.bak-*` backups so
/// `git add -A` never hashes them. Idempotent — writes only when an entry
/// is missing or the stale header is present, so it doesn't itself churn
/// the file watcher.
///
/// `world.fjalldb/` is excluded for cost and correctness (see
/// `git_autosave_commit`), NOT because it is derived — it holds the only
/// copy of every binary-collapsed leaf instance. The emitted header says so,
/// because a reader who assumes the DB is reproducible from the TOML will
/// delete it. Spaces written before that was understood carry a header
/// asserting the opposite, so it is replaced in place on next open.
fn ensure_autosave_gitignore(space_path: &std::path::Path) {
    /// Header emitted above the ignore entries.
    const HEADER: &str =
        "# Eustress autosave: excluded from git, but NOT derived.\n\
         # world.fjalldb/ is the authoritative store — it holds edits that\n\
         # exist nowhere else on disk. It is ignored because committing a\n\
         # live LSM every autosave interval is slow and can capture a torn\n\
         # state, not because it can be regenerated. Back it up; do not\n\
         # delete it, and do not treat a git checkout as a complete Space.\n";
    /// The header earlier builds wrote. Claims the DB is derived, which is
    /// the opposite of true and invites deleting it.
    const STALE_HEADER: &str =
        "# Eustress autosave: derived/binary + sidecar, not versioned";

    let path = space_path.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    // Replace the stale header in place, preserving every other line so a
    // user's own ignore rules survive.
    let had_stale = existing.lines().any(|l| l.trim() == STALE_HEADER);
    let mut content: String = if had_stale {
        let kept: Vec<&str> = existing
            .lines()
            .filter(|l| l.trim() != STALE_HEADER)
            .collect();
        let mut s = HEADER.to_string();
        s.push_str(&kept.join("\n"));
        if !s.ends_with('\n') {
            s.push('\n');
        }
        s
    } else {
        existing
    };

    // `.eustress/host/`: a hosting export an earlier build wrote inside the
    // Space (now written to the workspace's own cache).
    // `.eustress/hosts/`: a running host's join link, written again every
    // 10 s, when the workspace folder is itself inside a Space's repository.
    // `.eustress/snapshots/`: database checkpoints paired with snapshot
    // commits. They are stored outside git on purpose, and an unignored one
    // would be staged by the very commit it belongs to.
    // `world.fjalldb.*/`: the database copies a restore makes beside the live
    // one (`world.fjalldb.restoring/`, `world.fjalldb.pre-restore-<id>/`).
    // `world.fjalldb/` matches only its exact name, so without this entry a
    // pre-restore copy would be committed by the next autosave.
    // `RUNTIME_STATE`: the engine's per-machine files (see there).
    let needed = [
        "world.fjalldb/",
        "world.fjalldb.*/",
        ".eustress/trash/",
        ".eustress/host/",
        ".eustress/hosts/",
        ".eustress/snapshots/",
        "*.bak-*",
    ];
    let mut additions = String::new();
    for entry in needed.into_iter().chain(RUNTIME_STATE.iter().copied()) {
        if !content.lines().any(|l| l.trim() == entry) {
            additions.push_str(entry);
            additions.push('\n');
        }
    }

    if additions.is_empty() {
        // Nothing to add. Still persist if we rewrote the stale header.
        if had_stale {
            let _ = std::fs::write(&path, content);
        }
        return;
    }

    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    if !had_stale {
        content.push_str(HEADER);
    }
    content.push_str(&additions);
    let _ = std::fs::write(&path, content);
}

