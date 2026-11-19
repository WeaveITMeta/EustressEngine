//! The Player's side of ray tracing: the same `eustress_raytracing` Studio
//! runs, Auto by default, with what it asks of the app that runs it (its
//! "The host" docs) filled from the Player's own state.
//!
//! - The choice: `EUSTRESS_RAYTRACING` (auto, on, off) wins; else the
//!   `ray_tracing` key of `~/.eustress_engine/settings.json`, the one Studio
//!   saves, so one choice governs both apps on a machine; else Auto. The
//!   pause menu's Settings row changes it ([`RayTracingMenu`]), from the next
//!   launch; Off also stops a running session at once.
//! - The renderer: `main` asks [`decide`] before the app exists, and
//!   `build_app` hands its creation to `RenderPlugin`, so the GPU gate reads
//!   the device the Player renders with.
//! - [`HostFrame`]: loading until the world is open and its terrain is in;
//!   interactive while the window is focused and not minimized; every frame
//!   in the world counts; no frame cap.
//! - [`HostSpaceRoot`]: the Space folder a local launch opens, or
//!   `eustress-published/<id>` for a published simulation, so its
//!   texture-limit record holds from one play to the next. A joined host's
//!   world has none, so no record is kept for it.
//! - SMAA leaves a camera DLSS Ray Reconstruction denoises (DLSS
//!   anti-aliases the image itself) and comes back if DLSS leaves.
//! - A [`RaytracingNotice`] goes to the log. The Player's screen notice is the
//!   card that dims the whole window for a failure that stops play, which a
//!   change of lighting is not.
//! - The render error handler: ray tracing's stands in front of
//!   `gpu_recovery`'s, so a lost device is noted against ray tracing and then
//!   recovered exactly as before.
//!
//! Desktop only. On the web, Android and iOS the Player keeps Bevy's own
//! renderer creation and no ray tracing; the same names exist there so
//! `main` and the pause menu read the same either way.

use std::path::PathBuf;

use bevy::prelude::*;

/// The pause menu's ray-tracing row. The menu shows `row` when there is one,
/// shows `note` under the menu while that row is highlighted, and adds Left
/// (-1) or Right (+1) presses to `pending_step`; [`PlayerRaytracingPlugin`]
/// applies them.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct RayTracingMenu {
    pub row: Option<String>,
    pub note: String,
    pub pending_step: i32,
}

/// The renderer `RenderPlugin` uses, and the plugin that goes with it.
pub struct PlayerRender {
    pub creation: bevy::render::settings::RenderCreation,
    pub plugin: PlayerRaytracingPlugin,
}

/// The Space a launch is keyed by for the texture-limit record: the folder a
/// local launch opens, `eustress-published/<id>` for a published simulation,
/// none for a joined host.
pub fn space_for(launch: &crate::Launch) -> Option<PathBuf> {
    match launch {
        crate::Launch::Local { .. } => super::space_world::resolve_space(),
        crate::Launch::Published { sim_id, .. } => Some(PathBuf::from("eustress-published").join(sim_id)),
        crate::Launch::Join { .. } => None,
    }
}

/// Studio's wording names its own Settings dialog; the Player's choice is in
/// the pause menu.
pub fn player_wording(text: &str) -> String {
    text.replace("Settings > Graphics > Ray tracing", "the pause menu's Settings (Esc), Ray tracing")
}

