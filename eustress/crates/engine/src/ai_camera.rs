//! # Independent AI camera
//!
//! A second, **off-screen** camera the AI drives and captures so it can "see
//! what it's doing" without displacing the user's viewport.
//!
//! ## Why it can't displace the real camera
//!
//! A Bevy camera renders to a target. The user's editor camera targets the
//! **window**; this camera carries a `RenderTarget::Image` component pointing
//! at an off-screen GPU texture that's never shown. They render in parallel to
//! different destinations, so adding this camera never changes who owns the
//! window. The AI "sees" by reading that image back to a PNG, the same
//! `Screenshot` readback `viewport.capture` uses for the window, pointed at the
//! image instead.
//!
//! ## Powered down between captures
//!
//! It is modeled on the **working editor camera** (camera_controller.rs): a
//! plain `Camera3d` built from the same bundle. It is inactive while idle and
//! powered up only for the frames of a capture ([`process_ai_capture`]).
//!
//! ## One camera, in every Space
//!
//! It is a runtime service of the engine, not part of any Space: nothing in a
//! Space's files defines it, and a save never writes it (it has no
//! `BasePart`). There is always exactly one, named "AI Camera".
//! [`ensure_ai_camera_exists`] puts it back at its starting pose whenever none
//! exists, which covers every way one goes away: opening, reverting or
//! creating a Space (`space_ops::open_space` despawns every `Instance`), Stop
//! after Play, and a delete in the Explorer. A bridge call puts it back at
//! once ([`ensure_in_world`]) rather than wait a frame. The image it renders
//! into is made once and survives all of these.
//!
//! ## Explorer object
//!
//! It carries the same `classes::Instance { class_name: Camera }` the editor
//! camera carries, so the Explorer shows it as a Camera named "AI Camera". It
//! is an **independent root entity**, deliberately NOT a Bevy `ChildOf` the
//! editor camera, because childing would transform-couple them and destroy the
//! independence that is the whole point.

use bevy::prelude::*;
use bevy::camera::RenderTarget;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use std::path::PathBuf;

/// Off-screen render resolution for the AI camera.
pub const AI_CAM_WIDTH: u32 = 1280;
pub const AI_CAM_HEIGHT: u32 = 720;

/// The AI camera's name, in the Explorer and to every tool.
pub const AI_CAMERA_NAME: &str = "AI Camera";

/// Why a request for the AI camera fails in an engine with no renderer.
pub const UNAVAILABLE: &str = "the AI camera needs Studio's renderer, and this engine has none \
     (eustress-headless runs without a window or GPU). Open the Space in Studio to aim or capture \
     the AI camera.";

/// Where the AI camera starts, and where it is put back.
pub fn start_pose() -> Transform {
    Transform::from_xyz(12.0, 12.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y)
}

/// Marker for the AI camera entity.
#[derive(Component)]
pub struct AiCamera;

/// A queued capture: the output path and the exact size to render it at.
pub struct PendingCapture {
    pub path: PathBuf,
    /// Requested render width in pixels (never 0).
    pub width: u32,
    /// Requested render height in pixels (never 0).
    pub height: u32,
}

/// Largest side accepted for a capture. This is a sanity bound against a
/// typo allocating an absurd texture, not a resolution cap: it is the
/// 16384-pixel `max_texture_dimension_2d` every desktop wgpu adapter reports,
/// and the bridge handler rejects a request beyond it with a message instead
/// of letting wgpu abort.
pub const MAX_CAPTURE_SIDE: u32 = 16384;

/// Frames to let the AI camera render before screenshotting it.
///
/// The camera is powered down between captures (see [`process_ai_capture`]),
/// so its render target holds a stale frame, or none at all on the first
/// capture, at the moment a request arrives. The render graph needs a couple
/// of frames to produce a current image before the readback is meaningful.
const CAPTURE_WARMUP_FRAMES: u32 = 6;

/// Frames to keep the camera POWERED UP after the screenshot is queued.
///
/// This is the fix for captures coming back as a uniform image of the
/// `Image::new_fill` colour, a readback of a target nothing ever rendered
/// into. Powering down in the SAME frame the `Screenshot` spawned meant the
/// readback was serviced on a frame where no active camera targeted that
/// image. Bevy only prepares a view target for ACTIVE cameras, so the copy ran
/// against a texture that was never prepared.
///
/// Holding the camera on for a few extra frames lets the render graph actually
/// service the copy. The camera still powers down immediately afterwards, so
/// the "off-screen camera costs nothing while idle" property is preserved.
const CAPTURE_COOLDOWN_FRAMES: u32 = 4;

/// Off-screen render-target handle + pending-capture state for the AI camera.
#[derive(Resource, Default)]
pub struct AiCameraState {
    pub image: Option<Handle<Image>>,
    pub pending: Option<PendingCapture>,
    /// Frames the camera has rendered for the current request; 0 when idle.
    /// See [`CAPTURE_WARMUP_FRAMES`].
    warmup: u32,
    /// Frames remaining to keep the camera live AFTER a screenshot was queued,
    /// so the render graph can service the readback. See
    /// [`CAPTURE_COOLDOWN_FRAMES`].
    cooldown: u32,
}

pub struct AiCameraPlugin;

impl Plugin for AiCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AiCameraState>()
            .add_systems(Startup, (create_ai_camera_target, cpu_light_clustering))
            // The camera carries `NoAtmosphere` (see `ai_camera_bundle`), so it
            // opts OUT of SharedLightingPlugin's atmosphere + skybox attach,
            // which avoids the two-camera atmosphere panic (see the marker
            // comment there). Its sky is the editor view's, copied without the
            // atmosphere by `raytracing::sky::mirror_sky_to_capture_cameras`.
            .add_systems(Update, (ensure_ai_camera_exists, process_ai_capture).chain());
        // Tick-indexed captures (G1.06). Armed by `EUSTRESS_TICK_CAPTURE` or by
        // a bridge request that names `ticks`; without either there is no
        // `TickCaptureQueue` and neither system runs. The trigger is in
        // `FixedUpdate`, never `Update`: see `drive_tick_captures`.
        if let Some(queue) = TickCaptureQueue::from_env() {
            app.insert_resource(queue);
        }
        app.add_systems(FixedUpdate, drive_tick_captures.run_if(resource_exists::<TickCaptureQueue>))
            .add_systems(Last, tick_capture_frame_pacing.run_if(resource_exists::<TickCaptureQueue>));
    }
}

/// The environment switch that keeps Bevy's GPU light clustering.
const ENV_GPU_CLUSTERING: &str = "EUSTRESS_GPU_CLUSTERING";

