//! # eustress-headless — run a Space's simulation with no window
//!
//! The "Simulate" tier of HEADLESS_RUNTIME.md (§6): loads a `.eustress`
//! Space, ticks the REAL simulation stack (Avian physics + realism +
//! Rune/Luau scripts + WorldDb persistence), and advertises the Engine
//! Bridge on `<universe>/.eustress/engine.port` — so the MCP server and
//! the `eustress` CLI can drive it exactly like a running editor. No
//! winit, no GPU, no Slint.
//!
//! ```text
//! eustress-headless --space <dir> [--ticks N] [--tick-rate HZ]
//!                   [--no-autoplay] [--autoplay-delay-frames N]
//! eustress-headless --universe <dir> ...   # first Space inside
//! ```
//!
//! With `--ticks N` this is a batch runner: enter Play, run exactly N
//! deterministic sim ticks (60 Hz fixed timestep), stop (which fires the
//! standard recording export in `simulation::plugin::on_play_stop`), and
//! exit 0. Without it, the process runs until killed — a headless
//! dedicated simulator you drive over the bridge.
//!
//! Known v1 limits (HEADLESS_RUNTIME.md §10): no gltf loader is
//! registered, so glb-backed custom meshes don't decode (bare parts,
//! physics, scripts, sim values, and the bridge all work); `ai_camera` /
//! `viewport.capture` need the future `--render gpu` tier.

use std::path::PathBuf;
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;

use eustress_common::simulation::SimulationClock;
use eustress_engine::app_core;
use eustress_engine::play_mode::PlayModeState;
use eustress_engine::space;

const USAGE: &str = "\
eustress-headless — run a Space's simulation with no window

USAGE:
    eustress-headless --space <dir> [OPTIONS]
    eustress-headless --universe <dir> [OPTIONS]   (first Space inside)

OPTIONS:
    --space <dir>                A Space directory (holds Workspace/, world.fjalldb/, ...)
    --universe <dir>             A Universe directory; opens its first Space
    --ticks <N>                  Stop after N sim ticks (60 Hz fixed), export recording, exit
    --tick-rate <HZ>             Main-loop rate (default 60; sim fixed-step stays 60 Hz)
    --no-autoplay                Boot into Edit state; wait for run_simulation via bridge/MCP
    --autoplay-delay-frames <N>  Frames to wait before entering Play so the Space
                                 finishes loading (default 120 ≈ 2 s at 60 Hz)
    -h, --help                   Show this help";

// ─────────────────────────────────────────────────────────────────────────────
// Args — hand-rolled (the engine crate carries no clap; same policy as
// eustress-space)
// ─────────────────────────────────────────────────────────────────────────────

struct HeadlessArgs {
    space: PathBuf,
    ticks: Option<u64>,
    tick_rate: f64,
    autoplay: bool,
    autoplay_delay_frames: u64,
}

fn parse_args() -> Result<HeadlessArgs, String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut space: Option<PathBuf> = None;
    let mut universe: Option<PathBuf> = None;
    let mut ticks: Option<u64> = None;
    let mut tick_rate: f64 = 60.0;
    let mut autoplay = true;
    let mut autoplay_delay_frames: u64 = 120;

    let mut i = 0;
    while i < argv.len() {
        let take_value = |i: &mut usize| -> Result<String, String> {
            *i += 1;
            argv.get(*i)
                .cloned()
                .ok_or_else(|| format!("{} requires a value\n\n{USAGE}", argv[*i - 1]))
        };
        match argv[i].as_str() {
            "--space" => space = Some(PathBuf::from(take_value(&mut i)?)),
            "--universe" => universe = Some(PathBuf::from(take_value(&mut i)?)),
            "--ticks" => {
                ticks = Some(take_value(&mut i)?.parse::<u64>()
                    .map_err(|e| format!("--ticks: {e}"))?)
            }
            "--tick-rate" => {
                tick_rate = take_value(&mut i)?.parse::<f64>()
                    .map_err(|e| format!("--tick-rate: {e}"))?;
                if !(tick_rate > 0.0) {
                    return Err("--tick-rate must be > 0".into());
                }
            }
            "--no-autoplay" => autoplay = false,
            "--autoplay-delay-frames" => {
                autoplay_delay_frames = take_value(&mut i)?.parse::<u64>()
                    .map_err(|e| format!("--autoplay-delay-frames: {e}"))?
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument '{other}'\n\n{USAGE}")),
        }
        i += 1;
    }

    // Resolve --universe → its first Space (same helper the editor's
    // --universe flag uses).
    let space = match (space, universe) {
        (Some(s), _) => s,
        (None, Some(u)) => space::first_space_root_in_universe(&u)
            .ok_or_else(|| format!("--universe {}: no Space found inside", u.display()))?,
        (None, None) => return Err(format!("--space or --universe is required\n\n{USAGE}")),
    };
    if !space.is_dir() {
        return Err(format!("Space directory not found: {}", space.display()));
    }

    Ok(HeadlessArgs { space, ticks, tick_rate, autoplay, autoplay_delay_frames })
}

// ─────────────────────────────────────────────────────────────────────────────
// Run driver — autoplay, tick limit, drain, exit
// ─────────────────────────────────────────────────────────────────────────────

