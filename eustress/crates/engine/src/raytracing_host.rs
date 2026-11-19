//! Studio's side of ray tracing: what `eustress_raytracing` asks of the app
//! that runs it (its "The host" docs), filled from Studio's own state.
//!
//! - [`HostFrame`] from the window focus and power mode, the editing frame
//!   cap, Play mode and the Space loader: the frame-time guard measures Edit
//!   mode only, focused, at full speed, once loading has settled.
//! - [`HostSpaceRoot`] follows [`SpaceRoot`], the key of the open Space's
//!   texture-limit record.
//! - Every [`StudioCamera`] is a [`HostCamera`], so the AI capture camera
//!   gets the main view's sky and its deferred prepass.
//! - A [`RaytracingNotice`] becomes a notification.
//!
//! Added by `main` right after `RaytracingPlugin`.

use bevy::prelude::*;
use eustress_raytracing::{HostCamera, HostFrame, HostSpaceRoot, NoticeLevel, RaytracingNotice};

use crate::default_scene::StudioCamera;
use crate::notifications::NotificationManager;
use crate::play_mode::PlayModeState;
use crate::space::SpaceRoot;
use crate::window_focus::{IdleSettings, PowerMode, WindowFocusState};

pub struct RaytracingHostPlugin;

impl Plugin for RaytracingHostPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<RaytracingNotice>()
            .insert_resource(HostFrame::default())
            .add_observer(mark_host_camera)
            // Before `Last`, where the guards read them.
            .add_systems(PostUpdate, (fill_host_frame, follow_space_root))
            .add_systems(Update, show_notices);
        // The launch Space is `SpaceRoot` before any plugin builds.
        let root = app.world().get_resource::<SpaceRoot>().map(|root| HostSpaceRoot(root.0.clone()));
        if let Some(root) = root {
            app.insert_resource(root);
        }
    }
}

fn mark_host_camera(added: On<Add, StudioCamera>, mut commands: Commands) {
    commands.entity(added.entity).try_insert(HostCamera);
}

/// Studio's frame as the frame-time guard reads it. No focus state counts as
/// focused and at full speed, no Play state as Edit mode, no idle settings as
/// uncapped.
pub fn host_frame(
    loading: bool,
    focus: Option<&WindowFocusState>,
    idle: Option<&IdleSettings>,
    play: Option<&PlayModeState>,
) -> HostFrame {
    HostFrame {
        loading,
        interactive: focus.is_none_or(|f| f.focused && !f.minimized && f.power_mode == PowerMode::Active),
        measurable: play.is_none_or(|state| *state == PlayModeState::Editing),
        cap_ms: idle.map_or(0.0, |i| i.active_update_ms as f32),
    }
}

fn fill_host_frame(
    focus: Option<Res<WindowFocusState>>,
    idle: Option<Res<IdleSettings>>,
    play: Option<Res<State<PlayModeState>>>,
    mut host: ResMut<HostFrame>,
) {
    let frame = host_frame(
        crate::space::file_loader::bulk_load_active(),
        focus.as_deref(),
        idle.as_deref(),
        play.as_deref().map(State::get),
    );
    host.set_if_neq(frame);
}

/// Changed only when the Space does, so the texture-record watch reads a
/// change as a new Space.
fn follow_space_root(space: Option<Res<SpaceRoot>>, host: Option<Res<HostSpaceRoot>>, mut commands: Commands) {
    let Some(space) = space else { return };
    if host.is_some_and(|host| host.0 == space.0) {
        return;
    }
    commands.insert_resource(HostSpaceRoot(space.0.clone()));
}

fn show_notices(mut notices: MessageReader<RaytracingNotice>, notifications: Option<ResMut<NotificationManager>>) {
    let Some(mut notifications) = notifications else {
        // The guards log what they did; without the notification list there
        // is nowhere else to show it.
        notices.clear();
        return;
    };
    for notice in notices.read() {
        match notice.level {
            NoticeLevel::Warning => notifications.warning(notice.text.clone()),
            NoticeLevel::Info => notifications.info(notice.text.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn studio_cameras_are_host_cameras_and_the_space_follows() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).insert_resource(SpaceRoot(PathBuf::from("C:/Spaces/A")));
        app.add_plugins(RaytracingHostPlugin);
        assert_eq!(app.world().get_resource::<HostSpaceRoot>(), Some(&HostSpaceRoot(PathBuf::from("C:/Spaces/A"))));
        let studio = app.world_mut().spawn(StudioCamera).id();
        let other = app.world_mut().spawn(Camera3d::default()).id();
        app.update();
        assert!(app.world().get::<HostCamera>(studio).is_some(), "a Studio camera is not a host camera");
        assert!(app.world().get::<HostCamera>(other).is_none(), "a camera Studio does not own became a host camera");
        app.insert_resource(SpaceRoot(PathBuf::from("C:/Spaces/B")));
        app.update();
        assert_eq!(app.world().get_resource::<HostSpaceRoot>(), Some(&HostSpaceRoot(PathBuf::from("C:/Spaces/B"))));
    }

    #[test]
    fn notices_reach_the_notification_list() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).insert_resource(NotificationManager::new());
        app.add_plugins(RaytracingHostPlugin);
        app.world_mut().write_message(RaytracingNotice::warning("Ray tracing is too slow on this GPU. More."));
        app.update();
        let shown = &app.world().resource::<NotificationManager>().notifications;
        assert_eq!(shown.len(), 1, "the notice was not shown");
        assert_eq!(shown[0].message, "Ray tracing is too slow on this GPU. More.");
        assert_eq!(shown[0].level, crate::notifications::NotificationLevel::Warning);
    }

    #[test]
    fn studio_measures_edit_mode_only_focused_and_at_full_speed() {
        assert_eq!(host_frame(false, None, None, None), HostFrame::default());
        assert!(host_frame(true, None, None, None).loading);
        assert!(!host_frame(false, None, None, Some(&PlayModeState::Playing)).measurable);
        assert!(host_frame(false, None, None, Some(&PlayModeState::Editing)).measurable);
    }
}