/// Whether `value` (the switch's text, when it is set) asks for GPU clustering.
fn gpu_clustering_requested(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

/// Cluster lights on the CPU, so a static scene renders bit for bit the same
/// frame after frame and two captures from one pose are identical.
///
/// Bevy's GPU clustering fills each cluster's light list through atomics, in
/// whatever order the GPU's threads arrive. The shader then sums the lights
/// in list order, and a floating-point sum depends on its order, so where
/// lights overlap a few pixels land one unit apart from one frame to the
/// next. The CPU path builds the lists in a stable order. Clustering is one
/// setting for the whole renderer, so this covers every camera.
/// `EUSTRESS_GPU_CLUSTERING=1` keeps the GPU path, for a Space with so many
/// lights that clustering cost matters more than exact repeatability.
fn cpu_light_clustering(settings: Option<ResMut<bevy::light::cluster::GlobalClusterSettings>>) {
    if gpu_clustering_requested(std::env::var(ENV_GPU_CLUSTERING).ok().as_deref()) {
        return;
    }
    if let Some(mut settings) = settings {
        if settings.gpu_clustering.take().is_some() {
            tracing::info!("Light clustering runs on the CPU ({ENV_GPU_CLUSTERING}=1 keeps the GPU path)");
        }
    }
}

/// Make the off-screen image the AI camera renders into. Made once: every
/// camera [`ensure_ai_camera_exists`] puts back renders into the same image.
fn create_ai_camera_target(mut images: ResMut<Assets<Image>>, mut state: ResMut<AiCameraState>) {
    let size = Extent3d {
        width: AI_CAM_WIDTH,
        height: AI_CAM_HEIGHT,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // RENDER_ATTACHMENT so the camera can draw into it; COPY_SRC so the
    // screenshot readback can copy it out.
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_SRC
        | TextureUsages::RENDER_ATTACHMENT;
    state.image = Some(images.add(image));
}

/// The AI camera at its starting pose, rendering into `image`. Built from the
/// SAME `studio_camera_bundle` as the editor camera (identical projection,
/// tonemap, depth prepass). It differs in: render target (an off-screen Image,
/// not the window), the `AiCamera` marker, `Camera { order: -1 }` (so editor
/// tools' `order == 0` lookups never grab it), and, critically,
/// `NoAtmosphere`, so it is NOT a second atmosphere camera.
fn ai_camera_bundle(image: Handle<Image>) -> impl Bundle {
    (
        // SAME method as the editor camera (`studio_camera_bundle`): identical
        // by construction, so `SharedLightingPlugin` attaches the same
        // skybox/atmosphere/env-map to both and the shared mesh-view bind-group
        // layout matches. The ONLY intended differences are the off-screen
        // render target and the `AiCamera` marker.
        crate::default_scene::studio_camera_bundle(AI_CAMERA_NAME, start_pose()),
        // Distinct render order so the editor tools' `find(camera.order == 0)`
        // never grab THIS off-screen camera. Both the editor and AI camera come
        // from `studio_camera_bundle`, which defaults to order 0; that collision
        // made select/move/scale/rotate cast their picking ray from the AI
        // camera's pose. -1 renders this image pass before the window camera.
        // `is_active: false`: the camera is powered up only for the few frames
        // of an actual capture (see `process_ai_capture`). Bevy's default is
        // `true`, which had this off-screen 1280x720 camera rendering the whole
        // scene every frame for the entire session.
        Camera { order: -1, is_active: false, ..default() },
        AiCamera,
        // No `RenderLayers`, so it renders layer 0 only: the scene, and none of
        // the editor overlays on `editor_overlay::EDITOR_OVERLAY_LAYER` (hover
        // and selection wireframes, handles, gizmo lines). Those depend on the
        // mouse and the selection, so a capture that drew them would change
        // with them. It gets no `EustressCamera` (see `setup_camera_controller`),
        // the component that gives a camera the overlay layer.
        // Off-screen: render to our image, never the window, so it can't
        // displace the editor camera. MSAA is Off (inherited from
        // `studio_camera_bundle`, required by TAA); the image is single-sampled
        // and TAA does the anti-aliasing.
        RenderTarget::Image(image.into()),
        // ── AI-camera atmosphere opt-out (REQUIRED: fixes a hard wgpu panic) ──
        // Bevy 0.19's atmosphere serves one camera. `write_atmosphere_buffer`
        // takes `single()` over the cameras with an atmosphere (active or
        // not) and writes nothing when there are two, so the atmosphere
        // buffer has no binding, the view bind group leaves its three entries
        // out, and the pipelines that expect them make wgpu abort ("bind group
        // descriptor (21) != layout (24)"). `NoAtmosphere` keeps the EDITOR as
        // the single atmosphere camera. It also makes SharedLightingPlugin
        // skip the sky attach for this camera; `raytracing::sky` gives it the
        // editor's sky instead (a Skybox of the live sky cubemap and the
        // filtered environment map), neither of which is an atmosphere.
        eustress_common::plugins::lighting_plugin::NoAtmosphere,
    )
}

/// Keep exactly one AI camera: put it back, at its starting pose and on the
/// same image, whenever none exists. The editor camera's
/// `ensure_camera_exists` does the same for the window camera; this is the
/// one place the AI camera is spawned.
fn ensure_ai_camera_exists(
    mut commands: Commands,
    state: Res<AiCameraState>,
    cameras: Query<(), With<AiCamera>>,
) {
    if !cameras.is_empty() {
        return;
    }
    let Some(image) = state.image.clone() else {
        return;
    };
    commands.spawn(ai_camera_bundle(image));
    info!("📷 AI Camera ready (off-screen {AI_CAM_WIDTH}x{AI_CAM_HEIGHT}, independent of the editor camera)");
}

/// The AI camera, put back now when none exists, for a caller that holds the
/// World (a bridge handler) and cannot wait for [`ensure_ai_camera_exists`]'s
/// next frame. `Err` says why there can be none: an engine without a renderer.
pub fn ensure_in_world(world: &mut World) -> Result<Entity, String> {
    let image = match world.get_resource::<AiCameraState>() {
        None => return Err(UNAVAILABLE.to_string()),
        Some(state) => state.image.clone(),
    };
    let existing = {
        let mut cameras = world.query_filtered::<Entity, With<AiCamera>>();
        cameras.iter(world).next()
    };
    if let Some(camera) = existing {
        return Ok(camera);
    }
    let Some(image) = image else {
        return Err("the AI camera's render target is not ready yet; try again in a moment".to_string());
    };
    Ok(world.spawn(ai_camera_bundle(image)).id())
}

/// Queue a capture to `path` at the default size ([`AI_CAM_WIDTH`] x
/// [`AI_CAM_HEIGHT`]). A caller that names no size always gets the default, so
/// a capture after a larger one is not silently larger.
pub fn request_capture(state: &mut AiCameraState, path: PathBuf) {
    request_capture_sized(state, path, AI_CAM_WIDTH, AI_CAM_HEIGHT);
}

/// Queue a capture to `path`, rendered at exactly `width` x `height`. The
/// render target is resized once, on this request, and only when its current
/// size differs; the caller has already checked the sides are in
/// `1..=MAX_CAPTURE_SIDE`.
pub fn request_capture_sized(state: &mut AiCameraState, path: PathBuf, width: u32, height: u32) {
    state.pending = Some(PendingCapture { path, width: width.max(1), height: height.max(1) });
}

// ─────────────────────────────────────────────────────────────────────────────
// Tick-indexed captures (G1.06, `docs/PROMPTS/02_CAPTURE_HARNESS.md` invariant I4)
// ─────────────────────────────────────────────────────────────────────────────
//
// A capture can name a simulation tick, and the frame emitted is the frame of
// that tick, not of a wall-clock instant and not of a frame count.
//
// ## Which index "tick" means
//
// The tick index is the number of PHYSICS STEPS executed since the current Play
// run began: `PhysicsStepCounter::executed()` minus the counter's value when the
// run opened (`StepCountRun::start`, set on entering Play). Tick 0 is the
// world as Play opened it, before any step; tick N is the world after N steps.
//
// Why not `SimulationClock::tick_count`: its `advance` runs once per rendered
// FRAME in `PreUpdate` from the frame's wall delta, so on a slow machine it
// jumps by several ticks between two observations (59 then 61 skips 60), and it
// is zeroed under load (`clock.rs`, the `G1.07` finding). A tick that can be
// skipped cannot be captured exactly. The physics step counter moves by exactly
// one per `PhysicsSchedule` run, and a `FixedUpdate` system sees every value of
// it once, because `FixedUpdate` runs before `FixedPostUpdate` (where Avian
// steps) in each fixed iteration. The clock's `tick_count` is still recorded in
// the log beside it, so a disagreement is visible.
//
// ## The frame of that tick
//
// The trigger spawns the screenshot in the `FixedUpdate` run that observes the
// target step. A screenshot is read back several frames later, so the physics
// clock is HELD (`Time<Physics>` paused) from the moment of the trigger until
// the PNG is saved; the world the readback sees is the world of the tick. The
// step counter is read again when the save is observed and logged as
// `settled`; `settled - fired` is the readback offset and must be 0.
//
// ## Refusals
//
// Under `sim.time_scale != 1.0` the capture is refused with a named error and
// no frame is emitted. A tick that has already passed is a refusal too
// ("missed"), never relabelled to the nearest frame.

use crate::play_mode::{PlayModeState, PlayModeType, StartPlayEvent};
use crate::simulation::plugin::{hold_physics, StepCountRun};
use avian3d::prelude::{Physics, PhysicsTime, SubstepCount};
use bevy::app::AppExit;
use eustress_common::simulation::{PhysicsStepCounter, SimulationClock};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Comma-separated tick list (`0,60,300,600`); arms a run from the environment.
pub const ENV_TICKS: &str = "EUSTRESS_TICK_CAPTURE";
/// Folder the PNGs and the default log are written to.
pub const ENV_TICK_DIR: &str = "EUSTRESS_TICK_CAPTURE_DIR";
/// `WxH` render size (default 1280x720).
pub const ENV_TICK_SIZE: &str = "EUSTRESS_TICK_CAPTURE_SIZE";
/// Where the log is written (default `<dir>/tick_capture_log.json`).
pub const ENV_TICK_LOG: &str = "EUSTRESS_TICK_CAPTURE_LOG";
/// `1` exits the engine when the run is finished: code 0 on success, 1 otherwise.
pub const ENV_TICK_EXIT: &str = "EUSTRESS_TICK_CAPTURE_EXIT";
/// Frames per second the render loop is held to (a measurement aid that makes
/// the wall-clock to tick relationship differ; never a trigger).
pub const ENV_TICK_FRAME_CAP: &str = "EUSTRESS_TICK_CAPTURE_FRAME_CAP";
/// A `sim.time_scale` to set when the run is armed. Exists so a harness can
/// prove the refusal; `1.0` or unset leaves the clock alone.
pub const ENV_TICK_TIME_SCALE: &str = "EUSTRESS_TICK_CAPTURE_TIME_SCALE";

/// The only `sim.time_scale` a tick capture runs under.
pub const REQUIRED_TIME_SCALE: f64 = 1.0;
/// The log schema.
pub const TICK_LOG_SCHEMA: &str = "eustress.tick_capture_log/1";
/// CONFIG DEFAULT: `FixedUpdate` runs the Space must be loaded and the camera
/// present before an env-armed run starts Play (2 s at 60 Hz). A readiness
/// wait, not a trigger.
pub const TICK_SETTLE_RUNS: u32 = 120;
/// CONFIG DEFAULT: `FixedUpdate` runs the camera must have been active before a
/// capture is taken (it renders into a target that was just resized or powered
/// up, see [`CAPTURE_WARMUP_FRAMES`]). Physics is held while it waits.
pub const TICK_WARM_RUNS: u32 = 30;
/// CONFIG DEFAULT: seconds a readback may take before the run is failed.
pub const TICK_READBACK_TIMEOUT_S: u64 = 120;
/// CONFIG DEFAULT: seconds a whole run may take before it is failed.
pub const TICK_RUN_TIMEOUT_S: u64 = 600;

/// `0,60,300,600` as ticks. Strictly increasing: a list that is out of order or
/// repeats a tick is refused whole, never sorted or thinned (that would change
/// what was asked for).
pub fn parse_ticks(text: &str) -> Result<Vec<u64>, String> {
    let mut ticks = Vec::new();
    for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let t: u64 = item.parse().map_err(|_| format!("bad tick {item:?}; a tick is a whole number"))?;
        if let Some(&last) = ticks.last() {
            if t <= last {
                return Err(format!("ticks must be strictly increasing; {t} follows {last}"));
            }
        }
        ticks.push(t);
    }
    if ticks.is_empty() {
        return Err("the tick list is empty".to_string());
    }
    Ok(ticks)
}

/// The file name of the frame for `tick`: the tick is in the name.
pub fn tick_file_name(stem: &str, tick: u64) -> String {
    format!("{stem}_t{tick:06}.png")
}

/// The named error for a capture asked for under `scale`.
pub fn refusal_message(scale: f64) -> String {
    format!("capture refused: sim.time_scale={scale:?}, required {REQUIRED_TIME_SCALE:?}")
}

/// What one `FixedUpdate` run does about the queue.
#[derive(Clone, Debug, PartialEq)]
pub enum TickDecision {
    /// Nothing to do yet.
    Wait,
    /// The run is at the next requested tick; carries the step just observed.
    Fire(u64),
    /// The run is already past the next requested tick.
    Missed { tick: u64, at: u64 },
    /// `sim.time_scale` is not 1.0 while captures are outstanding.
    Refuse(String),
    /// Every tick was requested and every readback settled.
    Done,
}

/// The whole trigger decision as a pure function of the counters, so it is
/// tested without an engine. `run_step` is `None` while no Play run is open.
pub fn decide_tick(
    requested: &[u64],
    next: usize,
    in_flight: bool,
    run_step: Option<u64>,
    time_scale: f64,
) -> TickDecision {
    if next >= requested.len() && !in_flight {
        return TickDecision::Done;
    }
    if time_scale != REQUIRED_TIME_SCALE {
        return TickDecision::Refuse(refusal_message(time_scale));
    }
    if in_flight || next >= requested.len() {
        return TickDecision::Wait;
    }
    let Some(step) = run_step else {
        return TickDecision::Wait;
    };
    let want = requested[next];
    if step == want {
        TickDecision::Fire(step)
    } else if step > want {
        TickDecision::Missed { tick: want, at: step }
    } else {
        TickDecision::Wait
    }
}

/// One emitted (or attempted) frame.
#[derive(Clone, Debug)]
pub struct TickCaptureRecord {
    /// The tick the request named.
    pub requested: u64,
    /// The run step read at the moment the screenshot was spawned.
    pub fired: u64,
    /// The run step read when the saved PNG was observed; `None` until then.
    pub settled: Option<u64>,
    /// `PhysicsStepCounter::executed()` at fire time (not run-relative).
    pub physics_steps_total: u64,
    /// `SimulationClock::tick_count` at fire time, for comparison.
    pub clock_tick_count: u64,
    /// `FixedUpdate` runs since the queue was armed, at fire time.
    pub fixed_runs: u64,
    pub file: PathBuf,
    pub saved: Option<bool>,
    pub save_error: Option<String>,
    /// Whether the camera had been active for [`TICK_WARM_RUNS`] when fired.
    pub warm: bool,
    pub wall_s: f64,
}

type Settled = Arc<Mutex<Vec<(usize, Result<(), String>)>>>;

/// The tick-scheduled capture queue. Present only while a tick run is armed.
#[derive(Resource)]
pub struct TickCaptureQueue {
    requested: Vec<u64>,
    next: usize,
    dir: PathBuf,
    stem: String,
    width: u32,
    height: u32,
    /// Pause `Time<Physics>` from the trigger until the PNG is saved.
    hold_physics: bool,
    exit_when_done: bool,
    log_path: PathBuf,
    frame_cap: Option<f64>,
    time_scale_override: Option<f64>,
    /// An env-armed run opens Play itself; a bridge-armed run waits for one.
    start_play: bool,
    records: Vec<TickCaptureRecord>,
    in_flight: Option<usize>,
    in_flight_since: Option<Instant>,
    settled: Settled,
    held: bool,
    armed: bool,
    armed_at: Option<Instant>,
    resized: bool,
    settle_runs: u32,
    camera_active_runs: u32,
    play_started: bool,
    fixed_runs: u64,
    frames_rendered: u64,
    last_frame_end: Option<Instant>,
    time_scale_at_arm: f64,
    refused: Option<String>,
    missed: Vec<u64>,
    error: Option<String>,
    written: bool,
}

impl TickCaptureQueue {
    /// A queue for `requested` (already validated by [`parse_ticks`]).
    pub fn new(requested: Vec<u64>, dir: PathBuf, stem: &str, width: u32, height: u32) -> Self {
        let log_path = dir.join(format!("{stem}_tick_log.json"));
        Self {
            requested,
            next: 0,
            dir,
            stem: stem.to_string(),
            width: width.max(1),
            height: height.max(1),
            hold_physics: true,
            exit_when_done: false,
            log_path,
            frame_cap: None,
            time_scale_override: None,
            start_play: false,
            records: Vec::new(),
            in_flight: None,
            in_flight_since: None,
            settled: Arc::new(Mutex::new(Vec::new())),
            held: false,
            armed: false,
            armed_at: None,
            resized: false,
            settle_runs: 0,
            camera_active_runs: 0,
            play_started: false,
            fixed_runs: 0,
            frames_rendered: 0,
            last_frame_end: None,
            time_scale_at_arm: REQUIRED_TIME_SCALE,
            refused: None,
            missed: Vec::new(),
            error: None,
            written: false,
        }
    }

    /// A queue that fails at once with `message` and exits non-zero, so a bad
    /// environment is a loud failure and not a silently idle run.
    fn invalid(message: String, dir: PathBuf) -> Self {
        let mut q = Self::new(Vec::new(), dir, "tick", AI_CAM_WIDTH, AI_CAM_HEIGHT);
        q.error = Some(message);
        q.exit_when_done = true;
        q
    }

    /// Armed from the environment, or `None` when `EUSTRESS_TICK_CAPTURE` is unset.
    pub fn from_env() -> Option<Self> {
        let get = |k: &str| std::env::var(k).ok().filter(|s| !s.trim().is_empty());
        let ticks_text = get(ENV_TICKS)?;
        let dir = get(ENV_TICK_DIR)
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("eustress-tick-captures"));
        let exit = get(ENV_TICK_EXIT).map(|v| v != "0").unwrap_or(false);
        let size = match get(ENV_TICK_SIZE) {
            None => Ok((AI_CAM_WIDTH, AI_CAM_HEIGHT)),
            Some(s) => s
                .split_once(['x', 'X'])
                .and_then(|(w, h)| Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?)))
                .filter(|(w, h)| (1..=MAX_CAPTURE_SIDE).contains(w) && (1..=MAX_CAPTURE_SIDE).contains(h))
                .ok_or_else(|| format!("{ENV_TICK_SIZE} must be WxH within 1..={MAX_CAPTURE_SIDE}, got {s:?}")),
        };
        let mut q = match (parse_ticks(&ticks_text), size) {
            (Ok(ticks), Ok((w, h))) => Self::new(ticks, dir.clone(), "tick", w, h),
            (Err(e), _) | (_, Err(e)) => {
                tracing::error!("tick capture not armed: {e}");
                Self::invalid(e, dir.clone())
            }
        };
        q.exit_when_done = exit || q.error.is_some();
        q.start_play = true;
        if let Some(p) = get(ENV_TICK_LOG) {
            q.log_path = PathBuf::from(p);
        }
        q.frame_cap = get(ENV_TICK_FRAME_CAP).and_then(|s| s.parse::<f64>().ok()).filter(|f| *f > 0.0);
        q.time_scale_override = get(ENV_TICK_TIME_SCALE).and_then(|s| s.parse::<f64>().ok());
        Some(q)
    }

    /// Whether the run succeeded: every tick emitted at exactly its index.
    pub fn succeeded(&self) -> bool {
        self.error.is_none()
            && self.refused.is_none()
            && self.missed.is_empty()
            && self.records.len() == self.requested.len()
            && self.records.iter().all(|r| {
                r.saved == Some(true) && r.fired == r.requested && r.settled == Some(r.fired)
            })
    }

    /// The ticks of the frames that were saved, in the order they fired.
    pub fn emitted_ticks(&self) -> Vec<u64> {
        self.records.iter().filter(|r| r.saved == Some(true)).map(|r| r.fired).collect()
    }

    /// The log as JSON.
    pub fn log_json(&self, fixed_timestep_s: f64, substeps: Option<u64>) -> serde_json::Value {
        let wall_s = self.armed_at.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
        let rows: Vec<serde_json::Value> = self
            .records
            .iter()
            .map(|r| {
                serde_json::json!({
                    "requested_tick": r.requested,
                    "fired_tick": r.fired,
                    "settled_tick": r.settled,
                    "readback_offset_steps": r.settled.map(|s| s as i64 - r.fired as i64),
                    "physics_steps_total": r.physics_steps_total,
                    "clock_tick_count": r.clock_tick_count,
                    "fixed_runs_since_armed": r.fixed_runs,
                    "file": r.file.to_string_lossy(),
                    "saved": r.saved,
                    "save_error": r.save_error,
                    "camera_warm": r.warm,
                    "wall_s": r.wall_s,
                })
            })
            .collect();
        serde_json::json!({
            "schema": TICK_LOG_SCHEMA,
            "tick_source": "physics steps executed since the Play run began (PhysicsStepCounter minus StepCountRun.start)",
            "ticks_requested": self.requested,
            "ticks_emitted": self.emitted_ticks(),
            "frames": self.emitted_ticks().len(),
            "succeeded": self.succeeded(),
            "refused": self.refused,
            "missed": self.missed,
            "error": self.error,
            "width": self.width,
            "height": self.height,
            "hold_physics": self.hold_physics,
            "time_scale_at_arm": self.time_scale_at_arm,
            "fixed_timestep_s": fixed_timestep_s,
            "fixed_hz": if fixed_timestep_s > 0.0 { 1.0 / fixed_timestep_s } else { 0.0 },
            "substeps": substeps,
            "frame_cap": self.frame_cap,
            "wall_s": wall_s,
            "frames_rendered": self.frames_rendered,
            "mean_fps": if wall_s > 0.0 { self.frames_rendered as f64 / wall_s } else { 0.0 },
            "fixed_runs": self.fixed_runs,
            "records": rows,
        })
    }
}

