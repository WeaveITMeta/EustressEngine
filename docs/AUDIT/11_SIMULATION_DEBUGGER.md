# 11 — Simulation & Debugger

> SimulationClock, watchpoints, breakpoints, V-Cell physics, Watchman alerts,
> SITL / HIL integration, deterministic step, replay, time-travel debugging,
> script debugger UI. The "authoritative reality" subsystem.

## Pass changelog

- **P2 (2026-05-14):** New system doc; 12 features, 8 cards expanded, 12 wiring gaps. Maturity ~70%.

---

## Concept summary

The **Simulation** subsystem accelerates years of product behaviour into seconds of wall time. Tick-based, time-compressed (1x → 10⁹x). Three tiers:

- **Kernel Laws** (Rust): pure functions implementing physics equations from peer-reviewed literature — Nernst, Butler-Volmer, thermodynamics, mechanics. Live in [common::realism::laws](../../eustress/crates/common/src/realism/).
- **ECS State** (Bevy): physical state as components — `ElectrochemicalState`, `ThermodynamicState`, etc.
- **User Logic** (Rune scripts): product-specific behaviour and test scenarios driving simulations via watchpoints and setpoints.

**SITL** (software-in-the-loop) runs entirely in software (ECS + Bevy). **HIL** (hardware-in-the-loop) bridges to physical test rigs via MCP tools — the engine orchestrates parameters, pulls measurements through the same watchpoint-recording pipeline. The **Feedback Gate** controls promotion: a prototype must pass all SiTL scenarios before HIL.

**Watchman** is an agent that continuously monitors live telemetry, injects synthetic alerts into the Workshop pipeline when watchpoints breach thresholds. Claude (Repairman) diagnoses via `tail_telemetry` + `feedback_diff`, proposes fixes via `set_sim_value` (runtime parameter injection) or script edits. Loop until all watchpoints hold.

**Debugger UI** is **absent** today: no Slint breakpoint panel, no step/continue buttons, no locals inspector. Replay infrastructure exists for write; seek is not wired.

Maturity ~70%. Core simulation engine solid; debugging and replay skeleton-level.

---

## Implementation snapshot

**Crates / files:**
- [common/src/simulation/clock.rs](../../eustress/crates/common/src/simulation/) — `SimulationClock` (1x–10⁹x, accumulator-based fixed timestep, `max_ticks_per_frame` cap)
- [common/src/simulation/watchpoint.rs](../../eustress/crates/common/src/simulation/) — `WatchPointRegistry` (history 10k entries, min/max/avg, per-record-interval)
- [common/src/simulation/breakpoint.rs](../../eustress/crates/common/src/simulation/) — `BreakPointRegistry` (operators ≤, ==, ≥, ≠; one-shot; cooldown)
- [engine/src/simulation/electrochemistry.rs](../../eustress/crates/engine/src/simulation/) — V-Cell battery tick system
- [engine/src/soul/rune_ecs_module.rs](../../eustress/crates/engine/src/soul/) — `get_sim_value`, `set_sim_value`, `sim.record()`, `sim.add_breakpoint()`
- [common/src/streaming/sim_stream.rs](../../eustress/crates/common/src/streaming/) — publishes simulation records (write-only; no playback)
- [engine/src/play_mode.rs](../../eustress/crates/engine/src/) — F5/F6/F7/F8 controls simulation clock + Avian
- [docs/architecture/SITL_HIL_ARCHITECTURE.md](../architecture/SITL_HIL_ARCHITECTURE.md), [docs/development/SIMULATION_SYSTEM.md](../development/SIMULATION_SYSTEM.md), [BASELINE_SIMULATION.md](../development/BASELINE_SIMULATION.md), [VCELL_CASE_STUDY.md](../architecture/VCELL_CASE_STUDY.md)

