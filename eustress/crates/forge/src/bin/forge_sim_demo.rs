//! Headless Phase-2 sim seam demo (binding decision D2).
//!
//! Proves the residency-cell → Forge-SimCell seam END-TO-END with NO engine
//! and NO Bevy:
//!
//! 1. brings up a single-node [`RaftStateStore`],
//! 2. lowers a residency cell into a [`SimCell`] (world + N stub agents),
//! 3. SUBMITS it under `keys::simcell(id)` so the reconciler discovers it,
//! 4. registers one node exposing one GPU PER agent (each `AgentPolicy::gpu`
//!    member consumes one GPU device under `InterconnectLocalGpu`
//!    co-placement; the world member consumes none) + reconciles ONCE,
//! 5. asserts the gang committed ALL members (all-or-nothing) and the
//!    binding's `cell_id` round-trips back to the residency cell,
//! 6. prints the [`SimBinding`].
//!
//! Run (small scoped build — does NOT touch the engine):
//! ```text
//! cargo run -p eustress-forge --features sim-orchestration --bin forge-sim-demo
//! ```
//!
//! Flags (all optional, zero-dep parsing):
//! * `--cell X,Y,Z`   residency cell coords (default `1048576,1048576,1048576`
//!                    = the biased origin cell)
//! * `--agents N`     number of stub agents (default 2)
//! * `--node-id U`    raft node id (default 1)
//! * `--persist DIR`  open a persistent (Fjall-backed) store at DIR instead of
//!                    in-memory bootstrap

use std::sync::Arc;