/// Arm a tick run from a request that already holds the World (the bridge).
/// Refuses under `sim.time_scale != 1.0` with the named error before anything
/// is queued. The caller opens Play; the run's tick 0 is its first step.
pub fn arm_tick_captures(
    world: &mut World,
    ticks: Vec<u64>,
    dir: PathBuf,
    stem: &str,
    width: u32,
    height: u32,
) -> Result<PathBuf, String> {
    if let Some(clock) = world.get_resource::<SimulationClock>() {
        if clock.time_scale != REQUIRED_TIME_SCALE {
            return Err(refusal_message(clock.time_scale));
        }
    }
    let queue = TickCaptureQueue::new(ticks, dir, stem, width, height);
    let log = queue.log_path.clone();
    world.insert_resource(queue);
    Ok(log)
}

/// The trigger, once per `FixedUpdate` run. Reads the physics step counter; it
/// reads no wall clock and no frame count to decide WHEN to capture (wall time
/// appears only in the log and in two timeouts that fail a stuck run).
fn drive_tick_captures(
    mut commands: Commands,
    mut queue: ResMut<TickCaptureQueue>,
    ai: Res<AiCameraState>,
    mut cameras: Query<&mut Camera, With<AiCamera>>,
    mut images: ResMut<Assets<Image>>,
    counter: Res<PhysicsStepCounter>,
    run: Option<Res<StepCountRun>>,
    mut clock: ResMut<SimulationClock>,
    play_state: Option<Res<State<PlayModeState>>>,
    mut start_play: MessageWriter<StartPlayEvent>,
    mut physics: Option<ResMut<Time<Physics>>>,
    loader: Option<Res<crate::space::file_loader::DeferredServiceLoader>>,
    mut exit: MessageWriter<AppExit>,
    fixed: Res<Time<bevy::time::Fixed>>,
    substeps: Option<Res<SubstepCount>>,
) {
    let q = &mut *queue;
    if q.written {
        return;
    }
    q.fixed_runs += 1;
    if !q.armed {
        q.armed = true;
        q.armed_at = Some(Instant::now());
        if let Some(scale) = q.time_scale_override {
            clock.set_time_scale(scale);
        }
        q.time_scale_at_arm = clock.time_scale;
        let _ = std::fs::create_dir_all(&q.dir);
    }
    let run_step = run
        .as_ref()
        .and_then(|r| r.start)
        .map(|start| counter.executed().saturating_sub(start));

    // Readbacks that finished since the last run.
    let finished: Vec<(usize, Result<(), String>)> = q
        .settled
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default();
    for (idx, result) in finished {
        if let Some(rec) = q.records.get_mut(idx) {
            rec.settled = run_step;
            rec.saved = Some(result.is_ok());
            rec.save_error = result.err();
            rec.wall_s = q.armed_at.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
        }
        if q.in_flight == Some(idx) {
            q.in_flight = None;
            q.in_flight_since = None;
            if q.held {
                if let Some(p) = physics.as_mut() {
                    p.unpause();
                }
                q.held = false;
            }
        }
    }

    let mut decision = decide_tick(&q.requested, q.next, q.in_flight.is_some(), run_step, clock.time_scale);
    if decision == TickDecision::Wait {
        if q.in_flight_since.is_some_and(|t| t.elapsed() > Duration::from_secs(TICK_READBACK_TIMEOUT_S)) {
            q.error = Some(format!("a readback did not settle within {TICK_READBACK_TIMEOUT_S} s"));
            decision = TickDecision::Done;
        } else if q.armed_at.is_some_and(|t| t.elapsed() > Duration::from_secs(TICK_RUN_TIMEOUT_S)) {
            q.error = Some(format!(
                "the run did not reach tick {} within {TICK_RUN_TIMEOUT_S} s",
                q.requested.get(q.next).copied().unwrap_or(0)
            ));
            decision = TickDecision::Done;
        }
    }

    let mut finish = false;
    match decision {
        TickDecision::Done => finish = true,
        TickDecision::Refuse(msg) => {
            tracing::error!("tick capture: {msg}");
            q.refused = Some(msg);
            finish = true;
        }
        TickDecision::Missed { tick, at } => {
            tracing::error!("tick capture: tick {tick} was missed; the run is already at step {at}");
            q.missed.push(tick);
            q.error = Some(format!("tick {tick} was missed: the run is already at step {at}"));
            finish = true;
        }
        TickDecision::Wait => {
            // Keep the camera powered and the target at the requested size
            // until the run reaches the next tick.
            if let Ok(mut camera) = cameras.single_mut() {
                if !camera.is_active {
                    camera.is_active = true;
                }
                q.camera_active_runs = q.camera_active_runs.saturating_add(1);
            } else {
                q.camera_active_runs = 0;
            }
            if !q.resized {
                if let Some(handle) = ai.image.clone() {
                    let want = (q.width, q.height);
                    if let Some(mut img) = images.get_mut(&handle) {
                        if (img.width(), img.height()) != want {
                            img.resize(Extent3d { width: want.0, height: want.1, depth_or_array_layers: 1 });
                            tracing::info!("tick capture: AI Camera render target resized to {}x{}", want.0, want.1);
                            q.camera_active_runs = 0;
                        }
                        q.resized = true;
                    }
                }
            }
            if q.start_play && !q.play_started {
                let loaded = loader.map(|l| l.priority_done && l.pending.is_empty()).unwrap_or(true);
                let editing = play_state.as_ref().map(|s| *s.get() == PlayModeState::Editing).unwrap_or(false);
                if loaded && editing && q.resized && q.camera_active_runs > 0 {
                    q.settle_runs += 1;
                } else {
                    q.settle_runs = 0;
                }
                if q.settle_runs > TICK_SETTLE_RUNS && q.camera_active_runs >= TICK_WARM_RUNS {
                    start_play.write(StartPlayEvent { play_type: PlayModeType::Solo });
                    q.play_started = true;
                    tracing::info!("tick capture: Play started for a run of ticks {:?}", q.requested);
                }
            }
        }
        TickDecision::Fire(fired) => {
            let warm = q.camera_active_runs >= TICK_WARM_RUNS;
            let have_camera = cameras.single().is_ok();
            if (!warm || !have_camera) && q.hold_physics {
                // The target is not warm yet: hold the world at this tick and
                // try again next run. The tick does not pass while held.
                if !q.held {
                    if let Some(p) = physics.as_mut() {
                        hold_physics(p);
                    }
                    q.held = true;
                }
                if let Ok(mut camera) = cameras.single_mut() {
                    camera.is_active = true;
                    q.camera_active_runs = q.camera_active_runs.saturating_add(1);
                }
            } else if let Some(image) = ai.image.clone() {
                if q.hold_physics && !q.held {
                    if let Some(p) = physics.as_mut() {
                        hold_physics(p);
                    }
                    q.held = true;
                }
                let idx = q.records.len();
                // The name carries the tick that FIRED, read just now, not a
                // counter of requests satisfied.
                let file = q.dir.join(tick_file_name(&q.stem, fired));
                let _ = std::fs::remove_file(&file);
                q.records.push(TickCaptureRecord {
                    requested: q.requested[q.next],
                    fired,
                    settled: None,
                    physics_steps_total: counter.executed(),
                    clock_tick_count: clock.tick_count,
                    fixed_runs: q.fixed_runs,
                    file: file.clone(),
                    saved: None,
                    save_error: None,
                    warm,
                    wall_s: q.armed_at.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0),
                });
                q.next += 1;
                q.in_flight = Some(idx);
                q.in_flight_since = Some(Instant::now());
                tracing::info!(
                    "tick capture: tick {fired} fired (physics steps total {}, clock tick_count {}) -> {}",
                    counter.executed(),
                    clock.tick_count,
                    file.display()
                );
                let settled = q.settled.clone();
                commands
                    .spawn(bevy::render::view::screenshot::Screenshot::image(image))
                    .observe(
                        move |trigger: bevy::ecs::observer::On<
                            bevy::render::view::screenshot::ScreenshotCaptured,
                        >| {
                            let img = trigger.image.clone();
                            let out = file.clone();
                            let settled = settled.clone();
                            // Off the render thread: GPU readback, then PNG encode.
                            // The tick is satisfied only once the file is saved.
                            bevy::tasks::AsyncComputeTaskPool::get()
                                .spawn(async move {
                                    let result = match img.try_into_dynamic() {
                                        Ok(d) => d.save(&out).map_err(|e| format!("save {}: {e}", out.display())),
                                        Err(e) => Err(format!("conversion: {e}")),
                                    };
                                    if let Ok(mut g) = settled.lock() {
                                        g.push((idx, result));
                                    }
                                })
                                .detach();
                        },
                    );
            } else {
                q.error = Some("the AI camera's render target does not exist".to_string());
                finish = true;
            }
        }
    }

    if finish {
        if q.held {
            if let Some(p) = physics.as_mut() {
                p.unpause();
            }
            q.held = false;
        }
        if let Ok(mut camera) = cameras.single_mut() {
            camera.is_active = false;
        }
        q.written = true;
        let step = fixed.timestep().as_secs_f64();
        let json = q.log_json(step, substeps.as_ref().map(|s| u64::from(s.0)));
        if let Some(parent) = q.log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = q.log_path.with_extension("json.tmp");
        let written = serde_json::to_string_pretty(&json)
            .map_err(|e| e.to_string())
            .and_then(|text| std::fs::write(&tmp, text).map_err(|e| e.to_string()))
            .and_then(|_| std::fs::rename(&tmp, &q.log_path).map_err(|e| e.to_string()));
        match written {
            Ok(()) => tracing::info!(
                "tick capture: {} (ticks emitted {:?}) -> {}",
                if q.succeeded() { "done" } else { "FAILED" },
                q.emitted_ticks(),
                q.log_path.display()
            ),
            Err(e) => tracing::error!("tick capture: log not written to {}: {e}", q.log_path.display()),
        }
        if q.exit_when_done {
            exit.write(if q.succeeded() { AppExit::Success } else { AppExit::from_code(1) });
        }
    }
}

