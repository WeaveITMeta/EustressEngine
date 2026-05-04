# VCell Case Study — End-to-End Improvement Cycle

> Verification walkthrough exercising SiTL, MCP tools, LSP, and the
> proactive Watchman → Repairman feedback loop.

## Table of Contents

1. [Objective](#objective)
2. [Prerequisites](#prerequisites)
3. [Phase 1: Baseline Simulation](#phase-1-baseline-simulation)
4. [Phase 2: Proactive Detection](#phase-2-proactive-detection)
5. [Phase 3: Agent-Driven Repair](#phase-3-agent-driven-repair)
6. [Phase 4: Verification](#phase-4-verification)
7. [Phase 5: Git Feedback Loop](#phase-5-git-feedback-loop)
8. [Metrics Checklist](#metrics-checklist)
9. [Component Coverage](#component-coverage)

---

## Objective

Demonstrate that the Eustress Engine can autonomously detect a
simulation anomaly, diagnose it via the Workshop agent, apply a
corrective action through MCP tools, and verify the fix — all without
manual intervention after the initial simulation start.

The VCell (Voltec battery cell) electrochemistry model is the test
subject because it produces continuous telemetry with well-defined
safety thresholds (voltage, temperature, dendrite risk).

---

## Prerequisites

- Universe with a Space containing a VCell prototype entity
- Electrochemistry tick system producing `battery.*` sim values
- SoulScript attached to the VCell entity (Rune `.rune` file)
- Workshop panel open with a valid BYOK API key configured
- Watchman enabled (default: `WatchmanConfig.enabled = true`)

---

## Phase 1: Baseline Simulation

### MCP Tools Exercised

| Tool | Purpose |
|------|---------|
| `run_simulation` | Start the electrochemistry tick at 10x time scale |
| `get_simulation_state` | Confirm Playing mode and initial watchpoint values |
| `tail_telemetry` | Read first few telemetry samples from `telemetry.jsonl` |

### Expected Behaviour

1. User (or agent) calls `run_simulation(time_scale=10.0)`.
2. Engine-side `drain_sim_commands` reads `sim-commands.jsonl` and
   transitions `PlayModeState` to `Playing`.
3. `write_telemetry_log` begins appending 1 Hz JSONL entries.
4. `runtime-snapshot.json` updates at 4 Hz with sim values + ECS schema.
5. `get_simulation_state` returns `play_mode: "Playing"` with live values.

### Verification Command (Workshop Chat)

```
Run the VCell simulation at 10x speed and show me the initial battery state.
```

Claude should call `run_simulation`, then `get_simulation_state`, and
report voltage, SOC, temperature, etc.

---

## Phase 2: Proactive Detection

### Components Exercised

| Component | Role |
|-----------|------|
| `SimValuesResource` | Carries live battery values per frame |
| `WatchmanConfig` | Threshold rules (temperature > 60°C, voltage > 4.25V) |
| `watchman_monitor` | Polls every 5 seconds, compares against thresholds |
| `IdeationPipeline.add_user_message` | Injects synthetic alert |

### Expected Behaviour

1. The electrochemistry model's thermal runaway path drives
   `battery.temperature_c` above the 60°C threshold.
2. `watchman_monitor` detects the breach on its next 5-second poll.
3. A synthetic `[Watchman Alert]` message is injected into the
   Workshop pipeline with the Repairman prompt.
4. `dispatch_chat_request` fires on the next frame — Claude receives
   the alert as a "user" message with full tool access.

### Verification

- The Workshop chat panel should show a Watchman alert message
  without any user typing.
- The alert includes the breached key, current value, and threshold.
- `query_audit_log` should show the Claude API call triggered by the
  alert (topic: "workshop.agent.watchman").

---

## Phase 3: Agent-Driven Repair

### MCP Tools Exercised

| Tool | Purpose |
|------|---------|
| `tail_telemetry` | Claude reads recent temperature trend |
| `get_simulation_state` | Claude checks current sim state + all values |
| `feedback_diff` | Claude compares current branch vs last tag |
| `read_file` | Claude reads the VCell SoulScript for inspection |
| `set_sim_value` | Claude applies corrective parameter (e.g., reduce current) |

### Expected Behaviour

1. Claude (acting as Repairman) calls `tail_telemetry(count=20,
   key_filter="battery.temperature")` to see the temperature trend.
2. Claude calls `get_simulation_state` to see all current values.
3. Claude diagnoses the root cause — e.g., excessive charge current
   causing ohmic heating.
4. Claude calls `set_sim_value(key="battery.current", value=-1.5)`
   to reduce the charge rate.
5. The engine-side `drain_sim_commands` picks up the command and
   writes the value into `SimValuesResource` and the Rune thread-local.

### Verification

- Claude's response should reference specific telemetry data.
- `set_sim_value` should appear in the Workshop chat as an auto-executed
  tool card (no approval required for sim value writes).
- The runtime snapshot should reflect the updated current value.

---

## Phase 4: Verification

### MCP Tools Exercised

| Tool | Purpose |
|------|---------|
| `tail_telemetry` | Confirm temperature is stabilizing after intervention |
| `get_simulation_state` | Final state check |
| `stop_simulation` | End the run once stable |
| `query_audit_log` | Review all Claude calls in this session |

### Expected Behaviour

1. After the corrective `set_sim_value`, the Watchman continues
   monitoring. If temperature drops below threshold, no further
   alerts fire (30-second cooldown prevents oscillation storms).
2. Claude may proactively call `tail_telemetry` to confirm the trend
   is downward, then report success.
3. The simulation is stopped via `stop_simulation` (user or agent).
4. The recording is auto-exported to
   `.eustress/knowledge/recordings/{space}/sim_{timestamp}.json`.

### Verification

- Temperature trend in telemetry.jsonl should show a peak followed
  by a decline after the intervention.
- Total Watchman alerts should be ≤ 3 (max_alerts_per_run = 10).
- Recording JSON should contain the breakpoint event and the
  corrective parameter change.

---

## Phase 5: Git Feedback Loop

### MCP Tools Exercised

| Tool | Purpose |
|------|---------|
| `git_branch` | Create a `fix/thermal-runaway` branch |
| `write_file` | Update the SoulScript with improved thermal logic |
| `git_status` + `git_commit` | Stage and commit the fix |
| `feedback_diff` | Compare `fix/thermal-runaway` vs `main` |

### Expected Behaviour

1. If Claude determines the SoulScript needs a code change (not just
   a runtime parameter tweak), it creates a feature branch.
2. Claude edits the Rune script to add a thermal limiter:
   ```rune
   let temp = get_sim_value("battery.temperature_c");
   if temp > 55.0 {
       let reduced = get_sim_value("battery.current") * 0.5;
       set_sim_value("battery.current", reduced);
   }
   ```
3. Claude commits the change and uses `feedback_diff` to show the
   delta between the fix branch and `main`.
4. A second simulation run confirms the fix prevents thermal runaway.

### Verification

- `git_branch(action="list")` shows `fix/thermal-runaway`.
- `feedback_diff(from="main", to="fix/thermal-runaway")` shows the
  script change.
- Second simulation run completes without any Watchman alerts.

---

## Metrics Checklist

After completing the full cycle, the following metrics should be
observable:

| Metric | Source | Expected |
|--------|--------|----------|
| Telemetry entries | `telemetry.jsonl` line count | ≥ 30 (30+ seconds at 1 Hz) |
| Watchman alerts fired | `WatchmanState.alerts_fired` | 1–3 |
| Claude API calls | `query_audit_log(count=20)` | 3–8 (alert + diagnosis + verification) |
| Sim commands processed | Engine log `"MCP: ..."` lines | ≥ 2 (run + set_sim_value) |
| Recording exported | `.eustress/knowledge/recordings/` | 1 JSON file |
| LSP completions | Type `get_sim_value("bat` in SoulScript | Shows `battery.voltage`, etc. |
| Code actions | Hover over `set_sim_value(` line | Shows "Add watchpoint comment" |
| Git branch created | `git_branch(action="list")` | `fix/thermal-runaway` exists |
| Feedback diff | `feedback_diff(from="main", to="fix/...")` | Shows script change |

---

## Component Coverage

This case study exercises every system built in the expansion:

| Component | File | Status |
|-----------|------|--------|
| `tail_telemetry` MCP tool | `tools/src/simulation_tools.rs` | ✅ Phase 1, 3, 4 |
| `query_audit_log` MCP tool | `tools/src/simulation_tools.rs` | ✅ Phase 4 |
| `git_branch` MCP tool | `tools/src/git_tools.rs` | ✅ Phase 5 |
| `feedback_diff` MCP tool | `tools/src/git_tools.rs` | ✅ Phase 5 |
| `run_simulation` MCP tool | `tools/src/simulation_tools.rs` | ✅ Phase 1 |
| `stop_simulation` MCP tool | `tools/src/simulation_tools.rs` | ✅ Phase 4 |
| `get_simulation_state` MCP tool | `tools/src/simulation_tools.rs` | ✅ Phase 1, 3 |
| `drain_sim_commands` (engine) | `engine/src/simulation/plugin.rs` | ✅ Phase 1, 3 |
| `write_telemetry_log` (engine) | `engine/src/simulation/plugin.rs` | ✅ Phase 1–4 |
| ECS schema in snapshot | `engine/src/script_editor/runtime_snapshot.rs` | ✅ Phase 3 (LSP) |
| LSP ECS-aware completion | `engine/src/script_editor/lsp.rs` | ✅ Phase 5 (edit) |
| LSP agent code actions | `engine/src/script_editor/lsp.rs` | ✅ Phase 5 (edit) |
| Watchman monitor | `engine/src/workshop/watchman.rs` | ✅ Phase 2 |
| Watchman → Pipeline injection | `engine/src/workshop/watchman.rs` | ✅ Phase 2 |
| SiTL/HIL architecture doc | `docs/architecture/SITL_HIL_ARCHITECTURE.md` | ✅ Reference |
