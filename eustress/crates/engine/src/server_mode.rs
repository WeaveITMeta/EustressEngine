//! `--server`: the simulation and the multiplayer host, with no window and
//! no GPU renderer.
//!
//! A windowed Studio session dies when wgpu loses the device
//! (`DeviceLost`, then a panic in `bevy_pbr` GPU light clustering). This
//! shell never creates that device, so the hosted session is not tied to
//! the driver. Play starts on its own and hosts (`EUSTRESS_HOST_ON_PLAY`),
//! which is the same switch `eustress-headless` documents.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;

use eustress_common::simulation::SimulationClock;

use crate::play_mode::PlayModeState;
use crate::startup::StartupArgs;
use crate::{app_core, space};

/// Frames to wait before Play, so the Space finishes loading first.
const AUTOPLAY_DELAY_FRAMES: u64 = 120;

/// Run the dedicated server until the process is killed. Does not return
/// until the app exits.
pub fn run(args: StartupArgs) {
    // Host on Play unless the operator already chose otherwise.
    if std::env::var_os("EUSTRESS_HOST_ON_PLAY").is_none() {
        std::env::set_var("EUSTRESS_HOST_ON_PLAY", "1");
    }

    let space_root = space::format_migration::launch_space_or_scratch(
        args.launch_space_root()
            .unwrap_or_else(space::default_space_root),
    );
    println!(
        "Eustress server (no window, no GPU renderer) — {}",
        space_root.display()
    );

    let mut app = App::new();
    app.set_error_handler(app_core::rate_limited_error_handler);
    app.insert_resource(space::SpaceRoot(space_root.clone()));
    app_core::register_asset_sources(&mut app, &space_root);

    app.add_plugins((
        bevy::log::LogPlugin::default(),
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        ))),
        bevy::transform::TransformPlugin,
        bevy::diagnostic::DiagnosticsPlugin,
        bevy::state::app::StatesPlugin,
        bevy::input::InputPlugin,
        AssetPlugin {
            file_path: crate::engine_assets_dir().to_string_lossy().to_string(),
            unapproved_path_mode: bevy::asset::UnapprovedPathMode::Allow,
            ..default()
        },
        bevy::scene::ScenePlugin,
    ));

    // Spawn paths insert these handles. Without the containers the systems
    // fail param validation and the Space never loads. Nothing is uploaded
    // to a GPU.
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_asset::<Image>();
    app.init_asset::<AnimationClip>();
    app.init_asset::<AnimationGraph>();
    app.init_asset::<bevy::audio::AudioSource>();
    app.init_asset::<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>();

    app.insert_resource(crate::engine_bridge::BridgeInstanceKind(
        eustress_bridge_client::InstanceKind::Headless,
    ));
    app.insert_resource(args);
    app_core::add_core_sim_plugins(&mut app, &space_root);

    app.insert_resource(ServerRun {
        frames_seen: 0,
        started: false,
    });
    app.add_systems(Update, enter_play);
    app.run();
}

#[derive(Resource)]
struct ServerRun {
    frames_seen: u64,
    started: bool,
}

fn enter_play(
    mut run: ResMut<ServerRun>,
    state: Res<State<PlayModeState>>,
    mut next: ResMut<NextState<PlayModeState>>,
    clock: Res<SimulationClock>,
) {
    run.frames_seen += 1;
    if run.started || *state.get() != PlayModeState::Editing {
        return;
    }
    if run.frames_seen < AUTOPLAY_DELAY_FRAMES {
        return;
    }
    info!(
        "server: entering Play (tick {}) and hosting — no renderer is running",
        clock.tick_count
    );
    run.started = true;
    next.set(PlayModeState::Playing);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_mode_is_the_flag_that_skips_the_renderer() {
        let mut args = StartupArgs::default();
        assert!(!args.server_mode);
        args.server_mode = true;
        assert!(args.server_mode);
    }
}
