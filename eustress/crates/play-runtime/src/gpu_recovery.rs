//! # A lost graphics device, survived and recovered
//!
//! Studio and the Player share this, so both answer a graphics driver reset
//! (wgpu `DeviceLost`, "driver implementation is at fault") the same way: the
//! frame that notices it draws nothing more, the renderer is rebuilt once the
//! driver is back, and the simulation, scripts and networking never stop. A
//! session hosted here is never closed by a render error.
//!
//! ## Why a lost device ended the app (Bevy 0.19.0, wgpu 29.0.4)
//!
//! - wgpu reports the loss through the device-lost callback, from inside the
//!   call that noticed it. Bevy's callback logs `Caught DeviceLost error` and
//!   stores the error (`bevy_render` `src/error_handler.rs:127-130`). On 11-18
//!   the call was `surface.get_current_texture()` in `prepare_windows`
//!   (`src/view/window/mod.rs:300`, the `Couldn't get swap chain texture` line
//!   at `:333`), in the middle of the `Render` schedule.
//! - Bevy reads the stored error only in `update_state`, its `pre_extract` step
//!   (`error_handler.rs:185-188`, wired at `lib.rs:361`), before the next
//!   frame's extract. The render schedule is gated on `RenderState::Ready`
//!   once, before it starts (`lib.rs:423`, `:469`), so the rest of that frame's
//!   `Render` schedule ran on the dead device.
//! - wgpu drops device-lost errors instead of reporting them
//!   (`src/backend/wgpu_core.rs:297`), so every buffer made on the lost device
//!   is silently invalid, and `Buffer::get_mapped_range` on one panics
//!   (`wgpu_core.rs:2253`, `handle_error_fatal` at `:337-343`). Every
//!   `Prepare` system that writes a buffer does that:
//!   `prepare_clusters_for_cpu_clustering`,
//!   `prepare_generated_environment_map_bind_groups` and
//!   `prepare_material_bind_groups` all panicked in that same frame, CPU
//!   clustering included, so turning GPU clustering off was no protection.
//! - The panic ended the render thread; the main thread then found the render
//!   app gone and closed (`pipelined_rendering.rs:199-201`). The render error
//!   handler never ran.
//! - The panic cannot be caught and lived with: `World::try_schedule_scope`
//!   (bevy_ecs 0.19.0 `src/world/mod.rs:3834-3843`) takes a schedule out of
//!   the world while it runs and puts it back only on a normal return, so an
//!   unwinding `Render` schedule is gone for good.
//!
//! ## What happens now
//!
//! 1. [`log_layer`] watches for Bevy's `Caught DeviceLost error` line, the one
//!    signal Bevy gives while the frame is still running (wgpu keeps a single
//!    device-lost callback per device, and Bevy holds it), and sets [`LATCH`].
//!    Each app adds it to `LogPlugin::custom_layer`.
//! 2. Every `RenderSystems` set from `PrepareAssets` through `Cleanup` runs only
//!    while the latch is clear ([`gate_render_sets`]). Bevy decides a set's run
//!    conditions when the set's first system is about to run, so every set
//!    after the one that noticed the loss is skipped in that same frame, the
//!    way Bevy skips the whole schedule on the frames that follow.
//! 3. At the next frame boundary Bevy hands the error to the handler here. It
//!    waits [`SETTLE`] for the driver to finish resetting, checks that the
//!    driver makes devices again (a test device on the same adapter, on its
//!    own thread), then answers `RenderErrorPolicy::Recover` with the settings
//!    the app started its renderer with. `RenderStartup` runs on the new
//!    device and clears the latch.
//! 4. The simulation, scripts and networking are main-world systems and keep
//!    running throughout. Rebuilds are bounded ([`MAX_RECOVERIES`] inside
//!    [`RECOVERY_WINDOW`]), because a device that keeps failing makes the
//!    screen flash. Past that, drawing stops for the session; Studio and any
//!    hosted session keep running, and a Player that hosts nothing closes.
//!
//! ## What only a real loss can show
//!
//! The rebuild itself runs on the main thread inside Bevy (`block_on`), and
//! an app that compiles its pipelines synchronously recompiles them all on the
//! new device, so a hosted session pauses for that long once. Render resources
//! a plugin made outside `RenderStartup` still belong to the old device; using
//! one raises a validation error, which inside [`AFTERMATH`] counts as part of
//! the loss and takes another (bounded) rebuild.
//!
//! ## Switches
//!
//! | Variable | Meaning |
//! |---|---|
//! | `EUSTRESS_GPU_TEST_LOSS` | Seconds: destroy this app's own GPU device once, that long after it first draws, to test recovery without a driver reset ([`TEST_LOSS_SWITCH`]) |
//! | `EUSTRESS_GPU_REBUILD` | `0` or `off`: a lost device stops drawing for the session instead of rebuilding ([`REBUILD_SWITCH`]) |

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::ecs::schedule::Schedule;
use bevy::log::tracing::field::{Field, Visit};
use bevy::log::tracing::Event as TracingEvent;
use bevy::log::tracing::Subscriber;
use bevy::log::tracing_subscriber::layer::{Context, Layer};
use bevy::log::{BoxedLayer, Level};
use bevy::prelude::*;
use bevy::render::error_handler::{ErrorType, RenderError, RenderErrorHandler, RenderErrorPolicy};
use bevy::render::render_resource::PollType;
use bevy::render::renderer::{RenderAdapter, RenderDevice};
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use eustress_networking::HostSession;

/// Rebuilds allowed inside [`RECOVERY_WINDOW`] before drawing stops.
pub const MAX_RECOVERIES: usize = 3;

/// The span [`MAX_RECOVERIES`] is counted over.
pub const RECOVERY_WINDOW: Duration = Duration::from_secs(10 * 60);

