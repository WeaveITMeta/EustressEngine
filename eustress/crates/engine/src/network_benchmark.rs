//! Stress test: headless clients on the real session protocol.
//!
//! The bots live in `eustress_networking::stress`. This module owns the
//! Studio side: the machine cap, starting and stopping a run, and pushing
//! the live numbers into the dialog. When Studio itself is the host, the
//! player cap is raised for the run and this process's CPU and memory are
//! shown. A pasted join link targets that host instead (a local dedicated
//! server, or a Forge allocation that serves the session and published a link).

use std::time::{Duration, Instant};

use bevy::ecs::system::NonSend;
use bevy::prelude::*;
use sysinfo::{Pid, ProcessesToUpdate, System};

use eustress_networking::stress::{resolve_link, StressConfig, StressRun, StressSnapshot};
use eustress_networking::JoinLink;

/// Detected machine capabilities used to size the Players slider.
#[derive(Resource, Debug, Clone)]
pub struct MachineCapabilities {
    pub cpu_cores: usize,
    pub total_ram_mb: u64,
    pub recommended_max_clients: i32,
}

impl Default for MachineCapabilities {
    fn default() -> Self {
        Self {
            cpu_cores: 4,
            total_ram_mb: 8192,
            recommended_max_clients: 50,
        }
    }
}

impl MachineCapabilities {
    /// Probe the current machine and compute recommended limits.
    pub fn detect() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();

        let cpu_cores = sys.cpus().len().max(1);
        let total_ram_mb = sys.total_memory() / (1024 * 1024);

        // Each bot is one task plus its connection. Cap by cores and RAM,
        // and never offer more than the protocol run accepts.
        let by_cpu = (cpu_cores * 25) as i32;
        let by_ram = (total_ram_mb / 4) as i32;
        let recommended_max_clients = by_cpu.min(by_ram).min(500).max(10);

        info!(
            "Machine capabilities: {} cores, {} MB RAM → max {} stress clients",
            cpu_cores, total_ram_mb, recommended_max_clients
        );

        Self {
            cpu_cores,
            total_ram_mb,
            recommended_max_clients,
        }
    }
}

/// What Start asked for, kept until a host link exists.
struct PendingLaunch {
    config: StressConfig,
    /// Empty: use the server Studio is hosting, or the host file on 7777.
    join_link: String,
    asked_host: bool,
    previous_max: Option<u16>,
    /// True when the bots are connecting to the host inside this process.
    in_process: bool,
}

/// Bevy resource that owns the stress test lifecycle.
#[derive(Resource)]
pub struct StressTestState {
    pending: Option<PendingLaunch>,
    run: Option<StressRun>,
    /// Player cap to put back when the run ends.
    restore_max: Option<u16>,
    in_process_host: bool,
    status: String,
    report_extra: String,
    frame_ms: Vec<f32>,
    last_frame: Option<Instant>,
    sys: System,
    last_sample: Option<Instant>,
    server_cpu: f32,
    server_mem_mb: f32,
    finalized: bool,
}

impl Default for StressTestState {
    fn default() -> Self {
        Self {
            pending: None,
            run: None,
            restore_max: None,
            in_process_host: false,
            status: String::new(),
            report_extra: String::new(),
            frame_ms: Vec::new(),
            last_frame: None,
            sys: System::new(),
            last_sample: None,
            server_cpu: -1.0,
            server_mem_mb: -1.0,
            finalized: false,
        }
    }
}

impl StressTestState {
    pub fn request_stop(&mut self) {
        self.pending = None;
        if let Some(run) = &self.run {
            run.request_stop();
        }
        if self.status.is_empty() || self.run.is_some() {
            self.status = "Stopping…".into();
        }
    }

    pub fn is_running(&self) -> bool {
        self.pending.is_some() || self.run.as_ref().is_some_and(|run| run.is_running())
    }
}

/// Numbers from the dialog.
pub struct StressLaunch {
    pub clients: i32,
    pub avatar_hz: i32,
    pub duration_secs: i32,
    pub ramp_secs: i32,
    pub movement: bool,
    pub interactions: bool,
    pub download: bool,
    pub join_link: String,
}

