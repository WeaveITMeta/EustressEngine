# Iggy Clustering Architecture

> **Status**: In Development — integration points are built, clustering topology is planned.
> Current implementation uses a single-node Iggy server. This document defines the target
> multi-node topology and the per-system scaling strategy as the engine matures.
>
> Companion to: [EUSTRESS_FORGE.md](./EUSTRESS_FORGE.md) · [APEX_ENGINE.md](./APEX_ENGINE.md)

---

## Table of Contents

| Section | Topic |
|---|---|
| [1. Why Iggy Clustering](#1-why-iggy-clustering) | Motivation and design constraints |
| [2. Cluster Topology](#2-cluster-topology) | Node roles, stream layout, partition map |
| [3. Per-System Node Assignment](#3-per-system-node-assignment) | Which Eustress system lives on which node |
| [4. Scene Delta Pipeline](#4-scene-delta-pipeline) | Properties, move/rotate/scale, spawn/despawn |
| [5. Simulation Pipeline](#5-simulation-pipeline) | Monte Carlo, Rune scripts, VIGA iterations |
| [6. Workshop and IoT Pipeline](#6-workshop-and-iot-pipeline) | Physical twin telemetry, product convergence |
| [7. Agent Command Pipeline](#7-agent-command-pipeline) | CLI agent-in-the-loop, headless control |
| [8. Connection Pool Architecture](#8-connection-pool-architecture) | Persistent clients, no per-call reconnects |
| [9. Partition Strategy](#9-partition-strategy) | Shard keys per topic |
| [10. Backpressure and Drop Policy](#10-backpressure-and-drop-policy) | Bounded channels, tail-drop, offline behavior |
| [11. Forge Integration](#11-forge-integration) | Nomad job placement for Iggy nodes |
| [12. Migration Path](#12-migration-path) | Single-node → cluster rollout phases |

---

## 1. Why Iggy Clustering

Eustress uses Apache Iggy as the **append-only streaming backbone** replacing all prior
file-based caching. Every mutable state change — properties panel edits, transform gizmo
commits, simulation results, Rune script executions, IoT telemetry — is a message in Iggy.

A single Iggy node handles ~1 GB/s writes and easily covers a studio session or a small
game server. Clustering becomes necessary when:

- **Multiple concurrent sessions** share simulation history (workshop, multi-user studio)
- **Play server replication** needs sub-millisecond delta broadcast to N connected clients
- **IoT telemetry ingestion** from hundreds of workshop sensors exceeds single-node bandwidth
- **Workshop convergence analysis** runs across millions of stored product-iteration records
- **Disaster recovery** requires durable, replicated simulation history

The design principle is **Iggy mirrors Forge** — every Nomad job type that exists in
[EUSTRESS_FORGE.md](./EUSTRESS_FORGE.md) has a corresponding Iggy stream/topic assignment.
Iggy nodes are Nomad jobs, placed by the `ForgeController`, not hand-provisioned.

---

## 2. Cluster Topology

### Node Roles

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Iggy Cluster (3+ nodes)                            │
│                                                                             │
│   ┌──────────────────────┐   ┌──────────────────────┐   ┌────────────────┐  │
│   │   Node A — PRIMARY   │   │   Node B — REPLICA   │   │ Node C — READ  │  │
│   │                      │   │                      │   │   REPLICA      │  │
│   │  scene_deltas        │◄──│  scene_deltas        │   │                │  │
│   │  agent_commands      │   │  sim_results         │   │  sim_results   │  │
│   │  agent_observations  │   │  iteration_history   │   │  workshop_iter │  │
│   │  rune_scripts        │   │  workshop_iter       │   │  rune_scripts  │  │
│   │                      │   │  agent_*             │   │                │  │
│   │  [write path]        │   │  [write + replica]   │   │  [read-only]   │  │
│   └──────────────────────┘   └──────────────────────┘   └────────────────┘  │
│                                                                             │
│   ┌──────────────────────────────────────────────────────────────────────┐  │
│   │                  Forge Control Plane (Rust + Nomad)                   │  │
│   │   ForgeController → places Iggy Nomad jobs, monitors health,         │  │
│   │                     re-routes streams on failure                      │  │
│   └──────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Stream Layout

One Iggy **stream** per Eustress deployment. Topics within that stream are divided by
data class and write frequency:

```text
stream: "eustress"
├── scene_deltas          — high frequency, ~1M msg/s peak, 8 partitions
├── agent_commands        — low frequency, ordered, 1 partition
├── agent_observations    — low frequency, ordered, 1 partition
├── sim_results           — batch, ~1 msg per run_simulation(), 4 partitions
├── iteration_history     — batch, ~1 msg per VIGA cycle, 2 partitions
├── rune_scripts          — batch, ~1 msg per execute_and_apply(), 4 partitions
├── workshop_iterations   — batch, ~1 msg per optimize cycle, 2 partitions
└── iot_telemetry         — future, continuous, 16 partitions (one per sensor zone)
```

### Minimum Viable Cluster

For development, a single node handles everything. Production minimum is 3 nodes:
1 primary writer, 1 synchronous replica (durability), 1 read replica (Studio/CLI queries).

---

## 3. Per-System Node Assignment

Each Eustress system writes to or reads from a specific node role. The connection pool
(`IggyChangeQueue`, `SimStreamWriter`) points to the correct node via `IggyConfig.url`.

| Eustress System | Node Role | Topic(s) | Direction |
|---|---|---|---|
| **Properties Panel** (`slint_ui.rs`) | Primary | `scene_deltas` | Write |
| **Move / Rotate / Scale tools** | Primary | `scene_deltas` | Write |
| **Spawn / Despawn** (`spawn.rs`) | Primary | `scene_deltas` | Write |
| **Rename (Explorer)** | Primary | `scene_deltas` | Write |
| **Undo / Redo** (`undo.rs`) | Primary | `scene_deltas` | Write |
| **TOML Materializer** | Replica B | `scene_deltas` | Read (consumer group) |
| **Explorer live tree** | Replica B | `scene_deltas` | Read (consumer group) |
| **run_simulation()** | Replica B | `sim_results` | Write |
| **VIGA process_feedback()** | Replica B | `iteration_history` | Write |
| **Rune execute_and_apply()** | Replica B | `rune_scripts` | Write |
| **Workshop optimize cycle** | Replica B | `workshop_iterations` | Write |
| **CLI sim replay** | Read Replica C | `sim_results` | Read |
| **Studio convergence panel** | Read Replica C | `workshop_iterations` | Read |
| **CLI agent commands** | Primary | `agent_commands` | Write |
| **CLI agent observations** | Primary | `agent_observations` | Read |
| **IoT telemetry** (future) | Dedicated node | `iot_telemetry` | Write + Read |
| **Play server replication** (future) | Dedicated node | `play_state` | Write |

**Key insight:** High-frequency write systems (Properties, tools, spawn) hit the **Primary**
only. The **Read Replica** absorbs all analytics queries (CLI replay, Studio convergence)
so they never compete with the live editor write path.

---

## 4. Scene Delta Pipeline

### Current State

```text
slint_ui.rs::SlintAction::PropertyChanged
    → mutates Transform / BasePart / Instance directly in ECS
    → writes TOML synchronously to disk (blocking on main Bevy thread)
    ← NO Iggy delta emitted yet
```

### Target State with Clustering

```text
slint_ui.rs::SlintAction::PropertyChanged  (Bevy PostUpdate)
    → mutates ECS component (unchanged)
    → queue.send_delta(SceneDelta { kind, entity, payload })   [<1 µs]
        → tokio UnboundedSender → delta channel
            → run_delta_producer task (tokio background)
                → batch up to 512 deltas, linger 1ms
                    → IggyClient::send_messages → Primary Node
                        → scene_deltas topic, partition = entity % 8

TOML Materializer (separate consumer task, Node B)
    → polls scene_deltas consumer group
    → accumulates SceneMirror in memory
    → debounced write to disk every 250ms
    ← NO disk I/O on Bevy main thread

Explorer live tree (Slint subscriber task, Node B)
    → polls scene_deltas at offset = last_seen_seq
    → sends diff to Slint UI model
    ← Real-time tree without ECS snapshot extraction overhead

UndoStack.push() injection point
    → after every committed Action, call action_to_deltas(action) → Vec<SceneDelta>
    → queue.send_delta() for each
    ← Undo/redo stream is a complete replayable audit trail
```

### Scene Delta Node Scaling

Each of the 8 `scene_deltas` partitions maps to one partition leader. At 8 partitions,
a 3-node cluster handles partition leaders:

```text
Node A (Primary):    partitions 0, 1, 2, 3  (leader for fast-path write)
Node B (Replica):    partitions 4, 5, 6, 7  (leader for secondary sessions)
Node C (Read):       follower for all partitions (serves CLI + Studio queries)
```

Partition assignment per entity: `entity_id % 8`. Entities in the same model/folder
naturally co-locate on the same partition because ECS assigns sequential IDs.

---

## 5. Simulation Pipeline

### Current Implementation

```
run_simulation()
    → builds SimRecord (rkyv)
    → publish_sim_result_sync(config, record)
        → tokio::Handle::try_current().spawn(...)
            → SimStreamWriter::connect()   ← NEW TCP CONNECT EVERY CALL ← bottleneck
                → send_messages → sim_results topic
```

### Target Implementation

`SimStreamWriter` becomes a long-lived Bevy `Resource`, injected into `run_simulation()`:

```rust
// App setup
app.insert_resource(
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(
            SimStreamWriter::connect(&SimStreamConfig::from_iggy_config(&iggy_config))
        ).ok()
    })
);

// run_simulation signature (feature-gated):
pub fn run_simulation(
    scenario: &mut Scenario,
    config: &SimulationConfig,
    stream: Option<&SimStreamWriter>,  // injected from Bevy Resources
) -> SimulationResult
```

### Simulation Node Assignment

Simulation workloads are CPU-heavy (rayon parallel MC). The Iggy write for sim results
is a single message per run — negligible bandwidth. It routes to Replica B which also
serves as the write target for VIGA + Rune records, keeping all analytical history
co-located for replay joins:

```text
Node B Replica — owns all analytical write topics:
  sim_results           (1 msg per MC run, ~200 bytes rkyv)
  iteration_history     (1 msg per VIGA cycle, ~2KB with code)
  rune_scripts          (1 msg per execute_and_apply, ~500 bytes)
  workshop_iterations   (1 msg per optimize cycle, ~1KB)

Node C Read Replica — serves all read queries:
  CLI: eustress sim replay / best / convergence / scripts
  Studio: convergence panel, scenario dashboard
```

---

## 6. Workshop and IoT Pipeline

### Workshop Convergence Loop

```text
Workshop optimize cycle (Bevy system, Nomad: game-logic job):
    → snapshot Bevy World properties → properties_snapshot_toml (4KB max)
    → run_simulation() → SimRecord → Node B (sim_results)
    → execute_and_apply() → RuneScriptRecord → Node B (rune_scripts)
    → compute fitness score
    → publish WorkshopIterationRecord → Node B (workshop_iterations)

Studio convergence panel (Slint subscriber):
    → poll Node C (workshop_iterations) every 5s
    → render fitness curve across generations

CLI: eustress sim convergence --product "Bracket Assembly"
    → poll Node C (workshop_iterations) paginated
    → print table + best generation marker
```

### IoT Telemetry (Future — Dedicated Node)

Workshop physical sensors (GPS, accelerometer, torque, environmental) produce continuous
high-frequency data. This warrants a dedicated Iggy node with 16 partitions, one per
sensor zone (bench, shelf, zone A–P):

```text
Node D (IoT — future):
  iot_telemetry   (16 partitions, partition = sensor_zone_id % 16)
    → 100–1000 Hz per sensor, ~50 bytes per reading
    → consumers: WorkshopTwinPlugin (Bevy), GPS tracker, status dashboard
```

At 100 sensors × 100 Hz × 50 bytes = ~500 KB/s — well within single-node capacity.
A dedicated node ensures IoT noise never contends with scene delta write latency.

---

## 7. Agent Command Pipeline

The CLI agent-in-the-loop uses `agent_commands` (CLI → Studio) and `agent_observations`
(Studio → CLI). These are low-frequency, strictly ordered, and latency-sensitive:

```text
CLI: eustress agent --script "set_probability(...)"
    → IggySession::send_raw → Primary Node (agent_commands, partition 0)
        → run_command_consumer task in Studio reads polling loop (10ms interval)
            → decodes AgentCommand → Bevy Event IncomingAgentCommand
                → Bevy system executes action
                    → sends AgentObservation back → Primary (agent_observations)
                        → CLI poll_raw reads reply
```

Both topics stay on the **Primary node** to guarantee ordering (1 partition each) and
minimize command-to-observation round-trip latency.

**Target latency**: <15ms round trip on localhost, <50ms over LAN.

---

## 8. Connection Pool Architecture

### Current Problem

`publish_*_sync()` helpers in `sim_stream.rs` create a **new TCP connection per call**:

```rust
// CURRENT — creates new IggyClient on every run_simulation():
pub fn publish_sim_result_sync(config: SimStreamConfig, record: SimRecord) {
    handle.spawn(async move {
        match SimStreamWriter::connect(&config).await {  // ← 50–200ms TCP handshake
```

### Target Architecture

One persistent connection per system role, held for the process lifetime:

```text
Bevy App Resources:
  IggyChangeQueue           — holds delta_tx + obs_tx channels → 3 background tasks
                              (scene delta producer, observation producer, command consumer)
                              All 3 use persistent IggyClient connections.

  Arc<SimStreamWriter>      — 1 persistent connection to Replica B
                              Shared by: run_simulation, process_feedback,
                                         execute_and_apply, workshop cycle

  Arc<SimStreamReader>      — 1 persistent connection to Read Replica C
                              Shared by: Studio convergence panel, scenario dashboard

  Arc<SimStreamReader>      — CLI only (spawned once per CLI process, not Bevy)
```

```rust
// TARGET — IggyConfig addition:
pub struct IggyConfig {
    pub url: String,                      // Primary node URL
    pub replica_url: String,              // Replica B URL (sim writes)
    pub read_replica_url: String,         // Replica C URL (queries)
    pub stream_name: String,
    pub scene_delta_partitions: u32,      // default: 8
    pub sim_result_partitions: u32,       // default: 4
    pub channel_capacity: usize,          // default: 65_536
    pub drop_on_full: bool,               // default: true
    pub delta_linger_ms: u64,            // default: 1
    pub sim_linger_ms: u64,              // default: 0
    pub agent_poll_ms: u64,              // default: 10
}
```

---

## 9. Partition Strategy

### Shard Key Assignments

| Topic | Partition Count | Shard Key | Rationale |
|---|---|---|---|
| `scene_deltas` | 8 | `entity_id % 8` | Sequential ECS IDs → natural co-location |
| `sim_results` | 4 | `scenario_id_lo % 4` | Group all runs for one scenario |
| `iteration_history` | 2 | `session_id_lo % 2` | Group iterations by session |
| `rune_scripts` | 4 | `scenario_id_lo % 4` | Co-locate with sim_results for join |
| `workshop_iterations` | 2 | `product_id_lo % 2` | Group all cycles for one product |
| `agent_commands` | 1 | — | Strict ordering required |
| `agent_observations` | 1 | — | Strict ordering required |
| `iot_telemetry` | 16 | `sensor_zone_id % 16` | One zone per partition |

### Partitioning Code Change

```rust
// CURRENT in run_delta_producer — balanced round-robin:
let partitioning = Partitioning::balanced();

// TARGET — entity-keyed, co-locates related entities on same partition:
let partitioning = Partitioning::messages_key_u32(
    (delta.entity % config.scene_delta_partitions as u64) as u32
);

// For sim topics — scenario-keyed:
let partitioning = Partitioning::messages_key_u32(
    (record.scenario_id as u32) % config.sim_result_partitions
);
```

### Zero-Copy Read Path

`poll_all()` in `sim_stream.rs` currently clones each payload:

```rust
// CURRENT — one heap allocation per polled message:
all.push(msg.payload.to_vec());

// TARGET — pass Bytes directly (Bytes is Arc<[u8]>, clone is O(1)):
all.push(msg.payload.clone());  // returns sim_stream as Vec<Bytes>
// Callers: SimRecord::from_bytes(b.as_ref()) — Bytes derefs to &[u8], no copy
```

---

## 10. Backpressure and Drop Policy

### Current Problem

`unbounded_channel()` for scene deltas has no backpressure. If Iggy is offline or slow,
the channel grows without bound and will OOM the Studio process.

### Target Bounded Channel

```rust
// iggy_queue.rs — replace unbounded_channel:
let (delta_tx, delta_rx) = tokio::sync::mpsc::channel::<SceneDelta>(
    config.channel_capacity  // default 65_536
);

// send_delta — non-blocking, tail-drop on full:
pub fn send_delta(&self, delta: SceneDelta) {
    match self.delta_tx.try_send(delta) {
        Ok(_) => {}
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            // Silent drop — studio never stalls waiting for Iggy
            // Metrics counter incremented here for observability
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
            warn!("IggyChangeQueue: delta channel closed");
        }
    }
}
```

### Memory Budget

| Channel | Capacity | Message Size | Max RAM |
|---|---|---|---|
| `scene_deltas` | 65,536 | ~68–96 bytes | **~6 MB** |
| `agent_observations` | 1,024 | ~256 bytes | **~256 KB** |
| Sim records (in-process) | N/A | 1 per call, immediate | **<1 KB** |

**Total bounded memory for Iggy channels: ~6.3 MB worst case.**

### Offline Behavior

When Iggy is unreachable at startup, the engine continues without streaming:

```rust
// IggyPlugin::setup_iggy_queue system:
match init_iggy(&config).await {
    Ok(queue) => { commands.insert_resource(queue); }
    Err(e) => {
        warn!("Iggy unavailable ({e}) — running without streaming. TOML writes remain synchronous.");
        // No IggyChangeQueue resource inserted — all cfg(iggy-streaming) paths check
        // Option<Res<IggyChangeQueue>> and skip silently.
    }
}
```

When Iggy goes offline mid-session:
- Scene delta channel fills to capacity, new deltas tail-drop silently
- Sim/iteration records are lost for that session (acceptable — they re-run on demand)
- TOML write-back continues via the synchronous path in `slint_ui.rs`
- On reconnect, `IggyPlugin` re-initializes with a fresh connection

---

## 11. Forge Integration

### Iggy as Nomad Jobs

Iggy nodes are Nomad jobs placed by the `ForgeController`, not bare metal processes.
Each node is a `nomad job` with resource constraints:

```hcl
# infrastructure/forge/nomad/iggy-primary.hcl
job "iggy-primary" {
  type = "service"

  group "iggy" {
    count = 1

    task "iggy-server" {
      driver = "exec"
      config {
        command = "iggy-server"
        args    = ["--config", "/etc/iggy/primary.toml"]
      }
      resources {
        cpu    = 4000   # 4 cores — append-only log is CPU-light
        memory = 8192   # 8 GB — message buffer + index cache
      }
    }
  }
}
```

### Consul Service Discovery

Iggy nodes register with Consul. `IggyConfig` URLs are resolved dynamically:

```rust
// Target: resolve Iggy URL from Consul instead of hardcoded string
pub async fn resolve_iggy_url(consul_addr: &str, service: &str) -> String {
    // GET http://consul:8500/v1/health/service/{service}?passing=true
    // Returns: iggy://iggy:iggy@{node_ip}:{port}
}
```

`IggyConfig.url` becomes `iggy://iggy:iggy@iggy-primary.service.consul:8090` in production,
resolved by Consul DNS.

### ForgeController Iggy Health

The `ForgeController` monitors Iggy node health as part of its server lifecycle loop:

```rust
// forge-orchestration-0.4.2/src/controlplane/
// Adds Iggy health probe alongside game server health checks:
ForgeController::health_check()
    → GET iggy-primary.service.consul:8090/stats
    → if down: re-route IggyConfig.url to replica, notify Studio via
               agent_observations broadcast
```

---

## 12. Migration Path

### Phase 1 — Single Node (Current, In Development)

- One `iggy-server` process on localhost
- All topics on 1 partition each
- `publish_*_sync()` one-shot connection (known bottleneck, accepted for now)
- `UnboundedSender` for scene deltas (no backpressure, accepted for now)
- `IggyChangeQueue` wired to Properties Panel and tools — **not yet complete**

**Acceptance criteria**: Studio session works with Iggy running locally. CLI `sim replay`
returns records from a completed simulation run.

### Phase 2 — Connection Pool + Bounded Channels

Apply optimizations from [Section 8](#8-connection-pool-architecture) and
[Section 10](#10-backpressure-and-drop-policy):

- Replace `publish_*_sync()` with persistent `Arc<SimStreamWriter>` Bevy Resource
- Replace `unbounded_channel` with bounded channel, capacity 65,536
- Implement `send_delta()` tail-drop instead of `unwrap()`
- Wire `IggyChangeQueue.send_delta()` into `UndoStack::push()` injection point
- Wire `send_delta()` into `SlintAction::PropertyChanged` handler

**Acceptance criteria**: 1M deltas/sec through the engine without OOM. Iggy server
restart does not crash Studio.

### Phase 3 — Partitioned Topics

Apply [Section 9](#9-partition-strategy):

- Migrate `scene_deltas` to 8 partitions with entity shard key
- Migrate `sim_results` / `rune_scripts` to 4 partitions with scenario shard key
- Zero-copy read path in `sim_stream.rs` (`Vec<Bytes>` instead of `Vec<Vec<u8>>`)
- Add `replica_url` and `read_replica_url` to `IggyConfig`
- Split write path (Primary) from read path (Read Replica C)

**Acceptance criteria**: CLI `sim replay` does not add measurable latency to live Studio
editing. Properties panel edits remain <1µs hot path.

### Phase 4 — Multi-Node Cluster via Forge

- Deploy 3-node Iggy cluster as Nomad jobs (`iggy-primary`, `iggy-replica`, `iggy-read`)
- Wire `ForgeController` health monitoring to Iggy nodes
- Consul service discovery for `IggyConfig.url` resolution
- IoT telemetry dedicated node (Node D) with 16 partitions
- Play server replication topic on dedicated node

**Acceptance criteria**: Single Iggy node failure does not interrupt live Studio session.
ForgeController re-routes within <500ms. Workshop IoT telemetry ingests 1000+ sensors.

---

## Appendix: Current Implementation Files

| File | Role |
|---|---|
| `@eustress/crates/common/src/iggy_delta.rs` | `SceneDelta`, `DeltaKind`, topic constants, `AgentCommand/Observation` |
| `@eustress/crates/common/src/iggy_queue.rs` | `IggyChangeQueue` Resource, `IggyPlugin`, background producer/consumer tasks |
| `@eustress/crates/common/src/toml_materializer.rs` | `SceneMirror`, debounced TOML writer, Iggy consumer |
| `@eustress/crates/common/src/sim_record.rs` | `SimRecord`, `IterationRecord`, `RuneScriptRecord`, `WorkshopIterationRecord` |
| `@eustress/crates/common/src/sim_stream.rs` | `SimStreamWriter`, `SimStreamReader`, `bootstrap_sim_topics` |
| `@eustress/crates/cli/src/main.rs` | `eustress sim replay/best/convergence/scripts` subcommands |
| `@infrastructure/forge/nomad/` | Nomad job definitions (Iggy nodes planned here) |
