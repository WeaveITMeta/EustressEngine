//! # Saved Viewpoints
//!
//! Named editor views, persisted per Space at `.eustress/viewpoints.toml`
//! so the set is git-diffable and travels with the Space. A viewpoint is a
//! whole `camera_controller::ViewPose`: the orbit pivot, distance and angles
//! plus the Perspective (2D or 3D, the projection, the working plane), so a
//! view saved in 2D or orthographic comes back that way.
//!
//! Loading drives the camera controller, never the camera's `Transform`.
//! `EustressCamera` rebuilds the Transform from its own orbit state every
//! frame, so a Transform written from outside was gone by the next frame:
//! that is why loading a viewpoint used to do nothing at all.
//!
//! ## Ways in
//!
//! - View menu: "Save Viewpoint", then every saved viewpoint by name. Both
//!   travel as `set-view-mode` strings (see [`route_ui_command`]).
//! - `Action::SaveViewpoint` / `Action::NextViewpoint`: no default chord,
//!   rebindable in Keyboard Shortcuts, and reachable over the bridge's
//!   `action.invoke`.
//! - Messages: `SaveViewpointEvent { name }` (an empty name picks the next
//!   free "Viewpoint N"), `LoadViewpointEvent { name, animate }`,
//!   `DeleteViewpointEvent { name }`.
//!
//! ## File format
//!
//! Every field added after the first version is optional, so older files
//! still load: a viewpoint with only `position` and `rotation` comes back as
//! a 3D perspective view with its pivot the current orbit distance ahead.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::camera_controller::{
    CameraView, EustressCamera, ProjectionMode, ViewDimension, ViewPose,
};

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
    /// Camera position and rotation at rest. Kept for readers of the first
    /// format; loading prefers the orbit fields below when present.
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    /// The orbit pivot: the point the view is centred on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focal_target: Option<[f32; 3]>,
    /// Camera to pivot, in metres. Also the orthographic zoom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f32>,
    /// Orbit angles in radians; positive pitch looks down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yaw: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f32>,
    /// "3D" or "2D".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// "Perspective" or "Orthographic".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<String>,
    /// The axis view on screen ("Top", "Front", ...) or "Custom".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// The 2D working plane, as its view ("Front" is the XY plane).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<String>,
    pub created: String,
}