/// How long after the loss before the driver is asked for a device again.
/// Windows gives a hung GPU two seconds before it resets the driver (the
/// default `TdrDelay`), and a device asked for mid-reset can fail.
pub const SETTLE: Duration = Duration::from_secs(2);

/// How long to keep waiting for a driver that makes no devices before drawing
/// stops for the session.
pub const DRIVER_WAIT_LIMIT: Duration = Duration::from_secs(60);

/// How long after a failed test device before the next one is made.
pub const PROBE_RETRY: Duration = Duration::from_secs(2);

/// How long after a rebuild any render error still counts as part of the loss
/// (a resource left on the old device), not as a new fault.
pub const AFTERMATH: Duration = Duration::from_secs(15);

// ============================================================================
// The latch: the loss, seen while the frame is still running
// ============================================================================

/// Set when the device is lost, cleared when the renderer starts on a device.
pub struct LossLatch {
    lost: AtomicBool,
    startups: AtomicU64,
}

impl LossLatch {
    pub const fn new() -> Self {
        Self { lost: AtomicBool::new(false), startups: AtomicU64::new(0) }
    }

    /// The device is gone: the rest of this frame's render sets are skipped.
    pub fn mark_lost(&self) {
        self.lost.store(true, Ordering::Release);
    }

    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// How many times the renderer has started on a device (`RenderStartup`).
    pub fn startups(&self) -> u64 {
        self.startups.load(Ordering::Acquire)
    }

    /// `RenderStartup` ran: the renderer is on a working device again.
    fn renderer_started(&self) {
        self.lost.store(false, Ordering::Release);
        self.startups.fetch_add(1, Ordering::AcqRel);
    }
}

impl Default for LossLatch {
    fn default() -> Self {
        Self::new()
    }
}

/// The process's latch. There is one renderer per process.
pub static LATCH: LossLatch = LossLatch::new();

/// Where Bevy's device-lost callback logs from.
pub const BEVY_ERROR_TARGET: &str = "bevy_render::error_handler";

/// How Bevy's device-lost line begins (`error_handler.rs:128`).
pub const BEVY_DEVICE_LOST_LINE: &str = "Caught DeviceLost error";

/// `true` for Bevy's device-lost log line, and for nothing else it logs.
pub fn is_device_lost_line(level: &Level, target: &str, message: &str) -> bool {
    *level == Level::ERROR && target == BEVY_ERROR_TARGET && message.starts_with(BEVY_DEVICE_LOST_LINE)
}

/// A tracing layer that sets a latch when Bevy logs that the device is lost.
pub struct DeviceLossLayer {
    latch: &'static LossLatch,
}

impl DeviceLossLayer {
    pub const fn new(latch: &'static LossLatch) -> Self {
        Self { latch }
    }
}

impl<S: Subscriber> Layer<S> for DeviceLossLayer {
    fn on_event(&self, event: &TracingEvent<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        // Cheap checks first: this sees every event the app logs.
        if *metadata.level() != Level::ERROR || metadata.target() != BEVY_ERROR_TARGET {
            return;
        }
        let mut message = MessageText::default();
        event.record(&mut message);
        if is_device_lost_line(metadata.level(), metadata.target(), &message.0) {
            self.latch.mark_lost();
        }
    }
}

/// The `message` field of an event, as text.
#[derive(Default)]
struct MessageText(String);

impl Visit for MessageText {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_owned();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

/// For `LogPlugin::custom_layer`: the layer that sets [`LATCH`].
pub fn log_layer(_app: &mut App) -> Option<BoxedLayer> {
    Some(Box::new(DeviceLossLayer::new(&LATCH)))
}

/// The render sets that draw on the device: everything after the extracted
/// commands are applied, up to and including `Cleanup` (which maps readback
/// buffers). `ExtractCommands` and `PostCleanup` touch no device and keep the
/// render world's entities in step.
pub const GATED_SETS: [RenderSystems; 18] = [
    RenderSystems::PrepareAssets,
    RenderSystems::PrepareMeshes,
    RenderSystems::CreateViews,
    RenderSystems::Specialize,
    RenderSystems::PrepareViews,
    RenderSystems::Queue,
    RenderSystems::QueueMeshes,
    RenderSystems::QueueSweep,
    RenderSystems::PhaseSort,
    RenderSystems::Prepare,
    RenderSystems::PrepareResources,
    RenderSystems::PrepareResourcesBatchPhases,
    RenderSystems::PrepareResourcesWritePhaseBuffers,
    RenderSystems::PrepareResourcesCollectPhaseBuffers,
    RenderSystems::PrepareResourcesFlush,
    RenderSystems::PrepareBindGroups,
    RenderSystems::Render,
    RenderSystems::Cleanup,
];

/// Run each of [`GATED_SETS`] only while `latch` is clear. Each set gets its
/// own condition, so each is a separate checkpoint inside the frame: one
/// condition over all of them would be decided once, at the first.
pub fn gate_render_sets(schedule: &mut Schedule, latch: &'static LossLatch) {
    for set in GATED_SETS {
        schedule.configure_sets(set.run_if(move || !latch.is_lost()));
    }
}

// ============================================================================
// The decision (pure)
// ============================================================================

/// A render error, as recovery sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    DeviceLost,
    OutOfMemory,
    Validation,
    Internal,
}

impl Fault {
    /// A device-lost error, or any error whose text says the device is lost.
    pub fn of(ty: ErrorType, description: &str) -> Self {
        if ty == ErrorType::DeviceLost || describes_lost_device(description) {
            return Fault::DeviceLost;
        }
        match ty {
            ErrorType::OutOfMemory => Fault::OutOfMemory,
            ErrorType::Validation => Fault::Validation,
            _ => Fault::Internal,
        }
    }
}