**Working:**
- ✅ SimulationClock (compression 1x–10⁹x)
- ✅ WatchPointRegistry + BreakPointRegistry
- ✅ Rune sim API (`get/set_sim_value`, `record`, `add_breakpoint`)
- ✅ Play/Pause/Stop hot keys
- ✅ TOML hot-reload of instance state (position, material, electrochemistry)
- ✅ V-Cell electrochemistry tick (Nernst, Butler-Volmer, thermal runaway)
- ✅ ECS ↔ Rune bidirectional bindings
- ✅ Watchman proactive alerts (5-sec poll, 30-sec cooldown, 10-alert cap)
- ✅ EustressStream publishing of simulation results

**Missing:**
- ⚠️ Avian deterministic step is single-run only; cross-platform untested
- 🟡 Replay / time-travel: recording works (write-only), seek/restore not wired
- 🔴 Script debugger UI (no Slint breakpoint, step, locals panel)
- 🔴 HIL adapter SDK (pattern documented, no shipped SDK)
- 🔴 Parameter sweep loop (single value via `set_sim_value`; no parallel branching)

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | SimulationClock (time compression) | ✅ |
| 2 | WatchPointRegistry | ✅ |
| 3 | BreakPointRegistry | ✅ |
| 4 | Watchman proactive alerts | ✅ |
| 5 | V-Cell electrochemistry tick | ✅ |
| 6 | ECS ↔ Rune bindings | ✅ |
| 7 | SITL MCP tools (`run_simulation`, `set_sim_value`, …) | ✅ |
| 8 | Physics determinism (cross-platform) | 🟡 |
| 9 | Replay / time-travel (seek) | 🟡 write-only |
| 10 | Script debugger UI | 🔴 |
| 11 | HIL adapter SDK | 🔴 |
| 12 | Parameter sweep loop | 🔴 |

---

## Detailed per-feature cards (top 8)

### Feature 1 — SimulationClock

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** all
**Sub-features:** 1x–10⁹x time scale · accumulator-based fixed timestep · `max_ticks_per_frame` cap (anti spiral) · pause / step / resume API

**Concept.** The wall clock ticks Bevy frames at ~60 Hz; the simulation clock decouples — at scale = 1, sim ticks match real-time; at scale = 10⁶, a year of sim time elapses per real-time minute. Accumulator pattern prevents drift; `max_ticks_per_frame` (typically 4) prevents the "spiral of death".

**Forecasted feedback (R)**
- R1.1 Cross-platform determinism in accumulator arithmetic untested (x86-64 vs. ARM rounding).
- R1.2 Wall-time acceleration for long-running sims (10k cycle battery test at 60 Hz = 166s) is capped at frame rate; need uncapped headless mode.
- R1.3 No way to "tick forever as fast as the CPU allows".

**Implications (I)**
- *Cross-system:* [11] is the rhythm section for [07_AI_PLATFORM] Watchman and [10_TELEMETRY] sim events.
- *Strategic:* "year-in-a-minute" is the marketing pillar for engineering customers (battery, drug discovery, etc.).

**Risks (X)** — X1.1 Cross-platform divergence ruins HIL handoff.

**Mitigations (M)** — M1.1 Add a determinism regression suite (replay same scenario on 3 platforms; CRC tick-by-tick).

---

### Feature 2 — WatchPointRegistry

**State:** ✅ · **Effort:** Done · **Risk:** Med (memory) · **Touches:** [02], [07], [10], [11]
**Sub-features:** per-watchpoint history (10k entries) · min/max/avg stats · per-record interval · per-watchpoint colour for Timeline · in-memory ring buffer

**Concept.** Each watchpoint observes a named scalar (`battery.voltage`, `motor.rpm`, `inventory.coin`) over time. Stored in a 10k-entry VecDeque; Timeline panel plots; Workshop AI tails to detect anomalies.

**Forecasted feedback (R)**
- R2.1 In-memory only — lost on restart (telemetry.jsonl mitigates but seek not wired).
- R2.2 Large-scale (1M watchpoints across 10k scenarios) OOM risk untested.
- R2.3 Type safety: string-keyed values lose schema.
- R2.4 No serialization for parameter-sweep snapshot.

**Implications (I)**
- *Cross-system:* Timeline panel + Workshop context inject + telemetry stream all read from this.
- *Architectural:* the registry is the universal observe-protocol; new physics domains must register.