/// Start, or wait for the local server if Studio is not hosting and no link
/// was pasted. Returns the status line for the Output.
pub fn begin(
    state: &mut StressTestState,
    host: Option<&mut eustress_networking::HostSession>,
    request: Option<&mut crate::multiplayer::HostRequest>,
    launch: StressLaunch,
) -> String {
    if state.is_running() {
        return "A stress test is already running.".into();
    }
    let clients = launch.clients.max(1) as u32;
    let config = StressConfig {
        clients,
        duration: Duration::from_secs(launch.duration_secs.max(1) as u64),
        ramp: Duration::from_secs(launch.ramp_secs.max(0) as u64),
        avatar_hz: launch.avatar_hz.max(1) as u32,
        movement: launch.movement,
        interactions: launch.interactions,
        download_world: launch.download,
    };
    state.finalized = false;
    state.frame_ms.clear();
    state.last_frame = Some(Instant::now());
    state.report_extra.clear();
    state.server_cpu = -1.0;
    state.server_mem_mb = -1.0;
    state.run = None;
    state.restore_max = None;
    state.pending = Some(PendingLaunch {
        config,
        join_link: launch.join_link.trim().to_string(),
        asked_host: false,
        previous_max: None,
        in_process: false,
    });
    try_launch(state, host, request)
}

/// Start the bots once a join link is known. Called from Start and once a
/// frame while a launch is waiting for the local server.
pub fn try_launch(
    state: &mut StressTestState,
    host: Option<&mut eustress_networking::HostSession>,
    request: Option<&mut crate::multiplayer::HostRequest>,
) -> String {
    let Some(pending) = state.pending.as_mut() else {
        return state.status.clone();
    };
    let link = if !pending.join_link.is_empty() {
        let mut paths = vec![eustress_bridge_client::default_workspace_root()];
        if let Ok(cwd) = std::env::current_dir() {
            paths.push(cwd);
        }
        match resolve_link(&pending.join_link, &paths) {
            Ok(link) => link,
            Err(reason) => {
                state.pending = None;
                state.status = reason.clone();
                return reason;
            }
        }
    } else if let Some(link) = request.as_ref().and_then(|r| r.join_link()) {
        pending.in_process = true;
        link
    } else if let Some(link) = read_local_host_file() {
        pending.in_process = false;
        link
    } else {
        if !pending.asked_host {
            if let Some(request) = request {
                request.request_start();
                pending.asked_host = true;
            }
        }
        let status = if pending.asked_host {
            "Starting the local server. The bots connect once it is listening.".to_string()
        } else {
            "No host is listening. Start the local server (F9) or paste a join link.".to_string()
        };
        state.status = status.clone();
        return status;
    };

    if pending.in_process {
        if let Some(host) = host {
            let cap = (pending.config.clients as u16).saturating_add(8);
            if pending.previous_max.is_none() {
                pending.previous_max = Some(host.max_players());
            }
            if host.max_players() < cap {
                host.set_max_players(cap);
            }
        }
    }
    let previous_max = pending.previous_max;
    let in_process = pending.in_process;
    let config = pending.config.clone();
    let label = format!("{}:{}", link.host, link.port);
    match StressRun::start(link, config) {
        Ok(run) => {
            state.restore_max = previous_max;
            state.in_process_host = in_process;
            state.pending = None;
            state.run = Some(run);
            state.status = format!("Connecting to {label}");
            state.status.clone()
        }
        Err(reason) => {
            state.pending = None;
            state.status = reason.clone();
            reason
        }
    }
}

