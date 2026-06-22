# Forge ↔ EustressEngine Integration Guide

**Target:** `forge-orchestration = "0.6"` · for engineers/agents working in the **EustressEngine** repo
**Status:** Forge is the orchestration substrate; it is functional and tested, but young (see §9 — honest constraints).

This document tells EustressEngine how to consume the latest Forge. It is written to be self-contained: an agent working only in the EustressEngine repo can follow it without reading Forge's source.

---

## 0. What Forge is — and is not — for Eustress

Forge is the **orchestration layer that sits beneath Eustress's 12 layers**. It answers *where do thousands of world-shards-plus-their-agents run, co-located, on real accelerators, without stalling or fragmenting* — and *where does world state live durably*. It does **not** contain any intelligence, world physics, or governance logic.

| Forge provides (you call it) | Eustress provides (you build it) |
|---|---|
| Gang co-placement of a world + its agents (all-or-nothing) | The world shard (L1–L3 substrate/physics) |
| Tick-deadline scheduling (frame budget per cell) | The Kernel + Rune DSL + validator (L12) — **Phase 0** |
| Durable, replicated state store (multi-node Raft, fjall-backed) | The agents (SpatialVortex policies, L8) and their inference |
| MoE request routing (timescale / GPU / version aware) | The RSI loop + spectral consensus merge (L9/L10) |
| A reconcile control loop (converge desired→actual, reschedule on failure) | The world-model and its prediction/fitness signal |

**Mental model:** *Eustress describes its cells to Forge (resource needs, co-placement, tick deadline). Forge decides placement and guarantees co-location, ordering, and durable state. Eustress then runs the world + agents on those placements.* Forge schedules **where/when**; Eustress executes **what**.

### Layer mapping (why Forge's primitives exist)

| Eustress layer | Forge primitive |
|---|---|
| L1–L4 world shard + L8 agent policies, as one unit | `SimCell` (`scheduler::sim`) |
| L7 Collective — world is useless without agents & vice versa | **gang scheduling** (`GangScheduler`, all-or-nothing) |
| RSI tick (OBSERVE→…→ACT frame budget) | **tick-deadline** (`SimCell::tick` / `next_deadline`, `deadline::TickDeadlineScheduler`) |
| Part-V temporal experts (reflex/tactical/strategic) | `moe` routers |
| L4 State — TOML/git-backed world memory + live state bus | `storage::RaftStateStore` (`raft-persist`) via the `StateStore` trait |
| Spatial interest management | `Region3D` on a `SimCell` |
| The live control plane | `scheduler::reconcile::Reconciler` |

---

## 1. Add the dependency

In the EustressEngine crate that owns orchestration (e.g. a new `eustress/crates/orchestration`, or `engine`):

```toml
[dependencies]
# Multi-node Raft + crash-durable fjall-backed world state.
forge-orchestration = { version = "0.6", features = ["raft-persist"] }
tokio = { version = "1", features = ["full"] }
```

