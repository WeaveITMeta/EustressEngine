//! `--server`: the simulation and the multiplayer host, with no window and
//! no GPU renderer.
//!
//! A windowed Studio session rides out a lost GPU device (it stops drawing
//! and rebuilds its renderer, `eustress_play_runtime::gpu_recovery`), but its
//! hosted session still pauses for the rebuild. This shell never creates
//! that device, so the hosted session is not tied to the driver at all.
//! Play starts on its own and hosts (`EUSTRESS_HOST_ON_PLAY`), which is the
//! same switch `eustress-headless` documents.
//!
//! The process is the server alone ([`crate::multiplayer::ServerView`]): it
//! has no Player of its own, so `Players` holds only the players who join
//! and `PlayerAdded` fires only for them.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;

use eustress_common::simulation::SimulationClock;

use crate::play_mode::PlayModeState;
use crate::startup::StartupArgs;
use crate::{app_core, space};

/// Frames to wait before Play, so the Space finishes loading first.
const AUTOPLAY_DELAY_FRAMES: u64 = 120;

/// The most frames Play waits for the Space's services to finish loading.
const SERVICES_WAIT_FRAMES: u64 = 1800;

/// Whether the Space's services have all loaded: the priority pass built the
/// deferred queue and the queue is empty (the same test the loader uses to
/// finish, `space/file_loader.rs`).
fn services_loaded(priority_done: bool, pending: usize) -> bool {
    priority_done && pending == 0
}

/// The server's frame rate when `EUSTRESS_SERVER_HZ` is not set.
const DEFAULT_SERVER_HZ: f64 = 60.0;
/// The lowest and highest frame rate `EUSTRESS_SERVER_HZ` may ask for. A
/// session with players in it never runs below 60 Hz; only an empty one
/// slows down (`EUSTRESS_SERVER_IDLE_HZ`).
const MIN_SERVER_HZ: f64 = 60.0;
const MAX_SERVER_HZ: f64 = 120.0;
/// The editor caps a frame's virtual time at 33 ms (app_core), which is
/// right for a window at 60 Hz or more.
const EDITOR_MAX_VIRTUAL_DELTA_SECS: f64 = 0.033;

/// The server's frame rate from `EUSTRESS_SERVER_HZ`: a number of frames a
/// second, clamped to 60..=120. Unset, empty, not a number, or not finite
/// reads as 60. Physics keeps its fixed 60 Hz step at any frame rate; a
/// higher rate runs more frames (scripts, replication, status) between
/// steps.
fn server_hz_from(raw: Option<&str>) -> f64 {
    raw.and_then(|text| text.trim().parse::<f64>().ok())
        .filter(|hz| hz.is_finite())
        .map(|hz| hz.clamp(MIN_SERVER_HZ, MAX_SERVER_HZ))
        .unwrap_or(DEFAULT_SERVER_HZ)
}

/// The most virtual time one server frame may carry. An empty server's idle
/// rate is below 30 Hz, where a frame is longer than the editor's 33 ms cap
/// and the simulation would run slower than real time, so the cap grows to
/// two frame periods.
fn server_max_virtual_delta(hz: f64) -> Duration {
    if hz >= DEFAULT_SERVER_HZ {
        return Duration::from_secs_f64(EDITOR_MAX_VIRTUAL_DELTA_SECS);
    }
    Duration::from_secs_f64(EDITOR_MAX_VIRTUAL_DELTA_SECS.max(2.0 / hz))
}

/// The server's frame rate while nobody is in it or joining, when
/// `EUSTRESS_SERVER_IDLE_HZ` is not set. An empty frame costs about the same
/// as a full one, so 5 Hz spends about a twelfth of what 60 Hz does on it. A
/// joining player's `Hello` waits at most one idle frame (200 ms) before the
/// host reads it; the transport's own keep-alive and timeouts run on their
/// own thread, and the session's waits (15 s admission, 20 s Welcome) are far
/// longer.
const DEFAULT_IDLE_HZ: f64 = 5.0;
/// The lowest idle rate `EUSTRESS_SERVER_IDLE_HZ` may ask for.
const MIN_IDLE_HZ: f64 = 1.0;
/// Seconds the server keeps its full rate after the last sign of anyone: a
/// player in the world or downloading it, or any session notice (a `Hello`
/// waiting for the gallery's admission raises one). Longer than the 15 s the
/// host waits for an admission, so a whole join runs at the full rate.
const JOIN_HOLD_SECS: f64 = 30.0;

