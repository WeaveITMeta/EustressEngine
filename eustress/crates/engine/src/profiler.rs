//! Opt-in per-system frame micro-profiler.
//!
//! Goal: find what is eating the frame budget when a very large scene
//! (hundreds of thousands of entities) is loaded, by attributing wall-clock
//! time to individual Bevy **systems** across every schedule.
//!
//! ## How it works
//!
//! Bevy wraps each system's execution in a `tracing` span named `"system"`
//! (and `"system_commands"` for the deferred-`Commands` apply phase), each
//! carrying a `name` field holding the system's full path. Those spans only
//! exist when `bevy_ecs` is built with its `trace` feature — which is why the
//! engine's `profiling` feature turns on `bevy/trace` (see `Cargo.toml`).
//!
//! We install a [`tracing_subscriber::Layer`] into the SAME global subscriber
//! that Bevy's `LogPlugin` builds (via its `custom_layer` hook), so we share
//! one dispatcher with the whole process — including the render sub-app, whose
//! extract / prepare / queue systems run on the same global `tracing`
//! dispatcher and therefore flow through this layer too.
//!
//! For each `"system"` span the layer records `enter` → `exit` wall time and
//! accumulates, per system name, a running `(total Duration, call count)` over
//! a rolling window of N frames. Once the window closes (or a one-shot capture
//! fires after the scene settles) it writes two artifacts to the current
//! working directory:
//!
//! * `eustress_profile.txt` — a ranked table (rank, system, total ms over the
//!   window, avg ms/frame, % of mean frame time, call count) for an AI/human
//!   to read. The top 20 are also logged at `warn!`/`info!` so they land in
//!   captured stdout.
//! * `eustress_profile.svg` — an `inferno` flamegraph built from one folded
//!   stack line per system (`system_name total_micros`), for a human to open.
//!
//! ## Two tools, two build costs
//!
//! * **Phase profiler (always compiled, the default):** needs no `tracing`
//!   spans, so it is in every build (debug / release / `run-studio`) and adds
//!   only a handful of cheap systems per frame. Dormant until `EUSTRESS_PROFILE`
//!   is set. Run the ordinary binary with that env var — no feature flag, no
//!   Bevy rebuild, no build thrash. Attributes the frame to its six top-level
//!   phases; the dominant phase is the bottleneck's location.
//! * **Per-system trace layer (feature `profiling`, opt-in deep dive):** the
//!   `mod enabled` block below. This is the ONLY path that enables
//!   `bevy_ecs/trace`, which recompiles the whole Bevy stack — use it
//!   deliberately, ideally with its own `--target-dir`.
//!
//! ## Cost when off
//!
//! * Feature `profiling` **off** (default): the per-system layer is absent and
//!   `bevy_ecs/trace` is not enabled, so Bevy never constructs the system
//!   spans. The phase profiler is present but dormant — one `OnceLock` read per
//!   marker system per frame when `EUSTRESS_PROFILE` is unset.
//! * Feature **on** but env `EUSTRESS_PROFILE` **unset**: the layer is
//!   installed but every callback early-returns on an atomic load, and the
//!   per-layer filter rejects all callsites, so the practical cost is a single
//!   relaxed atomic read on the system-span callsite path.
//! * Feature **on** + `EUSTRESS_PROFILE=1`: full capture + periodic dump.
//!
//! Env knobs:
//! * `EUSTRESS_PROFILE` — set to any non-empty value to arm capture.
//! * `EUSTRESS_PROFILE_FRAMES` — window length in frames (default 120).

// The always-on phase profiler below uses the Bevy prelude and std timing in
// every build, so these imports are unconditional. The per-system trace layer
// keeps its own imports inside `mod enabled` (feature-gated).
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Bevy plugin that arms the per-system profiler.
///
/// When the `profiling` feature is disabled this is an inert marker whose
/// `build` does nothing — added unconditionally in `main.rs` so the call site
/// never needs its own `#[cfg]`.
pub struct ProfilerPlugin;

// ───────────────────────── always-on phase profiler ─────────────────────
// Coarse, zero-rebuild companion to the per-system trace layer. It needs NO
// `tracing` spans and therefore NO `bevy_ecs/trace`, so it compiles into EVERY
// build and adds only a few cheap systems per frame. Capture stays dormant
// until `EUSTRESS_PROFILE` is set (guarded by a single `OnceLock<bool>` read).
//
// It attributes wall-clock to the six top-level frame phases by stamping an
// `Instant` as each main-world schedule begins (First → PreUpdate → Update →
// PostUpdate → Last) plus the residual gap between one frame's `Last` and the
// next frame's `First`, which captures the render sub-app (extract / prepare /
// queue / draw) and present + vsync wait. The six phases sum to the full frame
// period, so the largest share *is* the bottleneck's location:
//   * 06_render+present dominates → GPU / draw-call / present bound
//   * 04_PostUpdate dominates     → transform propagation + visibility (O(N))
//   * 03_Update dominates         → game / physics / UI systems
// Output: `eustress_profile_phases.txt` (ranked) in the working directory,
// also echoed to the log.

/// Read `EUSTRESS_PROFILE` exactly once; capture is armed iff it is non-empty.
pub(crate) fn phase_armed() -> bool {
    static ARMED: OnceLock<bool> = OnceLock::new();
    *ARMED.get_or_init(|| {
        std::env::var_os("EUSTRESS_PROFILE")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    })
}

/// Phase-profiler window length in frames (env `EUSTRESS_PROFILE_FRAMES`,
/// default 1 — dump every frame, ideal when a single frame already costs
/// seconds).
pub(crate) fn phase_window() -> u64 {
    static W: OnceLock<u64> = OnceLock::new();
    *W.get_or_init(|| {
        std::env::var("EUSTRESS_PROFILE_FRAMES")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(1)
    })
}

// ── Render thread, extract and main-thread CPU ──────────────────────────
//
// With pipelined rendering the frame period is the longer of two things that
// run side by side, plus the extract step that hands data from one to the
// other: the main world (First to Last) and the render thread (the render
// world's schedules). The phases above time the main world only, and
// `06_render+present+wait` is extract plus any time the main thread spent
// waiting for the render thread to hand the render world back. These counters
// split that phase and time the render thread itself, so every window states
// which side bounds the frame. None of it is installed unless
// `EUSTRESS_PROFILE` is set.
//
// CPU time comes from `QueryThreadCycleTime` (Windows): the cycles a thread
// actually ran. It leaves out time the thread sat blocked (on the GPU, a
// channel or a lock) and does not grow when another process takes the core,
// so it stays comparable while a build runs beside the engine.

/// Render-thread wall time, CPU cycles, runs and shadow views over the
/// window, added by `render_run_end` and read by `dump_phases`.
static RENDER_WALL_NS: AtomicU64 = AtomicU64::new(0);
static RENDER_CYCLES: AtomicU64 = AtomicU64::new(0);
static RENDER_RUNS: AtomicU64 = AtomicU64::new(0);
static RENDER_SHADOW_VIEWS: AtomicU64 = AtomicU64::new(0);
/// Time in the render world's extract function (main thread).
static EXTRACT_NS: AtomicU64 = AtomicU64::new(0);
/// Time in the pipelined hand-off: waiting for the render world to come
/// back from the render thread, then extract.
static HANDOFF_NS: AtomicU64 = AtomicU64::new(0);

/// Duplicated handle to the thread that runs the main world (Windows).
static MAIN_THREAD_HANDLE: OnceLock<usize> = OnceLock::new();

/// Cycles the main-world thread has run so far, when known.
fn main_thread_cycles() -> Option<u64> {
    MAIN_THREAD_HANDLE.get().and_then(|h| thread_cycles::of(*h))
}

/// Per-thread CPU cycles through `QueryThreadCycleTime`; `None` off Windows.
mod thread_cycles {
    #[cfg(windows)]
    mod sys {
        use std::ffi::c_void;

        #[link(name = "kernel32")]
        extern "system" {
            pub fn GetCurrentThread() -> *mut c_void;
            pub fn GetCurrentProcess() -> *mut c_void;
            pub fn QueryThreadCycleTime(thread: *mut c_void, cycles: *mut u64) -> i32;
            pub fn DuplicateHandle(
                source_process: *mut c_void,
                source: *mut c_void,
                target_process: *mut c_void,
                target: *mut *mut c_void,
                desired_access: u32,
                inherit: i32,
                options: u32,
            ) -> i32;
        }

        pub const DUPLICATE_SAME_ACCESS: u32 = 2;
    }

    /// Cycles the calling thread has run so far.
    #[cfg(windows)]
    pub fn current() -> Option<u64> {
        let mut cycles = 0u64;
        // SAFETY: the pseudo-handle names the calling thread and is always
        // valid; `cycles` is a valid out-pointer.
        let ok = unsafe { sys::QueryThreadCycleTime(sys::GetCurrentThread(), &mut cycles) };
        (ok != 0).then_some(cycles)
    }

    #[cfg(not(windows))]
    pub fn current() -> Option<u64> {
        None
    }

    /// A real handle to the calling thread that any thread can query. The
    /// pseudo-handle means "whichever thread asks", so it cannot be shared.
    /// The duplicate is never closed; it lives as long as the process.
    #[cfg(windows)]
    pub fn handle_to_current() -> Option<usize> {
        let mut handle: *mut std::ffi::c_void = std::ptr::null_mut();
        // SAFETY: both pseudo-handles are valid in the calling process and
        // thread, and `handle` is a valid out-pointer.
        let ok = unsafe {
            let process = sys::GetCurrentProcess();
            sys::DuplicateHandle(
                process,
                sys::GetCurrentThread(),
                process,
                &mut handle,
                0,
                0,
                sys::DUPLICATE_SAME_ACCESS,
            )
        };
        (ok != 0 && !handle.is_null()).then_some(handle as usize)
    }

    #[cfg(not(windows))]
    pub fn handle_to_current() -> Option<usize> {
        None
    }

    /// Cycles the thread behind `handle` (from `handle_to_current`) has run.
    #[cfg(windows)]
    pub fn of(handle: usize) -> Option<u64> {
        let mut cycles = 0u64;
        // SAFETY: `handle` came from `handle_to_current` and is never closed.
        let ok = unsafe { sys::QueryThreadCycleTime(handle as *mut std::ffi::c_void, &mut cycles) };
        (ok != 0).then_some(cycles)
    }

    #[cfg(not(windows))]
    pub fn of(_handle: usize) -> Option<u64> {
        None
    }