impl Viewpoint {
    fn capture(name: String, pose: ViewPose) -> Self {
        let rest = pose.camera_transform();
        Viewpoint {
            name,
            position: rest.translation.to_array(),
            rotation: rest.rotation.to_array(),
            focal_target: Some(pose.pivot.to_array()),
            distance: Some(pose.distance),
            yaw: Some(pose.yaw),
            pitch: Some(pose.pitch),
            mode: Some(pose.dimension.label().to_string()),
            projection: Some(pose.projection.label().to_string()),
            view: Some(pose.view.name().to_string()),
            plane: Some(pose.plane.name().to_string()),
            created: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// The pose this viewpoint describes. `fallback_distance` places the
    /// pivot for a viewpoint written before pivots were recorded.
    fn pose(&self, fallback_distance: f32) -> Option<ViewPose> {
        let position = Vec3::from(self.position);
        let rotation = Quat::from_array(self.rotation);
        if !position.is_finite() || !rotation.is_finite() || rotation.length_squared() < 1e-6 {
            return None;
        }
        let rest = Transform::from_translation(position).with_rotation(rotation.normalize());

        let mut pose = match (self.focal_target.map(Vec3::from), self.distance, self.yaw, self.pitch) {
            (Some(pivot), Some(distance), Some(yaw), Some(pitch)) => ViewPose {
                pivot,
                distance,
                yaw,
                pitch,
                ..ViewPose::from_camera_transform(&rest, distance)
            },
            // A pivot without angles: face the saved way, orbiting the pivot.
            (Some(pivot), ..) => ViewPose {
                pivot,
                ..ViewPose::from_camera_transform(&rest, position.distance(pivot).max(0.01))
            },
            _ => ViewPose::from_camera_transform(&rest, fallback_distance.max(0.01)),
        };

        if let Some(mode) = self.mode.as_deref() {
            pose.dimension = if mode.eq_ignore_ascii_case("2d") {
                ViewDimension::TwoD
            } else {
                ViewDimension::ThreeD
            };
        }
        if let Some(projection) = self.projection.as_deref() {
            pose.projection = if projection.eq_ignore_ascii_case("orthographic") {
                ProjectionMode::Orthographic
            } else {
                ProjectionMode::Perspective
            };
        }
        if let Some(view) = self.view.as_deref().and_then(CameraView::from_name) {
            pose.view = view;
        }
        if let Some(plane) = self
            .plane
            .as_deref()
            .and_then(CameraView::from_name)
            .filter(|v| *v != CameraView::Custom)
        {
            pose.plane = plane;
        }
        Some(pose)
    }
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

/// "Viewpoint N" with N one past the highest already used, so names never
/// repeat even after a delete.
fn next_free_name(file: &ViewpointsFile) -> String {
    let highest = file
        .viewpoints
        .iter()
        .filter_map(|v| v.name.strip_prefix("Viewpoint ")?.trim().parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("Viewpoint {}", highest + 1)
}

// ============================================================================
// Events
// ============================================================================

/// Snapshot the current view. An empty `name` picks the next "Viewpoint N";
/// an existing name is overwritten.
#[derive(Event, Message, Debug, Clone)]
pub struct SaveViewpointEvent { pub name: String }

/// Go to a saved view: eased when `animate`, else at once.
#[derive(Event, Message, Debug, Clone)]
pub struct LoadViewpointEvent { pub name: String, pub animate: bool }

#[derive(Event, Message, Debug, Clone)]
pub struct DeleteViewpointEvent { pub name: String }

/// The open Space's saved viewpoint names, in file order: the View menu's
/// list, and the ring `NextViewpoint` steps through. Re-read when the Space
/// changes and after every save or delete.
#[derive(Resource, Default, Debug, Clone)]
pub struct ViewpointNames {
    pub names: Vec<String>,
    /// Bumped on every change, so the UI sync can skip steady frames.
    pub revision: u64,
    space: Option<std::path::PathBuf>,
    last_loaded: Option<String>,
}

impl ViewpointNames {
    fn set(&mut self, file: &ViewpointsFile) {
        let names: Vec<String> = file.viewpoints.iter().map(|v| v.name.clone()).collect();
        if names != self.names {
            self.names = names;
            self.revision += 1;
        }
    }

    /// The viewpoint after the one last loaded, wrapping; the first if none.
    fn next(&self) -> Option<String> {
        if self.names.is_empty() {
            return None;
        }
        let after = self
            .last_loaded
            .as_ref()
            .and_then(|last| self.names.iter().position(|n| n == last))
            .map(|i| (i + 1) % self.names.len())
            .unwrap_or(0);
        Some(self.names[after].clone())
    }
}

/// Requests raised where no `MessageWriter` fits (the Slint action drain).
enum ViewpointRequest {
    Save,
    Load(String),
}

static VIEWPOINT_INBOX: std::sync::Mutex<Vec<ViewpointRequest>> = std::sync::Mutex::new(Vec::new());

/// Take a `set-view-mode` string that names a viewpoint: `viewpoint-save`, or
/// `viewpoint:<name>` to go to one. Returns false for anything else, which
/// the caller then treats as a camera `ViewCommand`.
pub fn route_ui_command(command: &str) -> bool {
    let request = if command == "viewpoint-save" {
        ViewpointRequest::Save
    } else if let Some(name) = command.strip_prefix("viewpoint:") {
        ViewpointRequest::Load(name.to_string())
    } else {
        return false;
    };
    if let Ok(mut inbox) = VIEWPOINT_INBOX.lock() {
        inbox.push(request);
    }
    true
}

// ============================================================================
// Plugin
// ============================================================================

pub struct SavedViewpointsPlugin;

impl Plugin for SavedViewpointsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewpointNames>()
            .add_message::<SaveViewpointEvent>()
            .add_message::<LoadViewpointEvent>()
            .add_message::<DeleteViewpointEvent>()
            // Read for the viewpoint actions. Registered by the UI plugin
            // too; here as well so the reader is valid without it.
            .add_message::<crate::ui::MenuActionEvent>()
            .add_systems(Update, (
                refresh_viewpoint_names,
                route_viewpoint_requests,
                handle_save_viewpoint,
                handle_load_viewpoint,
                handle_delete_viewpoint,
            ).chain());
    }
}

// ============================================================================
// Handlers
// ============================================================================

fn refresh_viewpoint_names(
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut names: ResMut<ViewpointNames>,
) {
    let Some(space) = space_root else { return };
    if names.space.as_ref() == Some(&space.0) {
        return;
    }
    names.space = Some(space.0.clone());
    names.last_loaded = None;
    let file = load_file(&space.0);
    names.set(&file);
}

/// Viewpoint actions and View-menu requests into viewpoint messages.
fn route_viewpoint_requests(
    mut menu_events: MessageReader<crate::ui::MenuActionEvent>,
    names: Res<ViewpointNames>,
    mut save: MessageWriter<SaveViewpointEvent>,
    mut load: MessageWriter<LoadViewpointEvent>,
) {
    use crate::keybindings::Action;
    for event in menu_events.read() {
        match event.action {
            Action::SaveViewpoint => {
                save.write(SaveViewpointEvent { name: String::new() });
            }
            Action::NextViewpoint => match names.next() {
                Some(name) => {
                    load.write(LoadViewpointEvent { name, animate: true });
                }
                None => info!("📷 No saved viewpoints in this Space yet (View ▸ Save Viewpoint)"),
            },
            _ => {}
        }
    }

    let pending: Vec<ViewpointRequest> = VIEWPOINT_INBOX
        .lock()
        .map(|mut inbox| inbox.drain(..).collect())
        .unwrap_or_default();
    for request in pending {
        match request {
            ViewpointRequest::Save => {
                save.write(SaveViewpointEvent { name: String::new() });
            }
            ViewpointRequest::Load(name) => {
                load.write(LoadViewpointEvent { name, animate: true });
            }
        }
    }
}

fn handle_save_viewpoint(
    mut events: MessageReader<SaveViewpointEvent>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    cameras: Query<&EustressCamera, With<Camera3d>>,
    mut names: ResMut<ViewpointNames>,
    play_state: Option<Res<State<crate::play_mode::PlayModeState>>>,
    mut commands: Commands,
) {
    let Some(space) = space_root else { return };
    let play_state = play_state.map_or(crate::play_mode::PlayModeState::Editing, |s| *s.get());
    for event in events.read() {
        // During Play a save waits for Stop: it writes the Space's
        // `.eustress/viewpoints.toml`. Refused where it is read, so the
        // request is consumed.
        if let Some(notice) = crate::keybindings::play_refusal(play_state, crate::keybindings::Action::SaveViewpoint) {
            commands.queue(move |world: &mut World| crate::keybindings::say_play_refusal(world, notice));
            continue;
        }
        let Some(cam) = cameras.iter().next() else {
            warn!("📷 Save viewpoint: no editor camera");
            continue;
        };
        let mut file = load_file(&space.0);
        let name = match event.name.trim() {
            "" => next_free_name(&file),
            given => given.to_string(),
        };
        let viewpoint = Viewpoint::capture(name.clone(), cam.pose());
        // Overwriting keeps the viewpoint's place in the list.
        match file.viewpoints.iter_mut().find(|v| v.name == name) {
            Some(existing) => *existing = viewpoint,
            None => file.viewpoints.push(viewpoint),
        }
        match save_file(&space.0, &file) {
            Ok(_) => info!("📷 Saved viewpoint '{}'", name),
            Err(e) => warn!("📷 Save viewpoint '{}' failed: {}", name, e),
        }
        names.set(&file);
        names.last_loaded = Some(name);
    }
}

/// The viewpoint a load request names. The View menu shows each name through
/// `display_text::shown_line` and sends the shown text back, so a name that is
/// long or spans lines (a hand-edited or shared `viewpoints.toml`) arrives in
/// its bounded form: the exact name wins, else the one that shows as the
/// request.
fn viewpoint_index(file: &ViewpointsFile, requested: &str) -> Option<usize> {
    file.viewpoints
        .iter()
        .position(|v| v.name == requested)
        .or_else(|| {
            file.viewpoints
                .iter()
                .position(|v| crate::ui::display_text::bound_line(&v.name) == requested)
        })
}

fn handle_load_viewpoint(
    mut events: MessageReader<LoadViewpointEvent>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut cameras: Query<(&mut EustressCamera, &Camera), With<Camera3d>>,
    mut names: ResMut<ViewpointNames>,
) {
    let Some(space) = space_root else { return };
    for event in events.read() {
        let file = load_file(&space.0);
        let Some(viewpoint) = viewpoint_index(&file, &event.name).map(|i| &file.viewpoints[i]) else {
            warn!("📷 Load viewpoint '{}': not found", event.name);
            continue;
        };
        let Ok((mut cam, camera)) = cameras.single_mut() else { continue };
        // During Play with a character nobody is looking through this camera.
        if !camera.is_active {
            continue;
        }
        let Some(pose) = viewpoint.pose(cam.distance) else {
            warn!("📷 Load viewpoint '{}': its pose is not finite", event.name);
            continue;
        };
        cam.go_to(pose, event.animate);
        names.last_loaded = Some(viewpoint.name.clone());
        info!("📷 Viewpoint '{}'{}", event.name, if event.animate { "" } else { " (snap)" });
    }
}

fn handle_delete_viewpoint(
    mut events: MessageReader<DeleteViewpointEvent>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut names: ResMut<ViewpointNames>,
    play_state: Option<Res<State<crate::play_mode::PlayModeState>>>,
    mut commands: Commands,
) {
    let Some(space) = space_root else { return };
    let play_state = play_state.map_or(crate::play_mode::PlayModeState::Editing, |s| *s.get());
    for event in events.read() {
        // During Play a delete waits for Stop, as a save does.
        if let Some(notice) = crate::keybindings::play_refusal_to(play_state, "delete a viewpoint") {
            commands.queue(move |world: &mut World| crate::keybindings::say_play_refusal(world, notice));
            continue;
        }
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
        names.set(&file);
    }
}

/// List all saved viewpoint names — used by pickers.
pub fn list_viewpoints(space_root: &std::path::Path) -> Vec<String> {
    load_file(space_root).viewpoints.into_iter().map(|v| v.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pose() -> ViewPose {
        ViewPose {
            pivot: Vec3::new(4.0, 2.0, -6.0),
            distance: 18.0,
            yaw: 0.6,
            pitch: 0.35,
            dimension: ViewDimension::ThreeD,
            projection: ProjectionMode::Orthographic,
            view: CameraView::Custom,
            plane: CameraView::Front,
        }
    }

    #[test]
    fn a_saved_viewpoint_round_trips_through_the_file_format() {
        let pose = sample_pose();
        let file = ViewpointsFile { viewpoints: vec![Viewpoint::capture("A".into(), pose)] };
        let text = toml::to_string_pretty(&file).unwrap();
        let back: ViewpointsFile = toml::from_str(&text).unwrap();
        let restored = back.viewpoints[0].pose(99.0).unwrap();
        assert_eq!(restored.pivot, pose.pivot);
        assert_eq!(restored.distance, pose.distance);
        assert_eq!((restored.yaw, restored.pitch), (pose.yaw, pose.pitch));
        assert_eq!(restored.projection, ProjectionMode::Orthographic);
        assert_eq!(restored.dimension, ViewDimension::ThreeD);
    }

    #[test]
    fn a_first_format_viewpoint_still_loads_with_its_pivot_ahead() {
        // What the first version wrote: position and rotation only.
        let text = r#"
            [[viewpoint]]
            name = "Old"
            position = [0.0, 10.0, 20.0]
            rotation = [0.0, 0.0, 0.0, 1.0]
            created = "2026-01-01T00:00:00Z"
        "#;
        let file: ViewpointsFile = toml::from_str(text).unwrap();
        let pose = file.viewpoints[0].pose(20.0).unwrap();
        // Identity rotation looks down -Z, so the pivot sits 20 m that way.
        assert!((pose.pivot - Vec3::new(0.0, 10.0, 0.0)).length() < 1e-4);
        assert_eq!(pose.distance, 20.0);
        assert_eq!(pose.dimension, ViewDimension::ThreeD);
        // And the camera it describes is where the file said.
        let rest = pose.camera_transform();
        assert!((rest.translation - Vec3::new(0.0, 10.0, 20.0)).length() < 1e-3);
    }

    #[test]
    fn a_2d_viewpoint_keeps_its_plane() {
        let mut pose = sample_pose();
        pose.dimension = ViewDimension::TwoD;
        pose.plane = CameraView::Top;
        let restored = Viewpoint::capture("Plan".into(), pose).pose(1.0).unwrap();
        assert_eq!(restored.dimension, ViewDimension::TwoD);
        assert_eq!(restored.plane, CameraView::Top);
    }

    #[test]
    fn new_names_never_reuse_a_number() {
        let mut file = ViewpointsFile::default();
        assert_eq!(next_free_name(&file), "Viewpoint 1");
        file.viewpoints.push(Viewpoint::capture("Viewpoint 3".into(), sample_pose()));
        file.viewpoints.push(Viewpoint::capture("Lobby".into(), sample_pose()));
        assert_eq!(next_free_name(&file), "Viewpoint 4");
    }

    #[test]
    fn a_menu_label_that_was_bounded_still_finds_its_viewpoint() {
        let long = format!("{}\nsecond line", "x".repeat(900));
        let mut file = ViewpointsFile::default();
        for name in ["Lobby", long.as_str()] {
            file.viewpoints.push(Viewpoint::capture(name.into(), sample_pose()));
        }
        // A plain name is found by itself.
        assert_eq!(viewpoint_index(&file, "Lobby"), Some(0));
        // The menu shows the long one bounded and sends that text back.
        let shown = crate::ui::display_text::bound_line(&long);
        assert_ne!(shown, long);
        assert!(shown.chars().count() < 300 && !shown.contains('\n'));
        assert_eq!(viewpoint_index(&file, &shown), Some(1));
        // Exact still wins, and a stranger finds nothing.
        assert_eq!(viewpoint_index(&file, &long), Some(1));
        assert_eq!(viewpoint_index(&file, "Nowhere"), None);
    }

    #[test]
    fn next_viewpoint_wraps_around() {
        let mut names = ViewpointNames::default();
        names.names = vec!["A".into(), "B".into()];
        assert_eq!(names.next().as_deref(), Some("A"));
        names.last_loaded = Some("A".into());
        assert_eq!(names.next().as_deref(), Some("B"));
        names.last_loaded = Some("B".into());
        assert_eq!(names.next().as_deref(), Some("A"));
    }
}

/// During Play the viewpoints file is not written: a save or a delete is
/// consumed and refused with a notice. Loading is left alone.
#[cfg(test)]
mod play_refusal_tests {
    use super::*;
    use crate::play_mode::PlayModeState;
    use bevy::ecs::system::RunSystemOnce;

    /// A world in `state` over a fresh Space folder whose viewpoints file
    /// holds "A".
    fn world_in(state: PlayModeState, tag: &str) -> (World, std::path::PathBuf) {
        let space = std::env::temp_dir().join(format!("eustress_viewpoints_play_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&space);
        let pose = ViewPose {
            pivot: Vec3::new(4.0, 2.0, -6.0),
            distance: 18.0,
            yaw: 0.6,
            pitch: 0.35,
            dimension: ViewDimension::ThreeD,
            projection: ProjectionMode::Orthographic,
            view: CameraView::Custom,
            plane: CameraView::Front,
        };
        save_file(
            &space,
            &ViewpointsFile {
                viewpoints: vec![Viewpoint::capture("A".into(), pose)],
            },
        )
        .unwrap();
        let mut world = World::new();
        world.insert_resource(State::new(state));
        world.insert_resource(crate::space::SpaceRoot(space.clone()));
        // `new()`, not the derived default, which keeps no notice.
        world.insert_resource(crate::notifications::NotificationManager::new());
        world.init_resource::<crate::ui::OutputConsole>();
        world.init_resource::<ViewpointNames>();
        world.init_resource::<Messages<SaveViewpointEvent>>();
        world.init_resource::<Messages<DeleteViewpointEvent>>();
        // The editor camera a save reads its pose from, so a save that got
        // past its refusal would write.
        world.spawn((EustressCamera::default(), Camera3d::default()));
        (world, space)
    }

    /// The toasts, each of which is also a line in the Output panel.
    fn notices(world: &World) -> Vec<String> {
        let toasts: Vec<String> = world.resource::<crate::notifications::NotificationManager>().notifications.iter().map(|n| n.message.clone()).collect();
        let output: Vec<String> = world.resource::<crate::ui::OutputConsole>().entries.iter().map(|e| e.message.clone()).collect();
        assert_eq!(output, toasts, "the Output panel and the toasts disagree");
        toasts
    }

    #[test]
    fn play_writes_no_viewpoint() {
        for (state, tag) in [(PlayModeState::Playing, "playing"), (PlayModeState::Paused, "paused")] {
            let (mut world, space) = world_in(state, tag);
            world.resource_mut::<Messages<SaveViewpointEvent>>().write(SaveViewpointEvent { name: "B".into() });
            world.resource_mut::<Messages<DeleteViewpointEvent>>().write(DeleteViewpointEvent { name: "A".into() });
            world.run_system_once(handle_save_viewpoint).expect("the system runs");
            world.run_system_once(handle_delete_viewpoint).expect("the system runs");
            assert_eq!(list_viewpoints(&space), ["A"], "{state:?}: the file changed");
            assert_eq!(notices(&world), ["Stop Play to use Save Viewpoint.", "Stop Play to delete a viewpoint."], "{state:?}");
            let _ = std::fs::remove_dir_all(&space);
        }
    }

    /// The save runs while editing, with the same camera, so the test above
    /// is not passing on a save that has nothing to write.
    #[test]
    fn editing_saves_a_viewpoint() {
        let (mut world, space) = world_in(PlayModeState::Editing, "editing_save");
        world.resource_mut::<Messages<SaveViewpointEvent>>().write(SaveViewpointEvent { name: "B".into() });
        world.run_system_once(handle_save_viewpoint).expect("the system runs");
        assert_eq!(list_viewpoints(&space), ["A", "B"]);
        assert!(notices(&world).is_empty(), "{:?}", notices(&world));
        let _ = std::fs::remove_dir_all(&space);
    }

    /// The delete runs while editing, so the test above is not passing on a
    /// handler that writes nothing anywhere.
    #[test]
    fn editing_deletes_a_viewpoint() {
        let (mut world, space) = world_in(PlayModeState::Editing, "editing");
        world.resource_mut::<Messages<DeleteViewpointEvent>>().write(DeleteViewpointEvent { name: "A".into() });
        world.run_system_once(handle_delete_viewpoint).expect("the system runs");
        assert!(list_viewpoints(&space).is_empty(), "{:?}", list_viewpoints(&space));
        assert!(notices(&world).is_empty(), "{:?}", notices(&world));
        let _ = std::fs::remove_dir_all(&space);
    }
}