**Risks (X)** — X2.1 OOM at scale; X2.2 type-confusion bugs.

**Mitigations (M)** — M2.1 Chunked storage (mmap-backed ring); M2.2 typed watchpoint via generic.

---

### Feature 3 — BreakPointRegistry

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [02], [11]
**Sub-features:** operators ≤, <, ==, ≥, >, ≠ · one-shot vs. recurring · cooldown · debounce per-key

**Concept.** Pause the simulation when a watchpoint crosses a threshold (`battery.voltage < 3.0` → pause). One-shot mode auto-clears after firing.

**Forecasted feedback (R)** — R3.1 Cooldown is wall-time not sim-time; at 10⁶x scale, miss short spikes or alert-storm.

**Implications (I)** — *Cross-system:* the debugger UI (Feature 10) will surface these.

**Mitigations (M)** — M3.1 Add `cooldown_simulation_ticks` parallel to `cooldown_ticks`.

---

### Feature 4 — Watchman proactive alerts

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [07], [11]
**Sub-features:** 5-sec poll · 30-sec cooldown per-key · 10-alert run-cap · synthetic Workshop message injection · IdeationPipeline integration

**Concept.** A background agent watches the watchpoint registry; when anything breaches a configured threshold, it injects a synthetic user message into the Workshop conversation so Claude (Repairman) diagnoses + proposes fixes.

**Forecasted feedback (R)**
- R4.1 Thresholds hardcoded in `WatchmanConfig::default()`; no UI for runtime change.
- R4.2 No MCP `set_watchman_threshold`.

**Implications (I)** — *Strategic:* the human-in-the-loop bridge for AI-assisted engineering; the recursive feedback loop foundation.

**Mitigations (M)** — M4.1 Studio panel for watchman thresholds; MCP tool to set them remotely.

---

### Feature 8 — Physics determinism

**State:** 🟡 single-run OK; cross-platform unknown · **Effort:** XL · **Risk:** Critical (HIL handoff) · **Touches:** [01], [03], [05], [11]
**Sub-features:** Avian deterministic step · per-tick CRC across platforms · float-mode lock (no fast-math) · serialised RigidBody / Collider state · replay regression tests

**Concept.** Avian's physics step is deterministic per-run on the same hardware. Cross-platform (macOS / Windows / Linux) determinism is unproven. Critical for HIL handoff, replay, and multiplayer prediction/reconciliation.

**Forecasted feedback (R)**
- R8.1 No determinism test suite.
- R8.2 ARM vs. x86-64 floating-point rounding diverges over thousands of ticks.
- R8.3 No RigidBody state serialisation → replay can't restore mid-run.

**Implications (I)**
- *Cross-system:* [03_MULTIPLAYER] client prediction + reconciliation depend on this.
- *Architectural:* may need to enforce SIMD-free math paths for portability.
- *Strategic:* SITL → HIL claim is unverifiable without it.

**Risks (X)** — X8.1 Customer sees results differ across platforms → trust loss.

**Mitigations (M)**
- M8.1 Add a `--deterministic` flag that disables fast-math + lock SIMD path.
- M8.2 Per-tick state CRC; CI compares across platforms; fail on drift.

---

### Feature 9 — Replay / time-travel (seek)

**State:** 🟡 write-only · **Effort:** L · **Risk:** Med · **Touches:** [02], [11]
**Sub-features:** tick-level state snapshots · disk-backed ring · seek-to-tick · differential snapshots · replay playback UI

**Concept.** Capture full simulation state at every tick (or every N ticks). Seek to any tick → restore state → resume. Time-travel debugger UX: scroll the timeline, see the world rewind.

**Forecasted feedback (R)**
- R9.1 telemetry.jsonl writes events but no read/seek API.
- R9.2 Full state snapshot per tick at 120 Hz = lots of disk; differential snapshots needed.
- R9.3 Avian state serialization is the prerequisite (Feature 8).
- R9.4 Replay 500k ticks linearly = slow; differential snapshots + checkpointing.

**Implications (I)** — *Strategic:* time-travel debugging is the killer feature for engineering customers.