/// The choice one step from the one at `index` (`RayTracingMode::index`),
/// Left (`step` < 0) or Right, wrapping: Auto, On, Off.
pub fn stepped(index: i32, step: i32) -> i32 {
    (index + step.signum()).rem_euclid(3)
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
pub use desktop::*;

#[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))]
pub use elsewhere::*;

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod desktop {
    use std::path::{Path, PathBuf};

    use bevy::anti_alias::smaa::Smaa;
    use bevy::prelude::*;
    use bevy::render::settings::WgpuSettings;
    use bevy::window::PrimaryWindow;
    use eustress_raytracing::policy;
    use eustress_raytracing::{
        FallbackReason, HostFrame, HostSpaceRoot, NoticeLevel, RasterFallbackRequest, RayTracingMode, RaytracingLaunch,
        RaytracingNote, RaytracingNotice, RaytracingPlugin,
    };

    use super::{player_wording, stepped, PlayerRender, RayTracingMenu};
    use crate::systems::space_world::{OpenSpace, SpaceTerrainLoad};

    /// Called by `main` before the app exists, after `EUSTRESS_SPACE` is set:
    /// the switch, the GPU gate and the guard records, with the Player's own
    /// `WgpuSettings` (Bevy's defaults, which it has always used).
    pub fn decide(space: Option<PathBuf>) -> PlayerRender {
        let root = space.clone().unwrap_or_default();
        let (creation, launch) = eustress_raytracing::render_creation(WgpuSettings::default(), Path::new(&root));
        PlayerRender { creation, plugin: PlayerRaytracingPlugin { launch, space } }
    }

    /// Before `DefaultPlugins`: Bevy's DLSS plugin reads the project id while
    /// it builds. Nothing in a build without DLSS.
    pub fn before_default_plugins(app: &mut App) {
        eustress_raytracing::insert_dlss_project_id(app);
    }

    /// Once `App::run` has returned: a normal close takes the crash guard's
    /// launch marker down.
    pub fn note_clean_exit() {
        policy::note_clean_exit();
    }

    /// Ray tracing and the Player's side of it. Added after `DefaultPlugins`.
    pub struct PlayerRaytracingPlugin {
        launch: RaytracingLaunch,
        space: Option<PathBuf>,
    }

    /// What the Settings file says, apart from `EUSTRESS_RAYTRACING`: the
    /// choice the next launch takes.
    #[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
    struct SavedChoice(RayTracingMode);

    impl Plugin for PlayerRaytracingPlugin {
        fn build(&self, app: &mut App) {
            let saved = policy::requested_mode(None, policy::saved_settings_text().as_deref()).0;
            app.add_plugins(RaytracingPlugin::new(self.launch.clone()))
                .add_message::<RaytracingNotice>()
                .insert_resource(HostFrame { loading: true, ..default() })
                .insert_resource(SavedChoice(saved))
                .init_resource::<RayTracingMenu>()
                .add_systems(PostUpdate, fill_host_frame)
                .add_systems(Update, (log_notices, apply_menu_steps, update_menu).chain());
            if let Some(space) = &self.space {
                app.insert_resource(HostSpaceRoot(space.clone()));
            }
            eustress_raytracing::keep_off_denoised_cameras::<Smaa>(app);
        }
    }

    /// The Player's frame as the frame-time guard reads it.
    pub fn player_frame(world_open: bool, terrain_ready: bool, focused: bool, minimized: bool) -> HostFrame {
        HostFrame { loading: !(world_open && terrain_ready), interactive: focused && !minimized, measurable: true, cap_ms: 0.0 }
    }

    fn fill_host_frame(
        open: Option<Res<OpenSpace>>,
        terrain: Option<Res<SpaceTerrainLoad>>,
        windows: Query<&Window, With<PrimaryWindow>>,
        mut host: ResMut<HostFrame>,
    ) {
        let (focused, minimized) = windows
            .single()
            .map_or((true, false), |w| (w.focused, w.physical_width() == 0 || w.physical_height() == 0));
        let frame = player_frame(
            open.is_some_and(|open| open.root.is_some()),
            terrain.is_none_or(|terrain| terrain.ready()),
            focused,
            minimized,
        );
        host.set_if_neq(frame);
    }

    fn log_notices(mut notices: MessageReader<RaytracingNotice>) {
        for notice in notices.read() {
            let text = player_wording(&notice.text);
            match notice.level {
                NoticeLevel::Warning => warn!("Ray tracing: {text}"),
                NoticeLevel::Info => info!("Ray tracing: {text}"),
            }
        }
    }

    /// Left and Right on the menu's row: the next choice, saved for the next
    /// launch of either app. Off also stops a running session at once, as
    /// Studio's Settings does.
    fn apply_menu_steps(
        mut menu: ResMut<RayTracingMenu>,
        mut saved: ResMut<SavedChoice>,
        mut requests: MessageWriter<RasterFallbackRequest>,
    ) {
        let step = std::mem::take(&mut menu.pending_step);
        if step == 0 {
            return;
        }
        let Some(next) = RayTracingMode::from_index(stepped(saved.0.index(), step)) else { return };
        match policy::save_mode(next) {
            Ok(()) => info!("Ray tracing: {} saved; it applies at the next launch", next.label()),
            Err(e) => {
                warn!("Ray tracing: could not save the choice ({e})");
                return;
            }
        }
        saved.0 = next;
        if next == RayTracingMode::Off {
            requests.write(RasterFallbackRequest(FallbackReason::TurnedOff));
        }
    }

    /// The row's text and the note under it.
    pub fn menu_row(saved: RayTracingMode, running: RayTracingMode) -> String {
        if saved == running {
            format!("Ray tracing: {}", saved.label())
        } else {
            format!("Ray tracing: {} (next launch)", saved.label())
        }
    }

    fn update_menu(
        saved: Res<SavedChoice>,
        launch: Res<RaytracingLaunch>,
        note: Option<Res<RaytracingNote>>,
        mut menu: ResMut<RayTracingMenu>,
    ) {
        let row = Some(menu_row(saved.0, launch.mode));
        let note = note.map(|note| player_wording(&note.0)).unwrap_or_default();
        if menu.row != row || menu.note != note {
            menu.row = row;
            menu.note = note;
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use RayTracingMode::{Auto, Off, On};

        #[test]
        fn the_row_says_when_a_choice_waits_for_the_next_launch() {
            assert_eq!(menu_row(Auto, Auto), "Ray tracing: Auto");
            assert_eq!(menu_row(Off, Auto), "Ray tracing: Off (next launch)");
            assert_eq!(menu_row(On, Off), "Ray tracing: On (next launch)");
        }

        #[test]
        fn left_and_right_walk_auto_on_off_and_wrap() {
            let next = |mode: RayTracingMode, step| RayTracingMode::from_index(stepped(mode.index(), step)).unwrap();
            assert_eq!(next(Auto, 1), On);
            assert_eq!(next(On, 1), Off);
            assert_eq!(next(Off, 1), Auto);
            assert_eq!(next(Auto, -1), Off);
            assert_eq!(next(On, -5), Auto, "a step is one choice, however hard the key was held");
        }

        #[test]
        fn the_guard_counts_frames_only_in_an_open_world_in_a_focused_window() {
            assert!(player_frame(false, true, true, false).loading, "no world yet");
            assert!(player_frame(true, false, true, false).loading, "terrain still building");
            let playing = player_frame(true, true, true, false);
            assert!(!playing.loading && playing.interactive && playing.measurable);
            assert!(!player_frame(true, true, false, false).interactive, "unfocused");
            assert!(!player_frame(true, true, true, true).interactive, "minimized");
        }

        #[test]
        fn notices_name_the_players_menu() {
            let text = player_wording("To keep it, choose On in Settings > Graphics > Ray tracing.");
            assert_eq!(text, "To keep it, choose On in the pause menu's Settings (Esc), Ray tracing.");
        }
    }
}

#[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))]
mod elsewhere {
    use std::path::PathBuf;

    use bevy::prelude::*;

    use super::{PlayerRender, RayTracingMenu};

    /// No ray tracing here: Bevy's own renderer creation, as always.
    pub fn decide(_space: Option<PathBuf>) -> PlayerRender {
        PlayerRender { creation: default(), plugin: PlayerRaytracingPlugin }
    }

    pub fn before_default_plugins(_app: &mut App) {}

    pub fn note_clean_exit() {}

    /// Only the menu's resource, with no row.
    pub struct PlayerRaytracingPlugin;

    impl Plugin for PlayerRaytracingPlugin {
        fn build(&self, app: &mut App) {
            app.init_resource::<RayTracingMenu>();
        }
    }
}