    /// Cycles per nanosecond, measured once by spinning the calling thread
    /// three times for 10 ms and keeping the highest rate (a spin that lost
    /// the core reads low).
    pub fn per_ns() -> Option<f64> {
        static RATE: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
        *RATE.get_or_init(|| {
            let mut best: Option<f64> = None;
            for _ in 0..3 {
                let c0 = current()?;
                let t0 = std::time::Instant::now();
                while t0.elapsed() < std::time::Duration::from_millis(10) {
                    std::hint::spin_loop();
                }
                let c1 = current()?;
                let ns = t0.elapsed().as_nanos() as f64;
                let rate = c1.saturating_sub(c0) as f64 / ns;
                best = Some(best.map_or(rate, |b| b.max(rate)));
            }
            best
        })
    }
}

/// Runs just before the render world's schedules, on the render thread.
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
struct RenderTimingBegin;

/// Runs just after the render world's schedules, on the render thread.
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
struct RenderTimingEnd;

/// When the render thread began its current run (render world).
#[derive(Resource, Default)]
struct RenderRunStart {
    at: Option<Instant>,
    cycles: Option<u64>,
}

fn render_run_begin(world: &mut World) {
    let mut start = world.resource_mut::<RenderRunStart>();
    start.at = Some(Instant::now());
    start.cycles = thread_cycles::current();
}

/// Adds this run's wall time, cycles and shadow views (the root views that
/// are not cameras: a point light's six cube faces, a spot light's one).
fn render_run_end(
    world: &mut World,
    mut shadow_views: Local<
        Option<QueryState<(), With<bevy::core_pipeline::schedule::RootNonCameraView>>>,
    >,
) {
    let now = Instant::now();
    let cycles = thread_cycles::current();
    let (at, started_cycles) = {
        let start = world.resource::<RenderRunStart>();
        (start.at, start.cycles)
    };
    if let Some(at) = at {
        RENDER_WALL_NS.fetch_add(now.saturating_duration_since(at).as_nanos() as u64, Relaxed);
    }
    if let (Some(c0), Some(c1)) = (started_cycles, cycles) {
        RENDER_CYCLES.fetch_add(c1.saturating_sub(c0), Relaxed);
    }
    RENDER_RUNS.fetch_add(1, Relaxed);
    let state = shadow_views.get_or_insert_with(|| QueryState::new(world));
    RENDER_SHADOW_VIEWS.fetch_add(state.iter(world).count() as u64, Relaxed);
}