/// The idle frame rate from `EUSTRESS_SERVER_IDLE_HZ`, clamped to
/// `1..=full_hz`. Unset, empty, not a number, or not finite reads as 5.
/// A value at or above the full rate turns the idle rate off.
fn idle_hz_from(raw: Option<&str>, full_hz: f64) -> f64 {
    raw.and_then(|text| text.trim().parse::<f64>().ok())
        .filter(|hz| hz.is_finite())
        .unwrap_or(DEFAULT_IDLE_HZ)
        .clamp(MIN_IDLE_HZ, full_hz.max(MIN_IDLE_HZ))
}

/// Whether the server has someone to serve this frame: Play or hosting has
/// not started yet (the Space loads one service a frame, and Play's waits
/// count frames), a player is in the world, an admitted player is
/// downloading it, or the session raised a notice.
fn server_busy(hosting_started: bool, players: usize, admitted: usize, notice: bool) -> bool {
    !hosting_started || players > 0 || admitted > 0 || notice
}

/// Seconds a server that entered Play may go with no session hosting and
/// none on the way (asked for, baking, binding) before it gives up. The
/// first frames of Play are in that state until the session is asked for,
/// so the wait is far longer than those.
const HOST_FAIL_GRACE_SECS: f64 = 10.0;

/// The server's pace: its full and idle rates, and when it last had anyone.
/// The runner reads it after each frame to choose the wait before the next.
#[derive(Resource, Debug, Clone)]
struct ServerPace {
    full_hz: f64,
    idle_hz: f64,
    /// Real seconds since start at which the server last had anyone.
    last_busy: f64,
    idle: bool,
    /// `EUSTRESS_SERVER_EMPTY_EXIT_SECS`: leave after this long with nobody.
    empty_exit_secs: Option<f64>,
    /// The run is meant to host (`EUSTRESS_HOST_ON_PLAY`). A run told not to
    /// host is a simulation with no window and never fails this way.
    host_expected: bool,
    /// Real seconds since start at which Play was on with no session hosting
    /// and none on the way; `None` while one runs or is starting.
    not_hosting_since: Option<f64>,
    /// `EUSTRESS_SERVER_IDLE_PAUSE=1`: while the server idles with nobody in
    /// it, simulated time is paused, so an empty server costs its idle
    /// frames and nothing else. Off by default: an empty world then keeps
    /// running, as a creator's own server does.
    idle_pause: bool,
}

impl ServerPace {
    fn new(full_hz: f64, idle_hz: f64, empty_exit_secs: Option<f64>) -> Self {
        Self {
            full_hz,
            idle_hz,
            last_busy: 0.0,
            idle: false,
            empty_exit_secs,
            host_expected: false,
            not_hosting_since: None,
            idle_pause: false,
        }
    }

    /// Record whether the started server hosts a session or has one on the
    /// way. Returns true once it has had neither for
    /// [`HOST_FAIL_GRACE_SECS`]: the bake, the bind or the listener failed,
    /// and nobody can join.
    fn host_failed(&mut self, now: f64, play_started: bool, hosting_off: bool) -> bool {
        if !self.host_expected || !play_started || !hosting_off {
            self.not_hosting_since = None;
            return false;
        }
        let since = *self.not_hosting_since.get_or_insert(now);
        now - since >= HOST_FAIL_GRACE_SECS
    }

    /// Record this frame. Returns true when the rate changed.
    fn observe(&mut self, now: f64, busy: bool) -> bool {
        if busy {
            self.last_busy = now;
        }
        let idle = self.idle_hz < self.full_hz && now - self.last_busy >= JOIN_HOLD_SECS;
        let changed = idle != self.idle;
        self.idle = idle;
        changed
    }

