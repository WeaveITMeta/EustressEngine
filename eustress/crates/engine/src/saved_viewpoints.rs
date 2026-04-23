//! # Saved Viewpoints (Phase 2)
//!
//! Named camera-pose snapshots persisted per-universe. Save the
//! current camera transform under a name; recall jumps the primary
//! camera back (optionally animated). Stored at
//! `.eustress/viewpoints.toml` so the set is git-diffable and
//! portable across collaborators.
//!
//! ## Events
//!
//! - `SaveViewpointEvent { name }` — snapshot current camera pose.
//! - `LoadViewpointEvent { name, animate }` — restore pose. Animate
//!   tweens over 250ms; false snaps instantly.
//! - `DeleteViewpointEvent { name }` — remove from disk.
//!
//! Numpad bindings for viewpoint slots live in `keybindings.rs` as a
//! follow-up — for now the events are fired via MCP or a UI picker.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// On-disk schema
// ============================================================================

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ViewpointsFile {
    #[serde(default, rename = "viewpoint")]
    pub viewpoints: Vec<Viewpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewpoint {
    pub name: String,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    /// Optional focal target (for orbit cameras); if present, the
    /// Load handler sets the orbit center too.
    #[serde(default)]
    pub focal_target: Option<[f32; 3]>,
    pub created: String,
}

fn file_path(space_root: &std::path::Path) -> std::path::PathBuf {
    space_root.join(".eustress").join("viewpoints.toml")
}

fn load_file(space_root: &std::path::Path) -> ViewpointsFile {
    match std::fs::read_to_string(file_path(space_root)) {
        Ok(s) => toml::from_str(&s).unwrap_or_default(),
        Err(_) => ViewpointsFile::default(),
    }
}

fn save_file(space_root: &std::path::Path, file: &ViewpointsFile) -> std::io::Result<()> {
    let path = file_path(space_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let s = toml::to_string_pretty(file).map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::write(path, s)
}

// ============================================================================
// Events
// ============================================================================

#[derive(Event, Message, Debug, Clone)]
pub struct SaveViewpointEvent { pub name: String }

#[derive(Event, Message, Debug, Clone)]
pub struct LoadViewpointEvent { pub name: String, pub animate: bool }

#[derive(Event, Message, Debug, Clone)]
pub struct DeleteViewpointEvent { pub name: String }

/// In-flight camera tween — active while the primary camera animates
/// between poses. Cleared on arrival. One tween at a time.
#[derive(Resource, Default, Clone)]
pub struct ViewpointTween {
    pub target_pos: Vec3,
    pub target_rot: Quat,
    pub start_pos: Vec3,
    pub start_rot: Quat,
    pub elapsed: f32,
    pub duration: f32,
    pub active: bool,
}

// ============================================================================
// Plugin
// ============================================================================

pub struct SavedViewpointsPlugin;

impl Plugin for SavedViewpointsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewpointTween>()
            .add_message::<SaveViewpointEvent>()
            .add_message::<LoadViewpointEvent>()
            .add_message::<DeleteViewpointEvent>()
            .add_systems(Update, (
                handle_save_viewpoint,
                handle_load_viewpoint,
                handle_delete_viewpoint,
                drive_viewpoint_tween,
            ));
    }
}

// ============================================================================
// Handlers
// ============================================================================

fn handle_save_viewpoint(
    mut events: MessageReader<SaveViewpointEvent>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    cameras: Query<(&Camera, &Transform)>,
) {
    let Some(space) = space_root else { return };
    for event in events.read() {
        let Some((_, t)) = cameras.iter().find(|(c, _)| c.order == 0) else { continue };

        let mut file = load_file(&space.0);
        file.viewpoints.retain(|v| v.name != event.name);
        file.viewpoints.push(Viewpoint {
            name: event.name.clone(),
            position: t.translation.to_array(),
            rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
            focal_target: None,
            created: chrono::Utc::now().to_rfc3339(),
        });
        match save_file(&space.0, &file) {
            Ok(_) => info!("📷 Saved viewpoint '{}'", event.name),
            Err(e) => warn!("📷 Save viewpoint '{}' failed: {}", event.name, e),
        }
    }
}

fn handle_load_viewpoint(
    mut events: MessageReader<LoadViewpointEvent>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut cameras: Query<(&Camera, &mut Transform)>,
    mut tween: ResMut<ViewpointTween>,
) {
    let Some(space) = space_root else { return };
    for event in events.read() {
        let file = load_file(&space.0);
        let Some(vp) = file.viewpoints.iter().find(|v| v.name == event.name) else {
            warn!("📷 Load viewpoint '{}': not found", event.name);
            continue;
        };
        let target_pos = Vec3::from(vp.position);
        let target_rot = Quat::from_array(vp.rotation);

        let Some((_, mut t)) = cameras.iter_mut().find(|(c, _)| c.order == 0) else { continue };
        if !event.animate {
            t.translation = target_pos;
            t.rotation    = target_rot;
            tween.active = false;
            info!("📷 Loaded viewpoint '{}' (snap)", event.name);
        } else {
            tween.start_pos = t.translation;
            tween.start_rot = t.rotation;
            tween.target_pos = target_pos;
            tween.target_rot = target_rot;
            tween.elapsed = 0.0;
            tween.duration = 0.25; // 250ms
            tween.active = true;
            info!("📷 Tweening to viewpoint '{}' (250ms)", event.name);
        }
    }
}

fn handle_delete_viewpoint(
    mut events: MessageReader<DeleteViewpointEvent>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
    let Some(space) = space_root else { return };
    for event in events.read() {
        let mut file = load_file(&space.0);
        let before = file.viewpoints.len();
        file.viewpoints.retain(|v| v.name != event.name);
        if file.viewpoints.len() == before {
            warn!("📷 Delete viewpoint '{}': not found", event.name);
            continue;
        }
        match save_file(&space.0, &file) {
            Ok(_)  => info!("📷 Deleted viewpoint '{}'", event.name),
            Err(e) => warn!("📷 Delete viewpoint '{}' failed: {}", event.name, e),
        }
    }
}

fn drive_viewpoint_tween(
    time: Res<Time>,
    mut tween: ResMut<ViewpointTween>,
    mut cameras: Query<(&Camera, &mut Transform)>,
) {
    if !tween.active { return; }
    tween.elapsed += time.delta_secs();
    let t_norm = (tween.elapsed / tween.duration).clamp(0.0, 1.0);
    // Ease-out cubic — snappy arrival matches Blender/Maya feel.
    let eased = 1.0 - (1.0 - t_norm).powi(3);
    let Some((_, mut t)) = cameras.iter_mut().find(|(c, _)| c.order == 0) else { return };
    t.translation = tween.start_pos.lerp(tween.target_pos, eased);
    t.rotation    = tween.start_rot.slerp(tween.target_rot, eased);
    if t_norm >= 1.0 { tween.active = false; }
}

/// List all saved viewpoint names — used by pickers.
pub fn list_viewpoints(space_root: &std::path::Path) -> Vec<String> {
    load_file(space_root).viewpoints.into_iter().map(|v| v.name).collect()
}