/// Counts rendered frames (a diagnostic for the log, never a trigger) and, when
/// `EUSTRESS_TICK_CAPTURE_FRAME_CAP` is set, holds the render loop to that
/// many frames per second by sleeping out the rest of each frame.
fn tick_capture_frame_pacing(mut queue: ResMut<TickCaptureQueue>) {
    queue.frames_rendered += 1;
    if let Some(fps) = queue.frame_cap {
        let target = Duration::from_secs_f64(1.0 / fps);
        if let Some(last) = queue.last_frame_end {
            let spent = last.elapsed();
            if spent < target {
                std::thread::sleep(target - spent);
            }
        }
        queue.last_frame_end = Some(Instant::now());
    }
}

/// Drive a queued capture: power the camera up, let it render, screenshot the
/// off-screen image, then power it back down.
///
/// PERF: this is why the camera is not simply left on. Left active, this
/// off-screen 1280x720 camera rendered a COMPLETE second view of the scene
/// every frame for the entire session (depth prepass, shadows, opaque,
/// transparent, post), doubling render cost to keep a target warm for the
/// rare capture.
///
/// A request with no camera to render (a Space just opened, or the camera was
/// deleted) waits for [`ensure_ai_camera_exists`] to put one back and starts
/// its warm-up over, so the screenshot is never taken of an image nothing
/// renders into.
fn process_ai_capture(
    mut commands: Commands,
    mut state: ResMut<AiCameraState>,
    mut cam: Query<&mut Camera, With<AiCamera>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Cooldown: a screenshot is in flight. Keep the camera ACTIVE so the render
    // graph prepares its view target and services the readback, then power down.
    if state.cooldown > 0 {
        state.cooldown -= 1;
        if state.cooldown == 0 {
            if let Ok(mut c) = cam.single_mut() {
                c.is_active = false;
            }
        }
        return;
    }

    if state.pending.is_none() {
        return; // idle: camera stays powered down, costing nothing
    }

    let Ok(mut camera) = cam.single_mut() else {
        state.warmup = 0;
        return;
    };
    // Resize the render target to the requested size, on request only: the
    // size is compared first, so a repeat at the same size reallocates
    // nothing. This runs only here (never in a per-frame path, never during a
    // cooldown whose readback may still be in flight) and touches the image
    // asset's size alone; Msaa, Hdr and `NoAtmosphere` are not involved. A
    // real resize restarts the warm-up so the render graph rebuilds the view
    // target at the new size before anything is read back.
    if let (Some(handle), Some(want)) = (
        state.image.clone(),
        state.pending.as_ref().map(|p| (p.width, p.height)),
    ) {
        let needs_resize = images
            .get(&handle)
            .map(|img| (img.width(), img.height()) != want)
            .unwrap_or(false);
        if needs_resize {
            if let Some(mut img) = images.get_mut(&handle) {
                img.resize(Extent3d { width: want.0, height: want.1, depth_or_array_layers: 1 });
                info!("📷 AI Camera render target resized to {}x{}", want.0, want.1);
            }
            state.warmup = 0;
        }
    }
    // Power up (again, for a camera put back mid-request) and let it render a
    // few frames before reading back.
    if !camera.is_active {
        camera.is_active = true;
    }
    if state.warmup < CAPTURE_WARMUP_FRAMES {
        state.warmup += 1;
        return;
    }

    // Ready: consume the request. The camera deliberately stays ACTIVE here;
    // `cooldown` powers it down a few frames later.
    let Some(pending) = state.pending.take() else {
        return;
    };
    state.warmup = 0;
    // Set BEFORE the early return below so a missing image can never leave the
    // camera stuck on.
    state.cooldown = CAPTURE_COOLDOWN_FRAMES;
    let Some(image) = state.image.clone() else {
        return;
    };
    let out_path = pending.path;
    commands
        .spawn(bevy::render::view::screenshot::Screenshot::image(image))
        .observe(
            move |trigger: bevy::ecs::observer::On<
                bevy::render::view::screenshot::ScreenshotCaptured,
            >| {
                let img = trigger.image.clone();
                let out = out_path.clone();
                // Off the render thread: GPU readback, then PNG encode.
                bevy::tasks::AsyncComputeTaskPool::get()
                    .spawn(async move {
                        match img.try_into_dynamic() {
                            Ok(d) => {
                                if let Err(e) = d.save(&out) {
                                    tracing::warn!("ai_camera.capture save failed: {}", e);
                                } else {
                                    tracing::info!("ai_camera.capture → {:?}", out);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("ai_camera.capture conversion failed: {}", e)
                            }
                        }
                    })
                    .detach();
            },
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_clustering_is_kept_only_when_asked_for() {
        for on in ["1", "true", "YES", " on "] {
            assert!(gpu_clustering_requested(Some(on)), "{on:?}");
        }
        for off in ["", "0", "false", "no", "off", "2"] {
            assert!(!gpu_clustering_requested(Some(off)), "{off:?}");
        }
        assert!(!gpu_clustering_requested(None));
    }

    fn state_with_image() -> AiCameraState {
        AiCameraState { image: Some(Handle::<Image>::default()), ..default() }
    }

    fn ai_cameras(app: &mut App) -> Vec<(Entity, Transform, String, isize, bool)> {
        let world = app.world_mut();
        let mut query = world.query_filtered::<
            (Entity, &Transform, &eustress_common::classes::Instance, &Camera, &RenderTarget),
            With<AiCamera>,
        >();
        query
            .iter(world)
            .map(|(entity, tf, instance, camera, target)| {
                assert!(matches!(target, RenderTarget::Image(_)), "the AI camera must render off-screen");
                (entity, *tf, instance.name.clone(), camera.order, camera.is_active)
            })
            .collect()
    }

    /// Whatever removes it (a Space opening despawns every `Instance`, the
    /// Explorer deletes it), exactly one AI camera comes back at its starting
    /// pose, named, off-screen, powered down and out of the editor's order.
    #[test]
    fn the_ai_camera_comes_back_whenever_it_is_gone() {
        let mut app = App::new();
        app.insert_resource(state_with_image());
        app.add_systems(Update, ensure_ai_camera_exists);

        app.update();
        let first = ai_cameras(&mut app);
        assert_eq!(first.len(), 1, "one at startup");
        let (first_entity, pose, name, order, active) = first[0].clone();
        assert_eq!(name, AI_CAMERA_NAME);
        assert_eq!(order, -1);
        assert!(!active, "powered down while idle");
        assert!(pose.translation.abs_diff_eq(start_pose().translation, 1e-4));

        // A Space opens: `open_space` despawns every Instance, children first.
        app.world_mut().entity_mut(first_entity).insert(Transform::from_xyz(1.0, 2.0, 3.0));
        let instances: Vec<Entity> = {
            let world = app.world_mut();
            let mut q = world.query_filtered::<Entity, With<eustress_common::classes::Instance>>();
            q.iter(world).collect()
        };
        for entity in instances {
            app.world_mut().despawn(entity);
        }
        app.update();
        let after_open = ai_cameras(&mut app);
        assert_eq!(after_open.len(), 1, "one after a Space opens");
        assert_ne!(after_open[0].0, first_entity);
        assert!(after_open[0].1.translation.abs_diff_eq(start_pose().translation, 1e-4), "back at the starting pose");

        // A delete in the Explorer.
        app.world_mut().despawn(after_open[0].0);
        app.update();
        assert_eq!(ai_cameras(&mut app).len(), 1, "one after a delete");

        // And it stays one.
        app.update();
        app.update();
        assert_eq!(ai_cameras(&mut app).len(), 1, "never a second");
    }

    /// A bridge call finds the camera, or puts it back at once, and never
    /// makes a second.
    #[test]
    fn a_bridge_call_puts_the_camera_back_at_once() {
        let mut world = World::new();
        world.insert_resource(state_with_image());
        let made = ensure_in_world(&mut world).expect("a camera");
        assert!(world.get::<AiCamera>(made).is_some());
        assert_eq!(ensure_in_world(&mut world).unwrap(), made, "the same camera, not a second");
        let mut q = world.query_filtered::<Entity, With<AiCamera>>();
        assert_eq!(q.iter(&world).count(), 1);
    }

    /// An engine with no renderer (no AI camera plugin) says so, instead of
    /// "entity not found".
    #[test]
    fn without_a_renderer_the_camera_is_unavailable() {
        let mut world = World::new();
        let why = ensure_in_world(&mut world).unwrap_err();
        assert!(why.contains("headless"), "{why}");
        let mut q = world.query_filtered::<Entity, With<AiCamera>>();
        assert_eq!(q.iter(&world).count(), 0);
    }

    /// A capture asked for while no camera exists waits for one, warms it up
    /// from the start, and only then reads back; a camera removed mid-warm-up
    /// restarts the count.
    #[test]
    fn a_capture_waits_for_a_camera_to_render() {
        let mut app = App::new();
        let mut state = state_with_image();
        request_capture(&mut state, std::env::temp_dir().join("eus_ai_camera_unused.png"));
        app.insert_resource(state);
        app.insert_resource(Assets::<Image>::default());
        app.add_systems(Update, process_ai_capture);

        // No camera: the request waits.
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().resource::<AiCameraState>().pending.is_some());
        assert_eq!(app.world().resource::<AiCameraState>().warmup, 0);

        // A camera appears: it powers up and warms up.
        let camera = app.world_mut().spawn(ai_camera_bundle(Handle::<Image>::default())).id();
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().get::<Camera>(camera).unwrap().is_active, "powered up for the capture");
        assert_eq!(app.world().resource::<AiCameraState>().warmup, 3);

        // It is removed mid-warm-up: the count starts over, the request waits.
        app.world_mut().despawn(camera);
        app.update();
        assert_eq!(app.world().resource::<AiCameraState>().warmup, 0);
        assert!(app.world().resource::<AiCameraState>().pending.is_some());

        // A new camera warms up the full count before any readback.
        let camera = app.world_mut().spawn(ai_camera_bundle(Handle::<Image>::default())).id();
        for _ in 0..CAPTURE_WARMUP_FRAMES {
            app.update();
        }
        assert!(app.world().get::<Camera>(camera).unwrap().is_active);
        assert_eq!(app.world().resource::<AiCameraState>().warmup, CAPTURE_WARMUP_FRAMES);
        assert!(app.world().resource::<AiCameraState>().pending.is_some(), "no readback before the warm-up ends");
    }

    /// A sized request resizes the render target once, on that request; a
    /// second request at the same size reallocates nothing and keeps its
    /// warm-up; a later request at another size resizes again.
    #[test]
    fn a_sized_request_resizes_the_target_on_request_only() {
        let mut app = App::new();
        let mut images = Assets::<Image>::default();
        let handle = images.add(Image::new_fill(
            Extent3d { width: AI_CAM_WIDTH, height: AI_CAM_HEIGHT, depth_or_array_layers: 1 },
            TextureDimension::D2,
            &[0, 0, 0, 255],
            TextureFormat::Bgra8UnormSrgb,
            RenderAssetUsages::default(),
        ));
        let mut state = AiCameraState { image: Some(handle.clone()), ..default() };
        request_capture_sized(&mut state, std::env::temp_dir().join("eus_ai_camera_unused.png"), 3840, 2160);
        app.insert_resource(state);
        app.insert_resource(images);
        app.add_systems(Update, process_ai_capture);
        app.world_mut().spawn(ai_camera_bundle(handle.clone()));

        app.update();
        let size = |app: &App| {
            let img = app.world().resource::<Assets<Image>>().get(&handle).unwrap();
            (img.width(), img.height())
        };
        assert_eq!(size(&app), (3840, 2160), "resized to the requested size");
        assert_eq!(app.world().resource::<AiCameraState>().warmup, 1);

        // Same size again: no resize, so the warm-up keeps counting.
        app.update();
        app.update();
        assert_eq!(size(&app), (3840, 2160));
        assert_eq!(app.world().resource::<AiCameraState>().warmup, 3, "no reset without a resize");

        // A different size on a later request resizes again.
        {
            let mut state = app.world_mut().resource_mut::<AiCameraState>();
            request_capture_sized(&mut state, std::env::temp_dir().join("eus_ai_camera_unused2.png"), 1920, 1080);
        }
        app.update();
        assert_eq!(size(&app), (1920, 1080));
    }
}