fn read_local_host_file() -> Option<JoinLink> {
    let port = std::env::var("EUSTRESS_HOST_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7777u16);
    let root = eustress_bridge_client::default_workspace_root();
    eustress_networking::native::read_host_file(&root, port)
}

pub struct NetworkBenchmarkPlugin;

impl Plugin for NetworkBenchmarkPlugin {
    fn build(&self, app: &mut App) {
        let caps = MachineCapabilities::detect();
        app.insert_resource(caps)
            .init_resource::<StressTestState>()
            .add_systems(Update, sync_stress_test_to_ui);
    }
}

fn sync_stress_test_to_ui(
    mut stress: ResMut<StressTestState>,
    caps: Res<MachineCapabilities>,
    mut host: Option<ResMut<eustress_networking::HostSession>>,
    mut request: Option<ResMut<crate::multiplayer::HostRequest>>,
    slint: Option<NonSend<crate::ui::slint_ui::SlintUiState>>,
) {
    if stress.pending.is_some() {
        try_launch(&mut stress, host.as_deref_mut(), request.as_deref_mut());
    }
    sample_host(&mut stress);
    let running =
        stress.run.as_ref().is_some_and(|run| run.is_running()) || stress.pending.is_some();
    if running {
        if let Some(last) = stress.last_frame {
            let dt = last.elapsed().as_secs_f32() * 1000.0;
            if dt > 0.0 && dt < 1000.0 && stress.frame_ms.len() < 20_000 {
                stress.frame_ms.push(dt);
            }
        }
        stress.last_frame = Some(Instant::now());
    }

    let snap = stress.run.as_ref().map(|run| run.snapshot());
    if let Some(snap) = snap.as_ref() {
        if !snap.running && !stress.finalized {
            stress.finalized = true;
            stress.report_extra = host_note(&stress);
            if let Some(max) = stress.restore_max.take() {
                if let Some(host) = host.as_deref_mut() {
                    host.set_max_players(max);
                }
            }
            stress.status = "Finished".into();
        }
    }

    let Some(slint) = slint else { return };
    let ui = &slint.window;
    let max = caps.recommended_max_clients;
    if ui.get_stress_max_clients() != max {
        ui.set_stress_max_clients(max);
    }
    ui.set_stress_running(running);
    let Some(snap) = snap else {
        if !stress.status.is_empty() {
            ui.set_stress_status(stress.status.clone().into());
        }
        return;
    };
    push_snapshot(ui, &snap, &stress);
}

fn push_snapshot(
    ui: &crate::ui::slint_ui::StudioWindow,
    snap: &StressSnapshot,
    state: &StressTestState,
) {
    let progress = if snap.duration_secs > 0.0 {
        (snap.elapsed_secs / snap.duration_secs).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ui.set_stress_progress(progress);
    let status = if state.status.is_empty() {
        format!(
            "{} connected · {:.0}s / {:.0}s · {:.0} Hz",
            snap.connected, snap.elapsed_secs, snap.duration_secs, snap.tick_hz
        )
    } else if snap.running {
        format!(
            "{} · {} connected · {:.0}s / {:.0}s",
            state.status, snap.connected, snap.elapsed_secs, snap.duration_secs
        )
    } else {
        state.status.clone()
    };
    ui.set_stress_status(status.into());
    ui.set_stress_total_messages(snap.frames_received.min(i32::MAX as u64) as i32);
    ui.set_stress_errors((snap.failed + snap.disconnects + snap.kicks).min(i32::MAX as u32) as i32);
    ui.set_stress_avg_latency(snap.relay_mean_ms);
    ui.set_stress_joined(snap.joined.min(i32::MAX as u32) as i32);
    ui.set_stress_join_failures(snap.failed.min(i32::MAX as u32) as i32);
    ui.set_stress_disconnects(snap.disconnects.min(i32::MAX as u32) as i32);
    ui.set_stress_join_p95_ms(snap.join_p95_ms);
    ui.set_stress_relay_p50_ms(snap.relay_p50_ms);
    ui.set_stress_relay_p95_ms(snap.relay_p95_ms);
    ui.set_stress_relay_loss(snap.relay_loss);
    ui.set_stress_rtt_ms(snap.rtt_ms);
    ui.set_stress_down_kbps(snap.down_kbps);
    ui.set_stress_up_kbps(snap.up_kbps);
    ui.set_stress_bot_cores(snap.bot_cores);
    ui.set_stress_connected(snap.connected.min(i32::MAX as u32) as i32);
    ui.set_stress_tick_hz(snap.tick_hz);
    ui.set_stress_server_cpu(state.server_cpu);
    ui.set_stress_server_mem_mb(state.server_mem_mb);
    let frame_p95 = percentile(&state.frame_ms, 0.95);
    ui.set_stress_host_frame_p95_ms(frame_p95);
    let budget = if frame_p95 <= 0.0 {
        0.0
    } else {
        frame_p95 / 16.7
    };
    ui.set_stress_host_budget(budget);
    let report = if state.report_extra.is_empty() {
        snap.report.clone()
    } else {
        format!("{}\n{}", snap.report, state.report_extra)
    };
    ui.set_stress_report(report.into());
}

fn sample_host(state: &mut StressTestState) {
    if !state.in_process_host {
        state.server_cpu = -1.0;
        state.server_mem_mb = -1.0;
        return;
    }
    let due = state
        .last_sample
        .is_none_or(|t| t.elapsed() >= Duration::from_secs(1));
    if !due {
        return;
    }
    state.last_sample = Some(Instant::now());
    let pid = Pid::from_u32(std::process::id());
    state
        .sys
        .refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    if let Some(proc) = state.sys.process(pid) {
        state.server_cpu = proc.cpu_usage();
        state.server_mem_mb = proc.memory() as f32 / (1024.0 * 1024.0);
    }
}

fn host_note(state: &StressTestState) -> String {
    let frame_p95 = percentile(&state.frame_ms, 0.95);
    if state.in_process_host {
        format!(
            "Host process: CPU {:.0}%   memory {:.0} MB\n\
             This process is the host, and it is also running the bots, so the CPU includes both.\n\
             Studio frame p95: {frame_p95:.1} ms",
            state.server_cpu.max(0.0),
            state.server_mem_mb.max(0.0),
        )
    } else {
        format!(
            "Server CPU and memory: not visible from this process. The host is another process or machine.\n\
             Studio frame p95: {frame_p95:.1} ms"
        )
    }
}

fn percentile(values: &[f32], p: f64) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((v.len() - 1) as f64 * p).round() as usize;
    v[idx.min(v.len() - 1)]
}