/// Lifecycle of a headless run. Editing at boot (Space loads while physics
/// is paused) → Playing (autoplay or bridge command) → Editing again when
/// the tick target is hit (fires the standard recording export + world
/// restore) → a short drain so those OnEnter(Editing) systems and deferred
/// commands complete → AppExit.
#[derive(Resource)]
struct HeadlessRun {
    ticks: Option<u64>,
    autoplay: bool,
    autoplay_delay_frames: u64,
    frames_seen: u64,
    started: bool,
    stop_issued: bool,
    drain_frames_left: u8,
}

fn headless_run_driver(
    mut run: ResMut<HeadlessRun>,
    clock: Res<SimulationClock>,
    state: Res<State<PlayModeState>>,
    mut next_state: ResMut<NextState<PlayModeState>>,
    mut exit: MessageWriter<AppExit>,
) {
    run.frames_seen += 1;

    match state.get() {
        PlayModeState::Editing if !run.started => {
            if run.autoplay && run.frames_seen >= run.autoplay_delay_frames {
                info!(
                    "▶ headless: entering Play after {}-frame load window{}",
                    run.frames_seen,
                    match run.ticks {
                        Some(n) => format!(" (will stop after {n} sim ticks)"),
                        None => " (running until killed or stopped via bridge)".to_string(),
                    }
                );
                run.started = true;
                next_state.set(PlayModeState::Playing);
            }
            // Without autoplay we idle in Editing; run_simulation via the
            // MCP sim-command drain or the bridge flips the state, and the
            // Playing arm below takes over from there.
        }
        PlayModeState::Playing => {
            run.started = true;
            if let Some(target) = run.ticks {
                if clock.tick_count >= target {
                    info!(
                        "⏹ headless: tick target reached ({} ≥ {target}) — stopping (export fires on enter-Edit)",
                        clock.tick_count
                    );
                    run.stop_issued = true;
                    next_state.set(PlayModeState::Editing);
                }
            }
        }
        PlayModeState::Editing if run.stop_issued => {
            // OnEnter(Editing) systems (recording export, safety-net world
            // restore) ran when the transition applied; give deferred
            // commands a couple of frames to flush, then exit cleanly.
            if run.drain_frames_left == 0 {
                info!("✅ headless: run complete — exiting");
                exit.write(AppExit::Success);
            } else {
                run.drain_frames_left -= 1;
            }
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Main
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("error: {msg}");
            std::process::exit(2);
        }
    };

    println!("Starting Eustress Headless — space: {}", args.space.display());

    let mut app = App::new();
    app.set_error_handler(app_core::rate_limited_error_handler);

    // Asset sources MUST be registered before AssetPlugin (added below).
    app_core::register_asset_sources(&mut app, &args.space);

    // Base shell: schedule runner instead of winit, plus the minimal
    // infrastructure the core tier assumes from the editor's
    // DefaultPlugins — logging, transforms, states, assets, scenes, input
    // message/resource registration (no window feeds it; scripts polling
    // UserInputService just see nothing pressed).
    app.add_plugins((
        bevy::log::LogPlugin::default(),
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / args.tick_rate,
        ))),
        bevy::transform::TransformPlugin,
        bevy::diagnostic::DiagnosticsPlugin,
        bevy::state::app::StatesPlugin,
        bevy::input::InputPlugin,
        AssetPlugin {
            file_path: "assets".to_string(),
            // Match the windowed shell: allow absolute-path asset loads so
            // user-imported Universe assets (images / Gaussian-splat clouds,
            // which live above the `space://` root and are addressed
            // absolutely) load in headless runs too, instead of being
            // rejected as "unapproved". See the windowed AssetPlugin in
            // `main.rs` for the full rationale.
            unapproved_path_mode: bevy::asset::UnapprovedPathMode::Allow,
            ..default()
        },
        bevy::scene::ScenePlugin,
    ));

    // Render-asset containers WITHOUT the render pipeline: spawn paths
    // insert Mesh3d / MeshMaterial3d handles, so `Assets<T>` must exist or
    // those systems fail param validation and silently skip. The assets
    // live CPU-side and are simply never uploaded.
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_asset::<Image>();
    app.init_asset::<AnimationClip>();
    // Character animation graphs (SharedAnimationPlugin's
    // create_animation_graphs skips without the container).
    app.init_asset::<AnimationGraph>();
    app.init_asset::<bevy::audio::AudioSource>();
    // Decal material — the space file-loader systems (spawn.rs decal path)
    // take ResMut<Assets<ForwardDecalMaterial<StandardMaterial>>>; without
    // this they fail param validation and the ENTIRE disk spawn path
    // silently skips (observed on first headless bring-up).
    app.init_asset::<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>();

    // Point the Space loader at the requested Space (mirrors the editor's
    // --space override; init_resource inside the core is then a no-op).
    app.insert_resource(space::SpaceRoot(args.space.clone()));

    // The shared core simulation tier — identical to the editor's.
    app_core::add_core_sim_plugins(&mut app, &args.space);

    // The run driver (autoplay → tick limit → drain → exit).
    app.insert_resource(HeadlessRun {
        ticks: args.ticks,
        autoplay: args.autoplay,
        autoplay_delay_frames: args.autoplay_delay_frames,
        frames_seen: 0,
        started: false,
        stop_issued: false,
        drain_frames_left: 4,
    });
    app.add_systems(Update, headless_run_driver);

    app.run();
    println!("✅ Eustress Headless closed gracefully");
}