**Risks (X)** — X9.1 Snapshot interval drives storage; misconfigured = unusable.

**Mitigations (M)** — M9.1 Differential snapshots (one full every N, diffs between); M9.2 configurable per-project.

---

### Feature 10 — Script debugger UI

**State:** 🔴 · **Effort:** L (3–4 weeks) · **Risk:** Med · **Touches:** [02], [11]
**Sub-features:** Slint breakpoint panel · step / continue buttons · locals inspector · watch expressions · call-stack view

**Concept.** A panel inside the Studio (or external in LSP) that hooks Rune / Luau VM execution: set breakpoints, step line-by-line, inspect local variables, evaluate watch expressions. Standard debugger UX.

**Forecasted feedback (R)**
- R10.1 No code today.
- R10.2 Rune VM debugger hooks need to be exposed (mlua has them; Rune partial).
- R10.3 Slint panel + drain-pattern integration.
- R10.4 Source-line mapping requires `.rune` source map.

**Implications (I)** — *Strategic:* serious scripting requires serious debugging.

**Risks (X)** — X10.1 No debugger = scripters abandon the platform for ones with one.

**Mitigations (M)** — M10.1 Phase 1: print + breakpoint only. Phase 2: step + locals. Phase 3: watch expressions.

---

### Feature 11 — HIL adapter SDK

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [07], [11]
**Sub-features:** Python + Rust SDK · MCP-compatible interface · example bridges (LabVIEW, Arduino, SCPI / PyVISA) · authentication · safety interlocks

**Concept.** A customer with a physical test rig (battery cycler, motor dyno) builds a small bridge that exposes the rig as MCP tools. Engine calls `hil.set_voltage(3.7)`; bridge talks to the SCPI instrument; engine reads `hil.measured_current()`. Same API surface as SITL — only the data source differs.

**Forecasted feedback (R)**
- R11.1 Pattern designed but no shipped SDK.
- R11.2 Safety-critical: a buggy script must never drive the rig out of bounds.
- R11.3 Lab-network latency (LAN ~5 ms; serial ~50 ms) — tag the data accordingly.

**Implications (I)** — *Strategic:* the differentiating bet — game engines don't bridge to physical rigs.

**Risks (X)** — X11.1 Safety failure → expensive hardware damaged or human injured.

**Mitigations (M)** — M11.1 Interlock layer between sim setpoint and rig — must respect hardware safe-envelope limits.

---

## Wiring / import gaps (top 12)

1. Slint debugger panel (breakpoint, step, locals)
2. Tick-level state snapshots (with differential support)
3. Replay seek API + MCP `seek_simulation_to_tick`
4. Time-travel frame-stepping (Slint step button)
5. Cross-platform determinism test suite
6. Avian RigidBody / Collider state serialisation
7. Auto-record-on-play flag
8. Parameter-sweep MCP tool (`run_parameter_sweep`)
9. Simulation validation report (pass/fail per watchpoint)
10. HIL adapter SDK (Python + Rust) with example bridges
11. Determinism regression tests in CI
12. Wire sim_stream → engine SimulationPlugin (on Stop publish results to `simulation.results`)

---

## Cross-system dependencies

- **[02_STUDIO]** debugger UI + watchpoint/breakpoint panels.
- **[03_MULTIPLAYER]** deterministic step is shared (prediction/reconciliation).
- **[07_AI_PLATFORM]** Watchman alerts, Repairman LLM, physics-grounded prompts.
- **[10_TELEMETRY]** sim-stream tee + watchpoint topic.
- **[12_INFRASTRUCTURE]** CI determinism regression tests.

---

## Open questions

- Q11.1 Cross-platform determinism: tick-by-tick CRC or statistical equivalence?
- Q11.2 HIL SDK ship: SDK + examples, or docs + customer-builds?
- Q11.3 Replay performance at 1M ticks — differential snapshots strategy.
- Q11.4 Watchpoint scaling — chunked ring or stream-to-disk?
- Q11.5 Breakpoint semantics on script edit while paused — replay or reject?
- Q11.6 Reproducibility-score metric for customer-facing HIL → trust signal.