use eustress_forge::sim::{self, cell, driver, CellCoord};
use eustress_forge::sim::{GpuResources, NodeId, NodeResources, RaftStateStore, StateStore};
use eustress_forge::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    println!("=== Eustress Forge Sim Demo (Phase 2) ===");
    println!(
        "cell={:?} agents={} node_id={} store={}",
        args.cell,
        args.agents,
        args.node_id,
        args.persist.as_deref().unwrap_or("<in-memory>")
    );

    // 1. Bring up a single-node Raft state store. bootstrap_single_node is a
    //    1-voter cluster (no initialize_cluster needed). open_persistent is
    //    the Fjall-backed variant (requires the raft-persist feature, which
    //    sim-orchestration pulls in).
    let store: Arc<dyn StateStore> = match &args.persist {
        Some(dir) => {
            let path = std::path::Path::new(dir);
            Arc::new(
                RaftStateStore::open_persistent(args.node_id, path)
                    .await
                    .map_err(eustress_forge::EustressForgeError::Orchestration)?,
            )
        }
        None => Arc::new(
            RaftStateStore::bootstrap_single_node(args.node_id)
                .await
                .map_err(eustress_forge::EustressForgeError::Orchestration)?,
        ),
    };

    // 2. Lower the residency cell into a SimCell (world + N stub agents).
    let spec = cell::SimCellSpec::new(args.cell).with_vortex_agents(args.agents);
    let sc = cell::to_sim_cell(&spec);
    let expected_members = spec.member_count(); // 1 world + N agents
    println!(
        "Lowered SimCell id={} members={} (1 world + {} agents)",
        sc.id, expected_members, args.agents
    );

    // 3. SUBMIT under keys::simcell(id) so reconcile_once discovers it.
    sim::cell_sync::submit_sim_cell(store.as_ref(), &sc).await?;
    println!("Submitted SimCell to store under keys::simcell(\"{}\")", sc.id);

    // Diagnostic: confirm the write is visible to the SAME read path the
    // reconciler uses (list_prefix(SIMCELLS) + store_get_json). If this prints
    // 0 keys, the issue is store visibility (Raft apply lag); if it prints the
    // cell but reconcile still schedules 0, the issue is gang placement.
    let keys_found = store
        .list_prefix(sim::keys::SIMCELLS)
        .await
        .map_err(eustress_forge::EustressForgeError::Orchestration)?;
    println!("Diagnostic: list_prefix(SIMCELLS) -> {} key(s): {:?}", keys_found.len(), keys_found);
    let readback: Option<sim::SimCell> = sim::store_get_json(
        store.as_ref(),
        &sim::keys::simcell(&sc.id),
    )
    .await
    .map_err(eustress_forge::EustressForgeError::Orchestration)?;
    println!(
        "Diagnostic: read-back SimCell present={} agents={}",
        readback.is_some(),
        readback.as_ref().map(|c| c.agents.len()).unwrap_or(0)
    );

    // 4. Build a single-node Reconciler with a GPU node registered, then
    //    reconcile ONCE. Each AgentPolicy::gpu member consumes ONE GPU device
    //    (the world member consumes none), and InterconnectLocalGpu requires
    //    them ALL on the same node — so the node must expose at least
    //    `args.agents` GPUs or the gang fails atomically (all-or-nothing).
    let gpu_count = args.agents.max(1) as u32;
    let mut gpu_node = NodeResources::new(NodeId::new(), 16000, 65536);
    for dev in 0..gpu_count {
        gpu_node = gpu_node.with_gpu(GpuResources::new(dev, "demo-gpu", 24576));
    }
    println!("Registering node with {gpu_count} GPU(s) (one per agent member)");
    let mut rec = driver::single_node_reconciler(store.clone(), gpu_node)?;
    let (report, bindings) = driver::reconcile_once(&mut rec).await?;
    println!(
        "reconcile_once -> sim_scheduled={} scheduled={} pending={}",
        report.sim_scheduled, report.scheduled, report.pending
    );

    // 5. Assert all-or-nothing gang placement + round-trip the cell id.
    assert_eq!(
        report.sim_scheduled, 1,
        "expected exactly one cell gang-scheduled, got {}",
        report.sim_scheduled
    );
    let b = driver::find_binding(&bindings, &sc.id)
        .unwrap_or_else(|| panic!("no SimBinding for cell {}", sc.id));
    assert!(
        driver::binding_is_complete(b, expected_members),
        "gang must place ALL {} members or NONE; got {} placements",
        expected_members,
        b.placements.len()
    );
    assert_eq!(
        cell::parse_cell_id(&b.cell_id),
        Some(args.cell),
        "binding cell_id must round-trip back to the residency cell"
    );

    // 6. Print the binding.
    println!(
        "SimBinding: cell={} placements={:?} reservations={}",
        b.cell_id,
        b.placements,
        b.reservations.len()
    );
    println!(
        "=== Done: sim_scheduled={} gang COMMITTED all {} members ===",
        report.sim_scheduled, expected_members
    );
    Ok(())
}

/// Minimal zero-dependency flag parsing (mirrors the engine's
/// generate-benchmark-map bin style).
struct Args {
    cell: CellCoord,
    agents: usize,
    node_id: u64,
    persist: Option<String>,
}

impl Args {
    fn parse() -> Self {
        let mut cell: CellCoord = (1 << 20, 1 << 20, 1 << 20); // biased origin cell
        let mut agents = 2usize;
        let mut node_id = 1u64;
        let mut persist: Option<String> = None;

        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            match flag.as_str() {
                "--cell" => {
                    if let Some(v) = it.next() {
                        let parts: Vec<u32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                        if parts.len() == 3 {
                            cell = (parts[0], parts[1], parts[2]);
                        } else {
                            eprintln!("--cell expects X,Y,Z; keeping default {cell:?}");
                        }
                    }
                }
                "--agents" => {
                    if let Some(v) = it.next() {
                        agents = v.parse().unwrap_or(agents);
                    }
                }
                "--node-id" => {
                    if let Some(v) = it.next() {
                        node_id = v.parse().unwrap_or(node_id);
                    }
                }
                "--persist" => {
                    persist = it.next();
                }
                other => eprintln!("ignoring unknown flag: {other}"),
            }
        }
        Self { cell, agents, node_id, persist }
    }
}