    fn hz(&self) -> f64 {
        if self.idle { self.idle_hz } else { self.full_hz }
    }

    fn period(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.hz())
    }

    /// Whether the server has had nobody for `EUSTRESS_SERVER_EMPTY_EXIT_SECS`.
    fn empty_exit_due(&self, now: f64) -> bool {
        self.empty_exit_secs.is_some_and(|limit| now - self.last_busy >= limit)
    }
}

/// When the next frame is due: one period after the last one was due, so a
/// sleep that ran long is paid back by a shorter one and the rate holds on
/// average (a sleep for "what is left of the period" ran each frame a few
/// milliseconds long, and 60 Hz measured 50). A frame that is already more
/// than a period late starts a fresh cadence from now: the server does not
/// burst to catch up after a long frame.
fn next_frame_due(due: std::time::Instant, now: std::time::Instant, period: Duration) -> std::time::Instant {
    let next = due + period;
    if now > next + period {
        now
    } else {
        next
    }
}

/// Runs frames at the pace [`ServerPace`] sets: a frame, then a sleep until
/// the next frame is due ([`next_frame_due`]). The period is read after
/// every frame, so the first frame that sees a player is followed by a
/// full-rate wait.
fn server_runner(mut app: App) -> bevy::app::AppExit {
    if app.plugins_state() != bevy::app::PluginsState::Cleaned {
        while app.plugins_state() == bevy::app::PluginsState::Adding {
            bevy::tasks::tick_global_task_pools_on_main_thread();
        }
        app.finish();
        app.cleanup();
    }
    let mut due = std::time::Instant::now();
    loop {
        app.update();
        if let Some(exit) = app.should_exit() {
            return exit;
        }
        let period = app
            .world()
            .get_resource::<ServerPace>()
            .map(ServerPace::period)
            .unwrap_or(Duration::from_secs_f64(1.0 / DEFAULT_SERVER_HZ));
        let now = std::time::Instant::now();
        due = next_frame_due(due, now, period);
        if due > now {
            std::thread::sleep(due - now);
        }
    }
}

