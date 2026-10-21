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
    // The API base is read once, the first time anything asks for it.
    // These must be set before the plugins start.
    crate::dedicated::apply_server_env(&args);
    crate::dedicated::arm_stop_signals();
    crate::dedicated::ensure_server_token(args.sim_id.as_deref());
    crate::dedicated::start_api_latency_probe();

    let mut script_note = String::new();
    let mut server_scripts = false;
    let space_root = if let Some(sim) = args.sim_id.as_deref() {
        let token = std::env::var("EUSTRESS_SERVER_TOKEN")
            .ok()
            .or_else(|| std::env::var("EUSTRESS_AUTHOR_TOKEN").ok());
        let dir = crate::dedicated::download_dir(sim);
        match space::gallery_edit::download_for_host(sim, token.as_deref(), &dir) {
            Ok(world) => {
                println!("{}", world.note);
                script_note = world.note;
                server_scripts = world.from_source;
                world.space_root
            }
            Err(error) => {
                eprintln!("eustress-engine --server: {error}");
                std::process::exit(1);
            }
        }
    } else {
        space::format_migration::launch_space_or_scratch(
            args.launch_space_root()
                .unwrap_or_else(space::default_space_root),
        )
    };
    println!(
        "Eustress server (no window, no GPU renderer) — {}",
        space_root.display()
    );
    if let Some(path) = args.status_file.as_ref() {
        crate::dedicated::write_status(
            path,
            &crate::dedicated::DedicatedStatus {
                pid: std::process::id(),
                phase: "starting".into(),
                port: args.port.unwrap_or(crate::dedicated::HOST_PORT),
                sim_id: args.sim_id.clone().unwrap_or_default(),
                server_scripts,
                script_note: script_note.clone(),
                public_host: std::env::var("EUSTRESS_HOST_PUBLIC").unwrap_or_default(),
                server_id: crate::dedicated::server_id_from_env(),
                region: crate::dedicated::region_from_env().unwrap_or_default(),
                ..Default::default()
            },
        );
    }

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
        saw_pending: false,
        started: false,
        note: script_note,
        server_scripts,
    });
    app.add_systems(Update, (enter_play, watch_dedicated_stop, write_dedicated_status));
    app.run();
}

#[derive(Resource)]
struct ServerRun {
    frames_seen: u64,
    /// The Space open set `PendingWorldDbOpen` at least once.
    saw_pending: bool,
    started: bool,
    note: String,
    server_scripts: bool,
}

fn enter_play(
    mut run: ResMut<ServerRun>,
    state: Res<State<PlayModeState>>,
    mut next: ResMut<NextState<PlayModeState>>,
    clock: Res<SimulationClock>,
    #[cfg(feature = "world-db")] pending: Option<Res<crate::space::world_db_plugin::PendingWorldDbOpen>>,
) {
    run.frames_seen += 1;
    if run.started || *state.get() != PlayModeState::Editing {
        return;
    }
    // Do not enter Play on the first frame, before the open system has set
    // its pending record. Once that record has been seen, wait until it is
    // cleared. If the open never starts, wait the usual 120 frames.
    #[cfg(feature = "world-db")]
    {
        let opening = pending.as_ref().is_some_and(|pending| pending.0.is_some());
        if opening {
            run.saw_pending = true;
            return;
        }
        if !run.saw_pending && run.frames_seen < AUTOPLAY_DELAY_FRAMES {
            return;
        }
    }
    #[cfg(not(feature = "world-db"))]
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

fn watch_dedicated_stop(args: Res<StartupArgs>, mut exit: MessageWriter<bevy::app::AppExit>) {
    let asked = args.status_file.as_ref().is_some_and(|path| crate::dedicated::stop_file_present(path));
    if asked || crate::dedicated::stop_asked() {
        info!("server: stop requested");
        exit.write(bevy::app::AppExit::Success);
    }
}

fn write_dedicated_status(
    args: Res<StartupArgs>,
    run: Res<ServerRun>,
    request: Option<Res<crate::multiplayer::HostRequest>>,
    host: Option<Res<eustress_networking::session::HostSession>>,
    time: Res<Time>,
    mut last: Local<f64>,
) {
    let Some(path) = args.status_file.as_ref() else { return };
    let now = time.elapsed_secs_f64();
    if *last != 0.0 && now - *last < 1.0 {
        return;
    }
    *last = now;
    let hardware = crate::dedicated::hardware_class();
    let mut status = crate::dedicated::read_status(path).unwrap_or_default();
    status.pid = std::process::id();
    status.phase = if request.as_ref().is_some_and(|r| r.is_hosting()) { "hosting" } else { "starting" }.into();
    status.port = request.as_ref().and_then(|r| r.listen_port()).unwrap_or(args.port.unwrap_or(crate::dedicated::HOST_PORT));
    status.listening = request.as_ref().is_some_and(|r| r.is_hosting());
    status.pin = request.as_ref().and_then(|r| r.certificate_pin()).map(|pin| eustress_networking::join_link::hex32(&pin)).unwrap_or_default();
    status.players = host.as_ref().map(|h| h.player_count()).unwrap_or(0);
    status.max_players = host.as_ref().map(|h| h.max_players()).unwrap_or(status.max_players);
    status.sim_id = args.sim_id.clone().unwrap_or(status.sim_id);
    status.public_host = std::env::var("EUSTRESS_HOST_PUBLIC").unwrap_or(status.public_host);
    status.server_scripts = run.server_scripts;
    if status.script_note.is_empty() {
        status.script_note = run.note.clone();
    }
    status.server_id = crate::dedicated::server_id_from_env();
    status.region = crate::dedicated::region_from_env().unwrap_or_default();
    status.latency_ms = crate::dedicated::api_latency_ms();
    status.gpu = hardware.get("gpu").and_then(|v| v.as_str()).unwrap_or("").to_string();
    status.cpu = hardware.get("cpu").and_then(|v| v.as_str()).unwrap_or("").to_string();
    status.hardware_tier = hardware.get("tier").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    crate::dedicated::write_status(path, &status);
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