/// `true` when wgpu's text says the device itself is gone.
pub fn describes_lost_device(text: &str) -> bool {
    text.contains("DeviceLost")
        || text.contains("Device is lost")
        || text.contains("device is lost")
        || text.contains("driver implementation is at fault")
}

/// What the app around the renderer needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Situation {
    /// A session is hosted from this app: it must not close.
    pub hosting: bool,
    /// The app stays open when recovery gives up (Studio: unsaved work).
    pub keep_app: bool,
    /// A rebuild may be asked for ([`rebuild_allowed`]).
    pub can_rebuild: bool,
}

/// Set to `0` or `off`, a lost device stops drawing for the session instead of
/// rebuilding the renderer (the answer Studio gave before 11-18), for a
/// machine where a rebuild proves worse than no picture.
pub const REBUILD_SWITCH: &str = "EUSTRESS_GPU_REBUILD";

/// Whether a lost device may be answered with a rebuild: the settings name
/// backends (Bevy's `Recover` asserts on that, `error_handler.rs:62`), and
/// [`REBUILD_SWITCH`] does not turn it off.
pub fn rebuild_allowed(has_backends: bool, switch: Option<&str>) -> bool {
    let off = switch.is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "off" | "false" | "no"));
    has_backends && !off
}

/// What to do about one render error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Draw nothing this frame and ask again next frame.
    Wait,
    /// Rebuild the renderer now.
    Recover,
    /// Stop drawing for this session; everything else keeps running.
    StopDrawing,
    /// Close the app. `after_loss`: recovery from a lost device gave up (as
    /// opposed to an error that is not a lost device).
    Close { after_loss: bool },
}

/// What recovery remembers between errors.
#[derive(Debug, Default, Clone)]
pub struct LossState {
    recoveries: Vec<Instant>,
    lost_since: Option<Instant>,
    last_recovery: Option<Instant>,
    stopped: Option<Verdict>,
}

impl LossState {
    /// Rebuilds made inside [`RECOVERY_WINDOW`] before `now`.
    pub fn recoveries_in_window(&self, now: Instant) -> usize {
        self.recoveries.iter().filter(|t| now.saturating_duration_since(**t) < RECOVERY_WINDOW).count()
    }

    /// Drawing has stopped for this session.
    pub fn is_stopped(&self) -> bool {
        self.stopped.is_some()
    }
}

/// The answer to a render error of kind `fault` at `now`. Bevy asks again
/// every frame while the renderer is not drawing, so this is called often and
/// must give the same answer to the same question. `driver_ready` is asked
/// only once the loss has settled.
pub fn decide(
    state: &mut LossState,
    fault: Fault,
    now: Instant,
    situation: Situation,
    driver_ready: impl FnOnce() -> bool,
) -> Verdict {
    if let Some(stopped) = state.stopped {
        // A session hosted since drawing stopped keeps the app open too.
        return if situation.hosting { Verdict::StopDrawing } else { stopped };
    }
    let in_aftermath = state.last_recovery.is_some_and(|t| now.saturating_duration_since(t) < AFTERMATH);
    let part_of_a_loss = fault == Fault::DeviceLost || state.lost_since.is_some() || in_aftermath;
    if !part_of_a_loss {
        // Not a lost device: a renderer fault a rebuild would hit again. An
        // app that must stay open (a hosted session, or Studio with unsaved
        // work) stops drawing; only a Player that hosts nothing closes.
        let verdict = if situation.hosting || situation.keep_app {
            Verdict::StopDrawing
        } else {
            Verdict::Close { after_loss: false }
        };
        state.stopped = Some(verdict);
        return verdict;
    }
    let give_up = if situation.hosting || situation.keep_app {
        Verdict::StopDrawing
    } else {
        Verdict::Close { after_loss: true }
    };
    let lost_since = *state.lost_since.get_or_insert(now);
    state.recoveries.retain(|t| now.saturating_duration_since(*t) < RECOVERY_WINDOW);
    if !situation.can_rebuild || state.recoveries.len() >= MAX_RECOVERIES {
        state.lost_since = None;
        state.stopped = Some(give_up);
        return give_up;
    }
    let waited = now.saturating_duration_since(lost_since);
    if waited < SETTLE {
        return Verdict::Wait;
    }
    if !driver_ready() {
        if waited >= DRIVER_WAIT_LIMIT {
            state.lost_since = None;
            state.stopped = Some(give_up);
            return give_up;
        }
        return Verdict::Wait;
    }
    state.recoveries.push(now);
    state.lost_since = None;
    state.last_recovery = Some(now);
    Verdict::Recover
}

// ============================================================================
// Renderer health, for the app's notice
// ============================================================================

/// Whether the renderer is drawing. Studio shows it in the window title, which
/// Windows draws while Studio's own drawing is paused.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RendererHealth {
    #[default]
    Drawing,
    Recovering,
    Stopped,
}

/// The plain notice for `health`; none while drawing.
pub fn health_notice(health: RendererHealth) -> Option<&'static str> {
    match health {
        RendererHealth::Drawing => None,
        RendererHealth::Recovering => Some("The graphics driver reset. Recovering..."),
        RendererHealth::Stopped => {
            Some("Drawing stopped: the graphics driver kept failing. The simulation keeps running.")
        }
    }
}

// ============================================================================
// The plugin
// ============================================================================

