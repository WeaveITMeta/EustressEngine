//! # Sim orchestration (Phase 6 engine seam + thin Phase 3 driver)
//!
//! The IN-ENGINE half of the residency-cell → Forge-SimCell gang-placement
//! seam. As the camera streams residency cells into existence
//! ([`super::residency`]), this plugin lowers each NEWLY-resident cell into a
//! [`eustress_forge::sim::SimCell`] (1 world + 1 stub agent) and gang-schedules
//! it through the in-forge reconciler, exactly once per cell.
//!
//! ## Why this is DISTINCT from [`crate::forge::ForgePlugin`]
//!
//! [`crate::forge::ForgePlugin`] is the connect-only SDK path (reqwest →
//! Nomad/R2 dedicated-server deployment). THIS plugin drives the
//! `eustress-forge` ORCHESTRATION crate's `sim` module — a separate crate, a
//! separate concern, no shared code. They are wired through different Cargo
//! features (`forge` vs `sim-orchestration`).
//!
//! ## Architecture (matches the engine forge.rs threading idiom)
//!
//! `crates/forge` is engine-free / Bevy-free, and its reconcile calls are
//! `async`. So:
//!
//! 1. A Bevy system ([`sys_detect_resident_cells`], in [`SimOrchestrationSet`]
//!    ordered `.after(`[`ResidencyChainSet`]`)`) reads
//!    [`super::residency::ResidencyState::resident_cells`], and for any cell
//!    NOT yet in the idempotent [`SimPlacedCells`] latch, ENQUEUES the cell on
//!    a channel and records the latch. Bevy systems do nothing async.
//! 2. A spawned `std::thread` owning a current-thread tokio runtime (the SAME
//!    pattern as [`crate::forge::connect_to_forge`]) DRAINS the channel and,
//!    per cell, runs the verified forge sequence:
//!    `to_sim_cell` → `submit_sim_cell` → `single_node_reconciler` (one node
//!    with GPUs ≥ agents-per-cell so the `InterconnectLocalGpu` gang fits) →
//!    `reconcile_once` → read the [`eustress_forge::sim::SimBinding`].
//! 3. For THIS thin test the "launch" of a gang-placed cell is a `tracing`
//!    log line (`sim_scheduled=1`, the committed placements). No process is
//!    actually spawned.
//!
//! ## THIN scope (binding decision for this run)
//!
//! This is observe → submit → reconcile → LOG only. There is NO RSI inference
//! and NO Rune-rewrite this run. The future agent-Rune-rewrite admission gate
//! is [`crate::soul::kernel::validate_rune_rewrite`] — see the [`RewriteGate`]
//! documented seam below. `crates/forge` is engine-free and MUST NEVER call
//! that gate; when RSI lands, the rewrite is validated HERE (engine side)
//! before any rewritten agent shape is lowered into a `SimCell`.
//!
//! Everything in this module is behind `#[cfg(feature = "sim-orchestration")]`
//! so the default build is completely unaffected.

#![cfg(feature = "sim-orchestration")]

use std::collections::HashSet;

use bevy::prelude::*;
use crossbeam::channel::{Receiver, Sender};

use eustress_forge::sim::{self, cell, driver, CellCoord};
use eustress_forge::sim::{memory_store, GpuResources, NodeId, NodeResources};

use super::residency::ResidencyState;
use super::world_db_binary::ResidencyChainSet;

/// Number of stub agents co-placed with each cell's world process. 1 keeps the
/// gang minimal (1 world + 1 GPU agent ⇒ a single-GPU node fits it). The world
/// member consumes no GPU; only the agent does.
const AGENTS_PER_CELL: usize = 1;

/// SystemSet for the sim-orchestration Bevy systems. Ordered
/// `.after(`[`ResidencyChainSet`]`)` so the resident-cell snapshot it reads
/// reflects THIS frame's residency scan.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SimOrchestrationSet;

/// What the Bevy side hands the worker thread: one residency cell to lower +
/// gang-place. (Kept a plain `CellCoord` for the thin driver; a richer request
/// — agent specs from a Kernel-validated Rune rewrite — is the future shape.)
type PlacementRequest = CellCoord;

/// Outcome the worker sends back for logging on the main thread (so the log is
/// frame-ordered and visible in the engine console, not buried on a side
/// thread). The worker ALSO logs directly; this channel exists so the result
/// is observable from Bevy for later UI/telemetry without changing the worker.
#[derive(Debug, Clone)]
pub struct PlacementResult {
    pub cell: CellCoord,
    /// Number of cells the reconciler gang-scheduled this pass (1 on success).
    pub sim_scheduled: usize,
    /// Members placed in the committed binding (= 1 world + agents on success).
    pub placed_members: usize,
    /// `true` iff the whole gang (world + all agents) committed together.
    pub complete: bool,
    /// `Some(msg)` if the forge sequence errored for this cell.
    pub error: Option<String>,
}