#[cfg(test)]
mod tick_capture_tests {
    use super::*;

    #[test]
    fn a_tick_list_is_taken_whole_or_refused_whole() {
        assert_eq!(parse_ticks("0,60,300,600").unwrap(), vec![0, 60, 300, 600]);
        assert_eq!(parse_ticks(" 0 , 60 ").unwrap(), vec![0, 60]);
        assert!(parse_ticks("60,0").is_err(), "out of order is refused, never sorted");
        assert!(parse_ticks("0,60,60").is_err(), "a repeat is refused, never dropped");
        assert!(parse_ticks("0,x").is_err());
        assert!(parse_ticks("").is_err());
    }

    #[test]
    fn the_tick_is_in_the_file_name() {
        assert_eq!(tick_file_name("tick", 60), "tick_t000060.png");
        assert_eq!(tick_file_name("tick", 600), "tick_t000600.png");
    }

    #[test]
    fn the_trigger_fires_on_the_exact_step_only() {
        let list = [0, 60, 300, 600];
        // No run open: wait. At the step: fire. Before it: wait.
        assert_eq!(decide_tick(&list, 0, false, None, 1.0), TickDecision::Wait);
        assert_eq!(decide_tick(&list, 0, false, Some(0), 1.0), TickDecision::Fire(0));
        assert_eq!(decide_tick(&list, 1, false, Some(59), 1.0), TickDecision::Wait);
        assert_eq!(decide_tick(&list, 1, false, Some(60), 1.0), TickDecision::Fire(60));
        // Past it: a refusal that names the tick, never the nearest frame.
        assert_eq!(decide_tick(&list, 1, false, Some(61), 1.0), TickDecision::Missed { tick: 60, at: 61 });
        // A readback in flight holds the next tick back.
        assert_eq!(decide_tick(&list, 2, true, Some(300), 1.0), TickDecision::Wait);
        // All requested and settled.
        assert_eq!(decide_tick(&list, 4, false, Some(700), 1.0), TickDecision::Done);
        // The last readback still outstanding is not done.
        assert_eq!(decide_tick(&list, 4, true, Some(600), 1.0), TickDecision::Wait);
    }