/// Recovery from a lost graphics device. Add it after `DefaultPlugins`, and
/// add [`log_layer`] to `LogPlugin::custom_layer`.
pub struct GpuRecoveryPlugin {
    /// The settings the renderer is rebuilt with: the ones the app started it
    /// with, so a rebuilt device has the same features.
    pub rebuild_with: WgpuSettings,
    /// `None`: the app stays open when recovery gives up (Studio). `Some`: it
    /// closes then, unless it hosts a session, after this tells the user why.
    pub close_when_given_up: Option<fn(&mut World)>,
}

impl Plugin for GpuRecoveryPlugin {
    fn build(&self, app: &mut App) {
        let switch = std::env::var(REBUILD_SWITCH).ok();
        app.insert_resource(GpuRecovery {
            rebuild_with: self.rebuild_with.clone(),
            may_rebuild: rebuild_allowed(self.rebuild_with.backends.is_some(), switch.as_deref()),
            close_when_given_up: self.close_when_given_up,
            state: LossState::default(),
            probe: DriverProbe::default(),
            last: None,
            startups_at_recover: None,
        })
        .init_resource::<RendererHealth>()
        .insert_resource(RenderErrorHandler(on_render_error))
        .add_systems(Update, notice_renderer_back);
        // No render app without a renderer (a headless server).
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.edit_schedule(Render, |schedule| gate_render_sets(schedule, &LATCH));
            render_app.add_systems(RenderStartup, renderer_started);
            if let Some(delay) = test_loss_delay(std::env::var(TEST_LOSS_SWITCH).ok().as_deref()) {
                render_app
                    .insert_resource(TestLoss { delay, armed_at: None, fired: false })
                    .add_systems(Render, lose_device_for_test.in_set(RenderSystems::ExtractCommands));
            }
        }
    }
}

/// Recovery's state in the main world.
#[derive(Resource)]
pub struct GpuRecovery {
    rebuild_with: WgpuSettings,
    may_rebuild: bool,
    close_when_given_up: Option<fn(&mut World)>,
    state: LossState,
    probe: DriverProbe,
    last: Option<Verdict>,
    startups_at_recover: Option<u64>,
}

fn renderer_started() {
    LATCH.renderer_started();
}

/// Bevy calls this for every render error, with the main world and then the
/// render world, and again every frame while the renderer is not drawing.
fn on_render_error(error: &RenderError, main: &mut World, render: &mut World) -> RenderErrorPolicy {
    let Some(mut recovery) = main.remove_resource::<GpuRecovery>() else {
        return (RenderErrorHandler::default().0)(error, main, render);
    };
    let now = Instant::now();
    let fault = Fault::of(error.ty, &error.description);
    if fault == Fault::DeviceLost {
        // Also when the log layer is missing: the next frame draws nothing.
        LATCH.mark_lost();
    }
    let hosting = main.contains_resource::<HostSession>();
    let situation = Situation {
        hosting,
        keep_app: recovery.close_when_given_up.is_none(),
        can_rebuild: recovery.may_rebuild,
    };
    let adapter = main.get_resource::<RenderAdapter>().cloned();
    let verdict = {
        let GpuRecovery { state, probe, .. } = &mut recovery;
        decide(state, fault, now, situation, || probe.ready(adapter.as_ref(), now))
    };
    let first = recovery.last != Some(verdict);
    recovery.last = Some(verdict);
    let still_running = if hosting {
        "the simulation, scripts and the hosted session keep running"
    } else {
        "the simulation and scripts keep running"
    };
    let policy = match verdict {
        Verdict::Wait => {
            if first {
                warn!(
                    kind = ?error.ty,
                    reason = %error.description,
                    "The graphics driver reset (GPU device lost). Drawing is paused and {still_running}; the renderer is rebuilt once the driver is back."
                );
            }
            set_health(main, RendererHealth::Recovering);
            RenderErrorPolicy::StopRendering
        }
        Verdict::Recover => {
            warn!(
                "Rebuilding the renderer after the graphics driver reset ({} of {MAX_RECOVERIES} allowed in {} minutes).",
                recovery.state.recoveries_in_window(now),
                RECOVERY_WINDOW.as_secs() / 60
            );
            recovery.probe = DriverProbe::default();
            recovery.startups_at_recover = Some(LATCH.startups());
            set_health(main, RendererHealth::Recovering);
            RenderErrorPolicy::Recover(RenderCreation::Automatic(Box::new(recovery.rebuild_with.clone())))
        }
        Verdict::StopDrawing => {
            if first {
                error!(
                    kind = ?error.ty,
                    reason = %error.description,
                    "Drawing stops for this session after a render error; {still_running}."
                );
            }
            set_health(main, RendererHealth::Stopped);
            RenderErrorPolicy::StopRendering
        }
        Verdict::Close { after_loss } => {
            if first {
                error!(kind = ?error.ty, reason = %error.description, "Closing after a render error.");
                if after_loss {
                    if let Some(tell) = recovery.close_when_given_up {
                        tell(main);
                    }
                }
                main.write_message(AppExit::error());
            }
            set_health(main, RendererHealth::Stopped);
            RenderErrorPolicy::StopRendering
        }
    };
    main.insert_resource(recovery);
    policy
}

fn set_health(main: &mut World, health: RendererHealth) {
    if let Some(mut current) = main.get_resource_mut::<RendererHealth>() {
        current.set_if_neq(health);
    }
}

/// After a rebuild, `RenderStartup` has run on the new device: drawing again.
fn notice_renderer_back(recovery: Option<ResMut<GpuRecovery>>, health: Option<ResMut<RendererHealth>>) {
    let (Some(mut recovery), Some(mut health)) = (recovery, health) else { return };
    let Some(at) = recovery.startups_at_recover else { return };
    if LATCH.startups() <= at {
        return;
    }
    recovery.startups_at_recover = None;
    recovery.last = None;
    if *health == RendererHealth::Recovering {
        *health = RendererHealth::Drawing;
        info!("The renderer is rebuilt and drawing again.");
    }
}