/// Channel + latch resource. Holds the request `Sender` (Bevy → worker) and the
/// result `Receiver` (worker → Bevy). The worker thread keeps the matching
/// `Receiver`/`Sender` ends; both live for the process lifetime.
#[derive(Resource)]
pub struct SimOrchestrationChannel {
    /// Bevy enqueues placement requests here; the worker drains them.
    pub requests: Sender<PlacementRequest>,
    /// Bevy drains results here for logging/telemetry.
    pub results: Receiver<PlacementResult>,
}

/// Idempotent per-cell latch: a cell whose [`CellCoord`] is in here has already
/// been enqueued for gang placement, so it is never submitted twice (the
/// "place a cell once" invariant). NOT cleared on eviction in the thin driver —
/// a cell placed once stays latched for the session.
#[derive(Resource, Default)]
pub struct SimPlacedCells(pub HashSet<CellCoord>);

/// Session ledger of gang-placement outcomes, appended by
/// [`sys_drain_results`] and read over the engine bridge (`sim.bindings`)
/// so an external orchestrator (MCP client) can SEE forge's real placement
/// state alongside the `scene.overview` cell digest — same `sim-cell-…`
/// ids, so the two surfaces join on cell identity. Bounded (oldest rows
/// dropped past [`Self::CAP`]) — a session places each cell once, so the
/// cap is a defensive bound, not an expected ceiling.
#[derive(Resource, Default)]
pub struct SimBindingsLedger(pub Vec<PlacementResult>);

impl SimBindingsLedger {
    pub const CAP: usize = 4_096;
}

/// The Phase-6 sim-orchestration plugin. DISTINCT from
/// [`crate::forge::ForgePlugin`].
pub struct SimOrchestrationPlugin;

impl Plugin for SimOrchestrationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimPlacedCells>()
            .init_resource::<SimBindingsLedger>()
            // The new set runs AFTER the residency load/mirror/evict chain so
            // the resident-cell snapshot it reads is this frame's. This adds
            // ONLY the ordering edge; it does not touch the chain's internals.
            .configure_sets(Update, SimOrchestrationSet.after(ResidencyChainSet))
            // Spawn the worker thread + install the channel resource before the
            // detector system runs.
            .add_systems(Startup, sys_spawn_worker)
            .add_systems(
                Update,
                (sys_detect_resident_cells, sys_drain_results).in_set(SimOrchestrationSet),
            );
    }
}

/// Startup: spawn the worker thread (std::thread + current-thread tokio
/// runtime, mirroring [`crate::forge::connect_to_forge`]) and install the
/// channel resource. The worker owns all `async` forge calls so no Bevy system
/// ever blocks a frame on reconcile I/O.
fn sys_spawn_worker(mut commands: Commands) {
    let (req_tx, req_rx) = crossbeam::channel::unbounded::<PlacementRequest>();
    let (res_tx, res_rx) = crossbeam::channel::unbounded::<PlacementResult>();

    std::thread::Builder::new()
        .name("sim-orchestration".into())
        .spawn(move || worker_main(req_rx, res_tx))
        .expect("spawn sim-orchestration worker thread");

    commands.insert_resource(SimOrchestrationChannel {
        requests: req_tx,
        results: res_rx,
    });

    info!("sim-orchestration: worker thread started (Phase 6 seam, thin Phase 3 driver)");
}

/// Worker thread body. Owns a current-thread tokio runtime and a blocking loop
/// that drains placement requests, running the verified forge sequence per
/// cell. Exits when the request channel closes (all senders dropped, i.e. app
/// teardown).
fn worker_main(requests: Receiver<PlacementRequest>, results: Sender<PlacementResult>) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            error!("sim-orchestration: failed to build tokio runtime: {e}");
            return;
        }
    };

    // Blocking recv loop — one cell at a time keeps the thin driver simple and
    // deterministic. `recv()` returns Err when the Sender (held in the Bevy
    // resource) is dropped at shutdown, ending the thread cleanly.
    while let Ok(cell) = requests.recv() {
        let result = rt.block_on(place_cell(cell));
        let res = match result {
            Ok(res) => res,
            Err(e) => PlacementResult {
                cell,
                sim_scheduled: 0,
                placed_members: 0,
                complete: false,
                error: Some(e),
            },
        };
        log_placement(&res);
        // Best-effort hand-back to Bevy; if the receiver is gone (shutdown),
        // the worker keeps going / exits on the next recv.
        let _ = results.send(res);
    }
    info!("sim-orchestration: worker thread exiting (request channel closed)");
}

