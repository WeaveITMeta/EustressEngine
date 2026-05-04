# Software-in-the-Loop (SiTL) & Hardware-in-the-Loop (HIL) Architecture

> Eustress Engine — simulation pipeline from intent to physical validation.

## Table of Contents

1. [Overview](#overview)
2. [SiTL — Software-in-the-Loop](#sitl)
3. [HIL — Hardware-in-the-Loop](#hil)
4. [Feedback Loop Architecture](#feedback-loop)
5. [MCP Tool Surface](#mcp-tool-surface)
6. [Proactive Workshop Agent](#proactive-workshop-agent)
7. [VCell Case Study Flow](#vcell-case-study-flow)

---

## Overview

Eustress separates product validation into two stages:

```
INTENT → [SiTL] → FEEDBACK GATE → [HIL] → PHYSICAL PRODUCT
           ↑                          |
           └──── Repairman Loop ──────┘
```

- **SiTL** runs entirely in software: ECS simulation, Rune scripts,
  electrochemistry tick, watchpoints, scenarios. Zero hardware required.
- **HIL** bridges to physical test rigs, lab instruments, and fabrication
  equipment. Eustress acts as the orchestrator, pushing parameters and
  pulling measurements via MCP tools and external adapters.
- The **Feedback Gate** is the promotion boundary — a prototype branch
  must pass all SiTL scenario criteria before it is eligible for HIL.

---

## SiTL — Software-in-the-Loop

### Architecture

```
┌─────────────────────────────────────────────────────┐
│  Eustress Engine (Bevy ECS)                         │
│                                                     │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────┐ │
│  │ Rune Scripts  │  │ Electrochemistry│ │ Physics  │ │
│  │ (SoulScripts) │  │ Tick System    │  │ (Avian3D)│ │
│  └──────┬───────┘  └──────┬───────┘  └────┬──────┘ │
│         │                 │                │        │
│         ▼                 ▼                ▼        │
│  ┌──────────────────────────────────────────────┐   │
│  │           SimValuesResource (watchpoints)     │   │
│  └──────────────────────┬───────────────────────┘   │
│                         │                           │
│         ┌───────────────┼───────────────┐           │
│         ▼               ▼               ▼           │
│  runtime-snapshot.json  telemetry.jsonl  Timeline    │
│  (4 Hz disk write)      (append-only)    (Slint UI) │
└─────────────────────────────────────────────────────┘
         │                │
         ▼                ▼
  ┌─────────────┐  ┌─────────────┐
  │ LSP Hover   │  │ MCP Tools   │
  │ (live vals) │  │ (Workshop/  │
  │             │  │  external)  │
  └─────────────┘  └─────────────┘
```

### Key Components

- **SimValuesResource** — central registry of named numeric watchpoints.
  Written by Rune scripts (`set_sim_value`), electrochemistry tick, and
  physics systems. Read by everything downstream.

- **runtime-snapshot.json** — written at 4 Hz by the engine. Contains
  `play_state`, `sim_values`, and `generated_at`. The LSP reads this for
  inline hover telemetry. MCP tools read it for `get_sim_value`,
  `list_sim_values`, and `get_simulation_state`.

- **telemetry.jsonl** — append-only log of watchpoint samples. Each line
  is `{ "t": "<rfc3339>", "values": { "key": f64 } }`. The
  `tail_telemetry` MCP tool reads this for time-series analysis.

- **sim-commands.jsonl** — command queue from MCP tools to the engine.
  `run_simulation`, `stop_simulation`, and `set_sim_value` all append
  here. The engine drains on the next frame tick.

- **Scenario Engine** — branches, evidence collection, pruning. Each
  scenario branch runs independently with its own parameter set.

### SiTL MCP Tools

| Tool | Purpose |
|------|---------|
| `run_simulation` | Start simulation with time scale and auto-stop |
| `stop_simulation` | Stop and return to Edit mode |
| `get_simulation_state` | Play state + all watchpoints |
| `get_sim_value` | Read a single watchpoint |
| `set_sim_value` | Write a watchpoint (initial conditions) |
| `list_sim_values` | All active watchpoints |
| `tail_telemetry` | Time-series watchpoint history |
| `query_stream_events` | Recent Eustress Stream events |

---

## HIL — Hardware-in-the-Loop

### Concept

HIL connects Eustress to physical test equipment. The engine remains the
orchestrator — it sets parameters, triggers measurements, and ingests
results through the same MCP tool interface used for SiTL.

```
┌───────────────────────┐      MCP       ┌────────────────────┐
│  Eustress Engine      │◄──────────────►│  HIL Adapter       │
│  (Workshop / Agent)   │   tool calls   │  (Python/Rust)     │
│                       │                │                    │
│  set_sim_value(...)   │ ──────────────►│  Set DAC output    │
│  run_simulation(...)  │ ──────────────►│  Trigger test rig  │
│  tail_telemetry(...)  │ ◄──────────────│  Stream ADC data   │
│  get_sim_value(...)   │ ◄──────────────│  Read measurement  │
└───────────────────────┘                └────────────────────┘
                                                  │
                                                  ▼
                                         ┌────────────────────┐
                                         │  Physical Hardware  │
                                         │  - Potentiostat     │
                                         │  - Thermal chamber  │
                                         │  - Load cell        │
                                         │  - Scope / LA       │
                                         └────────────────────┘
```

### HIL Adapter Contract

An HIL adapter is any process that:

1. Connects to the engine via MCP or the Engine Bridge
   (`<universe>/.eustress/engine.port`)
2. Translates `set_sim_value` calls into hardware commands
3. Translates hardware measurements back into `sim_values` that the
   engine's telemetry pipeline can ingest
4. Respects the same `sim-commands.jsonl` queue for `run`/`stop`

The adapter can be:
- A Python script using `pymcp` or `ureq` to POST to the MCP server
- A Rust crate that connects via the Engine Bridge WebSocket
- A LabVIEW VI that writes directly to `telemetry.jsonl`
- An Arduino/ESP32 firmware that streams over serial, with a thin bridge

### Promotion Gate: SiTL → HIL

A prototype branch is promoted to HIL when:

1. All scenario scripts pass (exit code 0, no panic)
2. All watchpoint bounds hold (e.g. `battery.temperature_c < 60.0`)
3. The diff between the branch and its parent is reviewed
   (`feedback_diff` tool)
4. The agent (or user) explicitly approves via `git_branch create`

The gate is currently human-in-the-loop. Future: automated promotion
via a `promote_to_hil` tool that validates criteria and creates the
HIL test plan.

---

## Feedback Loop Architecture

### The Three Pillars

```
┌────────────┐     stream events     ┌────────────┐
│ ARCHITECT  │ ◄──────────────────── │  WATCHMAN  │
│            │                       │            │
│ Generates  │     proposes fix      │ Monitors   │
│ structured │ ──────────────────►   │ live state │
│ specs from │                       │ via tail_  │
│ intent     │      validates        │ telemetry  │
│            │ ◄──────────────────── │            │
└────────────┘                       └────────────┘
      │                                    ▲
      │ generates                          │ detects
      ▼                                    │ anomaly
┌────────────┐                             │
│ REPAIRMAN  │ ────────────────────────────┘
│            │   reads telemetry, proposes
│ Diagnoses  │   parameter/script changes,
│ root cause │   re-runs simulation
└────────────┘
```

### Proactive Feedback Cycle

1. **Watchman** continuously monitors via `tail_telemetry` and
   `get_simulation_state`. Detects when a watchpoint exits bounds.

2. **Repairman** receives the anomaly signal. Uses `query_audit_log`
   to understand what changed, `feedback_diff` to compare against the
   last known-good state, and proposes a fix (parameter change or
   script edit).

3. **Architect** validates the fix against the original spec. If
   approved, applies via `set_sim_value` or `write_file` (for script
   changes), then re-runs with `run_simulation`.

4. The cycle repeats until all watchpoints hold or max iterations.

### MCP-Driven Improvement Cycle (VCell Example)

```
Workshop Agent:
  1. get_simulation_state          → "Editing, no sim running"
  2. set_sim_value("battery.mode", 2.0)  → configure discharge
  3. run_simulation(time_scale=10.0, duration_s=3600)
  4. [wait for completion]
  5. tail_telemetry(keys=["battery.soc","battery.temperature_c"])
  6. → detect: temperature_c exceeded 55°C at cycle 847
  7. feedback_diff("v0001", "v0002", path="Workspace/VCell")
  8. → identify: thermal_resistance changed from 2.5 to 2.0
  9. set_sim_value("thermal_resistance", 2.2)  → try intermediate
  10. run_simulation(time_scale=10.0, duration_s=3600)
  11. tail_telemetry → temperature_c peaked at 52°C ✓
  12. git_commit(message="v0003: thermal_resistance=2.2, temp<55°C")
  13. git_branch(action="create", name="v0003")
```

---

## MCP Tool Surface (Complete)

### Filesystem
| Tool | Description |
|------|-------------|
| `read_file` | Read file contents |
| `write_file` | Write file contents |
| `list_directory` | List directory contents |

### Entity (ECS)
| Tool | Description |
|------|-------------|
| `create_entity` | Create entity with components |
| `query_entities` | Query entities by criteria |
| `update_entity` | Update entity components |
| `delete_entity` | Delete entity |

### Simulation
| Tool | Description |
|------|-------------|
| `run_simulation` | Start simulation with time scale |
| `stop_simulation` | Stop simulation |
| `get_simulation_state` | Full sim state snapshot |
| `get_sim_value` | Read watchpoint |
| `set_sim_value` | Write watchpoint |
| `list_sim_values` | All watchpoints |
| `tail_telemetry` | Time-series telemetry |
| `get_tagged_entities` | Find entities by tag |
| `add_tag` / `remove_tag` | Manage entity tags |
| `raycast` | 3D scene raycast (needs Engine Bridge) |

### Git / Version Control
| Tool | Description |
|------|-------------|
| `git_status` | Working tree status |
| `git_commit` | Stage and commit |
| `git_log` | Commit history |
| `git_diff` | Uncommitted changes |
| `git_branch` | List/create/switch/delete branches |
| `feedback_diff` | Compare any two refs |

### Scripting
| Tool | Description |
|------|-------------|
| `execute_rune` | Run Rune script |
| `execute_luau` | Run Luau script |
| `image_to_code` | Vision → Rune script |
| `image_to_geometry` | Vision → 3D entities |
| `document_to_code` | Document → Rune script |
| `generate_docs` | Generate documentation |

### Audit & Memory
| Tool | Description |
|------|-------------|
| `query_audit_log` | Claude API call trail |
| `query_stream_events` | Recent Stream events |
| `remember` / `recall` | Persistent memory |
| `list_rules` / `list_workflows` | Introspection |

### Data & Network
| Tool | Description |
|------|-------------|
| `datastore_get` / `datastore_set` | Persistent key-value |
| `http_request` | External HTTP calls |
| `stage_file_change` | Proposed diff staging |

### AI / Embedvec
| Tool | Description |
|------|-------------|
| `find_similar_entities` | Semantic entity search |
| `suggest_swap_template` | Part swap suggestions |
| `suggest_contextual_edits` | Context-aware edits |
| `suggest_tool_defaults` | Tool parameter suggestions |

---

## VCell Case Study Flow

The complete improvement cycle for one battery cell prototype:

```
1. SETUP
   create_entity("VCell_v0001", class="Part", parent="Workspace")
   write_file("Workspace/VCell_v0001/battery_hud.rune", <script>)
   git_commit("v0001: baseline 21700 NMC cell")
   git_branch(create, "v0001")

2. SIMULATE
   set_sim_value("battery.capacity_ah", 5.0)
   set_sim_value("battery.mode", 2.0)  // discharge
   run_simulation(time_scale=100.0, duration_s=36000)  // 10 hours

3. ANALYZE
   tail_telemetry(keys=["battery.soc","battery.voltage","battery.temperature_c"])
   get_simulation_state()
   query_audit_log(count=5)

4. ITERATE
   feedback_diff("v0001", "HEAD")
   set_sim_value("battery.target_current", 10.0)  // 2C charge
   run_simulation(time_scale=100.0, duration_s=7200)

5. PROMOTE
   git_commit("v0002: 2C charge validated, temp<55°C")
   git_branch(create, "v0002")
   feedback_diff("v0001", "v0002", stat_only=true)

6. REPEAT until all scenarios pass
   → Real metrics accumulate: cycles simulated, failures caught,
     branches promoted, turnaround time.
```

This cycle works identically from:
- **Workshop** (in-engine chat)
- **External IDE** (Cursor/Windsurf via MCP server)
- **CLI** (direct MCP tool calls)