// ============================================================================
// Is the driver back? A test device on its own thread
// ============================================================================

const PROBE_IDLE: u8 = 0;
const PROBE_RUNNING: u8 = 1;
const PROBE_READY: u8 = 2;
const PROBE_FAILED: u8 = 3;

/// Bevy's rebuild panics when the driver gives it no adapter or no device
/// (`renderer/mod.rs`: `expect(GPU_NOT_FOUND_ERROR_MESSAGE)`, then
/// `request_device(..).await.unwrap()`), and it runs on the main thread, so a
/// rebuild asked for mid-reset would close the app. A test device made first
/// on the adapter the app used says the driver accepts new devices.
#[derive(Default)]
struct DriverProbe {
    state: Arc<AtomicU8>,
    started: Option<Instant>,
}

impl DriverProbe {
    /// Whether the driver made a test device. Starts one when none is running.
    fn ready(&mut self, adapter: Option<&RenderAdapter>, now: Instant) -> bool {
        // Nothing to test with: Bevy's rebuild is the test.
        let Some(adapter) = adapter else { return true };
        match self.state.load(Ordering::Acquire) {
            PROBE_READY => true,
            PROBE_RUNNING => false,
            PROBE_FAILED if self.started.is_some_and(|t| now.saturating_duration_since(t) < PROBE_RETRY) => false,
            _ => {
                self.start(adapter.clone(), now);
                false
            }
        }
    }

    fn start(&mut self, adapter: RenderAdapter, now: Instant) {
        self.started = Some(now);
        let state = self.state.clone();
        state.store(PROBE_RUNNING, Ordering::Release);
        let spawned = std::thread::Builder::new().name("gpu-driver-probe".into()).spawn(move || {
            let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                bevy::tasks::block_on(adapter.request_device(&Default::default())).is_ok()
            }))
            .unwrap_or(false);
            state.store(if made { PROBE_READY } else { PROBE_FAILED }, Ordering::Release);
        });
        if spawned.is_err() {
            // No thread for the test: let Bevy's rebuild be the test.
            self.state.store(PROBE_READY, Ordering::Release);
        }
    }
}

// ============================================================================
// A loss on purpose, to test recovery without resetting the driver
// ============================================================================

/// Set to a number of seconds, the app destroys its own GPU device that long
/// after its renderer first draws, once per run: the same path a driver reset
/// takes through Bevy and this module, without touching the driver or any
/// other app on the GPU.
pub const TEST_LOSS_SWITCH: &str = "EUSTRESS_GPU_TEST_LOSS";

/// The delay [`TEST_LOSS_SWITCH`] asks for: a finite number of seconds above
/// zero, else none.
pub fn test_loss_delay(value: Option<&str>) -> Option<Duration> {
    let seconds: f64 = value?.trim().parse().ok()?;
    (seconds.is_finite() && seconds > 0.0).then(|| Duration::from_secs_f64(seconds))
}

/// The armed test loss, in the render world.
#[derive(Resource)]
struct TestLoss {
    delay: Duration,
    armed_at: Option<Instant>,
    fired: bool,
}

/// Runs with the extracted commands, before every gated set, so the frame
/// that destroys the device is skipped from there on, as a real loss is.
fn lose_device_for_test(mut test: ResMut<TestLoss>, device: Res<RenderDevice>) {
    if test.fired {
        // A destroyed device reports its loss from a poll once its queue is
        // empty (a real loss reports at once), and with the render sets
        // skipped nothing else polls it.
        if LATCH.is_lost() {
            let _ = device.poll(PollType::Poll);
        }
        return;
    }
    let armed_at = *test.armed_at.get_or_insert_with(Instant::now);
    if armed_at.elapsed() < test.delay {
        return;
    }
    test.fired = true;
    warn!("{TEST_LOSS_SWITCH}: destroying the GPU device on purpose to test recovery");
    LATCH.mark_lost();
    device.wgpu_device().destroy();
    let _ = device.poll(PollType::Wait { submission_index: None, timeout: Some(Duration::from_secs(5)) });
}

// ============================================================================
// GPU load while a session runs on one machine
// ============================================================================

/// The fastest a simulating Studio updates while unfocused: 60 per second,
/// the hosted-session floor, which also keeps the 60 Hz simulation clock at
/// one tick per frame.
pub const SIMULATING_UNFOCUSED_MS: u64 = 16;

/// How often an app that is simulating (or hosting) updates: `None` means
/// continuously, as fast as the GPU allows. Focused, at the app's own frame
/// cap (`active_update_ms`, `0` for none). Unfocused, never uncapped: another
/// window has the user, often a Player on the same GPU.
pub fn simulating_update_interval(focused: bool, active_update_ms: u64) -> Option<Duration> {
    if focused {
        return (active_update_ms > 0).then(|| Duration::from_millis(active_update_ms));
    }
    let ms = if active_update_ms == 0 { SIMULATING_UNFOCUSED_MS } else { active_update_ms.max(SIMULATING_UNFOCUSED_MS) };
    Some(Duration::from_millis(ms))
}

/// Ray tracing's switch (`eustress_raytracing::ENV_SWITCH`), which wins over
/// the setting both apps read from the same `settings.json`.
pub const RAY_TRACING_SWITCH: &str = "EUSTRESS_RAYTRACING";

/// Whether a Player draws without ray tracing: when it joins a host on this
/// computer (they share one GPU), unless the switch was set by hand.
pub fn same_machine_ray_tracing_off(joining_this_computer: bool, switch_set: bool) -> bool {
    joining_this_computer && !switch_set
}