**Feature flags:**
- *(default, no features)* — in-memory / file state store, single-binary scheduler. Fine for unit tests.
- `raft` — multi-node Raft consensus over an HTTP transport (replication + leader failover) using an in-memory log.
- `raft-persist` — adds a **crash-durable, disk-backed** Raft log + snapshots via [fjall](https://github.com/fjall-rs/fjall) (survives process restart). **Recommended for real world state.** Implies `raft`.

Build with a recent stable Rust (developed/tested on 1.95; the manifest declares MSRV 1.75 but openraft+fjall may effectively require newer — pin your toolchain).

---

## 2. Map Eustress concepts → Forge types

```rust
use std::time::Duration;
use forge_orchestration::scheduler::sim::{SimCell, SimWorld, AgentPolicy, CoPlacement, Region3D};
use forge_orchestration::scheduler::ResourceRequirements;
```

| Eustress concept | Forge type | How to build it |
|---|---|---|
| World shard (its compute footprint) | `SimWorld` | `SimWorld::cpu(cpu_millis, memory_mb)` or `SimWorld::new(ResourceRequirements)` |
| A **SpatialVortex** agent (policy inference) | `AgentPolicy` | `AgentPolicy::gpu(name, cpu_millis, memory_mb, gpu_memory_mb)` (GPU-backed) or `AgentPolicy::new(name, ResourceRequirements)` |
| World + its agents (one schedulable unit) | `SimCell` | `SimCell::new(id, world, tick).with_agent(a).with_co_placement(CoPlacement::InterconnectLocalGpu)` |
| Region of interest (interest management) | `Region3D` | `.with_region(Region3D::new(x, y, z, radius))` |
| Tick / frame deadline | `SimCell::tick` + `next_deadline` | `SimCell::new(id, world, Duration::from_millis(50))` (deadline defaults to `now + tick`) |

`CoPlacement` choices:
- `InterconnectLocalGpu` — world + agents on one node, GPUs prefer peer/contiguous devices (keep the world↔agent tensor exchange on-box). **Use this for SpatialVortex-in-a-world.**
- `SameNode` — one node, no GPU-locality preference.
- `Spread` — members on distinct nodes (replicated worlds / fault isolation).

---

## 3. The minimal working demonstration (first milestone)

Goal: one **eustress cell** — a world shard + a few SpatialVortex agents — gang-placed by Forge, with the RSI tick loop running and world state persisted. This is the "working demonstration soon" target.

```rust
use std::sync::Arc;
use std::time::Duration;
use forge_orchestration::{ForgeBuilder, Result};
use forge_orchestration::scheduler::NodeResources;
use forge_orchestration::scheduler::sim::{SimCell, SimWorld, AgentPolicy, CoPlacement, Region3D};
use forge_orchestration::storage::RaftStateStore;
use forge_orchestration::types::{GpuResources, NodeId};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Durable, recoverable world state (L4). Single node for the demo.
    let store = Arc::new(RaftStateStore::open_persistent(1, std::path::Path::new("./eustress_state")).await?);
    let forge = ForgeBuilder::new().with_store(store).build()?;

    // 2. Register the box(es) Forge may place cells on (its GPUs are the locality target).
    forge.register_node(
        NodeResources::new(NodeId::new(), 64_000, 262_144) // 64 cores, 256 GB
            .with_gpu(GpuResources::new(0, "NVIDIA H100", 81_920).with_tensor_cores(true))
            .with_gpu(GpuResources::new(1, "NVIDIA H100", 81_920).with_tensor_cores(true)),
    );

    // 3. Describe ONE eustress cell: a world shard + 2 SpatialVortex agents, co-located,
    //    ticking at the simulation frame cadence. (Resource numbers = your real footprints.)
    let cell = SimCell::new("habitat-0", SimWorld::cpu(8_000, 16_384), Duration::from_millis(50))
        .with_region(Region3D::new(0.0, 0.0, 0.0, 256.0))           // spatial interest footprint
        .with_agent(AgentPolicy::gpu("spatialvortex-0", 1_000, 2_048, 40_960))
        .with_agent(AgentPolicy::gpu("spatialvortex-1", 1_000, 2_048, 40_960))
        .with_co_placement(CoPlacement::InterconnectLocalGpu)
        .with_priority(100);
    forge.submit_sim_cell(cell).await?;

    // 4. Drive the reconcile loop: it gang-schedules the cell onto the node.
    let mut reconciler = forge.new_reconciler().await?;
    let report = reconciler.reconcile_once().await?;
    assert_eq!(report.sim_scheduled, 1, "the cell must gang-place all-or-nothing");

    // 5. Read where Forge placed the cell (which node, which member→node map).
    for binding in reconciler.sim_bindings() {
        println!("cell {} placed: {:?}", binding.cell_id, binding.placements);
        // -> hand `binding` to the Eustress sim driver (§4): launch the world + agents HERE.
    }

    // In production: `reconciler.run(shutdown_rx).await` keeps converging on a timer.
    Ok(())
}
```

**What this proves:** Forge admits the cell only if the world **and** both agents fit together on interconnect-local GPUs (no half-deadlock), persists the binding durably, and hands you the placement. Running it is the first real validation of the substrate.

---

## 4. The tick loop — who does what

Forge **places** the cell. Eustress **runs** it. The per-tick loop lives in your sim driver, not in Forge:

```text
Forge (once / on change):   reconcile -> gang-place cell -> SimBinding { cell, members -> node/GPUs }
Eustress (every tick, on the placed node):
    OBSERVE world state            (read from RaftStateStore / your live state)
    -> SpatialVortex inference      (run each agent's policy; this is YOUR L8 code)
    -> UPDATE world model           (RSI: hypothesize / fork / evaluate / merge — YOUR L5/L9/L10)
    -> REWRITE Rune (iff Kernel-valid)   (YOUR L12 validator gates this)
    -> ACT in the world
    -> persist world delta          (store.set(key, toml/msgpack bytes) — durable via raft-persist)
    -> respect the tick deadline    (the cell's `tick` cadence; advance_deadline() each frame)
```

- **Tick deadline:** Forge's `TickDeadlineScheduler` / the cell's `tick` cadence is how you express "this cell must advance to frame N by time T." Use it to order which cells get the next scheduling slot when the box is contended (most-urgent-tick-first). Missed ticks are first-class: `MissPolicy::{Late, Drop, Backpressure{max_lag}}` (`scheduler::deadline`).
- **World state:** read/write through the `StateStore` trait on your `RaftStateStore` (`get`/`set`/`delete`/`list_prefix`, keyed by your TOML/state-bus keys). It is durable (survives restart) and, in multi-node mode, replicated.
- **MoE routing:** route an agent's inference to the right timescale expert with `forge_orchestration::moe` (`LoadAwareMoERouter`, `GpuAwareMoERouter`, `VersionAwareMoERouter`).

---

## 5. Phase 0 first — the Kernel (do not skip)

Per `eustress_engine_architecture.pdf`: **"Do not write a single line of Layer 1 code before the Kernel is specified."** Forge does **not** provide the Kernel or Rune DSL — those are Eustress's. Before the demo means anything:

1. Spec the **Kernel + Rune DSL + validator** (L12): universe laws as a formal grammar, Rune syntax/semantics, an accept/reject validator + test suite.
2. SpatialVortex agents in the demo cell should be **Kernel-validated Rune programs** (stub them initially if needed, but the validation seam must exist).
3. The RSI rewrite step (§4) must call your Kernel validator before committing a Rune rewrite — Forge will faithfully keep running whatever you place, but it does not know your universe laws.

---

## 6. Single-node vs multi-node + durability

| Mode | How | Use for |
|---|---|---|
| In-memory (default) | `MemoryStore` / `FileStore` | unit tests, no durability |
| Single-node, durable | `RaftStateStore::open_persistent(node_id, dir)` *(raft-persist)* | dev box, the demo, durable world state |
| Multi-node cluster | `RaftStateStore::start_node(node_id, bind_addr)` on each box, then `initialize_cluster(members)` on one *(raft)* | replicated, fault-tolerant world state across machines |

Multi-node replicates writes and elects a new leader if one dies (tested: replication + failover). Reads are leader-local read-your-writes (see §9).

---

## 7. Forge public API quick-map

```
forge_orchestration
├── ForgeBuilder / Forge        runtime: with_store, build, register_node, submit_sim_cell,
│                               new_reconciler, run, route, submit_job, scale_job
├── scheduler::sim              SimCell, SimWorld, AgentPolicy, CoPlacement, Region3D,
│                               MemberRole, GangGroup, SimMember
├── scheduler::gang             GangScheduler, GangDecision, GangReservation
├── scheduler::deadline         DeadlineQueue, TickDeadlineScheduler, MissPolicy, DeadlineEntry
├── scheduler::reconcile        Reconciler, Assignment, SimBinding, ReconcileReport, MetricsSource
├── scheduler                   NodeResources, ResourceRequirements, Workload,
│                               BinPackScheduler, SpreadScheduler, GpuLocalityScheduler, LearnedScheduler
├── storage                     RaftStateStore, StateStore, MemoryStore, FileStore, keys
├── moe                         MoERouter + Default/LoadAware/RoundRobin/GpuAware/VersionAware routers
├── autoscaler                  Autoscaler, AutoscalerConfig
└── types                       NodeId, GpuResources, Region, Expert, Shard
```

---

## 8. Suggested build order in EustressEngine

1. **Kernel + Rune DSL + validator** (Phase 0; Eustress-only).
2. Add `forge-orchestration = "0.6"` to an orchestration crate.
3. Wrap one SpatialVortex agent + a minimal world shard as a `SimCell` (§3).
4. Get the cell gang-placed by Forge and persist a trivial world state to the durable store (§3, §6).
5. Drive the RSI tick loop on the placed cell (§4); persist world delta each tick.
6. **Measure the one thing that matters:** does the agents' world-model prediction accuracy climb over ticks under eustress parameters? That is the milestone that earns the next dollar of compute.
7. Scale out: multi-node cluster (§6), more cells, interest-managed regions.

---

## 9. Honest constraints (read before you rely on it)

- **Multi-node Raft is tested in-process** (3 nodes on localhost: replication + leader failover). It is *not* yet hardened at real cluster scale, under partitions, or with large/long-running logs.
- **GPU "locality" is approximate** — co-placement prefers contiguous/peer device IDs, not real NVLink/NVSwitch topology. Good enough to express "keep world+agents on-box"; refine when you have real topology data.
- **Reads are leader-local read-your-writes**, not linearizable across a multi-node cluster. Fine for world state owned by one cell; design accordingly for cross-cell reads.
- **No real GPU-workload validation yet.** The demo in §3 *is* that validation — it is the first time Forge schedules a real Eustress cell. Expect to find rough edges and file them back to Forge.
- Forge does **not** launch your agent processes for you (no built-in container/process driver wired for Eustress yet). At single-node you run the placed cell in-process; at multi-node you use the `SimBinding` placement to decide where your runtime launches it.

---

*Forge `0.6` · generated for EustressEngine integration. Keep this guide versioned with the Forge dependency.*