/// The status a server writes when it cannot download its world: phase
/// `host-failed` (`dedicated::HOST_FAILED_PHASE`) with the reason, so the
/// launcher frees the slot and says why (a hosted session refused for a
/// listing with no source world among them).
fn download_failed_status(sim_id: &str, error: &str, port: u16) -> crate::dedicated::DedicatedStatus {
    crate::dedicated::DedicatedStatus {
        pid: std::process::id(),
        phase: crate::dedicated::HOST_FAILED_PHASE.into(),
        port,
        sim_id: sim_id.to_string(),
        error: error.to_string(),
        server_id: crate::dedicated::server_id_from_env(),
        region: crate::dedicated::region_from_env().unwrap_or_default(),
        ..Default::default()
    }
}

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
        // This server's own directory: the download clears it first, and a
        // directory shared by every server of the listing would take the
        // world, the status file and the stop file from a running one.
        let dir = crate::dedicated::server_download_dir(
            sim,
            &crate::dedicated::server_id_from_env(),
            args.status_file.as_deref(),
        );
        match space::gallery_edit::download_for_host(sim, token.as_deref(), &dir) {
            Ok(world) => {
                println!("{}", world.note);
                script_note = world.note;
                server_scripts = world.from_source;
                world.space_root
            }
            Err(error) => {
                eprintln!("eustress-engine --server: {error}");
                // A launcher reads why: a hosted session refused for a listing
                // with no source world reads as host-failed, never as a
                // session that is still starting.
                if let Some(path) = args.status_file.as_ref() {
                    crate::dedicated::write_status(
                        path,
                        &download_failed_status(
                            sim,
                            &error,
                            args.port.unwrap_or(crate::dedicated::HOST_PORT),
                        ),
                    );
                }
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

    let server_hz = server_hz_from(std::env::var("EUSTRESS_SERVER_HZ").ok().as_deref());
    println!("Eustress server frame rate: {server_hz} Hz (EUSTRESS_SERVER_HZ, 60 to 120, default 60)");
    let idle_hz = idle_hz_from(std::env::var("EUSTRESS_SERVER_IDLE_HZ").ok().as_deref(), server_hz);
    println!("Eustress server idle rate: {idle_hz} Hz with nobody in it (EUSTRESS_SERVER_IDLE_HZ, 1 to the full rate, default 5)");
    let empty_exit_secs = crate::dedicated::empty_exit_secs_from_env();
    match empty_exit_secs {
        Some(secs) => println!("Eustress server leaves after {secs} s with nobody in it (EUSTRESS_SERVER_EMPTY_EXIT_SECS)"),
        None => println!("Eustress server stays up with nobody in it (EUSTRESS_SERVER_EMPTY_EXIT_SECS is unset)"),
    }

    let mut app = App::new();
    app.set_error_handler(app_core::rate_limited_error_handler);
    app.insert_resource(space::SpaceRoot(space_root.clone()));
    app_core::register_asset_sources(&mut app, &space_root);

    app.add_plugins((
        bevy::log::LogPlugin::default(),
        // `server_runner` paces the frames instead (full rate, or the idle
        // rate while nobody is in the session).
        MinimalPlugins.build().disable::<ScheduleRunnerPlugin>(),
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
    // add_core_sim_plugins inserts the editor's 33 ms cap; a slow server
    // frame needs room for its whole period (see server_max_virtual_delta).
    if let Some(mut virtual_time) = app.world_mut().get_resource_mut::<Time<bevy::time::Virtual>>() {
        virtual_time.set_max_delta(server_max_virtual_delta(server_hz));
    }

    app.insert_resource(ServerRun {
        frames_seen: 0,
        saw_pending: false,
        started: false,
        note: script_note,
        server_scripts,
    });
    let mut pace = ServerPace::new(server_hz, idle_hz, empty_exit_secs);
    pace.host_expected = crate::multiplayer::env_flag("EUSTRESS_HOST_ON_PLAY");
    pace.idle_pause = crate::multiplayer::env_flag("EUSTRESS_SERVER_IDLE_PAUSE");
    if pace.idle_pause {
        println!("Eustress server pauses the simulation while nobody is in it (EUSTRESS_SERVER_IDLE_PAUSE)");
    }
    app.insert_resource(pace);
    // The server alone: Play seeds no Player of its own, so the session's
    // players are exactly the ones who join (`play_datamodel::seed`).
    app.insert_resource(crate::multiplayer::ServerView);
    // `EUSTRESS_PROFILE=1` logs where each frame's time goes, the same
    // phase profiler Studio has; unset, it costs one flag check a phase.
    app.add_plugins(crate::profiler::ProfilerPlugin);
    app.add_systems(Update, (enter_play, watch_dedicated_stop, write_dedicated_status));
    // After Update, so it sees what the session did this frame.
    app.add_systems(PostUpdate, pace_server);
    app.set_runner(server_runner);
    // A server that failed ends with a non-zero code, so whatever started it
    // (a fleet host, a service manager) tells it from a clean stop.
    if let bevy::app::AppExit::Error(code) = app.run() {
        std::process::exit(i32::from(code.get()));
    }
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
    deferred: Option<Res<crate::space::file_loader::DeferredServiceLoader>>,
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
    // The Space's services (ServerScriptService, StarterPlayer and the rest)
    // load one a frame after the priority pass. Play seeds its Luau VM from
    // what is loaded when it starts, so Play entered before the last service
    // lands starts no server script at all ("0 server and 0 client
    // script(s) queued"). Wait for the queue, but not forever.
    let services_loading = deferred.as_ref().is_some_and(|d| !services_loaded(d.priority_done, d.pending.len()));
    if services_loading && run.frames_seen < SERVICES_WAIT_FRAMES {
        return;
    }
    if services_loading {
        warn!("server: the Space's services were still loading after {SERVICES_WAIT_FRAMES} frames; entering Play anyway");
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
    // Real time: the status file keeps its beat while simulated time is
    // paused on an empty server.
    time: Res<Time<bevy::time::Real>>,
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
    status.seated = host.as_ref().map(|h| h.seated_count()).unwrap_or(0);
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

/// Choose the next frame's rate from who is in the session, keep simulated
/// time real at that rate, and leave once the server has been empty for
/// `EUSTRESS_SERVER_EMPTY_EXIT_SECS` (the same exit a stop takes, so the
/// gallery hears the session ended).
#[allow(clippy::too_many_arguments)]
fn pace_server(
    mut pace: ResMut<ServerPace>,
    run: Res<ServerRun>,
    args: Res<StartupArgs>,
    request: Option<Res<crate::multiplayer::HostRequest>>,
    host: Option<Res<eustress_networking::session::HostSession>>,
    mut notices: MessageReader<eustress_networking::session::NetNotice>,
    real: Res<Time<bevy::time::Real>>,
    mut virtual_time: ResMut<Time<bevy::time::Virtual>>,
    mut exit: MessageWriter<bevy::app::AppExit>,
) {
    let now = real.elapsed_secs_f64();
    let hosting_started = run.started && request.as_ref().is_some_and(|r| r.is_hosting());
    let players = host.as_ref().map(|h| h.player_count()).unwrap_or(0);
    // Every open connection is someone to serve: a player downloading the
    // world (in a session the gallery does not list, one has no admitted
    // id), one waiting for the gallery, and one that has not said `Hello`
    // yet. The last keeps simulated time running on a server that pauses
    // while it is empty, so the session's own wait for a `Hello`
    // (`session::HELLO_TIMEOUT`) ends and the seat comes back.
    let admitted = host.as_ref().map(|h| h.connection_count()).unwrap_or(0);
    let notice = !notices.is_empty();
    notices.clear();
    if pace.observe(now, server_busy(hosting_started, players, admitted, notice)) {
        virtual_time.set_max_delta(server_max_virtual_delta(pace.hz()));
        if pace.idle {
            if pace.idle_pause {
                // Simulated time stops with nobody to see it: no physics
                // step and no script time passes until someone joins.
                virtual_time.pause();
                info!("server: nobody in the session; {} Hz and the simulation paused until someone joins", pace.idle_hz);
            } else {
                info!("server: nobody in the session; {} Hz until someone joins", pace.idle_hz);
            }
        } else {
            if pace.idle_pause {
                virtual_time.unpause();
            }
            info!("server: someone is joining or playing; back to {} Hz", pace.full_hz);
        }
    }
    if pace.empty_exit_due(now) {
        info!("server: nobody for {:.0} s (EUSTRESS_SERVER_EMPTY_EXIT_SECS); leaving", now - pace.last_busy);
        if let Some(path) = args.status_file.as_ref() {
            let mut status = crate::dedicated::read_status(path).unwrap_or_default();
            status.phase = crate::dedicated::EMPTY_EXIT_PHASE.into();
            status.listening = false;
            crate::dedicated::write_status(path, &status);
        }
        exit.write(bevy::app::AppExit::Success);
        return;
    }
    // A server nobody can join holds a slot and a core for nothing: leave
    // with a failure, and whatever started it starts a fresh one.
    let hosting_off = request.as_ref().map_or(true, |r| r.phase() == crate::multiplayer::HostPhase::Off);
    if pace.host_failed(now, run.started, hosting_off) {
        error!(
            "server: no session is hosting {HOST_FAIL_GRACE_SECS:.0} s after Play began (the bake, the bind or the listener failed); leaving"
        );
        if let Some(path) = args.status_file.as_ref() {
            let mut status = crate::dedicated::read_status(path).unwrap_or_default();
            status.phase = crate::dedicated::HOST_FAILED_PHASE.into();
            status.listening = false;
            crate::dedicated::write_status(path, &status);
        }
        exit.write(bevy::app::AppExit::error());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_that_cannot_be_downloaded_reads_as_host_failed_with_its_reason() {
        let status = download_failed_status("pong", crate::space::gallery_edit::NO_SOURCE_FOR_SESSION, 7781);
        assert_eq!(status.phase, crate::dedicated::HOST_FAILED_PHASE);
        assert_eq!(status.error, crate::space::gallery_edit::NO_SOURCE_FOR_SESSION);
        assert_eq!((status.sim_id.as_str(), status.port), ("pong", 7781));
        assert!(!status.listening && status.join_link.is_empty(), "nothing to join");
        assert_eq!(status.pid, std::process::id());
    }

    #[test]
    fn server_mode_is_the_flag_that_skips_the_renderer() {
        let mut args = StartupArgs::default();
        assert!(!args.server_mode);
        args.server_mode = true;
        assert!(args.server_mode);
    }

    #[test]
    fn play_waits_until_every_service_has_loaded() {
        assert!(!services_loaded(false, 0), "the priority pass has not built the queue yet");
        assert!(!services_loaded(true, 3), "ServerScriptService may still be queued");
        assert!(services_loaded(true, 0));
    }

    #[test]
    fn server_hz_reads_the_env_value_and_clamps_it() {
        assert_eq!(server_hz_from(None), 60.0);
        assert_eq!(server_hz_from(Some("")), 60.0);
        assert_eq!(server_hz_from(Some("fast")), 60.0);
        assert_eq!(server_hz_from(Some("NaN")), 60.0);
        assert_eq!(server_hz_from(Some("inf")), 60.0);
        assert_eq!(server_hz_from(Some(" 90 ")), 90.0);
        assert_eq!(server_hz_from(Some("75.5")), 75.5);
        // 60 Hz is the floor for a session with players in it.
        assert_eq!(server_hz_from(Some("30")), 60.0);
        assert_eq!(server_hz_from(Some("1")), 60.0);
        assert_eq!(server_hz_from(Some("-5")), 60.0);
        assert_eq!(server_hz_from(Some("500")), 120.0);
    }

    #[test]
    fn frames_are_due_one_period_apart_and_a_long_sleep_is_paid_back() {
        let start = std::time::Instant::now();
        let period = Duration::from_micros(16_667);
        // A frame that took 5 ms: the next is due a full period after the last.
        assert_eq!(next_frame_due(start, start + Duration::from_millis(5), period), start + period);
        // A sleep that ran 3 ms long: the next frame is still due on the
        // original cadence, so the wait before it is 3 ms shorter.
        let late = start + period + Duration::from_millis(3);
        assert_eq!(next_frame_due(start + period, late, period), start + period * 2);
        // 600 frames whose every sleep overshoots by 3 ms still end on time.
        let mut due = start;
        for frame in 1..=600u32 {
            let woke = due + Duration::from_millis(3);
            due = next_frame_due(due, woke, period);
            assert_eq!(due, start + period * frame);
        }
        // More than a period late: no burst, a fresh cadence from now.
        let stalled = start + Duration::from_millis(500);
        assert_eq!(next_frame_due(start, stalled, period), stalled);
    }

    #[test]
    fn a_server_that_cannot_host_gives_up_after_the_grace() {
        let mut pace = ServerPace::new(60.0, 5.0, None);
        pace.host_expected = true;
        assert!(!pace.host_failed(0.0, false, true), "Play has not begun");
        assert!(!pace.host_failed(5.0, true, true), "the first frames of Play");
        assert!(!pace.host_failed(5.0 + HOST_FAIL_GRACE_SECS - 0.5, true, true));
        assert!(pace.host_failed(5.0 + HOST_FAIL_GRACE_SECS, true, true));
        // A session on the way, or running, starts the wait again.
        assert!(!pace.host_failed(100.0, true, false));
        assert!(!pace.host_failed(101.0, true, true));
        assert!(pace.host_failed(101.0 + HOST_FAIL_GRACE_SECS, true, true));
        // A run told not to host is a simulation with no window.
        let mut simulation = ServerPace::new(60.0, 5.0, None);
        assert!(!simulation.host_failed(1.0e6, true, true));
    }

    #[test]
    fn a_slow_server_frame_keeps_real_time() {
        // At 60 Hz and above the editor's 33 ms cap stands.
        assert_eq!(server_max_virtual_delta(60.0), Duration::from_secs_f64(0.033));
        assert_eq!(server_max_virtual_delta(120.0), Duration::from_secs_f64(0.033));
        // An idle rate below that: a frame's whole period fits, with room
        // for one late frame.
        for hz in [10.0, 20.0, 30.0] {
            assert!(server_max_virtual_delta(hz) >= Duration::from_secs_f64(1.0 / hz), "{hz} Hz");
        }
        assert_eq!(server_max_virtual_delta(10.0), Duration::from_secs_f64(0.2));
        // The idle rates below 10 Hz keep real time the same way.
        assert_eq!(server_max_virtual_delta(5.0), Duration::from_secs_f64(0.4));
        assert!(server_max_virtual_delta(1.0) >= Duration::from_secs(1));
    }

    #[test]
    fn idle_hz_reads_the_env_value_and_stays_at_or_under_the_full_rate() {
        assert_eq!(idle_hz_from(None, 60.0), 5.0);
        assert_eq!(idle_hz_from(Some(""), 60.0), 5.0);
        assert_eq!(idle_hz_from(Some("slow"), 60.0), 5.0);
        assert_eq!(idle_hz_from(Some("NaN"), 60.0), 5.0);
        assert_eq!(idle_hz_from(Some(" 2 "), 60.0), 2.0);
        assert_eq!(idle_hz_from(Some("0"), 60.0), 1.0);
        assert_eq!(idle_hz_from(Some("-3"), 60.0), 1.0);
        // At or above the full rate, the idle rate is the full rate: off.
        assert_eq!(idle_hz_from(Some("90"), 60.0), 60.0);
        assert_eq!(idle_hz_from(None, 120.0), 5.0);
    }

    #[test]
    fn a_server_is_busy_until_hosting_starts_and_while_anyone_is_there() {
        assert!(server_busy(false, 0, 0, false), "loading and entering Play run at the full rate");
        assert!(!server_busy(true, 0, 0, false));
        assert!(server_busy(true, 1, 0, false), "a player in the world");
        assert!(server_busy(true, 0, 1, false), "an admitted player downloading the world");
        assert!(server_busy(true, 0, 0, true), "a Hello waiting for the gallery raises a notice");
    }

    #[test]
    fn the_rate_drops_after_the_hold_and_returns_on_the_first_busy_frame() {
        let mut pace = ServerPace::new(60.0, 5.0, None);
        assert!(!pace.observe(0.0, true));
        assert_eq!(pace.hz(), 60.0);
        assert!(!pace.observe(JOIN_HOLD_SECS - 0.1, false), "still inside the hold");
        assert_eq!(pace.hz(), 60.0);
        assert!(pace.observe(JOIN_HOLD_SECS, false), "the hold ran out");
        assert_eq!(pace.hz(), 5.0);
        assert_eq!(pace.period(), Duration::from_millis(200));
        assert!(pace.observe(100.0, true), "a join is seen");
        assert_eq!(pace.hz(), 60.0, "the very next wait is a full-rate one");
        // An idle rate equal to the full rate never switches.
        let mut flat = ServerPace::new(60.0, 60.0, None);
        assert!(!flat.observe(1000.0, false));
        assert!(!flat.idle);
    }

    #[test]
    fn an_empty_server_leaves_only_when_asked_and_only_after_the_wait() {
        let mut never = ServerPace::new(60.0, 5.0, None);
        never.observe(0.0, true);
        assert!(!never.empty_exit_due(1.0e6));
        let mut fleet = ServerPace::new(60.0, 5.0, Some(300.0));
        fleet.observe(10.0, true);
        assert!(!fleet.empty_exit_due(309.9));
        assert!(fleet.empty_exit_due(310.0));
        fleet.observe(400.0, true);
        assert!(!fleet.empty_exit_due(500.0), "someone came back; the wait starts again");
    }
}