/// For a Player's `main`, before its renderer is made: a Player joining a host
/// on this computer draws without ray tracing. Says whether it did.
pub fn lighten_same_machine_player(joining_this_computer: bool) -> bool {
    let off = same_machine_ray_tracing_off(joining_this_computer, std::env::var_os(RAY_TRACING_SWITCH).is_some());
    if off {
        std::env::set_var(RAY_TRACING_SWITCH, "off");
    }
    off
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::log::tracing_subscriber::layer::SubscriberExt;
    use bevy::log::tracing_subscriber::registry::Registry;
    use std::cell::Cell;

    const STUDIO: Situation = Situation { hosting: false, keep_app: true, can_rebuild: true };
    const PLAYER: Situation = Situation { hosting: false, keep_app: false, can_rebuild: true };
    const HOST: Situation = Situation { hosting: true, keep_app: false, can_rebuild: true };

    /// Lose the device at `at` and drive it to a rebuild, with a driver that is back.
    fn lose_and_rebuild(state: &mut LossState, at: Instant, situation: Situation) -> Verdict {
        let first = decide(state, Fault::DeviceLost, at, situation, || true);
        if first != Verdict::Wait {
            return first;
        }
        decide(state, Fault::DeviceLost, at + SETTLE, situation, || true)
    }

    // Catches: a target, level or prefix check dropped or changed in
    // `is_device_lost_line`.
    #[test]
    fn only_bevys_device_lost_line_is_recognised() {
        let line = "Caught DeviceLost error: Unknown Unexpected error variant (driver implementation is at fault)";
        assert!(is_device_lost_line(&Level::ERROR, BEVY_ERROR_TARGET, line));
        assert!(!is_device_lost_line(&Level::WARN, BEVY_ERROR_TARGET, line));
        assert!(!is_device_lost_line(&Level::ERROR, "eustress_engine::crash", line));
        assert!(!is_device_lost_line(&Level::ERROR, BEVY_ERROR_TARGET, "Caught rendering error: Validation"));
    }

    // Catches: the visitor not reading the formatted `message` field
    // (`record_debug`), or the layer never setting its latch.
    #[test]
    fn the_layer_sets_the_latch_from_bevys_own_log_line() {
        static SEEN: LossLatch = LossLatch::new();
        let subscriber = Registry::default().with(DeviceLossLayer::new(&SEEN));
        bevy::log::tracing::subscriber::with_default(subscriber, || {
            bevy::log::error!(target: "bevy_render::error_handler", "Caught rendering error: {}", "Validation");
            assert!(!SEEN.is_lost(), "an uncaptured error is not a lost device");
            let (reason, text) = ("Unknown", "Unexpected error variant (driver implementation is at fault)");
            bevy::log::error!(target: "bevy_render::error_handler", "Caught DeviceLost error: {reason:?} {text}");
        });
        assert!(SEEN.is_lost());
    }

    #[derive(Resource, Default)]
    struct Ran(Vec<&'static str>);

    #[derive(Resource)]
    struct LoseOnce(bool);

    // The root cause, as a schedule: the loss is noticed inside
    // `PrepareViews` (prepare_windows), and the `Prepare` systems that
    // panicked on 11-18 must not run in that frame.
    // Catches: `gate_render_sets` not called, a set dropped from
    // `GATED_SETS` (Prepare, PrepareResources, PrepareBindGroups, Render),
    // one condition shared by all sets, or the latch never clearing.
    #[test]
    fn the_frame_that_loses_the_device_draws_nothing_after_it() {
        static FRAME: LossLatch = LossLatch::new();
        let mut world = World::new();
        world.init_resource::<Ran>();
        world.insert_resource(LoseOnce(true));
        let mut schedule = Render::base_schedule();
        gate_render_sets(&mut schedule, &FRAME);
        schedule.add_systems((
            (|mut ran: ResMut<Ran>| ran.0.push("extract commands")).in_set(RenderSystems::ExtractCommands),
            (|mut ran: ResMut<Ran>, mut lose: ResMut<LoseOnce>| {
                ran.0.push("prepare_windows");
                if std::mem::take(&mut lose.0) {
                    FRAME.mark_lost();
                }
            })
            .in_set(RenderSystems::PrepareViews),
            (|mut ran: ResMut<Ran>| ran.0.push("queue")).in_set(RenderSystems::Queue),
            (|mut ran: ResMut<Ran>| ran.0.push("prepare_clusters")).in_set(RenderSystems::PrepareResources),
            (|mut ran: ResMut<Ran>| ran.0.push("prepare_material_bind_groups")).in_set(RenderSystems::PrepareBindGroups),
            (|mut ran: ResMut<Ran>| ran.0.push("render")).in_set(RenderSystems::Render),
            (|mut ran: ResMut<Ran>| ran.0.push("cleanup")).in_set(RenderSystems::Cleanup),
            (|mut ran: ResMut<Ran>| ran.0.push("post cleanup")).in_set(RenderSystems::PostCleanup),
        ));

        schedule.run(&mut world);
        assert_eq!(world.resource::<Ran>().0, ["extract commands", "prepare_windows", "post cleanup"]);

        world.resource_mut::<Ran>().0.clear();
        schedule.run(&mut world);
        assert_eq!(world.resource::<Ran>().0, ["extract commands", "post cleanup"], "nothing draws until a rebuild");

        FRAME.renderer_started();
        world.resource_mut::<Ran>().0.clear();
        schedule.run(&mut world);
        assert_eq!(
            world.resource::<Ran>().0,
            [
                "extract commands",
                "prepare_windows",
                "queue",
                "prepare_clusters",
                "prepare_material_bind_groups",
                "render",
                "cleanup",
                "post cleanup"
            ],
            "a rebuilt renderer draws every set again"
        );
    }

    // Catches: no settle wait (rebuilding mid-reset), the driver test asked
    // before the loss settles, or a rebuild without the driver test.
    #[test]
    fn a_lost_device_waits_for_the_driver_then_rebuilds() {
        let start = Instant::now();
        let mut state = LossState::default();
        let asked = Cell::new(false);
        let ask = |ready: bool| {
            asked.set(true);
            ready
        };
        assert_eq!(decide(&mut state, Fault::DeviceLost, start, STUDIO, || ask(true)), Verdict::Wait);
        assert!(!asked.get(), "the driver is not asked before the loss settles");
        let settled = start + SETTLE;
        assert_eq!(decide(&mut state, Fault::DeviceLost, settled, STUDIO, || ask(false)), Verdict::Wait);
        assert!(asked.get());
        assert_eq!(decide(&mut state, Fault::DeviceLost, settled, STUDIO, || ask(true)), Verdict::Recover);
        assert_eq!(state.recoveries_in_window(settled), 1);
    }

    // Catches: an error that arrives while a loss is pending (the swap
    // chain's validation error on 11-18) or just after a rebuild being taken
    // for a new fault, which closes the app instead of finishing recovery.
    #[test]
    fn errors_while_a_loss_is_pending_belong_to_that_loss() {
        let start = Instant::now();
        let mut state = LossState::default();
        assert_eq!(decide(&mut state, Fault::DeviceLost, start, PLAYER, || true), Verdict::Wait);
        assert_eq!(decide(&mut state, Fault::Validation, start, PLAYER, || true), Verdict::Wait);
        let rebuilt = start + SETTLE;
        assert_eq!(decide(&mut state, Fault::Validation, rebuilt, PLAYER, || true), Verdict::Recover);
        let after = rebuilt + Duration::from_secs(1);
        assert_eq!(decide(&mut state, Fault::Validation, after, PLAYER, || true), Verdict::Wait, "aftermath");
        let later = after + AFTERMATH + SETTLE;
        let mut fresh = LossState::default();
        assert_eq!(
            decide(&mut fresh, Fault::Validation, later, PLAYER, || true),
            Verdict::Close { after_loss: false },
            "outside a loss, a validation error keeps the Player's old answer"
        );
    }

    // Catches: the rebuild limit dropped, a refused rebuild counted, or the
    // window never letting old rebuilds expire.
    #[test]
    fn rebuilds_are_limited_and_the_limit_resets_after_the_window() {
        let start = Instant::now();
        let mut state = LossState::default();
        for i in 0..MAX_RECOVERIES {
            let at = start + Duration::from_secs(60 * i as u64);
            assert_eq!(lose_and_rebuild(&mut state, at, PLAYER), Verdict::Recover, "rebuild {i}");
        }
        let next = start + Duration::from_secs(60 * MAX_RECOVERIES as u64);
        assert_eq!(lose_and_rebuild(&mut state, next, PLAYER), Verdict::Close { after_loss: true });
        assert_eq!(state.recoveries_in_window(next), MAX_RECOVERIES, "a refusal is not counted");

        let mut fresh = LossState::default();
        for i in 0..MAX_RECOVERIES {
            let at = start + Duration::from_secs(60 * i as u64);
            assert_eq!(lose_and_rebuild(&mut fresh, at, PLAYER), Verdict::Recover);
        }
        let past_window = start + RECOVERY_WINDOW + Duration::from_secs(60 * MAX_RECOVERIES as u64);
        assert_eq!(lose_and_rebuild(&mut fresh, past_window, PLAYER), Verdict::Recover);
    }

    // The goal of the 11-18 fix. Catches: any path to `Close` while hosting
    // (the `hosting` check dropped from the give-up, the non-loss answer, or
    // the stopped state).
    #[test]
    fn a_hosted_session_is_never_closed_by_a_render_error() {
        let start = Instant::now();
        for fault in [Fault::DeviceLost, Fault::OutOfMemory, Fault::Validation, Fault::Internal] {
            let mut state = LossState::default();
            for step in 0..200u64 {
                let at = start + Duration::from_secs(step * 7);
                let verdict = decide(&mut state, fault, at, HOST, || step % 3 == 0);
                assert!(!matches!(verdict, Verdict::Close { .. }), "{fault:?} at step {step} closed a hosted session");
            }
        }
        // Drawing stopped, then the Player starts hosting: it stays open.
        let mut state = LossState::default();
        assert_eq!(decide(&mut state, Fault::OutOfMemory, start, PLAYER, || true), Verdict::Close { after_loss: false });
        assert_eq!(decide(&mut state, Fault::OutOfMemory, start, HOST, || true), Verdict::StopDrawing);
    }

    // Catches: `keep_app` ignored for a render error that is not a lost
    // device. Studio and a Player sharing one GPU run it out of memory before
    // the driver gives the device up, and an out-of-memory error then closed
    // Studio with its unsaved work.
    #[test]
    fn studio_stays_open_on_a_render_error_that_is_not_a_loss() {
        let start = Instant::now();
        for fault in [Fault::OutOfMemory, Fault::Validation, Fault::Internal] {
            let mut studio = LossState::default();
            assert_eq!(decide(&mut studio, fault, start, STUDIO, || true), Verdict::StopDrawing, "{fault:?} closed Studio");
            let again = start + Duration::from_secs(1);
            assert_eq!(decide(&mut studio, fault, again, STUDIO, || true), Verdict::StopDrawing, "{fault:?}, the second time");
            let mut player = LossState::default();
            assert_eq!(
                decide(&mut player, fault, start, PLAYER, || true),
                Verdict::Close { after_loss: false },
                "{fault:?}: a Player that hosts nothing still closes"
            );
        }
    }

    // Catches: `keep_app` ignored (Studio closing after repeated losses), or
    // the Player no longer closing when it hosts nothing.
    #[test]
    fn giving_up_keeps_studio_open_and_closes_a_player_that_hosts_nothing() {
        let start = Instant::now();
        for (situation, expected) in [(STUDIO, Verdict::StopDrawing), (PLAYER, Verdict::Close { after_loss: true })] {
            let mut state = LossState::default();
            for i in 0..MAX_RECOVERIES {
                lose_and_rebuild(&mut state, start + Duration::from_secs(60 * i as u64), situation);
            }
            let next = start + Duration::from_secs(60 * MAX_RECOVERIES as u64);
            assert_eq!(lose_and_rebuild(&mut state, next, situation), expected);
            assert!(state.is_stopped());
            assert_eq!(decide(&mut state, Fault::DeviceLost, next + SETTLE, situation, || true), expected, "stays stopped");
        }
    }

    // Catches: waiting forever on a driver that never comes back, or giving
    // up before the wait limit.
    #[test]
    fn a_driver_that_never_returns_ends_recovery_after_the_wait_limit() {
        let start = Instant::now();
        let mut state = LossState::default();
        assert_eq!(decide(&mut state, Fault::DeviceLost, start, STUDIO, || false), Verdict::Wait);
        let almost = start + DRIVER_WAIT_LIMIT - Duration::from_millis(1);
        assert_eq!(decide(&mut state, Fault::DeviceLost, almost, STUDIO, || false), Verdict::Wait);
        assert_eq!(decide(&mut state, Fault::DeviceLost, start + DRIVER_WAIT_LIMIT, STUDIO, || false), Verdict::StopDrawing);
    }

    // Catches: asking Bevy to rebuild from settings without backends, which
    // fails its `assert!` (`error_handler.rs:62`) on the main thread.
    #[test]
    fn no_rebuild_without_backends() {
        let mut state = LossState::default();
        let situation = Situation { can_rebuild: false, ..HOST };
        assert_eq!(decide(&mut state, Fault::DeviceLost, Instant::now(), situation, || true), Verdict::StopDrawing);
    }

    // Catches: the switch ignored, read backwards, or overriding missing
    // backends.
    #[test]
    fn the_rebuild_switch_turns_rebuilding_off() {
        assert!(rebuild_allowed(true, None));
        assert!(rebuild_allowed(true, Some("1")));
        for off in ["0", "off", " OFF ", "false", "no"] {
            assert!(!rebuild_allowed(true, Some(off)), "{off:?}");
        }
        assert!(!rebuild_allowed(false, None), "no backends, no rebuild");
    }

    // Catches: a description that names a lost device read as an ordinary
    // validation error, or the reverse.
    #[test]
    fn errors_are_read_as_a_lost_device_by_type_or_by_text() {
        assert_eq!(Fault::of(ErrorType::DeviceLost, ""), Fault::DeviceLost);
        assert_eq!(Fault::of(ErrorType::Validation, "Parent device is lost"), Fault::DeviceLost);
        assert_eq!(Fault::of(ErrorType::Internal, "driver implementation is at fault"), Fault::DeviceLost);
        assert_eq!(Fault::of(ErrorType::Validation, "Buffer label is invalid"), Fault::Validation);
        assert_eq!(Fault::of(ErrorType::OutOfMemory, ""), Fault::OutOfMemory);
        assert_eq!(Fault::of(ErrorType::Internal, "shader"), Fault::Internal);
    }

    // Catches: a simulating app left uncapped while unfocused (the 11-18
    // Studio drew as fast as the GPU allowed beside a Player on that GPU), or
    // the focused cap ignored.
    #[test]
    fn a_simulating_app_is_never_uncapped_while_unfocused() {
        assert_eq!(simulating_update_interval(true, 8), Some(Duration::from_millis(8)));
        assert_eq!(simulating_update_interval(true, 0), None, "0 keeps the focused app uncapped, as asked");
        assert_eq!(simulating_update_interval(false, 0), Some(Duration::from_millis(SIMULATING_UNFOCUSED_MS)));
        assert_eq!(simulating_update_interval(false, 8), Some(Duration::from_millis(SIMULATING_UNFOCUSED_MS)));
        assert_eq!(simulating_update_interval(false, 33), Some(Duration::from_millis(33)), "a lower cap still holds");
    }

    // Catches: the test loss armed by an empty, zero, negative or unreadable
    // value (a stray variable must never destroy the device).
    #[test]
    fn the_test_loss_is_armed_only_by_a_positive_number_of_seconds() {
        assert_eq!(test_loss_delay(Some("30")), Some(Duration::from_secs(30)));
        assert_eq!(test_loss_delay(Some(" 2.5 ")), Some(Duration::from_millis(2500)));
        for off in [None, Some(""), Some("0"), Some("-5"), Some("soon"), Some("inf"), Some("NaN")] {
            assert_eq!(test_loss_delay(off), None, "{off:?}");
        }
    }

    // Catches: ray tracing turned off for a remote host, or a hand-set switch
    // overridden.
    #[test]
    fn only_a_player_on_the_hosts_computer_drops_ray_tracing() {
        assert!(same_machine_ray_tracing_off(true, false));
        assert!(!same_machine_ray_tracing_off(false, false));
        assert!(!same_machine_ray_tracing_off(true, true));
    }

    #[test]
    fn the_notice_is_plain_and_absent_while_drawing() {
        assert_eq!(health_notice(RendererHealth::Drawing), None);
        assert_eq!(health_notice(RendererHealth::Recovering), Some("The graphics driver reset. Recovering..."));
        assert!(health_notice(RendererHealth::Stopped).is_some_and(|n| n.contains("keeps running")));
    }
}