    #[test]
    fn compression_is_refused_with_a_named_error_before_any_frame() {
        let list = [0, 60];
        let want = "capture refused: sim.time_scale=2.0, required 1.0".to_string();
        assert_eq!(decide_tick(&list, 0, false, None, 2.0), TickDecision::Refuse(want.clone()));
        assert_eq!(decide_tick(&list, 0, false, Some(0), 2.0), TickDecision::Refuse(want.clone()));
        assert_eq!(refusal_message(2.0), want);
        // Compression that starts after a capture is in flight refuses too.
        assert_eq!(decide_tick(&list, 1, true, Some(0), 2.0), TickDecision::Refuse(want));
    }

    #[test]
    fn a_run_succeeds_only_when_every_tick_fired_and_settled_exactly() {
        let mut q = TickCaptureQueue::new(vec![0, 60], std::env::temp_dir(), "tick", 64, 64);
        assert!(!q.succeeded(), "no frames yet");
        let rec = |requested: u64, fired: u64, settled: Option<u64>, saved: Option<bool>| TickCaptureRecord {
            requested,
            fired,
            settled,
            physics_steps_total: 0,
            clock_tick_count: 0,
            fixed_runs: 0,
            file: PathBuf::from("x.png"),
            saved,
            save_error: None,
            warm: true,
            wall_s: 0.0,
        };
        q.records = vec![rec(0, 0, Some(0), Some(true)), rec(60, 60, Some(60), Some(true))];
        assert!(q.succeeded());
        assert_eq!(q.emitted_ticks(), vec![0, 60]);
        // A readback that settled a step late is a failure, not a pass.
        q.records[1].settled = Some(61);
        assert!(!q.succeeded());
        q.records[1].settled = Some(60);
        q.refused = Some(refusal_message(2.0));
        assert!(!q.succeeded());
    }
}