/// Run the verified forge gang-placement sequence for ONE residency cell.
///
/// Mirrors `crates/forge/src/bin/forge_sim_demo.rs` exactly, but uses the
/// simplest store that compiles ([`memory_store`], an in-memory
/// `MemoryStore` behind a `BoxedStateStore`) so the thin test needs no on-disk
/// Raft. Each cell gets a fresh store: a cell is an independent gang, and the
/// idempotent latch guarantees one placement per cell.
async fn place_cell(cell: CellCoord) -> Result<PlacementResult, String> {
    // 1. Simplest store that compiles — in-memory, no on-disk Raft.
    let store = memory_store();

    // 2. Lower the residency cell into a SimCell (1 world + N stub agents).
    let spec = cell::SimCellSpec::new(cell).with_vortex_agents(AGENTS_PER_CELL);
    let sc = cell::to_sim_cell(&spec);
    let expected_members = spec.member_count(); // 1 world + AGENTS_PER_CELL

    // 3. Submit under keys::simcell(id) so reconcile_once auto-discovers it.
    sim::cell_sync::submit_sim_cell(store.as_ref(), &sc)
        .await
        .map_err(|e| format!("submit_sim_cell: {e}"))?;

    // 4. Register ONE node with GPUs ≥ agents-per-cell. Each AgentPolicy::gpu
    //    member consumes ONE GPU and InterconnectLocalGpu forces same-node, so
    //    the node must expose at least AGENTS_PER_CELL GPUs or the gang fails
    //    atomically (sim_scheduled stays 0, no error). The world member uses
    //    none.
    let gpu_count = AGENTS_PER_CELL.max(1) as u32;
    let mut node = NodeResources::new(NodeId::new(), 16_000, 65_536);
    for dev in 0..gpu_count {
        node = node.with_gpu(GpuResources::new(dev, "sim-orch-gpu", 24_576));
    }

    // 5. Build the single-node reconciler + reconcile ONCE.
    let mut rec = driver::single_node_reconciler(store.clone(), node)
        .map_err(|e| format!("single_node_reconciler: {e}"))?;
    let (report, bindings) = driver::reconcile_once(&mut rec)
        .await
        .map_err(|e| format!("reconcile_once: {e}"))?;

    // 6. Read the gang binding for this cell (the "launch" payload — a log
    //    line for the thin test).
    let (placed_members, complete) = match driver::find_binding(&bindings, &sc.id) {
        Some(b) => (b.placements.len(), driver::binding_is_complete(b, expected_members)),
        None => (0, false),
    };

    Ok(PlacementResult {
        cell,
        sim_scheduled: report.sim_scheduled,
        placed_members,
        complete,
        error: None,
    })
}

/// Emit the thin-test "launch" log line for a placement outcome.
fn log_placement(res: &PlacementResult) {
    if let Some(err) = &res.error {
        warn!(
            "sim-orchestration: cell {:?} placement ERRORED: {}",
            res.cell, err
        );
    } else if res.complete {
        info!(
            "sim-orchestration: LAUNCH cell {:?} — sim_scheduled={} gang COMMITTED all {} members",
            res.cell, res.sim_scheduled, res.placed_members
        );
    } else {
        warn!(
            "sim-orchestration: cell {:?} NOT gang-placed (sim_scheduled={} placed_members={}); \
             check GPUs>=agents-per-cell",
            res.cell, res.sim_scheduled, res.placed_members
        );
    }
}

/// Detect NEWLY-resident residency cells and enqueue each ONCE for gang
/// placement. Idempotent via the [`SimPlacedCells`] latch. Runs in
/// [`SimOrchestrationSet`] (`.after(`[`ResidencyChainSet`]`)`), so
/// [`ResidencyState::resident_cells`] reflects this frame's scan.
///
/// ## RewriteGate seam (DOCUMENTED, NOT WIRED this run)
///
/// In the future RSI loop, between observing a cell and lowering it to a
/// `SimCell`, an agent may propose a Rune rewrite. That rewrite is admitted
/// ONLY by the engine-side gate
/// [`crate::soul::kernel::validate_rune_rewrite`]`(source, &laws)` (re-exported
/// at `soul::kernel`), where `laws = UniverseLaws::load_for_active_universe()`.
/// The validated agent shape would then feed `SimCellSpec::agents` here. THIS
/// is the only place that gate is called — `crates/forge` is engine-free and
/// must NEVER reach back into the kernel. For the thin driver there is no
/// rewrite: agents are stub spatial-vortex defaults.
fn sys_detect_resident_cells(
    residency: Res<ResidencyState>,
    channel: Res<SimOrchestrationChannel>,
    mut placed: ResMut<SimPlacedCells>,
) {
    // Only meaningful for a streaming (large) Space; a small Space has the
    // residency manager idle, so resident_cells() is empty and this no-ops.
    for cell in residency.resident_cells() {
        if placed.0.insert(cell) {
            // First time we've seen this cell resident — enqueue once.
            if let Err(e) = channel.requests.send(cell) {
                // Worker gone (shutdown) — un-latch so a future run can retry,
                // and stop trying this frame.
                placed.0.remove(&cell);
                warn!("sim-orchestration: request channel closed ({e}); skipping cell {cell:?}");
                break;
            }
        }
    }
}

/// Drain worker results on the main thread into the [`SimBindingsLedger`].
/// The worker already logs each outcome; this keeps the channel from filling
/// AND makes every placement outcome readable over the engine bridge
/// (`sim.bindings`) for external orchestrators.
fn sys_drain_results(
    channel: Res<SimOrchestrationChannel>,
    mut ledger: ResMut<SimBindingsLedger>,
) {
    while let Ok(res) = channel.results.try_recv() {
        ledger.0.push(res);
        if ledger.0.len() > SimBindingsLedger::CAP {
            let overflow = ledger.0.len() - SimBindingsLedger::CAP;
            ledger.0.drain(..overflow);
        }
    }
}