/// Wraps both extract functions and brackets the render world's schedules.
/// Called from `ProfilerPlugin::finish`, before pipelined rendering moves the
/// render app to its own thread.
fn install_render_timing(app: &mut App) {
    use bevy::render::{Render, RenderApp, RenderScheduleOrder};

    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        // Extract itself: the render world's own extract function.
        if let Some(mut inner) = render_app.take_extract() {
            render_app.set_extract(move |main_world, render_world| {
                let t0 = Instant::now();
                inner(main_world, render_world);
                EXTRACT_NS.fetch_add(t0.elapsed().as_nanos() as u64, Relaxed);
            });
        }
        render_app.init_resource::<RenderRunStart>();
        render_app.add_systems(RenderTimingBegin, render_run_begin);
        render_app.add_systems(RenderTimingEnd, render_run_end);
        render_app.edit_schedule(RenderTimingBegin, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        render_app.edit_schedule(RenderTimingEnd, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        if let Some(mut order) = render_app.world_mut().get_resource_mut::<RenderScheduleOrder>() {
            if order.labels.contains(&Render.intern()) {
                order.insert_before(Render, RenderTimingBegin);
                order.labels.push(RenderTimingEnd.intern());
            }
        }
    }
    // The pipelined hand-off: wait for the render world, then extract.
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(extract_app) = app.get_sub_app_mut(bevy::render::pipelined_rendering::RenderExtractApp) {
        if let Some(mut inner) = extract_app.take_extract() {
            extract_app.set_extract(move |main_world, world| {
                let t0 = Instant::now();
                inner(main_world, world);
                HANDOFF_NS.fetch_add(t0.elapsed().as_nanos() as u64, Relaxed);
            });
        }
    }
}

/// Accumulates per-phase wall-clock across a rolling window of frames.
#[derive(Resource)]
struct PhaseClock {
    /// The phase currently open: its label and the `Instant` it began.
    pending: Option<(&'static str, Instant)>,
    /// Main-thread cycles when the open phase began.
    pending_cycles: Option<u64>,
    /// When the current frame's `First` ran (start of the frame period).
    frame_start: Option<Instant>,
    /// When the previous frame's `Last` finished — start of the render gap.
    prev_frame_end: Option<Instant>,
    /// Main-thread cycles when the previous frame's `Last` finished.
    prev_frame_end_cycles: Option<u64>,
    /// `phase label -> (summed duration, sample count)` over the window.
    acc: HashMap<&'static str, (Duration, u64)>,
    /// `phase label -> main-thread cycles` over the window.
    acc_cycles: HashMap<&'static str, u64>,
    /// Frames completed in the current window.
    frames: u64,
    /// Window length (frames) before a dump.
    window: u64,
}

impl PhaseClock {
    fn new(window: u64) -> Self {
        Self {
            pending: None,
            pending_cycles: None,
            frame_start: None,
            prev_frame_end: None,
            prev_frame_end_cycles: None,
            acc: HashMap::new(),
            acc_cycles: HashMap::new(),
            frames: 0,
            window,
        }
    }

    /// Add the main-thread cycles run between `since` and `now` to `label`.
    fn add_cycles(&mut self, label: &'static str, since: Option<u64>, now: Option<u64>) {
        if let (Some(c0), Some(c1)) = (since, now) {
            *self.acc_cycles.entry(label).or_insert(0) += c1.saturating_sub(c0);
        }
    }

    /// Close the open phase (attributing its elapsed time) and open `label`.
    fn mark(&mut self, label: &'static str, now: Instant) {
        let cycles = main_thread_cycles();
        if let Some((prev, started)) = self.pending.take() {
            let e = self.acc.entry(prev).or_insert((Duration::ZERO, 0));
            e.0 += now.saturating_duration_since(started);
            e.1 += 1;
            let since = self.pending_cycles;
            self.add_cycles(prev, since, cycles);
        }
        self.pending = Some((label, now));
        self.pending_cycles = cycles;
    }
}

fn phase_first(mut clock: ResMut<PhaseClock>) {
    if !phase_armed() {
        return;
    }
    let now = Instant::now();
    let cycles = main_thread_cycles();
    // Gap since the previous frame's Last = render sub-app + present + vsync.
    if let Some(end) = clock.prev_frame_end.take() {
        let e = clock
            .acc
            .entry("06_render+present+wait")
            .or_insert((Duration::ZERO, 0));
        e.0 += now.saturating_duration_since(end);
        e.1 += 1;
        let since = clock.prev_frame_end_cycles.take();
        clock.add_cycles("06_render+present+wait", since, cycles);
    }
    clock.frame_start = Some(now);
    clock.pending = Some(("01_First", now));
    clock.pending_cycles = cycles;
}

fn phase_preupdate(mut clock: ResMut<PhaseClock>) {
    if phase_armed() {
        let now = Instant::now();
        clock.mark("02_PreUpdate", now);
    }
}

fn phase_update(mut clock: ResMut<PhaseClock>) {
    if phase_armed() {
        let now = Instant::now();
        clock.mark("03_Update", now);
    }
}

fn phase_postupdate(mut clock: ResMut<PhaseClock>) {
    if phase_armed() {
        let now = Instant::now();
        clock.mark("04_PostUpdate", now);
    }
}

fn phase_last(mut clock: ResMut<PhaseClock>) {
    if phase_armed() {
        let now = Instant::now();
        clock.mark("05_Last", now);
    }
}

/// Point and spot lights, for the shadow census in each window.
type CensusLights<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        Option<&'static PointLight>,
        Option<&'static SpotLight>,
        &'static ViewVisibility,
        Option<&'static crate::space::file_loader::LoadedFromFile>,
    ),
    Or<(With<PointLight>, With<SpotLight>)>,
>;

/// Parent links and names, to find the service of a light that carries no
/// `LoadedFromFile` (instances loaded from a Space's database may not).
type CensusTree<'w, 's> = Query<'w, 's, (Option<&'static ChildOf>, Option<&'static Name>)>;

/// Runs after `phase_last`: close the `Last` phase, record the frame period,
/// and dump on a window boundary.
fn phase_frame_end(mut clock: ResMut<PhaseClock>, lights: CensusLights, tree: CensusTree) {
    if !phase_armed() {
        return;
    }
    let now = Instant::now();
    let cycles = main_thread_cycles();
    if let Some((prev, started)) = clock.pending.take() {
        let e = clock.acc.entry(prev).or_insert((Duration::ZERO, 0));
        e.0 += now.saturating_duration_since(started);
        e.1 += 1;
        let since = clock.pending_cycles.take();
        clock.add_cycles(prev, since, cycles);
    }
    clock.prev_frame_end = Some(now);
    clock.prev_frame_end_cycles = cycles;
    clock.frames += 1;
    if clock.frames >= clock.window {
        let census = shadow_light_census(&lights, &tree);
        dump_phases(&mut clock, &census);
    }
}

/// Shadow-casting point and spot lights drawn this frame, by the service
/// their file loaded under. Every such point light costs six shadow views a
/// frame and every spot light one, whether or not its light reaches anything
/// the camera sees. A light without `LoadedFromFile` is counted under its
/// topmost ancestor's name, marked "by parent": storage services hide what
/// they hold only through `LoadedFromFile`, so a storage service listed that
/// way is content that escaped the hiding.
fn shadow_light_census(lights: &CensusLights, tree: &CensusTree) -> String {
    let mut by_service: std::collections::BTreeMap<String, (u32, u32)> =
        std::collections::BTreeMap::new();
    for (entity, point, spot, visibility, loaded) in lights.iter() {
        if !visibility.get() {
            continue;
        }
        let point_shadows = point.is_some_and(|p| p.shadow_maps_enabled);
        let spot_shadows = spot.is_some_and(|s| s.shadow_maps_enabled);
        if !point_shadows && !spot_shadows {
            continue;
        }
        let service = match loaded {
            Some(l) => l.service.clone(),
            None => format!("{} (by parent)", root_name(tree, entity)),
        };
        let entry = by_service.entry(service).or_insert((0, 0));
        entry.0 += point_shadows as u32;
        entry.1 += spot_shadows as u32;
    }
    if by_service.is_empty() {
        return "none".to_string();
    }
    by_service
        .iter()
        .map(|(service, (points, spots))| format!("{service}: {points} point, {spots} spot"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Name of `entity`'s topmost ancestor: the service, for a loaded instance.
fn root_name(tree: &CensusTree, entity: Entity) -> String {
    let mut current = entity;
    for _ in 0..256 {
        match tree.get(current) {
            Ok((Some(child_of), _)) => current = child_of.parent(),
            Ok((None, name)) => {
                return name.map_or_else(|| "(unnamed root)".to_string(), |n| n.as_str().to_string());
            }
            Err(_) => break,
        }
    }
    "(unknown)".to_string()
}

/// Write `eustress_profile_phases.txt` (ranked phases) + echo to the log,
/// then reset the window.
fn dump_phases(clock: &mut PhaseClock, light_census: &str) {
    let frames = clock.frames.max(1);
    let mut rows: Vec<(&'static str, Duration)> =
        clock.acc.iter().map(|(k, v)| (*k, v.0)).collect();
    let total: Duration = rows.iter().map(|(_, d)| *d).sum();
    let frame_ms = (total.as_secs_f64() * 1000.0) / frames as f64;
    let fps = if frame_ms > 0.0 { 1000.0 / frame_ms } else { 0.0 };
    let denom = if frame_ms > 0.0 { frame_ms } else { 1.0 };
    // Largest share first; label breaks ties for a stable ordering.
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));

    let mut text = String::new();
    text.push_str(&format!(
        "Eustress phase profile — window = {frames} frame(s), mean frame = {frame_ms:.1} ms ({fps:.2} FPS)\n"
    ));
    text.push_str("phase                       ms/frame    %frame\n");
    text.push_str("-----                       --------    ------\n");
    for (label, dur) in &rows {
        let ms = (dur.as_secs_f64() * 1000.0) / frames as f64;
        let pct = (ms / denom) * 100.0;
        text.push_str(&format!("{label:<25}  {ms:>9.1}  {pct:>6.1}%\n"));
    }
    let detail = render_and_cpu_detail(clock, &rows, frames, light_census);
    text.push('\n');
    text.push_str(&detail);
    match std::fs::write("eustress_profile_phases.txt", &text) {
        Ok(()) => info!("profiler(phase): wrote eustress_profile_phases.txt — {frame_ms:.1} ms/frame ({fps:.2} FPS)"),
        Err(e) => warn!("profiler(phase): failed writing eustress_profile_phases.txt: {e}"),
    }
    warn!("profiler(phase): frame {frame_ms:.1} ms ({fps:.2} FPS) — phase breakdown:");
    for (label, dur) in &rows {
        let ms = (dur.as_secs_f64() * 1000.0) / frames as f64;
        info!("  {label:<25} {ms:>9.1} ms  {:>5.1}%", (ms / denom) * 100.0);
    }

    for line in detail.lines() {
        info!("  {line}");
    }

    clock.acc.clear();
    clock.acc_cycles.clear();
    clock.frames = 0;
}

/// The window's render-thread, extract, main-thread CPU and bound lines,
/// plus the performance switches that were off and the shadow census.
/// Reads and resets the render-side counters.
fn render_and_cpu_detail(
    clock: &PhaseClock,
    rows: &[(&'static str, Duration)],
    frames: u64,
    light_census: &str,
) -> String {
    let frames_f = frames.max(1) as f64;
    let per_frame_ms = |ns: u64| ns as f64 / 1.0e6 / frames_f;
    let rate = thread_cycles::per_ns();
    let cycles_ms = |cycles: u64| rate.map(|r| cycles as f64 / r / 1.0e6);

    let render_runs = RENDER_RUNS.swap(0, Relaxed);
    let render_wall_ns = RENDER_WALL_NS.swap(0, Relaxed);
    let render_cycles = RENDER_CYCLES.swap(0, Relaxed);
    let shadow_views = RENDER_SHADOW_VIEWS.swap(0, Relaxed);
    let extract_ns = EXTRACT_NS.swap(0, Relaxed);
    let handoff_ns = HANDOFF_NS.swap(0, Relaxed);

    let main_world_ms: f64 = rows
        .iter()
        .filter(|(label, _)| *label != "06_render+present+wait")
        .map(|(_, d)| d.as_secs_f64() * 1000.0)
        .sum::<f64>()
        / frames_f;
    let extract_ms = per_frame_ms(extract_ns);

    let mut out = String::new();
    out.push_str("-- render thread, extract and CPU (the render thread runs beside the main world; not in the sum above) --\n");
    let render_ms = if render_runs > 0 {
        let runs = render_runs as f64;
        let wall = render_wall_ns as f64 / 1.0e6 / runs;
        out.push_str(&format!("render_thread_wall_ms     {wall:>9.1}  per run ({render_runs} runs)\n"));
        match cycles_ms(render_cycles) {
            Some(cpu) => out.push_str(&format!(
                "render_thread_cpu_ms      {:>9.1}  per run\n",
                cpu / runs
            )),
            None => out.push_str("render_thread_cpu_ms            n/a\n"),
        }
        out.push_str(&format!(
            "shadow_views              {:>9.1}  per run (point-light faces and spot lights)\n",
            shadow_views as f64 / runs
        ));
        Some(wall)
    } else {
        out.push_str("render_thread_wall_ms           n/a  (no render runs timed)\n");
        None
    };
    out.push_str(&format!("extract_ms                {extract_ms:>9.1}  per frame (main thread)\n"));
    if handoff_ns > 0 {
        let wait = per_frame_ms(handoff_ns.saturating_sub(extract_ns));
        out.push_str(&format!(
            "wait_for_render_ms        {wait:>9.1}  per frame (main thread idle until the render world came back)\n"
        ));
    }
    let mut phase_cpu: Vec<(&'static str, u64)> =
        clock.acc_cycles.iter().map(|(k, v)| (*k, *v)).collect();
    phase_cpu.sort_by(|a, b| a.0.cmp(b.0));
    let total_cycles: u64 = phase_cpu.iter().map(|(_, c)| *c).sum();
    match cycles_ms(total_cycles) {
        Some(total) => {
            let parts: Vec<String> = phase_cpu
                .iter()
                .map(|(label, c)| {
                    let short = label.split('_').next().unwrap_or(label);
                    format!("{short} {:.1}", cycles_ms(*c).unwrap_or(0.0) / frames_f)
                })
                .collect();
            out.push_str(&format!(
                "main_thread_cpu_ms        {:>9.1}  per frame ({})\n",
                total / frames_f,
                parts.join(", ")
            ));
        }
        None => out.push_str("main_thread_cpu_ms              n/a\n"),
    }
    out.push_str(&format!("main_world_ms             {main_world_ms:>9.1}  per frame (01 to 05)\n"));
    match render_ms {
        Some(render) => {
            let main_side = main_world_ms + extract_ms;
            let (bound, slack) = if main_side >= render {
                ("main", main_side - render)
            } else {
                ("render", render - main_side)
            };
            out.push_str(&format!(
                "bound                     {bound}  (main world + extract {main_side:.1} ms against render thread {render:.1} ms; the other side idles {slack:.1} ms)\n"
            ));
        }
        None => out.push_str("bound                     unknown\n"),
    }
    let off = eustress_common::utils::perf_switches_off();
    out.push_str(&format!(
        "perf_switches_off         {}\n",
        if off.is_empty() { "none".to_string() } else { off.join(",") }
    ));
    out.push_str(&format!("shadow_lights_drawn       {light_census}\n"));
    out
}

// ───────────────────────────── Bevy plugin ──────────────────────────────
impl Plugin for ProfilerPlugin {
    fn build(&self, app: &mut App) {
        // Always-on, env-gated phase profiler: one marker system at the head of
        // each main-world schedule, plus a frame-end closer in `Last` ordered
        // right after the `Last` marker.
        app.insert_resource(PhaseClock::new(phase_window()))
            .add_systems(First, phase_first)
            .add_systems(PreUpdate, phase_preupdate)
            .add_systems(Update, phase_update)
            .add_systems(PostUpdate, phase_postupdate)
            .add_systems(Last, (phase_last, phase_frame_end).chain());

        // Log any system the stall watchdog caught running long (see
        // `stall_watch`). Always on; a no-op frame costs one mutex check.
        // It runs before the frame counter advances, so it logs the frame
        // number this frame's systems saw, as the script budget line does.
        app.add_systems(Last, stall_watch::report_stalls.before(bevy::diagnostic::update_frame_count));

        // Per-script timing of the scripting runtimes (see
        // `script_recorder`). Registers nothing unless
        // `EUSTRESS_SCRIPT_PROFILE` is set.
        script_recorder::install(app);

        // Frame staggering (`eustress_common::utils::stagger`): a bulk load
        // opens every slot, so while it lasts the load throttle alone decides
        // which frames the UI syncs run on.
        eustress_common::utils::set_stagger_hold(crate::space::file_loader::bulk_load_active);

        // LOAD-PHASE milestone 7: one-shot first-rendered-frame marker.
        // Always-added, env-gated on EUSTRESS_PROFILE like the phase
        // profiler; self-latches so it logs once per load.
        app.add_systems(Update, crate::space::load_phase::sys_mark_first_render);

        // `build` runs on the thread that later runs the main world, so this
        // is the handle whose cycles the phase markers read.
        if phase_armed() {
            if let Some(handle) = thread_cycles::handle_to_current() {
                let _ = MAIN_THREAD_HANDLE.set(handle);
            }
        }

        // Opt-in per-system trace layer (feature `profiling` only) — the single
        // path that enables `bevy_ecs/trace` and therefore costs a Bevy rebuild.
        #[cfg(feature = "profiling")]
        enabled::install_trace(app);
    }

    /// Times the render thread and the extract steps (see
    /// `install_render_timing`). Only when armed, so an unprofiled run keeps
    /// Bevy's extract functions and render schedule order untouched.
    fn finish(&self, app: &mut App) {
        if phase_armed() {
            install_render_timing(app);
        }
    }
}

/// The `LogPlugin::custom_layer` hook value.
///
/// With the feature **off** this resolves to `|_| None` so the LogPlugin
/// builds exactly as before. With the feature **on** it returns our boxed,
/// self-filtered profiling layer. `main.rs` plugs this into
/// `LogPlugin { custom_layer: profiler::custom_layer, .. }` unconditionally.
#[cfg(not(feature = "profiling"))]
pub fn custom_layer(_app: &mut App) -> Option<bevy::log::BoxedLayer> {
    stall_watch::layer()
}

// ─────────────────────────── stall watchdog ────────────────────────────
/// Names the system behind a long frame.
///
/// `frame_diagnostics` reports that a frame took seven seconds; it cannot say
/// which system took them, and a one-off stall is gone by the time a
/// profiling build is running. This layer watches every Bevy system span,
/// main and render world alike, and queues any single run longer than
/// `EUSTRESS_STALL_MS` (default 250; `0` turns it off) for
/// [`stall_watch::report_stalls`] to log as `STALL frame <n>: system <name> ran N ms`.
///
/// Cheap enough to leave on: the name is captured once per system (its span
/// is created once), and each run costs two `Instant::now()` calls and a
/// push and pop on a thread-local stack. No span lock is taken unless a run
/// is actually slow. The report is logged from an ordinary system rather
/// than from inside the layer, because emitting an event from a tracing
/// callback re-enters the subscriber.
pub mod stall_watch {
    use std::cell::RefCell;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use bevy::log::tracing_subscriber::{
        filter::filter_fn,
        layer::{Context, Layer},
        registry::LookupSpan,
    };
    use bevy::log::BoxedLayer;
    use tracing::field::{Field, Visit};
    use tracing::span;

    /// Slow runs waiting to be logged, capped so a pathological frame cannot
    /// grow it without bound.
    static SLOW: Mutex<Vec<(String, Duration)>> = Mutex::new(Vec::new());
    const MAX_QUEUED: usize = 64;

    thread_local! {
        /// Open system spans on this thread and when each was entered. A
        /// stack, not a slot: an exclusive system that runs a schedule
        /// (`FixedUpdate` inside `RunFixedMainLoop`) nests system spans.
        static OPEN: RefCell<Vec<(u64, Instant)>> = const { RefCell::new(Vec::new()) };
    }

    /// `EUSTRESS_STALL_MS`, read once. `None` switches the watchdog off.
    pub fn threshold() -> Option<Duration> {
        static V: OnceLock<Option<Duration>> = OnceLock::new();
        *V.get_or_init(|| {
            let ms = std::env::var("EUSTRESS_STALL_MS")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(250);
            (ms > 0).then(|| Duration::from_millis(ms))
        })
    }

    struct SystemName(String);

    #[derive(Default)]
    struct NameVisitor {
        name: Option<String>,
    }

    impl Visit for NameVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "name" {
                self.name = Some(value.to_owned());
            }
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            if field.name() == "name" && self.name.is_none() {
                self.name = Some(format!("{value:?}").trim_matches('"').to_owned());
            }
        }
    }

    struct StallLayer {
        threshold: Duration,
    }

    impl<S> Layer<S> for StallLayer
    where
        S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    {
        fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
            let Some(span) = ctx.span(id) else { return };
            let mut visitor = NameVisitor::default();
            attrs.record(&mut visitor);
            let name = visitor.name.unwrap_or_else(|| span.metadata().name().to_owned());
            span.extensions_mut().insert(SystemName(name));
        }

        fn on_enter(&self, id: &span::Id, _ctx: Context<'_, S>) {
            let entered = (id.into_u64(), Instant::now());
            OPEN.with(|open| open.borrow_mut().push(entered));
        }

        fn on_exit(&self, id: &span::Id, ctx: Context<'_, S>) {
            let now = Instant::now();
            let key = id.into_u64();
            let started = OPEN.with(|open| {
                let mut open = open.borrow_mut();
                let at = open.iter().rposition(|(open_id, _)| *open_id == key)?;
                Some(open.remove(at).1)
            });
            let Some(started) = started else { return };
            let elapsed = now.saturating_duration_since(started);
            if elapsed < self.threshold {
                return;
            }
            let name = ctx
                .span(id)
                .and_then(|span| span.extensions().get::<SystemName>().map(|n| n.0.clone()))
                .unwrap_or_else(|| "<unnamed system>".to_owned());
            if let Ok(mut slow) = SLOW.lock() {
                if slow.len() < MAX_QUEUED {
                    slow.push((name, elapsed));
                }
            }
        }
    }

    /// The layer for `LogPlugin::custom_layer`, filtered to Bevy's `system`
    /// spans so every other span and event bypasses it.
    pub fn layer() -> Option<BoxedLayer> {
        let threshold = threshold()?;
        let layer = StallLayer { threshold }
            .with_filter(filter_fn(|meta| meta.is_span() && meta.name() == "system"));
        Some(Box::new(layer))
    }

    /// Log what the watchdog caught since the last frame, with the frame's
    /// number (`FrameCount`), so a stall lines up with the script profile's
    /// budget line for the same frame (see `script_recorder`).
    pub fn report_stalls(frame: Option<bevy::prelude::Res<bevy::diagnostic::FrameCount>>) {
        let caught = match SLOW.lock() {
            Ok(mut slow) if !slow.is_empty() => std::mem::take(&mut *slow),
            _ => return,
        };
        let frame = frame.map_or(0, |frame| frame.0);
        for (name, elapsed) in caught {
            tracing::warn!(
                "🐢 STALL frame {frame}: system {name} ran {:.0} ms",
                elapsed.as_secs_f64() * 1000.0
            );
        }
    }
}

// ─────────────────────────── script recorder ───────────────────────────
/// Per-script, per-step timing of the scripting runtimes, drawn as an inferno
/// flame graph, to find which script a slow frame spent its time in.
///
/// The runtimes report to `eustress_common::utils::script_profile`, armed by
/// `EUSTRESS_SCRIPT_PROFILE`; without it nothing here is registered. Each
/// frame, in `Last`, before the frame counter advances:
/// - the frame's tally is taken, and when its scripts took more than
///   `EUSTRESS_SCRIPT_BUDGET_PCT` percent (default 25) of the frame interval,
///   one `🧪 SCRIPTS frame N:` line names the costliest steps, with the frame
///   number a `STALL` line from the same frame carries;
/// - the tally joins the current window. A window opens with the first frame
///   that ran script work and is written `EUSTRESS_SCRIPT_PROFILE_SECS`
///   seconds later (default 60), when a Play session ends and when the app
///   exits, as folded stacks (`runtime;script;step <µs>`, plus
///   `runtime;(runtime) <µs>` for the driver time no step took) and an
///   inferno flame graph of them.
///
/// The files go to `<Universe>/.eustress/profile/` (the Universe being the
/// nearest folder above the open Space that holds a `Spaces` folder), else
/// `<Space>/.eustress/profile/`, else the per-user data folder
/// (`%LOCALAPPDATA%\Eustress\profile` on Windows), never the working
/// directory, named `<unix ms>_scripts.folded` and `<unix ms>_scripts.svg`.
/// Only the newest `KEEP_WINDOWS` windows are kept. They hold names, step
/// names, times and counts only: names arrive sanitized from `script_profile`
/// (no `;`, no control characters, at most 96 characters), and inferno
/// escapes every frame name it writes into the SVG.
///
/// The files are written on a thread of their own, so a frame never waits on
/// the disk, except at exit, when the app waits for the last window.
mod script_recorder {
    use std::borrow::Cow;
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, OnceLock};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use bevy::prelude::*;
    use eustress_common::utils::script_profile::{self, FrameTally};

    /// Windows kept in the profile folder; older ones are deleted after each write.
    pub const KEEP_WINDOWS: usize = 20;

    /// Entries named on a budget line, costliest first.
    const BUDGET_LINE_ENTRIES: usize = 5;

    /// `EUSTRESS_SCRIPT_BUDGET_PCT`, read once: the share of a frame, in
    /// percent, the scripts may take before the frame is logged.
    fn budget_pct() -> f64 {
        static PCT: OnceLock<f64> = OnceLock::new();
        *PCT.get_or_init(|| {
            std::env::var("EUSTRESS_SCRIPT_BUDGET_PCT")
                .ok()
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|pct| pct.is_finite() && *pct >= 0.0)
                .unwrap_or(25.0)
        })
    }

    /// `EUSTRESS_SCRIPT_PROFILE_SECS`, read once: the wall time one window covers.
    fn window_length() -> Duration {
        static SECS: OnceLock<u64> = OnceLock::new();
        Duration::from_secs(*SECS.get_or_init(|| {
            std::env::var("EUSTRESS_SCRIPT_PROFILE_SECS")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|secs| *secs > 0)
                .unwrap_or(60)
        }))
    }

    /// Registers the recorder when `EUSTRESS_SCRIPT_PROFILE` is set, and
    /// nothing otherwise.
    pub(super) fn install(app: &mut App) {
        if !script_profile::armed() {
            return;
        }
        app.insert_resource(ScriptRecorder::default())
            .add_systems(First, sync_space_root)
            .add_systems(Last, script_frame_end.before(bevy::diagnostic::update_frame_count));
        crate::play_mode::add_session_end_systems(app, request_flush);
        info!(
            "script profile: armed; frames whose scripts take over {}% are logged, and a flame graph is written every {} s under <Universe>/.eustress/profile/",
            budget_pct(),
            window_length().as_secs()
        );
    }

    /// One step's key in a window: runtime, script, step.
    type StepKey = (&'static str, Arc<str>, Arc<str>);

    /// The script work gathered since the last window was written.
    #[derive(Default)]
    struct Window {
        /// When the first frame with script work in this window ended.
        opened: Option<Instant>,
        /// Frames with script work in this window.
        frames: u64,
        /// Summed time and calls per step.
        steps: HashMap<StepKey, (Duration, u64)>,
        /// Summed driver time per runtime.
        drivers: HashMap<&'static str, Duration>,
        dm_reads: u64,
        dm_writes: u64,
    }

    impl Window {
        /// Adds a frame's tally. A frame with no script work leaves the window
        /// as it is, so a window's frames are the frames that ran scripts.
        fn add(&mut self, tally: &FrameTally, now: Instant) {
            if tally.is_empty() {
                return;
            }
            self.opened.get_or_insert(now);
            self.frames += 1;
            for step in &tally.steps {
                let sums = self.steps.entry((step.runtime, step.script.clone(), step.step.clone())).or_default();
                sums.0 += step.total;
                sums.1 += step.calls;
            }
            for (runtime, elapsed) in &tally.drivers {
                *self.drivers.entry(*runtime).or_default() += *elapsed;
            }
            self.dm_reads += tally.dm_reads;
            self.dm_writes += tally.dm_writes;
        }

        /// Whether the window has covered its length of wall time.
        fn due(&self, now: Instant) -> bool {
            self.opened.is_some_and(|opened| now.saturating_duration_since(opened) >= window_length())
        }

        /// Empties the window into the text of its two files, stamped
        /// `stamp`. `None` when it holds no step of a microsecond or more.
        fn take_job(&mut self, now: Instant, stamp: u128, space_root: Option<PathBuf>) -> Option<WindowJob> {
            let window = std::mem::take(self);
            let opened = window.opened?;
            if window.steps.is_empty() {
                return None;
            }
            let folded = folded_stacks(&window.steps, &window.drivers);
            if folded.is_empty() {
                return None;
            }
            let frames = window.frames;
            let seconds = now.saturating_duration_since(opened).as_secs_f64();
            Some(WindowJob {
                folded,
                title: format!("Eustress scripts: {frames} frames over {seconds:.1} s"),
                subtitle: format!(
                    "Total microseconds per step over the window; divide by {frames} for one frame. DataModel: {} reads, {} writes.",
                    window.dm_reads, window.dm_writes
                ),
                stamp,
                space_root,
            })
        }
    }

    /// One window, ready to write.
    struct WindowJob {
        folded: String,
        title: String,
        subtitle: String,
        stamp: u128,
        space_root: Option<PathBuf>,
    }

    /// The recorder's state between frames.
    #[derive(Resource, Default)]
    pub struct ScriptRecorder {
        window: Window,
        /// When the previous frame's `Last` ran, for the frame interval.
        last_frame: Option<Instant>,
        /// Set when a Play session ends, so that frame writes the window.
        flush_requested: bool,
        /// The open Space, for the output folder.
        space_root: Option<PathBuf>,
        /// The last file stamp, so two windows never share a name.
        last_stamp: u128,
        /// Writer threads not yet seen to finish.
        writers: Vec<JoinHandle<()>>,
    }

    impl ScriptRecorder {
        /// Unix milliseconds, one past the last stamp when the clock has not
        /// moved on since.
        fn next_stamp(&mut self) -> u128 {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis());
            self.last_stamp = now.max(self.last_stamp + 1);
            self.last_stamp
        }

        /// Writes the window and starts a new one: on a thread of its own,
        /// or, with `wait` (the app is exiting), on this thread once the
        /// writers still running are done.
        fn flush(&mut self, now: Instant, wait: bool) {
            self.flush_requested = false;
            self.writers.retain(|writer| !writer.is_finished());
            let stamp = self.next_stamp();
            let job = self.window.take_job(now, stamp, self.space_root.clone());
            if wait {
                for writer in self.writers.drain(..) {
                    let _ = writer.join();
                }
                if let Some(job) = job {
                    write_window(job);
                }
                return;
            }
            let Some(job) = job else { return };
            match std::thread::Builder::new().name("script-profile-writer".into()).spawn(move || write_window(job)) {
                Ok(writer) => self.writers.push(writer),
                Err(e) => warn!("script profile: could not start a writer thread, so this window is lost: {e}"),
            }
        }
    }

    impl Drop for ScriptRecorder {
        // The app is closing. This also covers an exit that skips the last
        // frame's `Last` (the Slint window-close path): write what the window
        // holds and wait for every writer, since the process ends next.
        fn drop(&mut self) {
            self.flush(Instant::now(), true);
        }
    }

    /// Hands the open Space to `script_profile` (for script labels) and to
    /// the recorder (for the output folder) whenever it changes.
    fn sync_space_root(space_root: Option<Res<crate::space::SpaceRoot>>, mut recorder: ResMut<ScriptRecorder>) {
        match space_root {
            Some(root) if root.is_changed() || recorder.space_root.is_none() => {
                script_profile::set_space_root(Some(root.0.as_path()));
                recorder.space_root = Some(root.0.clone());
            }
            None if recorder.space_root.is_some() => {
                script_profile::set_space_root(None);
                recorder.space_root = None;
            }
            _ => {}
        }
    }

    /// A Play session ended: this frame's `Last` writes the window, so a
    /// window never mixes two sessions.
    fn request_flush(mut recorder: ResMut<ScriptRecorder>) {
        recorder.flush_requested = true;
    }

    /// Takes the frame's tally, logs the frame when its scripts went over
    /// budget, adds it to the window, and writes the window when it is due,
    /// when a Play session ended this frame, or when the app is exiting.
    fn script_frame_end(
        mut recorder: ResMut<ScriptRecorder>,
        frame: Option<Res<bevy::diagnostic::FrameCount>>,
        mut exit: MessageReader<AppExit>,
    ) {
        let now = Instant::now();
        let interval = recorder.last_frame.replace(now).map(|last| now.saturating_duration_since(last));
        let tally = script_profile::take_frame();
        if let Some(interval) = interval {
            let frame = frame.map_or(0, |frame| frame.0);
            if let Some(line) = budget_line(frame, &tally, interval, budget_pct()) {
                warn!("{line}");
            }
        }
        recorder.window.add(&tally, now);
        if exit.read().next().is_some() {
            recorder.flush(now, true);
        } else if recorder.flush_requested || recorder.window.due(now) {
            recorder.flush(now, false);
        }
    }

    /// Each runtime's driver time and step time in a frame.
    fn per_runtime(tally: &FrameTally) -> HashMap<&'static str, (Duration, Duration)> {
        let mut runtimes: HashMap<&'static str, (Duration, Duration)> = HashMap::new();
        for (runtime, driver) in &tally.drivers {
            runtimes.entry(*runtime).or_default().0 += *driver;
        }
        for step in &tally.steps {
            runtimes.entry(step.runtime).or_default().1 += step.total;
        }
        runtimes
    }

    /// The frame's script time: per runtime, its driver time, or the sum of
    /// its steps when that is larger (steps recorded outside any driver).
    fn script_time(tally: &FrameTally) -> Duration {
        per_runtime(tally).values().map(|(driver, steps)| (*driver).max(*steps)).sum()
    }

    /// The budget line for a frame whose scripts took more than `budget_pct`
    /// percent of `interval`, naming its costliest steps and each runtime's
    /// own time; `None` for a frame within budget.
    fn budget_line(frame: u32, tally: &FrameTally, interval: Duration, budget_pct: f64) -> Option<String> {
        if interval.is_zero() || tally.is_empty() {
            return None;
        }
        let scripts = script_time(tally);
        let pct = scripts.as_secs_f64() / interval.as_secs_f64() * 100.0;
        if pct <= budget_pct {
            return None;
        }
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        // (runtime, what, time), where `what` is "script step" or "(runtime)".
        let mut entries: Vec<(&str, String, Duration)> = tally
            .steps
            .iter()
            .map(|step| (step.runtime, format!("{} {}", step.script, step.step), step.total))
            .collect();
        for (runtime, (driver, steps)) in per_runtime(tally) {
            let own = driver.saturating_sub(steps);
            if !own.is_zero() {
                entries.push((runtime, "(runtime)".to_string(), own));
            }
        }
        entries.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| (a.0, &a.1).cmp(&(b.0, &b.1))));
        let mut line = format!("🧪 SCRIPTS frame {frame}: {:.1} ms of {:.1} ms ({pct:.0}%)", ms(scripts), ms(interval));
        for (runtime, what, time) in entries.iter().take(BUDGET_LINE_ENTRIES) {
            line.push_str(&format!(" | {} {what} {:.1}", script_profile::sanitize(runtime), ms(*time)));
        }
        if entries.len() > BUDGET_LINE_ENTRIES {
            line.push_str(&format!(" | {} more", entries.len() - BUDGET_LINE_ENTRIES));
        }
        line.push_str(&format!(" | dm reads {} writes {}", tally.dm_reads, tally.dm_writes));
        Some(line)
    }

    /// The window as folded stacks for inferno: `runtime;script;step <µs>`
    /// per step, and `runtime;(runtime) <µs>` for the driver time the
    /// runtime's steps did not take. Sorted, so a file reads the same way
    /// each time; a line under a microsecond is left out.
    fn folded_stacks(steps: &HashMap<StepKey, (Duration, u64)>, drivers: &HashMap<&'static str, Duration>) -> String {
        let mut step_time: HashMap<&'static str, Duration> = HashMap::new();
        let mut lines: Vec<String> = Vec::new();
        for ((runtime, script, step), (total, _calls)) in steps {
            *step_time.entry(*runtime).or_default() += *total;
            let micros = total.as_micros();
            if micros > 0 {
                lines.push(format!("{};{};{} {micros}", frame_name(runtime), frame_name(script), frame_name(step)));
            }
        }
        for (runtime, driver) in drivers {
            let own = driver.saturating_sub(step_time.get(runtime).copied().unwrap_or_default());
            let micros = own.as_micros();
            if micros > 0 {
                lines.push(format!("{};(runtime) {micros}", frame_name(runtime)));
            }
        }
        lines.sort();
        lines.iter().map(|line| format!("{line}\n")).collect()
    }

    /// A name as one folded frame: sanitized again (a runtime name is a code
    /// literal `script_profile` keeps as given), and never blank, since a
    /// blank frame would draw as a bar with no name.
    fn frame_name(name: &str) -> Cow<'_, str> {
        let name = script_profile::sanitize(name);
        if name.trim().is_empty() {
            Cow::Borrowed("(unnamed)")
        } else {
            name
        }
    }

    /// Renders folded stacks as an inferno flame graph. inferno XML-escapes
    /// every frame name it writes: inferno 0.11.21 writes a frame's tooltip
    /// at `src/flamegraph/mod.rs:649` and its label at
    /// `src/flamegraph/svg.rs:299`, both through quick-xml 0.26's
    /// `BytesText::new`, which escapes `<`, `>`, `&`, `'` and `"`.
    fn render_svg(folded: &str, title: &str, subtitle: &str, out: impl std::io::Write) -> Result<(), String> {
        let mut options = inferno::flamegraph::Options::default();
        options.title = title.to_string();
        options.subtitle = Some(subtitle.to_string());
        options.count_name = "µs".to_string();
        inferno::flamegraph::from_lines(&mut options, folded.lines(), out).map_err(|e| e.to_string())
    }

    /// Where profiles go: `<Universe>/.eustress/profile/` for an open Space
    /// inside a Universe, else `<Space>/.eustress/profile/`, else
    /// `<user_data>/Eustress/profile/` (`user_data` being the per-user data
    /// folder). Never the working directory: a Space path that is not
    /// absolute counts as no Space.
    fn profile_dir(space_root: Option<&Path>, user_data: Option<PathBuf>) -> Option<PathBuf> {
        if let Some(space) = space_root.filter(|space| space.is_absolute() && space.is_dir()) {
            let universe = space
                .ancestors()
                .skip(1)
                .take(16)
                .find(|dir| dir.join("Spaces").is_dir() || dir.join("spaces").is_dir());
            return Some(universe.unwrap_or(space).join(".eustress").join("profile"));
        }
        user_data.map(|dir| dir.join("Eustress").join("profile"))
    }

    /// Writes one window and prunes the old ones, logging what happened. Runs
    /// on a writer thread, or on the main thread at exit.
    fn write_window(job: WindowJob) {
        let dir = profile_dir(job.space_root.as_deref(), dirs::data_local_dir());
        match write_window_files(&job, dir) {
            Ok(svg) => info!("script profile: wrote {}", svg.display()),
            Err(e) => warn!("script profile: could not write a window: {e}"),
        }
    }

    /// Writes `<stamp>_scripts.folded` and `<stamp>_scripts.svg` into `dir`,
    /// then keeps only the newest [`KEEP_WINDOWS`] windows there. Returns the
    /// SVG's path.
    fn write_window_files(job: &WindowJob, dir: Option<PathBuf>) -> Result<PathBuf, String> {
        let dir = dir.ok_or("no Space is open and there is no per-user data folder")?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let folded = dir.join(format!("{}_scripts.folded", job.stamp));
        std::fs::write(&folded, &job.folded).map_err(|e| format!("{}: {e}", folded.display()))?;
        let svg = dir.join(format!("{}_scripts.svg", job.stamp));
        let rendered = std::fs::File::create(&svg)
            .map_err(|e| e.to_string())
            .and_then(|file| render_svg(&job.folded, &job.title, &job.subtitle, std::io::BufWriter::new(file)));
        // The old windows go whether or not this one rendered.
        if let Err(e) = prune(&dir, KEEP_WINDOWS) {
            warn!("script profile: could not prune {}: {e}", dir.display());
        }
        rendered.map_err(|e| format!("{}: {e}", svg.display()))?;
        Ok(svg)
    }

    /// Deletes all but the newest `keep` windows in `dir` and returns how
    /// many files went. Only files named `<digits>_scripts.folded` or
    /// `<digits>_scripts.svg` count, a window being the files that share the
    /// digits; nothing else in the folder is touched, links and folders
    /// included.
    fn prune(dir: &Path, keep: usize) -> std::io::Result<usize> {
        let mut windows: Vec<(String, PathBuf)> = Vec::new();
        for entry in std::fs::read_dir(dir)?.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            if let Some(stamp) = entry.file_name().to_str().and_then(window_stamp) {
                windows.push((stamp.to_string(), entry.path()));
            }
        }
        let mut stamps: Vec<&str> = windows.iter().map(|(stamp, _)| stamp.as_str()).collect();
        stamps.sort_by(|a, b| newer_first(a, b));
        stamps.dedup();
        let kept: HashSet<&str> = stamps.into_iter().take(keep).collect();
        let mut removed = 0;
        for (stamp, path) in &windows {
            if kept.contains(stamp.as_str()) {
                continue;
            }
            match std::fs::remove_file(path) {
                Ok(()) => removed += 1,
                // Another writer pruned it first.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(removed)
    }

    /// The digits of a window file's name: `<digits>_scripts.folded` or
    /// `<digits>_scripts.svg`, exactly.
    fn window_stamp(name: &str) -> Option<&str> {
        let stamp = name.strip_suffix("_scripts.folded").or_else(|| name.strip_suffix("_scripts.svg"))?;
        (!stamp.is_empty() && stamp.bytes().all(|b| b.is_ascii_digit())).then_some(stamp)
    }

    /// Orders stamps newest first, comparing their values without parsing
    /// them, so any number of digits works.
    fn newer_first(a: &str, b: &str) -> std::cmp::Ordering {
        let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
        b.len().cmp(&a.len()).then_with(|| b.cmp(a))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use eustress_common::utils::script_profile::StepTotal;

        const MS: Duration = Duration::from_millis(1);

        fn step(runtime: &'static str, script: &str, step: &str, total: Duration) -> StepTotal {
            StepTotal { runtime, script: Arc::from(script), step: Arc::from(step), total, calls: 1 }
        }

        fn key(runtime: &'static str, script: &str, step: &str) -> StepKey {
            (runtime, Arc::from(script), Arc::from(step))
        }

        /// An empty folder of this test's own under the system temp folder.
        fn temp_dir(name: &str) -> PathBuf {
            // The process id keeps parallel test binaries apart, and the name
            // keeps the tests within one apart.
            let dir = std::env::temp_dir().join(format!("eustress-script-profile-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temp dir");
            dir
        }

        /// A frame where Rune's driver covers its steps and Luau ran a step
        /// with no driver of its own.
        fn busy_frame() -> FrameTally {
            FrameTally {
                steps: vec![
                    step("rune", "Workspace/Car.rune", "on_update", 6 * MS),
                    step("luau", "ServerScriptService.Playground", "Heartbeat", 3 * MS),
                    step("rune", "Workspace/Car.rune", "on_init", MS / 10),
                ],
                drivers: vec![("rune", 7 * MS)],
                dm_reads: 1520,
                dm_writes: 340,
            }
        }

        #[test]
        fn script_time_is_each_runtimes_driver_or_its_steps_whichever_is_larger() {
            assert_eq!(script_time(&busy_frame()), 10 * MS);
            assert_eq!(script_time(&FrameTally::default()), Duration::ZERO);
        }

        #[test]
        fn a_frame_over_budget_names_its_costliest_entries() {
            let line = budget_line(12345, &busy_frame(), 38 * MS, 25.0).expect("10 of 38 ms is over 25%");
            assert_eq!(
                line,
                "🧪 SCRIPTS frame 12345: 10.0 ms of 38.0 ms (26%) \
                 | rune Workspace/Car.rune on_update 6.0 \
                 | luau ServerScriptService.Playground Heartbeat 3.0 \
                 | rune (runtime) 0.9 \
                 | rune Workspace/Car.rune on_init 0.1 \
                 | dm reads 1520 writes 340"
            );
        }

        #[test]
        fn a_frame_within_budget_logs_nothing() {
            assert_eq!(budget_line(1, &busy_frame(), 40 * MS, 25.0), None, "exactly 25% is within");
            assert_eq!(budget_line(1, &busy_frame(), 100 * MS, 25.0), None);
            assert_eq!(budget_line(1, &FrameTally::default(), MS, 25.0), None);
            assert_eq!(budget_line(1, &busy_frame(), Duration::ZERO, 25.0), None);
        }

        #[test]
        fn a_budget_line_names_at_most_five_entries() {
            let steps = (0..8).map(|i| step("rune", &format!("S{i}.rune"), "on_update", (i + 1) * MS)).collect();
            let tally = FrameTally { steps, ..FrameTally::default() };
            let line = budget_line(7, &tally, 40 * MS, 25.0).unwrap();
            assert_eq!(line.matches(" on_update ").count(), 5);
            assert!(line.contains("S7.rune on_update 8.0") && !line.contains("S2.rune"), "{line}");
            assert!(line.contains(" | 3 more | dm reads 0 writes 0"), "{line}");
        }

        #[test]
        fn folded_stacks_give_each_runtime_its_own_time() {
            let steps = HashMap::from([
                (key("rune", "Workspace/Car.rune", "on_update"), (6 * MS, 60)),
                (key("rune", "Workspace/Car.rune", "on_init"), (Duration::from_nanos(500), 1)),
                (key("luau", "Playground", "Heartbeat"), (3 * MS, 60)),
            ]);
            // Luau's driver is shorter than its steps (a step outside it), so
            // it has no own time rather than a negative one.
            let drivers = HashMap::from([("rune", 7 * MS), ("luau", 2 * MS)]);
            assert_eq!(
                folded_stacks(&steps, &drivers),
                "luau;Playground;Heartbeat 3000\nrune;(runtime) 999\nrune;Workspace/Car.rune;on_update 6000\n"
            );
        }

        #[test]
        fn a_blank_name_is_still_a_named_frame() {
            let steps = HashMap::from([(key("rune", " ", "on_update"), (MS, 1))]);
            assert_eq!(folded_stacks(&steps, &HashMap::new()), "rune;(unnamed);on_update 1000\n");
        }

        /// Security's case: a label a script could choose comes out escaped in
        /// the SVG, and as one frame on one line in the folded file.
        #[test]
        fn a_hostile_label_is_escaped_in_the_svg_and_one_frame_in_the_folded_file() {
            const LABEL: &str = "<script>alert(1)</script>\"&";
            const WORSE: &str = "<script>alert(1)</script>\"&;more;frames\nmore lines";
            let ((), captured) = script_profile::capture(|| {
                script_profile::record("rune", LABEL, "on_update", 3 * MS);
                script_profile::record("rune", WORSE, "on_update", 2 * MS);
            });
            let steps: HashMap<StepKey, (Duration, u64)> = captured
                .records
                .iter()
                .map(|r| (key(r.runtime, &r.script, &r.step), (r.elapsed, 1)))
                .collect();
            let folded = folded_stacks(&steps, &HashMap::new());
            assert_eq!(
                folded,
                "rune;<script>alert(1)</script>\"&;on_update 3000\n\
                 rune;<script>alert(1)</script>\"&_more_frames_more lines;on_update 2000\n"
            );
            for line in folded.lines() {
                let (stack, count) = line.rsplit_once(' ').unwrap();
                assert!(count.parse::<u64>().is_ok(), "{line}");
                assert_eq!(stack.split(';').count(), 3, "runtime, script and step, nothing more: {line}");
            }
            let mut svg = Vec::new();
            render_svg(&folded, "Eustress scripts", "test", &mut svg).unwrap();
            let svg = String::from_utf8(svg).unwrap();
            assert!(svg.contains("&lt;script&gt;alert(1)&lt;/script&gt;&quot;&amp;"), "escaped in the SVG");
            assert!(!svg.contains("<script>alert"), "never markup in the SVG");
        }

        #[test]
        fn a_window_counts_only_frames_that_ran_scripts() {
            let mut window = Window::default();
            let t0 = Instant::now();
            window.add(&FrameTally::default(), t0);
            assert!(window.opened.is_none(), "a frame with no script work opens no window");
            let frame = FrameTally {
                steps: vec![step("rune", "Car.rune", "on_update", 2 * MS)],
                drivers: vec![("rune", 3 * MS)],
                dm_reads: 5,
                dm_writes: 1,
            };
            window.add(&frame, t0);
            window.add(&FrameTally::default(), t0 + MS);
            window.add(&frame, t0 + 2 * MS);
            assert!(!window.due(t0 + 2 * MS));
            assert!(window.due(t0 + window_length()));
            let job = window.take_job(t0 + Duration::from_secs(30), 42, None).expect("a window with steps");
            assert_eq!(job.folded, "rune;(runtime) 2000\nrune;Car.rune;on_update 4000\n");
            assert_eq!(job.title, "Eustress scripts: 2 frames over 30.0 s");
            assert!(job.subtitle.contains("divide by 2 for one frame"), "{}", job.subtitle);
            assert!(job.subtitle.contains("10 reads, 2 writes"), "{}", job.subtitle);
            assert_eq!(job.stamp, 42);
            assert!(window.opened.is_none() && window.steps.is_empty() && window.frames == 0, "emptied");
            assert!(window.take_job(t0, 43, None).is_none(), "an empty window writes nothing");
        }

        #[test]
        fn a_window_with_only_driver_time_writes_nothing() {
            let mut window = Window::default();
            window.add(&FrameTally { drivers: vec![("rune", MS)], ..FrameTally::default() }, Instant::now());
            assert!(window.take_job(Instant::now(), 1, None).is_none());
        }

        #[test]
        fn stamps_never_repeat() {
            let mut recorder = ScriptRecorder::default();
            let first = recorder.next_stamp();
            let second = recorder.next_stamp();
            assert!(second > first);
            recorder.last_stamp = u128::from(u64::MAX);
            assert_eq!(recorder.next_stamp(), u128::from(u64::MAX) + 1, "never behind the last one");
        }

        #[test]
        fn profiles_go_to_the_universe_then_the_space_then_the_user_folder() {
            let root = temp_dir("dirs");
            let universe = root.join("Universe");
            let space = universe.join("Spaces").join("Garage");
            let lone = root.join("Lone");
            std::fs::create_dir_all(&space).unwrap();
            std::fs::create_dir_all(&lone).unwrap();
            let user = || Some(root.join("user"));
            assert_eq!(profile_dir(Some(space.as_path()), user()), Some(universe.join(".eustress").join("profile")));
            assert_eq!(profile_dir(Some(lone.as_path()), user()), Some(lone.join(".eustress").join("profile")));
            let in_user = Some(root.join("user").join("Eustress").join("profile"));
            assert_eq!(profile_dir(Some(root.join("missing").as_path()), user()), in_user);
            assert_eq!(profile_dir(Some(Path::new("relative/space")), user()), in_user);
            assert_eq!(profile_dir(None, user()), in_user);
            assert_eq!(profile_dir(None, None), None, "never the working directory");
            let _ = std::fs::remove_dir_all(&root);
        }

        #[test]
        fn a_window_is_written_as_a_folded_file_and_a_flame_graph() {
            let dir = temp_dir("write");
            let job = WindowJob {
                folded: "rune;Workspace/Car.rune;on_update 4000\n".to_string(),
                title: "Eustress scripts: 2 frames over 1.0 s".to_string(),
                subtitle: "test".to_string(),
                stamp: 1_700_000_000_123,
                space_root: None,
            };
            let svg = write_window_files(&job, Some(dir.clone())).unwrap();
            assert_eq!(svg, dir.join("1700000000123_scripts.svg"));
            let folded = std::fs::read_to_string(dir.join("1700000000123_scripts.folded")).unwrap();
            assert_eq!(folded, job.folded);
            let svg = std::fs::read_to_string(&svg).unwrap();
            assert!(svg.starts_with("<?xml") && svg.contains("Workspace/Car.rune"), "a flame graph");
            assert!(svg.contains("Eustress scripts: 2 frames over 1.0 s"));
            assert!(write_window_files(&job, None).is_err(), "no folder, no file");
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn pruning_keeps_the_newest_windows_and_touches_nothing_else() {
            let dir = temp_dir("prune");
            let stamp = |i: u64| 1_700_000_000_000 + i;
            for i in 1..=25 {
                std::fs::write(dir.join(format!("{}_scripts.folded", stamp(i))), "x").unwrap();
                std::fs::write(dir.join(format!("{}_scripts.svg", stamp(i))), "x").unwrap();
            }
            // The newest window's SVG failed to render; it is a window still.
            std::fs::write(dir.join(format!("{}_scripts.folded", stamp(26))), "x").unwrap();
            let others = [
                "notes.txt",
                "12_scripts.txt",
                "x12_scripts.svg",
                "12_scripts.svg.bak",
                "_scripts.svg",
                "12_Scripts.svg",
                "1_scripts.folded.tmp",
            ];
            for name in others {
                std::fs::write(dir.join(name), "keep").unwrap();
            }
            std::fs::create_dir(dir.join("5_scripts.svg")).unwrap();

            assert_eq!(prune(&dir, KEEP_WINDOWS).unwrap(), 12, "the six oldest windows, two files each");
            for i in 1..=6 {
                assert!(!dir.join(format!("{}_scripts.folded", stamp(i))).exists(), "window {i} went");
                assert!(!dir.join(format!("{}_scripts.svg", stamp(i))).exists(), "window {i} went");
            }
            for i in 7..=25 {
                assert!(dir.join(format!("{}_scripts.folded", stamp(i))).exists(), "window {i} stays");
                assert!(dir.join(format!("{}_scripts.svg", stamp(i))).exists(), "window {i} stays");
            }
            assert!(dir.join(format!("{}_scripts.folded", stamp(26))).exists());
            for name in others {
                assert_eq!(std::fs::read_to_string(dir.join(name)).unwrap(), "keep", "{name} is untouched");
            }
            assert!(dir.join("5_scripts.svg").is_dir(), "a folder is never a window");
            assert_eq!(prune(&dir, KEEP_WINDOWS).unwrap(), 0, "nothing more to prune");
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn window_stamps_are_exactly_digits_and_the_suffix() {
            assert_eq!(window_stamp("1700000000123_scripts.svg"), Some("1700000000123"));
            assert_eq!(window_stamp("1700000000123_scripts.folded"), Some("1700000000123"));
            for name in ["_scripts.svg", "12a_scripts.svg", "12_scripts.svg.bak", "12_Scripts.svg", "12_scripts.txt"] {
                assert_eq!(window_stamp(name), None, "{name}");
            }
            let mut stamps = vec!["9", "10", "0011", "2"];
            stamps.sort_by(|a, b| newer_first(a, b));
            assert_eq!(stamps, vec!["0011", "10", "9", "2"]);
        }
    }
}

// ────────────────────────────── feature ON ──────────────────────────────
#[cfg(feature = "profiling")]
pub use enabled::custom_layer;

#[cfg(feature = "profiling")]
mod enabled {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use bevy::log::BoxedLayer;
    use bevy::prelude::*;
    // Use Bevy's own re-exported `tracing_subscriber` so the `Registry` /
    // `Layer` types are byte-identical to those `BoxedLayer` expects. Pulling
    // a second `tracing_subscriber` as a direct dep risks a version skew that
    // would make the boxed layer fail to unify with `Layer<Registry>`.
    use bevy::log::tracing_subscriber::{
        filter::filter_fn,
        layer::{Context, Layer},
        registry::LookupSpan,
    };
    // `Field`/`Visit` come from the `tracing` facade (re-exporting
    // `tracing_core::field::*`). `tracing_subscriber::field` re-exports `Visit`
    // but NOT `Field`, so pulling both from `tracing::field` keeps a single,
    // version-unified source that matches the span field types Bevy emits.
    use tracing::field::{Field, Visit};
    use tracing::span;

    /// Output file names, written to the process CWD.
    const REPORT_TXT: &str = "eustress_profile.txt";
    const REPORT_SVG: &str = "eustress_profile.svg";
    /// Default rolling window length in frames.
    const DEFAULT_WINDOW_FRAMES: u64 = 120;
    /// How many rows to put in the text/SVG report, unless
    /// `EUSTRESS_PROFILE_TOP` asks for more (the long tail of small systems
    /// can add up to a large share of the frame).
    const REPORT_TOP_N: usize = 60;
    fn report_top_n() -> usize {
        static N: OnceLock<usize> = OnceLock::new();
        *N.get_or_init(|| {
            std::env::var("EUSTRESS_PROFILE_TOP")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(REPORT_TOP_N)
        })
    }
    /// How many rows to echo into the engine log.
    const LOG_TOP_N: usize = 20;

    /// Process-global profiler state, reachable from the non-capturing
    /// `custom_layer` fn pointer (which cannot close over anything).
    static PROFILER: OnceLock<Arc<ProfilerState>> = OnceLock::new();

    /// Get-or-init the global state, reading env knobs exactly once.
    fn state() -> &'static Arc<ProfilerState> {
        PROFILER.get_or_init(|| {
            let armed = std::env::var_os("EUSTRESS_PROFILE")
                .map(|v| !v.is_empty())
                .unwrap_or(false);
            let window = std::env::var("EUSTRESS_PROFILE_FRAMES")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(DEFAULT_WINDOW_FRAMES);
            Arc::new(ProfilerState {
                armed: AtomicBool::new(armed),
                window_frames: window,
                frames_in_window: AtomicU64::new(0),
                acc: Mutex::new(HashMap::new()),
            })
        })
    }

    /// Per-system accumulation: total wall time and number of executions
    /// observed within the current window.
    #[derive(Default, Clone, Copy)]
    struct Tally {
        total: Duration,
        calls: u64,
        /// The part of `total` spent on the main thread. Systems holding a
        /// NonSend resource (Slint, the billboard atlas) all run there, one
        /// after another, so this column is the serial part of the frame.
        main: Duration,
    }

    /// The thread that builds the app and runs the main schedule.
    static MAIN_THREAD: OnceLock<std::thread::ThreadId> = OnceLock::new();

    struct ProfilerState {
        /// Whether `EUSTRESS_PROFILE` armed capture. When false every hot-path
        /// callback returns immediately.
        armed: AtomicBool,
        /// Rolling window length in frames.
        window_frames: u64,
        /// Frames elapsed in the current window (bumped by the Bevy `Last`
        /// system; the layer never touches this).
        frames_in_window: AtomicU64,
        /// `system name -> (total, calls)` for the current window. Written
        /// from many system threads on every `on_exit`, drained on dump.
        acc: Mutex<HashMap<String, Tally>>,
    }

    impl ProfilerState {
        #[inline]
        fn is_armed(&self) -> bool {
            self.armed.load(Ordering::Relaxed)
        }
    }

    // Span-extension payloads. We stash the system name (captured from the
    // span's `name` field at creation) and the most-recent enter `Instant`
    // directly on the span via the registry's typed extension map, so the
    // hot path is just a typed get/insert — no name re-formatting per frame.
    struct SystemName(String);
    struct EnterAt(Instant);

    /// Visitor that lifts the `name` field out of a system span's attributes.
    /// Bevy records it as `name = <string>`; depending on the call site that
    /// arrives as either a string or a `Debug` value, so handle both.
    #[derive(Default)]
    struct NameVisitor {
        name: Option<String>,
    }

    impl Visit for NameVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "name" {
                self.name = Some(value.to_owned());
            }
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            if field.name() == "name" && self.name.is_none() {
                // Trim the surrounding quotes a `Debug` string would add.
                let s = format!("{value:?}");
                let s = s.trim_matches('"').to_owned();
                self.name = Some(s);
            }
        }
    }

    /// The profiling [`Layer`]. Holds an `Arc` to the global state so its
    /// callbacks can accumulate without going through the `OnceLock` each
    /// time. Construction is the only place that reaches the static.
    struct SystemTimingLayer {
        state: Arc<ProfilerState>,
    }

    impl<S> Layer<S> for SystemTimingLayer
    where
        S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    {
        fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
            if !self.state.is_armed() {
                return;
            }
            let Some(span) = ctx.span(id) else { return };
            let mut visitor = NameVisitor::default();
            attrs.record(&mut visitor);
            // Fall back to the span's static metadata name if the field was
            // absent for some reason — never panic, never skip silently.
            let mut name = visitor
                .name
                .unwrap_or_else(|| span.metadata().name().to_owned());
            // Bevy emits TWO spans per system per frame that carry the SAME
            // `name` field: `"system"` (the system body) and
            // `"system_commands"` (the deferred-`Commands` apply phase).
            // Tallying both under one key made every Commands-using system
            // report 2× calls/frame (e.g. 240 calls over a 120-frame window)
            // and folded apply-deferred time into the system's own time —
            // which read as "this system is registered twice". Disambiguate
            // so the report shows the body and the command-apply phase as
            // separate rows with honest call counts.
            if span.metadata().name() == "system_commands" {
                name.push_str(" [apply_deferred]");
            }
            span.extensions_mut().insert(SystemName(name));
        }

        fn on_enter(&self, id: &span::Id, ctx: Context<'_, S>) {
            if !self.state.is_armed() {
                return;
            }
            if let Some(span) = ctx.span(id) {
                // Overwrite any previous enter stamp: a system span is only
                // entered on one thread at a time, so the last enter wins and
                // pairs with the next exit.
                span.extensions_mut().replace(EnterAt(Instant::now()));
            }
        }

        fn on_exit(&self, id: &span::Id, ctx: Context<'_, S>) {
            if !self.state.is_armed() {
                return;
            }
            let now = Instant::now();
            let Some(span) = ctx.span(id) else { return };
            let ext = span.extensions();
            let Some(EnterAt(started)) = ext.get::<EnterAt>() else {
                return;
            };
            let elapsed = now.saturating_duration_since(*started);
            // Resolve the system name captured at span creation.
            let name: String = match ext.get::<SystemName>() {
                Some(SystemName(n)) => n.clone(),
                None => span.metadata().name().to_owned(),
            };
            drop(ext);

            if let Ok(mut map) = self.state.acc.lock() {
                let tally = map.entry(name).or_default();
                tally.total += elapsed;
                tally.calls += 1;
                if MAIN_THREAD.get() == Some(&std::thread::current().id()) {
                    tally.main += elapsed;
                }
            }
        }
    }

    /// Build the boxed, self-filtered profiling layer for `LogPlugin`.
    ///
    /// The per-layer `filter_fn` restricts this layer to the two Bevy system
    /// span names so it ignores every other span/event regardless of the
    /// global `EnvFilter`, and so disabled-callsite caching keeps the cost on
    /// unrelated spans at zero. When capture is not armed the filter rejects
    /// even the system spans, collapsing the layer to a no-op.
    pub fn custom_layer(_app: &mut App) -> Option<BoxedLayer> {
        let _ = MAIN_THREAD.set(std::thread::current().id());
        let st = state().clone();
        let armed = st.is_armed();
        if armed {
            info!(
                "profiler: EUSTRESS_PROFILE armed — capturing per-system timings over {}-frame windows; \
                 writing {REPORT_TXT} + {REPORT_SVG} to the working directory",
                st.window_frames
            );
        } else {
            // Installed-but-idle: confirm the build has the capability so a
            // user knows the knob exists. Cheap, one line, at startup only.
            info!("profiler: profiling feature built; set EUSTRESS_PROFILE=1 to capture per-system timings");
        }

        // Filter state for the closure: a cheap clone of the arm flag handle.
        let filter_state = st.clone();
        let layer = SystemTimingLayer { state: st }.with_filter(filter_fn(move |meta| {
            // Only care about spans (not events), only the two system spans,
            // and only while armed.
            if !filter_state.is_armed() {
                return false;
            }
            if !meta.is_span() {
                return false;
            }
            matches!(meta.name(), "system" | "system_commands")
        }));

        Some(Box::new(layer))
    }

    // ─────────────────── per-system trace installer ────────────────────
    // Invoked from the unified `ProfilerPlugin::build` (top-level) ONLY under
    // the `profiling` feature. Touches the global state so env vars are read +
    // logged once (idempotent via OnceLock) and adds the window-tick + dump to
    // `Last`. This is the path — and the only path — that pulls
    // `bevy_ecs/trace` and therefore triggers a full Bevy rebuild.
    pub(super) fn install_trace(app: &mut App) {
        let _ = state();
        app.add_systems(Last, tick_and_maybe_dump);
    }

    /// Bevy system: advance the window counter and, on a window boundary,
    /// drain the accumulator and write the report + flamegraph.
    fn tick_and_maybe_dump() {
        let st = state();
        if !st.is_armed() {
            return;
        }
        let n = st.frames_in_window.fetch_add(1, Ordering::Relaxed) + 1;
        if n < st.window_frames {
            return;
        }
        // Window closed: reset the counter and take a snapshot of the tallies.
        st.frames_in_window.store(0, Ordering::Relaxed);
        let snapshot: Vec<(String, Tally)> = {
            let mut map = match st.acc.lock() {
                Ok(m) => m,
                Err(_) => return,
            };
            let out = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
            map.clear();
            out
        };
        if snapshot.is_empty() {
            return;
        }
        dump(snapshot, st.window_frames);
    }

    /// Rank, write `eustress_profile.txt`, render `eustress_profile.svg`, and
    /// echo the top entries into the engine log.
    fn dump(mut rows: Vec<(String, Tally)>, window_frames: u64) {
        // Sort by total time descending — the slowest system first.
        rows.sort_by(|a, b| b.1.total.cmp(&a.1.total));

        // Mean frame time over the window = sum of all per-system time divided
        // by frame count. This is the denominator for the "% of frame" column.
        // (With Bevy's multi-threaded executor, summed system time can exceed
        // wall time because systems overlap; the percentage is therefore a
        // share of total CPU-system-time, which is still the right signal for
        // "which system dominates", and we say so in the header.)
        let total_all: Duration = rows.iter().map(|(_, t)| t.total).sum();
        let mean_frame_ms = (total_all.as_secs_f64() * 1000.0) / window_frames as f64;
        let denom_ms = if mean_frame_ms > 0.0 { mean_frame_ms } else { 1.0 };

        // ---- text report ----
        let mut text = String::new();
        text.push_str(&format!(
            "Eustress per-system profile — window = {window_frames} frames, {} systems observed\n",
            rows.len()
        ));
        text.push_str(&format!(
            "Mean summed system-time per frame: {mean_frame_ms:.2} ms (sum across overlapping threads)\n",
        ));
        let main_all: Duration = rows.iter().map(|(_, t)| t.main).sum();
        text.push_str(&format!(
            "Main-thread system time per frame: {:.2} ms (serial: NonSend systems and whatever else the main thread ran)\n",
            main_all.as_secs_f64() * 1000.0 / window_frames as f64,
        ));
        text.push_str("rank  total_ms   avg_ms/frame   %frame   calls   system\n");
        text.push_str("----  --------   ------------   ------   -----   ------\n");
        for (i, (name, tally)) in rows.iter().take(report_top_n()).enumerate() {
            let total_ms = tally.total.as_secs_f64() * 1000.0;
            let avg_ms = total_ms / window_frames as f64;
            let pct = (avg_ms / denom_ms) * 100.0;
            text.push_str(&format!(
                "{:>4}  {:>8.2}   {:>12.3}   {:>5.1}%   {:>5}   {}\n",
                i + 1,
                total_ms,
                avg_ms,
                pct,
                tally.calls,
                name,
            ));
        }

        // Main-thread systems by their main-thread time: the serial chain.
        let mut on_main: Vec<&(String, Tally)> =
            rows.iter().filter(|(_, t)| !t.main.is_zero()).collect();
        on_main.sort_by(|a, b| b.1.main.cmp(&a.1.main));
        text.push_str("\nmain-thread  ms/frame   system\n");
        for (name, tally) in on_main.iter().take(60) {
            text.push_str(&format!(
                "             {:>8.3}   {}\n",
                tally.main.as_secs_f64() * 1000.0 / window_frames as f64,
                name,
            ));
        }

        match std::fs::write(REPORT_TXT, &text) {
            Ok(()) => info!("profiler: wrote {REPORT_TXT} ({} systems)", rows.len()),
            Err(e) => warn!("profiler: failed writing {REPORT_TXT}: {e}"),
        }

        // ---- log echo (top N) ----
        warn!(
            "profiler: top {} systems by total time over {} frames (mean summed system-time {:.2} ms/frame):",
            LOG_TOP_N.min(rows.len()),
            window_frames,
            mean_frame_ms,
        );
        for (i, (name, tally)) in rows.iter().take(LOG_TOP_N).enumerate() {
            let total_ms = tally.total.as_secs_f64() * 1000.0;
            let avg_ms = total_ms / window_frames as f64;
            info!(
                "  #{:>2}  {:>8.2} ms total  {:>8.3} ms/frame  x{:<5}  {}",
                i + 1,
                total_ms,
                avg_ms,
                tally.calls,
                name,
            );
        }

        // ---- inferno flamegraph (opt-in) ----
        // The SVG render walks every folded stack and is multi-second in a debug
        // build with ~900 systems — which contaminates the very frame budget we
        // are trying to measure. Only render it when explicitly requested; the
        // ranked text report (above) is the primary artifact.
        if std::env::var_os("EUSTRESS_PROFILE_SVG").is_some() {
            render_flamegraph(&rows);
        }
    }

    /// Build folded-stack lines (`system_name total_micros`) and render them
    /// to `eustress_profile.svg` with `inferno`. Each system is a single,
    /// flat stack frame; the flamegraph degenerates to a sorted bar chart of
    /// per-system cost, which is exactly the "what's eating the frame" view.
    fn render_flamegraph(rows: &[(String, Tally)]) {
        // inferno splits a folded line on the LAST whitespace into
        // `stack` + `count`, and splits the stack on ';'. System names contain
        // neither problematic spaces in a way that breaks the trailing-count
        // split (the count is appended after a single space), but they DO
        // contain characters fine for a leaf frame. Sanitize ';' just in case
        // a closure name embeds one.
        let mut folded = String::new();
        for (name, tally) in rows {
            let micros = tally.total.as_micros();
            if micros == 0 {
                continue;
            }
            let leaf = name.replace(';', ":");
            folded.push_str(&format!("{leaf} {micros}\n"));
        }
        if folded.is_empty() {
            return;
        }

        let file = match std::fs::File::create(REPORT_SVG) {
            Ok(f) => f,
            Err(e) => {
                warn!("profiler: failed creating {REPORT_SVG}: {e}");
                return;
            }
        };
        let writer = std::io::BufWriter::new(file);

        let mut opts = inferno::flamegraph::Options::default();
        opts.title = "Eustress per-system frame profile".to_string();
        opts.subtitle = Some("total microseconds per system over the sample window".to_string());
        opts.count_name = "µs".to_string();
        // inferno requires lexically-sorted input when `no_sort` is set (it
        // rejects unsorted lines outright), so let it sort. The frame widths
        // still encode the per-system cost, which is the signal that matters.
        opts.no_sort = false;

        let lines = folded.lines();
        match inferno::flamegraph::from_lines(&mut opts, lines, writer) {
            Ok(()) => info!("profiler: wrote {REPORT_SVG}"),
            Err(e) => warn!("profiler: inferno flamegraph render failed: {e}"),
        }
    }
}
