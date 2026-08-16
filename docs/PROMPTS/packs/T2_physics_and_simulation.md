# T2 — Physics, Simulation Fidelity & Performance

**Status:** Authored pack. Every item below is `DRAFT` until an L1 promotes it to `READY`.
**Owns:** Gauntlet phase **G2 (Determinism & Numerical Trust)** items `G2.01`–`G2.18`, and the
physics half of phase **G5 (Scale & Streaming)**, items `G5.20`–`G5.23`. No other pack may claim
those IDs.
**Feeds:** primarily **W1 (Provable Quality)** and **W3 (Trust & Verifiability)**; `G5.22` also
feeds **W6 (Operator Leverage)**.

**Item Zero: `G2.01`.** Nothing else in this pack may start until `G2.01` is `PASSED`. Every other
item's exit criterion is a comparison, and there is currently nothing to compare against: no
physics baseline artifact exists in this repository, and the one determinism test that does exist
is behind a non-default cargo feature and has never been run in CI.

**Deployment note.** Each fenced block below is a complete prompt file. Write it verbatim to the
path in its heading before handing it to an executing agent. One file per item; never two items in
one file.

---

## Dependency graph

| ID | Title | Phase | Tier | `depends_on` |
|---|---|---|---|---|
| **G2.01** | **Simulation evidence harness and physics baseline ledger** | G2 | L | — (**ITEM ZERO**) |
| G2.02 | Determinism gate: byte-identical Avian reruns, runnable by command | G2 | M | G2.01 |
| G2.03 | Determinism through the full headless engine stack | G2 | L | G2.02 |
| G2.04 | Time-compression fidelity instrumentation | G2 | M | G2.01, G1.12 |
| G2.05 | Bounded-error integration under time compression | G2 | L | G2.04, G2.03, G1.12 |
| G2.06 | Sim-time alerting for Watchman and breakpoints | G2 | M | G2.04, G1.12 |
| G2.07 | Conservation invariants as a runnable suite | G2 | M | G2.01, G1.12 |
| G2.08 | Chemistry-agnostic 0-D cell model validated against a public dataset | G2 | L | G2.01, G1.12 |
| G2.09 | Single-particle model: solid-phase diffusion and diffusion overpotential | G2 | L | G2.08, G1.12 |
| G2.10 | P2D electrolyte transport (Doyle–Fuller–Newman) | G2 | XL | G2.09, G1.12 |
| G2.11 | Thermal coupling: temperature-dependent transport, validated on a sweep | G2 | L | G2.10, G1.12 |
| G2.12 | P2D numerical convergence and charge conservation | G2 | M | G2.10 |
| G2.13 | P2D under time compression: performance envelope with error bound held | G2 | L | G2.11, G2.12, G2.05, G1.12 |
| G2.14 | Fracture to Avian: crack criterion drives a real rigid-body split | G2 | L | G2.07, G1.12 |
| G2.15 | Beam FEA validated against the closed-form Euler–Bernoulli solution | G2 | L | G2.01, G1.12 |
| G2.16 | Reactor control loop: measured step response with overshoot and settling bounds | G2 | M | G2.03, G1.12 |
| G2.17 | Transient conduction validated against the analytic semi-infinite slab | G2 | M | G2.01 |
| G2.18 | Kernel-law declaration surface: Rune-declared law equals native kernel | G2 | M | G2.01 |
| G5.20 | Avian throughput curve: measured ms/step versus body count | G5 | M | G2.01 |
| G5.21 | Frame-time distribution under physics load | G5 | L | G5.20 |
| G5.22 | Multi-variant experiment throughput with a noise floor that makes ranking meaningful | G5 | L | G2.03, G2.08, G1.12 |
| G5.23 | Physics regression gate binary that can actually fail | G5 | M | G2.02, G2.07, G2.12, G2.17 |

Reading order is the table order. `G2.08`–`G2.13` are the V-Cell electrochemistry ladder and must
be executed in sequence; each one's error bound is tighter than its predecessor's and is only
meaningful because the predecessor's number was measured on the same rig.

---

## Standing facts every item in this pack restates

These are repeated inside every prompt body because an executing agent reads exactly one file.

- Eustress is an AI-native simulation substrate / world engine. It is never called a game engine.
- The licence is PolyForm Shield 1.0.0. Say source-available.
- The physics engine is **Avian** (`avian3d`). Never Rapier.
- Units are meter-native. Studs are a display unit only.
- A full engine build takes 10–15 minutes; one cargo build at a time; never kill a build mid-compile.
- Validate with `cargo run`, not `cargo check`.
- Every number is labelled `MEASURED` (with the command and artifact that produced it), `TARGET`,
  or `CONFIG DEFAULT`.

---

## Cross-pack file ownership

Items in this pack share source files with items in other packs. `docs/PROMPTS/04_FILE_OWNERSHIP.md` names one owner per contested path and is normative; where it conflicts with an item's scope list, it wins. This pack's own dependency-graph table records its internal edges together with the program-gate edges of `02_QUEUE.md` §8.2 and §8.4; the cross-pack edges arising from contested paths are these:

| Item | Now depends on | Contested path | Effect on this item's scope |
|---|---|---|---|
| `G2.01` | `G1.03` (G1) | `eustress/crates/engine/Cargo.toml` | may append to but not alter it |
| `G2.03` | `G1.07` (G1) | `eustress/crates/common/src/simulation/recorder.rs` | may no longer edit it |
| `G2.03` | `G7.03` (T4) | `eustress/crates/engine/src/bin/headless.rs` | may no longer edit it |
| `G2.04` | `G1.07` (G1) | `eustress/crates/common/src/simulation/clock.rs` | may no longer edit it |
| `G2.04` | `G1.07` (G1) | `eustress/crates/engine/src/simulation/plugin.rs` | may no longer edit it |
| `G2.05` | `G1.07` (G1) | `eustress/crates/common/src/simulation/clock.rs` | may no longer edit it |
| `G2.05` | `G1.07` (G1) | `eustress/crates/engine/src/simulation/plugin.rs` | may no longer edit it |
| `G2.14` | `G1.07` (G1) | `eustress/crates/engine/src/simulation/plugin.rs` | may no longer edit it |
| `G5.21` | `G1.08` (G1) | `eustress/crates/engine/src/frame_diagnostics.rs` | may no longer edit it |
| `G5.21` | `G1.08` (G1) | `eustress/crates/engine/src/profiler.rs` | may no longer edit it |
| `G5.22` | `G7.08` (T4) | `eustress/crates/tools/src/simulation_tools.rs` | may no longer edit it |
| `G5.23` | `G1.03` (G1) | `eustress/crates/engine/Cargo.toml` | may append to but not alter it |

An item blocked by one of these entries emits a `FILE-OWNERSHIP` decision packet to L0 (`docs/PROMPTS/04_FILE_OWNERSHIP.md` §6). It does not edit the file and does not work around it.

---


---

## docs/PROMPTS/items/G2.01_sim-evidence-harness-and-physics-baseline.md

````markdown
---
id: G2.01
title: Simulation evidence harness and physics baseline ledger
workload: W3
workload_secondary: [W1]
phase: G2
depends_on: [G1.03]
blocks: [G2.02, G2.04, G2.07, G2.08, G2.15, G2.17, G2.18, G5.20]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G2.01/physics_baseline.json
escalation: >
  If two consecutive runs of the same scenario on the same machine produce different state hashes,
  STALL immediately and report the divergence rather than weakening the hash. A non-reproducible
  baseline makes every downstream item in this pack unmeasurable.
status: DRAFT
notes: >
  ITEM ZERO for pack T2. Tier L rather than M because it adds a new binary to the workspace and the
  first build of `eustress-common --features physics` is a cold compile.
---

## 1. Objective

A single command produces a content-addressed **simulation evidence bundle**: a manifest recording
commit, host, seed, scenario id, tick count, and the SHA-256 of every emitted artifact, plus the
recording data itself. Running that command twice on the same machine at the same commit produces
byte-identical artifact hashes. The bundle for the four seed scenarios is committed as
`docs/PROMPTS/artifacts/G2.01/physics_baseline.json`, and it is the only number source the rest of
pack T2 is permitted to compare against.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — a world model an AI reasons
over and a document a human edits. It is never described as a game engine. The physics engine is
**Avian** (`avian3d`); never Rapier. Units are meter-native. The licence is PolyForm Shield 1.0.0
(source-available).

**Why this item exists.** There is no physics baseline artifact in this repository. Downstream items
in this pack say things like "RMS error must be under 60 mV" and "p99 frame time must be under 2x
p50" — none of those are checkable without a rig that produces the same number twice. You are
building that rig.

**What already exists and must be reused, not rebuilt.**

- `eustress/crates/common/src/simulation/recorder.rs` (266 lines) — `SimulationRecording`,
  `RecordingMetadata`, `TimeSeries`, `SimulationEvent`, and `export_json`. This is the recording
  format. Do not invent a second one.
- `eustress/crates/common/src/simulation/clock.rs` (193 lines) — `SimulationClock` with
  `simulation_time_s`, `wall_time_s`, `time_scale`, `fixed_timestep_s`, `accumulator_s`,
  `tick_count`, `tick_rate_hz`, `max_ticks_per_frame` (default 10, `CONFIG DEFAULT`).
- `eustress/crates/common/src/simulation/watchpoint.rs` (211 lines) — `WatchPoint` with `history`,
  `record(value, time_s, tick)`, `record_interval`.
- `eustress/crates/common/tests/determinism.rs` (117 lines) — builds a MinimalPlugins + Avian world
  with the engine's determinism pins (`Time::<Fixed>::from_hz(60.0)`, `SubstepCount(6)`,
  `SolverConfig::default()`, `Gravity(Vec3::NEG_Y * 9.80665)`), spawns a static floor and 16 dynamic
  cubes seeded from `GlobalRngSeed`, steps `FixedUpdate`, and hashes quantized transforms. **It is
  gated by `#![cfg(feature = "physics")]` and `physics` is NOT in
  `eustress/crates/common/Cargo.toml` `default = ["model-import", "geotiff", "streaming", "units_v1"]`
  — so a plain `cargo test -p eustress-common` compiles it to nothing.** Reuse its `build_world` and
  `hash_state` shape.
- `eustress/benches/instance-capacity/src/bin/avian_physics_bench.rs` — an existing standalone
  Avian baseline: 600 steps at 60 Hz, reports `overall_ms`, `early_ms` (first 100 steps),
  `steady_ms` (last 100 steps) and `awake_at_end` for the `falling` and `static_heavy` scenarios.
  Run with `cargo run --release --bin avian-physics-bench` from `eustress/benches/instance-capacity`.
- `eustress/crates/engine/src/bin/headless.rs` (291 lines) — `eustress-headless --space <dir>
  --ticks N [--tick-rate HZ] [--no-autoplay] [--autoplay-delay-frames N]`. With `--ticks N` it
  enters Play, runs exactly N sim ticks at a 60 Hz fixed step, stops (which fires the recording
  export in `eustress/crates/engine/src/simulation/plugin.rs` `on_play_stop`, line 128), and exits 0.
  It is `MinimalPlugins` + `ScheduleRunnerPlugin` — no GPU, no window.
- Recording export destination, from `plugin.rs` lines 154–177: `<universe>/.eustress/knowledge/
  recordings/<space_name>/sim_<timestamp>.json`, falling back to
  `<space_root>/.eustress/recordings/`.
- `eustress/crates/engine/src/space/mod.rs` line 119 `workspace_root()` — the **`EUSTRESS_WORKSPACE`
  environment variable overrides the workspace root and wins over the platform default.** This is the
  hook that makes runs reproducible from a scratch directory. Use it.

**What does not exist.** There are no Spaces committed to this repository — `find . -name
world.fjalldb` returns nothing. Your harness must scaffold its scenario Spaces from code under
`EUSTRESS_WORKSPACE`, not assume one on disk.

**Build reality.** A full engine build takes 10–15 minutes. Only one cargo build may run at a time;
the workspace shares a single `target/` and concurrent builds produce link failures. Never kill a
build mid-compile. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/common/src/simulation/` — additive only (a `evidence.rs` module is permitted)
- `eustress/benches/instance-capacity/src/bin/` — new bench binaries permitted
- `eustress/crates/engine/src/bin/` — a new `sim-evidence` binary is permitted
- `eustress/crates/engine/Cargo.toml`, `eustress/crates/common/Cargo.toml` — `[[bin]]` and feature
  entries only; a new third-party dependency requires a one-line justification in the result block
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json` — create it
- `docs/PROMPTS/artifacts/G2.01/` — create it

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/physics/` — determinism pins are `G2.02`'s subject, not yours
- `eustress/crates/engine/src/ui/` and `eustress/crates/engine/ui/` — this item is headless
- Any existing entry in `eustress/crates/engine/Cargo.toml` — the file is owned by `G1.03`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Widening a hash quantization, dropping a scenario, shortening the tick count, or excluding a field
  from the manifest are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The manifest must record what it cannot control: host OS, CPU model string, rustc version, commit
  SHA, and whether the working tree was dirty. A baseline taken from a dirty tree is still valid
  evidence provided the manifest says so.
- Do not hash floating-point state directly. Quantize to a fixed number of ulps or a fixed decimal
  and record the quantization in the manifest, exactly as `common/tests/determinism.rs` `hash_state`
  already does.
- The four seed scenarios are fixed: `avian_falling` (dynamic bodies under gravity),
  `avian_static_heavy` (many static colliders), `vcell_discharge_0p5c` (one
  `ElectrochemicalState` entity at 0.5C for 7200 sim-seconds), and `pendulum_conservation`
  (frictionless single pendulum, 3600 ticks). Adding a fifth is permitted; removing one is not.
- Batch verification. Twelve builds is the whole budget.

## 5. Exit criterion

### Criterion
Two consecutive invocations of the evidence command, on the same machine at the same commit,
produce **identical SHA-256 values for all four seed scenarios**, and the emitted
`physics_baseline.json` contains a numeric value for every one of the ten required baseline fields.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin sim-evidence
    ./target/release/sim-evidence --recipe ../docs/PROMPTS/harness/recipes/G2_sim_evidence.json \
        --out ../docs/PROMPTS/artifacts/G2.01/run_a.json
    echo "EXIT_A=$?"
    ./target/release/sim-evidence --recipe ../docs/PROMPTS/harness/recipes/G2_sim_evidence.json \
        --out ../docs/PROMPTS/artifacts/G2.01/run_b.json
    echo "EXIT_B=$?"
    ./target/release/sim-evidence --compare ../docs/PROMPTS/artifacts/G2.01/run_a.json \
        --against ../docs/PROMPTS/artifacts/G2.01/run_b.json \
        --emit ../docs/PROMPTS/artifacts/G2.01/physics_baseline.json
    echo "EXIT_CMP=$?"

Expected output shape (`physics_baseline.json`):

    {
      "commit": "71ccf6fe...", "tree_dirty": true,
      "host": {"os": "windows", "cpu": "<model string>", "rustc": "1.xx.x"},
      "quantization": {"position_decimals": 5, "rotation_decimals": 5},
      "reproducible": true,
      "scenarios": {
        "avian_falling":        {"hash": "sha256:...", "steps": 600,  "overall_ms": 0.00, "early_ms": 0.00, "steady_ms": 0.00},
        "avian_static_heavy":   {"hash": "sha256:...", "steps": 600,  "overall_ms": 0.00, "early_ms": 0.00, "steady_ms": 0.00},
        "vcell_discharge_0p5c": {"hash": "sha256:...", "ticks": 432000, "final_soc": 0.00, "final_terminal_v": 0.00, "wall_s": 0.0},
        "pendulum_conservation":{"hash": "sha256:...", "ticks": 3600, "energy_drift_frac": 0.0}
      }
    }

Pass condition:

    EXIT_A == 0 AND EXIT_B == 0 AND EXIT_CMP == 0
    AND physics_baseline.json ."reproducible" == true
    AND every scenario hash in run_a.json equals the same scenario hash in run_b.json
    AND none of overall_ms / early_ms / steady_ms / final_soc / final_terminal_v /
        energy_drift_frac is null

Read the emitted `reproducible` field and the hashes. Do not infer success from the files existing.

## 6. Critic gate

`critic_gate: []`. This item is mechanical: either two runs hash identically and the ten fields are
populated, or they do not. There is nothing here for a blinded evaluator to judge. The mechanical
criterion in §5 is unusually tight precisely because no Critic backstops it — the hash equality
check has no tolerance and the field-population check has no default.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — extend the existing avian_physics_bench driver into sim-evidence
   -> if still failing, MANDATORY approach change. Changing the quantization constant is NOT an
      approach change; moving from an in-process driver to an eustress-headless subprocess driver is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the number of scenarios hashing identically
                  across two runs does not increase
  - Budget      : 750k tokens or 12 builds consumed (150% of the L envelope)
  - Item-specific: same-machine, same-commit hash divergence (see front matter)
```

The stall packet must request exactly one of: LOWER (drop a named scenario from the seed set, with
the downstream items that lose their baseline named); FUND approach D; DEFER behind a named item;
KILL with a statement of what pack T2 loses.

## 8. Artifact

`docs/PROMPTS/artifacts/G2.01/physics_baseline.json`

A reader finds: the commit and dirty flag, host OS / CPU / rustc, the quantization used for hashing,
a `reproducible` boolean, and for each of the four seed scenarios its state hash, its step or tick
count, and its measured performance and physical outputs. Alongside it: `run_a.json`, `run_b.json`,
and the recipe `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`.

Everything else in pack T2 cites this file. A number that does not appear here, or that cannot be
regenerated by the command in §5, is not admissible evidence anywhere in this pack.

## 9. Definition of NOT done

- The command runs and writes JSON, but the two runs disagree on one scenario's hash and the report
  calls it "expected float noise". That is the exact failure this item exists to detect.
- Hashes match because the hash covers only tick counts and not physical state. Grep the hash
  function: if it does not read `Transform` (or `ElectrochemicalState`) values, it is a counter, not
  a state hash.
- The V-Cell scenario is run for 7200 sim-seconds at `time_scale = 1.0` in wall time, taking two
  hours. The scenario must specify its `time_scale` and its wall time must be recorded — that
  recorded number is what `G2.13` later has to beat.
- Scenario Spaces are hand-created on the author's machine rather than scaffolded under
  `EUSTRESS_WORKSPACE`, so a stranger cannot reproduce the run.
- `physics_baseline.json` reports `overall_ms` but omits `early_ms` and `steady_ms`, collapsing the
  transient and settled regimes that `avian_physics_bench` already separates. `G5.20` needs both.
- The manifest omits `tree_dirty`, so nobody can tell whether the baseline came from committed code.
````

---

## docs/PROMPTS/items/G2.02_determinism-gate-byte-identical-avian-reruns.md

````markdown
---
id: G2.02
title: Determinism gate — byte-identical Avian reruns, runnable by command
workload: W3
workload_secondary: []
phase: G2
depends_on: [G2.01]
blocks: [G2.03, G5.23, G1.11, G2.30, G7.14]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G2.02/determinism_gate.json
escalation: >
  If cross-process reruns diverge while in-process reruns match, STALL. That signature means the
  divergence source is outside Avian (allocator address ordering, HashMap iteration, thread count)
  and fixing it is a separate, larger item that must be funded explicitly.
status: DRAFT
notes: >
  Tier M: single crate, one measurement, no engine build required — the test lives in
  eustress-common and compiles far faster than eustress-engine.
---

## 1. Objective

`cargo test -p eustress-common --features physics --test determinism` exits 0 and asserts three
distinct properties that are currently untested: same-process rerun equality, cross-process rerun
equality, and equality against a checked-in golden hash. The golden hash is committed, so a future
change to the physics pins or the Avian version fails the test rather than passing silently.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine. The
physics engine is **Avian** (`avian3d`); never Rapier. Units are meter-native. Licence is PolyForm
Shield 1.0.0 (source-available).

**The honest current state.** `docs/AUDIT/11_SIMULATION_DEBUGGER.md` Feature 8 records: "Avian
deterministic step is single-run only; cross-platform untested," and risk R8.1: "No determinism test
suite." That audit line is one pass stale in one respect — a test file does exist — but its
substance holds, because the test never runs.

- `eustress/crates/common/tests/determinism.rs` (117 lines) exists. It builds a `MinimalPlugins` +
  `PhysicsPlugins::default()` world with the same pins the engine binary applies
  (`Time::<Fixed>::from_hz(60.0)`, `SubstepCount(6)`,
  `avian3d::dynamics::solver::SolverConfig::default()`, `Gravity(Vec3::NEG_Y * 9.80665)`), inserts
  `GlobalRngSeed`, spawns a static floor and 16 dynamic cuboids at seeded positions, calls
  `app.finish()` and `app.cleanup()`, steps `FixedUpdate`, and hashes quantized dynamic-body
  transforms into a `u64`.
- Its very first line is `#![cfg(feature = "physics")]`.
- `eustress/crates/common/Cargo.toml` line 123: `default = ["model-import", "geotiff", "streaming",
  "units_v1"]`; line 125: `physics = ["avian3d"]`. **`physics` is not a default feature.** Therefore
  `cargo test -p eustress-common` compiles this file to an empty crate and reports success having
  asserted nothing.
- `.github/workflows/ci.yml` runs three jobs — `cargo deny check`, naga WGSL validation, and a
  `cargo tree` dependency-presence grep. There is no `cargo test` anywhere in
  `.github/workflows/`. Approximately 2,061 `#[test]` functions exist across `eustress/crates/` and
  none of them run in CI. You are **not** permitted to change that here; CI is out of scope. Your job
  is to make the command exist and pass, so a later item can wire it.
- `eustress/crates/common/src/physics/determinism.rs` (56 lines) is the whole determinism module:
  a `GlobalRngSeed(0x5EED_E057_1234_ABCD)` resource, a `rng(stream)` helper, `sim_rng`, and
  `DeterminismPlugin`. Its own doc comment states Avian uses no RNG and the step is deterministic
  once the timestep, `SubstepCount`, and `SolverConfig` are pinned. That claim has never been
  verified. Verifying it is this item.

**Prerequisite already satisfied.** `G2.01` produced
`docs/PROMPTS/artifacts/G2.01/physics_baseline.json`, containing the host CPU / OS / rustc and the
`avian_falling` state hash. Your golden hash must be recorded with the same host block, because a
golden that does not say what machine produced it is not evidence.

**Build reality.** One cargo build at a time; never kill a build mid-compile. `eustress-common` with
`--features physics` is a cold compile of `avian3d` the first time — budget one full build for it.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/common/tests/determinism.rs`
- `eustress/crates/common/tests/` — new test files permitted
- `eustress/crates/common/src/physics/determinism.rs` — additive only
- `eustress/crates/common/Cargo.toml` — `[[test]]` / `dev-dependencies` only. **Do not add
  `physics` to `default`**; that would change what every other crate in the workspace compiles.
- `docs/PROMPTS/artifacts/G2.02/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/physics_baseline.json` — frozen; read it, never write it
- `eustress/crates/engine/` — the full-stack determinism question is `G2.03`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Coarsening the quantization from 5 decimals, reducing the body count below 16, shortening the step
  count, or replacing hash equality with an approximate comparison are all measurement changes. If
  the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Do not make the test pass by removing the assertion or by marking it `#[ignore]`.
- The cross-process check must genuinely be cross-process: spawn the test binary again (or a small
  helper bin) with `std::process::Command` and compare stdout hashes. Calling `build_world` twice in
  one process is the same-process check and does not substitute.
- Increase the step count to **1000 fixed steps** so the solver has left the transient. Record the
  step count in the artifact.
- If Avian is genuinely non-deterministic cross-process on this host, say so with the divergence
  magnitude and which body diverged first. That is a valid, valuable outcome — but it is a STALL,
  not a pass.

## 5. Exit criterion

### Criterion
The determinism test binary asserts and passes all three properties (same-process, cross-process,
golden) at **1000 fixed steps** with **zero** hash mismatches, and `determinism_gate.json` records
the golden hash together with the host block it was produced on.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo test -p eustress-common --features physics --test determinism -- --nocapture --test-threads=1
    echo "EXIT_TEST=$?"
    cargo test -p eustress-common --features physics --test determinism -- --nocapture --test-threads=1
    echo "EXIT_TEST_2=$?"
    cargo test -p eustress-common --features physics --test determinism -- --nocapture --test-threads=1
    echo "EXIT_TEST_3=$?"

Expected output shape (stdout of each invocation):

    determinism: steps=1000 bodies=16 seed=0x5EEDE0571234ABCD
    determinism: same_process   hash_a=0x… hash_b=0x… match=true
    determinism: cross_process  hash_a=0x… hash_b=0x… match=true
    determinism: golden         expected=0x… actual=0x… match=true
    test result: ok. 3 passed; 0 failed

Pass condition:

    EXIT_TEST == 0 AND EXIT_TEST_2 == 0 AND EXIT_TEST_3 == 0
    AND all three `match=true` lines appear in all three invocations
    AND docs/PROMPTS/artifacts/G2.02/determinism_gate.json ."golden_hash" equals the `expected`
        value printed above
    AND docs/PROMPTS/artifacts/G2.02/determinism_gate.json ."steps" == 1000

Grep the three exit-code markers. A test that prints "ok" while having compiled to an empty crate
also exits 0 — that is why the three `match=true` lines are part of the pass condition.

## 6. Critic gate

`critic_gate: []`. Hash equality is binary; a blinded evaluator adds nothing. The replacement is the
triple-invocation requirement in §5: a flaky determinism gate that passes one run in three is a
failure, and running the command three times is what catches it.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — extend the existing tests/determinism.rs in place
   -> if still failing, MANDATORY approach change. Adjusting the quantization is NOT an approach
      change; moving the scenario construction into a helper bin driven by std::process::Command is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same failing property (same/cross/golden)
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: cross-process diverges while same-process matches (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.02/determinism_gate.json`

A reader finds: the golden hash, the step count (1000), body count (16), the seed, the quantization
decimals, the host block copied from `G2.01`'s baseline, the `avian3d` version resolved from
`Cargo.lock`, and a `properties` object with `same_process`, `cross_process`, and `golden` each
true or false. This is the W3 evidence: a stranger with this repo can run one command and get the
same hash.

## 9. Definition of NOT done

- The test passes but was invoked without `--features physics`, so `#![cfg(feature = "physics")]`
  stripped the file. The command in §5 is literal for this reason; any other invocation is
  inadmissible.
- The golden hash is written by the test itself on first run, so it can never fail. The golden must
  be a checked-in constant or a checked-in file the test reads, not something it regenerates.
- Cross-process determinism is asserted by calling `build_world()` twice inside one `#[test]`.
- The step count stays at whatever the original file used and the artifact does not say what it was.
- `physics` gets added to `eustress-common`'s `default` feature list to make a shorter command work.
  That changes the compile surface of the whole workspace and is explicitly out of scope.
- The test passes on one of three invocations and the result block calls the other two "flaky
  runner". Flakiness in a determinism gate is the defect, not noise around it.
````

---

## docs/PROMPTS/items/G2.03_determinism-through-full-headless-stack.md

````markdown
---
id: G2.03
title: Determinism through the full headless engine stack
workload: W3
workload_secondary: [W1]
phase: G2
depends_on: [G2.02, G1.07, G7.03]
blocks: [G2.05, G2.16, G5.22]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G2.03/headless_determinism.json
escalation: >
  If divergence is traced to HashMap or HashSet iteration order in a simulation-affecting code path,
  STALL rather than converting every such map to an ordered map inside this item. Enumerate the
  offending call sites with path:line and request approach D explicitly.
status: DRAFT
notes: >
  Tier L: this touches the engine crate, so every iteration costs a 10-15 minute build.
---

## 1. Objective

Two invocations of `eustress-headless --ticks 3600` against the same scaffolded Space at the same
commit produce recording JSON files whose simulation content is byte-identical after stripping the
three wall-clock-derived metadata fields. The stripping rule is explicit and recorded, so no future
reader has to guess which fields were excused.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**Why the unit-level gate is not enough.** `G2.02` proved that a `MinimalPlugins` + Avian world
reruns identically. The engine adds Rune and Luau script execution, WorldDb (Fjall) persistence, the
realism tick systems, and the spawn ordering of a loaded Space. Any one of those can inject
nondeterminism that the unit test cannot see. `docs/architecture/HEADLESS_RUNTIME.md` line 294 states
the determinism gate as byte-identical recordings across two runs; **that gate is written and has
never been run.** P6 (`--render gpu`) in the same document at line 269 is still `new` and is not
your concern here.

**The exact machinery you are driving.**

- `eustress/crates/engine/src/bin/headless.rs` (291 lines). Usage from its own `USAGE` constant:
  `eustress-headless --space <dir> [--ticks N] [--tick-rate HZ] [--no-autoplay]
  [--autoplay-delay-frames N]`, or `--universe <dir>` to open the first Space inside. With
  `--ticks N` it enters Play, runs exactly N sim ticks at a 60 Hz fixed step, stops, and exits 0.
  It is `MinimalPlugins` + `ScheduleRunnerPlugin` — no winit, no GPU, no Slint.
- Its documented v1 limits: no gltf loader is registered, so glb-backed custom meshes do not decode;
  `ai_camera` / `viewport.capture` need the unbuilt `--render gpu` tier. Your scenario Space must
  therefore contain **only** bare parts, physics, scripts, and sim values.
- Stopping fires `on_play_stop` in `eustress/crates/engine/src/simulation/plugin.rs` line 128, which
  exports the recording to `<universe>/.eustress/knowledge/recordings/<space_name>/
  sim_<timestamp>.json` (lines 154–177), falling back to `<space_root>/.eustress/recordings/`.
- The recording structure is `SimulationRecording` in
  `eustress/crates/common/src/simulation/recorder.rs`: `metadata` (`RecordingMetadata` with `name`,
  `started_at`, `simulation_duration_s`, `wall_duration_s`, `total_ticks`, `compression_ratio`,
  `tags`), `series` (a `HashMap<String, TimeSeries>`), and `events`.
- **Three fields are legitimately wall-clock derived and must be stripped before comparison:**
  `metadata.started_at`, `metadata.wall_duration_s`, and `metadata.compression_ratio` (which is
  simulation time divided by wall time, from `SimulationClock::effective_compression`,
  `eustress/crates/common/src/simulation/clock.rs` line 131). Nothing else may be stripped.
- `series` is a `HashMap`, so its **JSON key order is not stable**. Canonicalise by sorting keys
  before hashing. Sorting is canonicalisation, not tolerance — record that you did it.
- `eustress/crates/engine/src/space/mod.rs` line 119 `workspace_root()`: the `EUSTRESS_WORKSPACE`
  environment variable overrides the workspace root and wins over the platform default. Scaffold
  your scenario Space under a scratch `EUSTRESS_WORKSPACE` so the run is reproducible on any machine.

**Prerequisite already satisfied.** `G2.02` produced
`docs/PROMPTS/artifacts/G2.02/determinism_gate.json` with a golden Avian hash and the host block.
If your headless runs diverge, check that golden first — if the unit gate still passes and the
headless run does not, the divergence is in the engine layer, which is exactly what this item is
for.

**Build reality.** A full engine build takes 10–15 minutes; one at a time; never kill it mid-compile.
Validate with `cargo run`, not `cargo check` — `cargo check` will not catch plugin-registration
failures.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/simulation/` — the recording and clock path
- `eustress/crates/engine/src/space/` — only if a spawn-ordering nondeterminism is proven, and only
  the specific ordering site, named in the result block
- `eustress/crates/engine/src/bin/` — a `headless-determinism` driver binary is permitted
- `docs/PROMPTS/artifacts/G2.03/`

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` and `docs/PROMPTS/artifacts/G2.02/` — frozen
- `eustress/crates/engine/src/ui/` and `eustress/crates/engine/ui/` — headless has no UI
- `eustress/crates/engine/src/photoreal.rs` and the render path generally
- `eustress/crates/engine/src/bin/headless.rs` — owned by `G7.03` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- `eustress/crates/common/src/simulation/recorder.rs` — owned by `G1.07` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Adding a fourth field to the strip list, comparing with a float tolerance, reducing the tick count
  below 3600, or removing the script from the scenario are all measurement changes. If the
  measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The scenario Space must exercise all four suspect layers: at least 8 Avian dynamic bodies, at
  least one Rune script writing a sim value each tick, at least one watchpoint recording, and
  WorldDb persistence enabled. A Space with only falling cubes proves nothing beyond `G2.02`.
- Do not disable a subsystem to achieve equality. If Rune must be excluded to pass, that is a
  finding to report, not a fix to apply.
- If you find a nondeterminism source, fix the smallest thing that removes it and name the
  `path:line` in the result block. Do not refactor adjacent code.

## 5. Exit criterion

### Criterion
The canonicalised SHA-256 of run A's recording equals the canonicalised SHA-256 of run B's recording,
over **3600 ticks**, with exactly three stripped metadata fields, and the artifact lists every
series key that participated in the hash.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin eustress-headless --bin headless-determinism
    ./target/release/headless-determinism \
        --scaffold "$TMP/g203_ws" \
        --ticks 3600 \
        --runs 2 \
        --strip metadata.started_at,metadata.wall_duration_s,metadata.compression_ratio \
        --out ../docs/PROMPTS/artifacts/G2.03/headless_determinism.json
    echo "EXIT_DET=$?"

Expected output shape (`headless_determinism.json`):

    {
      "ticks": 3600,
      "runs": 2,
      "stripped_fields": ["metadata.started_at","metadata.wall_duration_s","metadata.compression_ratio"],
      "series_keys_hashed": ["battery.soc","battery.terminal_voltage","body.0.y", "..."],
      "series_key_count": 14,
      "run_hashes": ["sha256:…","sha256:…"],
      "identical": true,
      "first_divergence": null,
      "subsystems_exercised": {"avian_bodies": 8, "rune_scripts": 1, "watchpoints": 4, "worlddb": true}
    }

Pass condition:

    EXIT_DET == 0
    AND headless_determinism.json ."identical" == true
    AND ."ticks" == 3600
    AND ."stripped_fields" has exactly 3 elements
    AND ."series_key_count" >= 8
    AND ."subsystems_exercised".rune_scripts >= 1 AND .avian_bodies >= 8 AND .worlddb == true

Read `identical` and the subsystem counts. A run that hashes identically because the recording was
empty must fail, which is what `series_key_count >= 8` enforces.

## 6. Critic gate

`critic_gate: []`. Byte equality is not a matter of judgement. The mechanical criterion carries the
weight, which is why it constrains not just the hash but the *content* being hashed — the subsystem
counts exist so the gate cannot be satisfied by a trivially empty run.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — canonicalise serialisation, then bisect divergence by series key
   -> if still failing, MANDATORY approach change. Adding a strip field is NOT an approach change;
      moving from JSON-diff bisection to per-tick state hashing to find the first diverging tick is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the first diverging tick index does not increase
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: divergence traced to HashMap/HashSet iteration order (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.03/headless_determinism.json`

A reader finds: tick count, run count, the exact stripped field list, the sorted series keys that
were hashed and their count, one hash per run, an `identical` boolean, a `first_divergence` object
(null on pass; `{tick, series_key, run_a_value, run_b_value}` on fail), and the exercised-subsystem
counts. Alongside it: both raw recording JSONs and the scaffold description for the scenario Space.

## 9. Definition of NOT done

- The runs match because `--no-autoplay` was passed and nothing ever ticked.
- The runs match because the scenario has no script, no watchpoints, and no WorldDb writes — which
  is `G2.02` with extra steps.
- A fourth field quietly joins the strip list (commonly `metadata.name`, if the name embeds a
  timestamp). If the recording name is nondeterministic, make the *name* deterministic; do not strip
  it.
- The comparison uses a float tolerance. "Byte-identical" means byte-identical after canonical key
  ordering; there is no epsilon in this item.
- `first_divergence` is reported as `null` on a failing run because the diff routine gave up. A
  failing run must still name the first diverging tick and series key — that is the whole diagnostic
  value.
- The recording exports to the platform Documents folder instead of the scratch
  `EUSTRESS_WORKSPACE`, so the run is not reproducible on another machine.
````

---

## docs/PROMPTS/items/G2.04_time-compression-fidelity-instrumentation.md

````markdown
---
id: G2.04
title: Time-compression fidelity instrumentation
workload: W3
workload_secondary: [W1]
phase: G2
depends_on: [G2.01, G1.07, G1.12]
blocks: [G2.05, G2.06, G7.10]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G2.04/step_fidelity.json
escalation: >
  If exposing step fidelity requires changing how many physics ticks execute per frame, STALL. This
  item measures the existing behaviour; changing it is G2.05 and must not be smuggled in here.
status: DRAFT
notes: >
  Deliberately an instrumentation-only item. Separating "make the loss visible" from "stop the loss"
  is what lets G2.05 be measured against a known-honest number.
---

## 1. Objective

The simulation clock reports, every frame, what fraction of the compressed simulation interval was
actually covered by executed physics steps. At `time_scale = 1.0` that fraction is at least 0.999;
at `time_scale = 1e6` it is whatever it truly is, and it is readable by name through the existing
sim-value surface rather than being invisible. The behaviour of the clock is unchanged — only its
observability.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**The defect, quoted from the source.** `eustress/crates/common/src/simulation/clock.rs`, function
`advance(&mut self, wall_delta_s: f64) -> u32`, lines 78–102:

```rust
let sim_delta = wall_delta_s * self.time_scale;
// Clock tracks the full compressed simulation time.
self.simulation_time_s += sim_delta;
self.accumulator_s += sim_delta;

let mut ticks = 0u32;
while self.accumulator_s >= self.fixed_timestep_s && ticks < self.max_ticks_per_frame {
    self.accumulator_s -= self.fixed_timestep_s;
    self.tick_count += 1;
    ticks += 1;
}

if ticks >= self.max_ticks_per_frame {
    self.accumulator_s = 0.0;
}

ticks
```

`simulation_time_s` advances by the **full** compressed delta. Executed ticks are capped at
`max_ticks_per_frame` (`CONFIG DEFAULT` = 10, `clock.rs` line 49). On saturation the accumulator is
**zeroed**, so the un-simulated remainder is discarded rather than carried. At `time_scale = 1e6`
and a 16.7 ms frame, `sim_delta` is 16 700 simulation-seconds while at most 10 × (1/60) s = 0.167 s
of stepping occurs — roughly one part in 100 000. `effective_compression()` (`clock.rs` line 131)
returns `simulation_time_s / wall_time_s` and therefore reports the intended compression correctly
while steps are being dropped. Nothing downstream flags it. The `advance` doc comment is honest
about the decoupling; no counter, no watchpoint, and no error surfaces it.

`docs/development/SIMULATION_SYSTEM.md` documents presets up to `BATTERY_CYCLE_TEST = 7.2e6x`. Every
"10 000 cycles in 10 seconds" claim in this project rests on the code above.

**Where sim values are read.** `eustress/crates/engine/src/simulation/plugin.rs` holds
`SimValuesResource`; the MCP tool `get_sim_value` reads from it, and `set_sim_value`
(`eustress/crates/tools/src/simulation_tools.rs` line 115) writes through
`<universe>/.eustress/sim-commands.jsonl`, which the engine drains on its next sim tick. Watchpoints
live in `eustress/crates/common/src/simulation/watchpoint.rs` (`WatchPoint::record(value, time_s,
tick)`).

**Prerequisite already satisfied.** `G2.01` built the `sim-evidence` binary and the recipe
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json`, and produced
`docs/PROMPTS/artifacts/G2.01/physics_baseline.json`. Use them; do not rebuild them. Adding a
scenario to that recipe is in scope for this item.

**Build reality.** 10–15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/common/src/simulation/watchpoint.rs` — additive
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json` — add the `time_compression_sweep` scenario
- `docs/PROMPTS/artifacts/G2.04/`

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` and `docs/PROMPTS/artifacts/G2.02/` — frozen
- The **body of the `while` loop and the cap logic** in `clock.rs::advance` — you may read it and
  measure it; you may not change how many ticks it executes. That is `G2.05`.
- `eustress/crates/engine/src/simulation/electrochemistry.rs`
- `eustress/crates/common/src/simulation/clock.rs` — owned by `G1.07` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- `eustress/crates/engine/src/simulation/plugin.rs` — owned by `G1.07` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Redefining fidelity so that a saturated frame scores 1.0, clamping the reported value, or
  excluding saturated frames from the average are all measurement changes. If the measurement is
  genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The definition of step fidelity is fixed and must be implemented exactly:
  `step_fidelity = (executed_ticks * fixed_timestep_s) / sim_delta` for a frame, and the cumulative
  form `cumulative_step_fidelity = (tick_count * fixed_timestep_s) / simulation_time_s`. When
  `sim_delta` is zero, fidelity is 1.0.
- Also expose `dropped_sim_seconds` (cumulative `sim_delta` minus cumulative executed step time) and
  `saturated_frames` (count of frames where the cap was hit).
- Do not "fix" the clock. A tempting one-line change is to stop zeroing the accumulator; that is a
  behaviour change with a spiral-of-death risk and it belongs to `G2.05`, which has the budget to
  measure the consequences.
- The three values must be readable by the same mechanism a user or agent already has:
  `get_sim_value` with keys `sim.step_fidelity`, `sim.dropped_sim_seconds`, `sim.saturated_frames`.

## 5. Exit criterion

### Criterion
Across a five-point `time_scale` sweep, the reported cumulative step fidelity is **≥ 0.999 at
`time_scale = 1.0`** and **≤ 0.01 at `time_scale = 1e6`**, and `dropped_sim_seconds` at `1e6`
exceeds 0.9 × the total simulated seconds. The clock's `tick_count` at each scale is unchanged from
the `G2.01` baseline for the same scenario.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin sim-evidence
    ./target/release/sim-evidence \
        --recipe ../docs/PROMPTS/harness/recipes/G2_sim_evidence.json \
        --scenario time_compression_sweep \
        --scales 1,100,10000,1000000,7200000 \
        --frames 600 \
        --out ../docs/PROMPTS/artifacts/G2.04/step_fidelity.json
    echo "EXIT_SWEEP=$?"

Expected output shape:

    {
      "frames_per_scale": 600,
      "fixed_timestep_s": 0.016666,
      "max_ticks_per_frame": 10,
      "points": [
        {"time_scale": 1.0,      "cumulative_step_fidelity": 1.000, "dropped_sim_seconds": 0.0,      "saturated_frames": 0,   "tick_count": 600},
        {"time_scale": 100.0,    "cumulative_step_fidelity": 0.000, "dropped_sim_seconds": 0.0,      "saturated_frames": 0,   "tick_count": 0},
        {"time_scale": 10000.0,  "cumulative_step_fidelity": 0.000, "dropped_sim_seconds": 0.0,      "saturated_frames": 0,   "tick_count": 0},
        {"time_scale": 1000000.0,"cumulative_step_fidelity": 0.000, "dropped_sim_seconds": 0.0,      "saturated_frames": 600, "tick_count": 6000},
        {"time_scale": 7200000.0,"cumulative_step_fidelity": 0.000, "dropped_sim_seconds": 0.0,      "saturated_frames": 600, "tick_count": 6000}
      ],
      "baseline_tick_counts_match": true
    }

Pass condition:

    EXIT_SWEEP == 0
    AND point[time_scale=1.0].cumulative_step_fidelity >= 0.999
    AND point[time_scale=1000000.0].cumulative_step_fidelity <= 0.01
    AND point[time_scale=1000000.0].dropped_sim_seconds
        >= 0.9 * (1000000.0 * 600 / 60.0)
    AND ."baseline_tick_counts_match" == true

Read the emitted fidelity values. Do not accept the file existing as evidence.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**. The mean is
irrelevant; D5 below 8.0 fails the item.

What D5 will look at here: whether an engineer reading the emitted series would correctly conclude
that at high compression the simulation is not stepping. Prioritise making the *shape* of the loss
legible — a fidelity series that collapses at the exact frame the cap engages, a `saturated_frames`
counter that matches, and a `dropped_sim_seconds` figure whose magnitude an engineer can sanity-check
by hand against `time_scale × wall_seconds`. Capture recipe:
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json`, scenario `time_compression_sweep`.

The Critic never sees anything you write about your own work, and every score it gives must cite a
specific value or `path:line`. Numbers that only make sense with your commentary attached will not
be credited.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — additive counters on SimulationClock, published via SimValuesResource
   -> if still failing, MANDATORY approach change. Renaming a field is NOT an approach change;
      moving from clock-internal counters to a separate FidelityLedger resource sampled per frame is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D5 moving < 0.5 AND the 1e6 fidelity value
                  moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: any change to executed tick counts vs the G2.01 baseline (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.04/step_fidelity.json`

A reader finds: the frame count and fixed timestep used, the `max_ticks_per_frame` in force, and one
row per `time_scale` giving cumulative step fidelity, dropped simulation seconds, saturated frame
count, and executed tick count — plus a boolean confirming tick counts are unchanged from the
`G2.01` baseline. This file is the honest statement of what time compression currently does, and
`G2.05` is measured against it.

## 9. Definition of NOT done

- Fidelity is reported per frame but never cumulatively, so a reader cannot tell how much simulation
  was lost over the whole run.
- `dropped_sim_seconds` is computed from `time_scale` alone rather than from the accumulator, so it
  stays correct only while the frame rate is constant.
- The clock is "fixed" as part of this item and `tick_count` no longer matches the `G2.01` baseline.
  That is `G2.05`'s job and this item's escalation trigger.
- The values exist as Rust fields but are not published to `SimValuesResource`, so `get_sim_value`
  returns nothing and no agent, script, or watchpoint can see them.
- Fidelity reads 1.0 at `time_scale = 1e6` because the implementation divided executed step time by
  executed step time.
- The sweep is run for 60 frames instead of 600, so the saturation regime is barely sampled.
````

---

## docs/PROMPTS/items/G2.05_bounded-error-integration-under-time-compression.md

````markdown
---
id: G2.05
title: Bounded-error integration under time compression
workload: W1
workload_secondary: [W3]
phase: G2
depends_on: [G2.04, G2.03, G1.07, G1.12]
blocks: [G2.13]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G2.05/compression_error.json
escalation: >
  If holding the error bound at time_scale = 1000 requires more wall time than the equivalent
  time_scale = 1 run, STALL. Compression that costs more than it saves is not compression, and the
  correct response is a decision about which scales are supported, not a slower integrator.
status: DRAFT
notes: >
  Tier L: engine-crate change plus a sweep, so every iteration is a full build.
---

## 1. Objective

Running the reference cell-discharge scenario at `time_scale = 1000` produces a terminal-voltage
trajectory whose RMS deviation from the `time_scale = 1` reference run is at most 5 mV over the
window SOC 0.9 to 0.1, and the supported compression range is declared explicitly with the measured
error at each supported scale. Compression stops being a number the clock reports and becomes a
number with an error bar.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**What `G2.04` established** (read its artifact,
`docs/PROMPTS/artifacts/G2.04/step_fidelity.json`). `SimulationClock::advance` in
`eustress/crates/common/src/simulation/clock.rs` lines 78-102 advances `simulation_time_s` by the
full compressed delta, caps executed physics ticks at `max_ticks_per_frame` (`CONFIG DEFAULT` = 10),
and **zeroes the accumulator on saturation**, discarding the remainder. `G2.04` made that loss
visible as `sim.step_fidelity`, `sim.dropped_sim_seconds`, `sim.saturated_frames`. It did not change
it. Changing it, within a stated error bound, is this item.

**One subsystem already tries to compensate, and you must not duplicate it.**
`eustress/crates/engine/src/simulation/electrochemistry.rs`, in `electrochemical_tick`, computes:

```rust
let frame_sim_dt = time.delta_secs_f64() * clock.time_scale;
let max_step = clock.dt() * clock.max_ticks_per_frame.max(1) as f64;
let dt = frame_sim_dt.min(max_step).max(clock.dt()) as f32;
```

so electrochemistry integrates by the compressed frame delta, clamped to the per-frame budget. That
clamp is exactly the drop `G2.04` measured, moved one layer up. It is a single explicit-Euler step of
size `dt`, which at large `dt` is both inaccurate and clamped. Your solution must supersede this
local workaround, not sit beside it.

**Available integrators - reuse, do not write your own.**
`eustress/crates/common/src/realism/numerics/ode/` contains `euler.rs`, `runge_kutta.rs` (RK4,
RK45), `verlet.rs`, and `implicit.rs` (BDF), re-exported through `ode::prelude`.

**The reference scenario.** `vcell_discharge_0p5c` from
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json`, created by `G2.01`: one entity with
`ElectrochemicalState` (defaults at `eustress/crates/common/src/realism/particles/components.rs`
line 524: `capacity_ah: 202.5`, `soc: 1.0`, `internal_resistance: 0.001`, `voltage: 2.23`)
discharged at 0.5C. `docs/PROMPTS/artifacts/G2.01/physics_baseline.json` records its `final_soc`,
`final_terminal_v`, and wall time at `time_scale = 1`.

**Prerequisite already satisfied.** `G2.03` proved the headless stack reruns byte-identically over
3600 ticks. That is what makes a 5 mV RMS bound meaningful: the run-to-run noise floor is zero, so
every millivolt of deviation is attributable to compression, not to jitter.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/common/src/realism/numerics/ode/` — additive
- `eustress/crates/engine/src/simulation/electrochemistry.rs` — the `dt` computation quoted above
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json` — add `compression_error_sweep`
- `docs/PROMPTS/artifacts/G2.05/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/`, `/G2.02/`, `/G2.03/`, `/G2.04/` — frozen
- `eustress/crates/common/src/realism/laws/electrochemistry.rs` — the law functions are
  chemistry-agnostic and correct; this item is about *time integration*, not the physics
- `eustress/crates/engine/src/ui/`
- `eustress/crates/common/src/simulation/clock.rs` — owned by `G1.07` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- `eustress/crates/engine/src/simulation/plugin.rs` — owned by `G1.07` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Narrowing the SOC window, comparing against a reference that is itself compressed, sampling the
  trajectory at fewer than 200 points, or reporting mean absolute error instead of RMS are all
  measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The reference trajectory is the `time_scale = 1` run and nothing else. It must be produced by the
  same binary at the same commit.
- Any approach that lets the accumulator grow without bound reintroduces the spiral of death. State
  in the result block what bounds per-frame work, and prove the run terminates.
- `time_scale = 1.0` behaviour must be bit-for-bit unchanged: rerun the `G2.03` headless determinism
  command and confirm the hash still matches. A compression fix that perturbs real-time simulation
  has broken the thing that already worked.
- Comparison is on **terminal voltage**, sampled at fixed SOC points (not fixed time points), so the
  two runs are compared at the same physical state rather than at the same clock reading.

## 5. Exit criterion

### Criterion
RMS deviation of terminal voltage between the `time_scale = 1000` run and the `time_scale = 1`
reference, over 400 evenly spaced SOC samples from 0.9 down to 0.1, is **<= 5.0 mV**, while the
`time_scale = 1` recording hash still matches the `G2.03` golden.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin sim-evidence --bin headless-determinism
    ./target/release/sim-evidence \
        --recipe ../docs/PROMPTS/harness/recipes/G2_sim_evidence.json \
        --scenario compression_error_sweep \
        --reference-scale 1 \
        --scales 10,100,1000,10000 \
        --soc-window 0.9,0.1 --soc-samples 400 \
        --out ../docs/PROMPTS/artifacts/G2.05/compression_error.json
    echo "EXIT_ERR=$?"
    ./target/release/headless-determinism \
        --scaffold "$TMP/g205_ws" --ticks 3600 --runs 2 \
        --strip metadata.started_at,metadata.wall_duration_s,metadata.compression_ratio \
        --out ../docs/PROMPTS/artifacts/G2.05/realtime_unchanged.json
    echo "EXIT_RT=$?"

Expected output shape (`compression_error.json`):

    {
      "reference_scale": 1.0, "soc_window": [0.9, 0.1], "soc_samples": 400,
      "reference_wall_s": 0.0,
      "points": [
        {"time_scale": 10.0,    "rms_terminal_v": 0.0000, "max_abs_terminal_v": 0.0000, "wall_s": 0.0, "step_fidelity": 1.000},
        {"time_scale": 100.0,   "rms_terminal_v": 0.0000, "max_abs_terminal_v": 0.0000, "wall_s": 0.0, "step_fidelity": 1.000},
        {"time_scale": 1000.0,  "rms_terminal_v": 0.0000, "max_abs_terminal_v": 0.0000, "wall_s": 0.0, "step_fidelity": 1.000},
        {"time_scale": 10000.0, "rms_terminal_v": 0.0000, "max_abs_terminal_v": 0.0000, "wall_s": 0.0, "step_fidelity": 0.000}
      ],
      "supported_max_scale_at_5mv": 1000.0
    }

Pass condition:

    EXIT_ERR == 0 AND EXIT_RT == 0
    AND point[time_scale=1000.0].rms_terminal_v <= 0.005
    AND point[time_scale=1000.0].wall_s < reference_wall_s
    AND realtime_unchanged.json ."identical" == true
    AND compression_error.json ."soc_samples" == 400

Read `rms_terminal_v` at scale 1000 and the `identical` flag. File existence is not a pass.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**. The mean is
irrelevant; D5 below 8.0 fails the item.

D5 will ask whether an electrochemist would accept the compressed trajectory as the same experiment.
Prioritise the *shape* of the curve at the knees — the initial IR drop and the end-of-discharge
voltage collapse are where a fixed-step integrator diverges first, and where `max_abs_terminal_v`
will be dominated. A run with 5 mV RMS but 80 mV of error concentrated in the last 5% of discharge
must be reported honestly, not averaged away. Capture recipe:
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json`, scenario `compression_error_sweep`.

The Critic never sees anything you write about your own work, and every score it gives cites a
specific value or `path:line`. Make the error localisable.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — sub-stepping with a carried accumulator and a per-frame work budget
   -> if still failing, MANDATORY approach change. Raising max_ticks_per_frame is NOT an approach
      change; moving from fixed-step sub-stepping to an adaptive RK45 step with error control is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D5 moving < 0.5 AND rms_terminal_v at scale 1000
                  moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: wall_s at scale 1000 exceeds reference_wall_s (see front matter)
```

The stall packet must request exactly one of: LOWER the supported scale to a stated value; FUND
approach D; DEFER behind a named item; KILL.

## 8. Artifact

`docs/PROMPTS/artifacts/G2.05/compression_error.json`

A reader finds: the reference scale and its wall time, the SOC window and sample count, and one row
per tested `time_scale` giving RMS and maximum absolute terminal-voltage deviation, wall time, and
the step fidelity from `G2.04`'s instrumentation — plus `supported_max_scale_at_5mv`, the highest
scale that holds the bound. Alongside it: `realtime_unchanged.json`, proving `time_scale = 1` still
hashes identically.

`supported_max_scale_at_5mv` is the number this project may publish. Any compression claim above it
is unsupported.

## 9. Definition of NOT done

- RMS clears 5 mV because the SOC window was narrowed to 0.8-0.2, excluding both knees.
- The bound holds but `wall_s` at scale 1000 exceeds the reference — the run got slower, so nothing
  was compressed.
- The fix raises `max_ticks_per_frame` from 10 to 10 000. That converts a silent error into an
  unbounded frame time and will fail `G5.21`.
- Real-time behaviour changes: `realtime_unchanged.json` reports `identical: false` and the result
  block calls it an improvement. `G2.03`'s golden is a contract.
- Error is reported as a single RMS with no `max_abs`, hiding a large localised divergence at end of
  discharge.
- The comparison samples both runs at fixed *time* points, so the two trajectories are compared at
  different physical states and the error is dominated by a time offset rather than by integration
  error.
````

---

## docs/PROMPTS/items/G2.06_sim-time-alerting-watchman-and-breakpoints.md

````markdown
---
id: G2.06
title: Sim-time alerting for Watchman and breakpoints
workload: W1
workload_secondary: [W3]
phase: G2
depends_on: [G2.04, G1.12]
blocks: []
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G2.06/alert_timing.json
escalation: >
  If switching Watchman to sim-time produces more than 200 alerts in a 600-frame run at
  time_scale = 1e6, STALL. Trading a missed-spike failure for an alert-storm failure is not a fix,
  and choosing the storm-control policy is a human decision.
status: DRAFT
notes: >
  Tier M: two small files in the engine crate, one measurement, but still one full engine build per
  iteration.
---

## 1. Objective

A threshold breach lasting 4 simulated seconds fires exactly one Watchman alert whether the run is
at `time_scale = 1` or `time_scale = 1e6`. Alert cooldown and poll interval are expressed in
simulation time, not wall time, and the wall-time behaviour remains available as an explicit, named
option rather than as the only behaviour.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**The defect, with line numbers.** `eustress/crates/engine/src/workshop/watchman.rs`:

- line 73 `pub struct WatchmanConfig`; line 79 `pub poll_interval: Duration`; line 81
  `pub alert_cooldown: Duration`
- lines 113-114 in `Default`: `poll_interval: Duration::from_secs(5)`,
  `alert_cooldown: Duration::from_secs(30)` (both `CONFIG DEFAULT`)
- line 130 `last_alert: HashMap<String, Instant>`
- line 163 `if now.duration_since(last) < config.poll_interval { return }`
- lines 194-201 `if now.duration_since(*last_alert_time) < config.alert_cooldown { … }` then
  `state.last_alert.insert(key.clone(), now)`

`now` is a `std::time::Instant` — **wall time**. `docs/AUDIT/11_SIMULATION_DEBUGGER.md` Feature 3
records the consequence as risk R3.1: "Cooldown is wall-time not sim-time; at 10^6x scale, miss
short spikes or alert-storm," with mitigation M3.1: "Add `cooldown_simulation_ticks` parallel to
`cooldown_ticks`." At `time_scale = 1e6`, a 30-second wall cooldown spans 30 million simulated
seconds — roughly a year of simulated time in which no second alert can fire — and a 5-second wall
poll interval means the monitor samples once per about 5 million simulated seconds. Any spike shorter
than that is invisible.

**The parallel defect in breakpoints.** `eustress/crates/common/src/simulation/breakpoint.rs`
line 75 `pub cooldown_ticks: u32`, line 103 default `0`, line 121 `with_cooldown(ticks)`, line 134
`if self.ticks_since_trigger < self.cooldown_ticks`. This one counts *executed ticks*, which under
the saturation `G2.04` measured is also not simulation time — at `time_scale = 1e6` ten executed
ticks can span millions of simulated seconds. Both surfaces must agree on what "cooldown" means.

**What `G2.04` gives you.** `sim.step_fidelity`, `sim.dropped_sim_seconds`, and
`sim.saturated_frames` are published to `SimValuesResource` and readable through `get_sim_value`.
`SimulationClock::simulation_time_s` (`eustress/crates/common/src/simulation/clock.rs`) is the
sim-time source of truth. `docs/PROMPTS/artifacts/G2.04/step_fidelity.json` records what fidelity
actually is at each scale — an alert policy expressed in sim time must still be honest when only a
small fraction of that sim time was actually stepped.

**Where alerts go.** Watchman injects synthetic messages into the Workshop pipeline; the config
resource is registered at `eustress/crates/engine/src/workshop/mod.rs` line 1760
(`.init_resource::<watchman::WatchmanConfig>()`).

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/workshop/watchman.rs`
- `eustress/crates/engine/src/workshop/mod.rs` — registration only
- `eustress/crates/common/src/simulation/breakpoint.rs`
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json` — add `alert_spike_sweep`
- `docs/PROMPTS/artifacts/G2.06/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.05/` — frozen
- `eustress/crates/common/src/simulation/clock.rs` — read it; `G2.04` and `G2.05` own it
- `eustress/crates/engine/src/ui/` — no panel work in this item

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Lengthening the injected spike beyond 4 simulated seconds, lowering the threshold so the signal
  never leaves breach, or counting "alerts that would have fired" instead of alerts actually injected
  are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Wall-time behaviour must remain reachable. Add a mode (for example
  `WatchmanConfig::cooldown_basis: SimTime | WallTime`) with `SimTime` as the default, rather than
  deleting the wall-time path. Long unattended real-time runs still want a wall-time floor.
- Poll interval and cooldown must use the same basis. A sim-time cooldown with a wall-time poll is
  the same bug with extra steps.
- The breakpoint cooldown must be expressible in **simulation seconds**, because `G2.04` proved
  executed ticks and simulation time diverge by orders of magnitude under compression. Keep the
  tick-based field for compatibility; add the sim-seconds field and prefer it.
- The existing 10-alert run cap stays. Report cap hits rather than silently dropping.

## 5. Exit criterion

### Criterion
For a threshold breach injected for exactly **4.0 simulated seconds**, the alert count is **exactly
1** at `time_scale = 1`, `1e3`, and `1e6`, and the total alert count over the 600-frame run at
`time_scale = 1e6` with a repeating breach is **>= 1 and <= 200**.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin sim-evidence
    ./target/release/sim-evidence \
        --recipe ../docs/PROMPTS/harness/recipes/G2_sim_evidence.json \
        --scenario alert_spike_sweep \
        --spike-sim-seconds 4.0 \
        --scales 1,1000,1000000 \
        --frames 600 \
        --out ../docs/PROMPTS/artifacts/G2.06/alert_timing.json
    echo "EXIT_ALERT=$?"

Expected output shape:

    {
      "spike_sim_seconds": 4.0,
      "cooldown_basis": "SimTime",
      "cooldown_sim_seconds": 30.0,
      "poll_sim_seconds": 5.0,
      "points": [
        {"time_scale": 1.0,       "single_spike_alerts": 1, "repeating_spike_alerts": 0, "cap_hits": 0, "step_fidelity": 1.000},
        {"time_scale": 1000.0,    "single_spike_alerts": 1, "repeating_spike_alerts": 0, "cap_hits": 0, "step_fidelity": 1.000},
        {"time_scale": 1000000.0, "single_spike_alerts": 1, "repeating_spike_alerts": 0, "cap_hits": 0, "step_fidelity": 0.000}
      ],
      "breakpoint_cooldown_basis": "SimSeconds"
    }

Pass condition:

    EXIT_ALERT == 0
    AND every point.single_spike_alerts == 1
    AND point[time_scale=1000000.0].repeating_spike_alerts >= 1
    AND point[time_scale=1000000.0].repeating_spike_alerts <= 200
    AND ."cooldown_basis" == "SimTime"
    AND ."breakpoint_cooldown_basis" == "SimSeconds"

Read the alert counts. "The alert appeared in the log somewhere" is not evidence — the count is the
measurement.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**.

D5 will ask whether an operator watching this system would be correctly informed. Prioritise: an
alert whose payload states the **simulation time** of the breach (not the wall time), a cooldown a
reader can verify by arithmetic against the emitted sim timestamps, and honest reporting when the
10-alert cap suppresses further alerts — silent suppression is the failure mode D5 penalises hardest.
Capture recipe: `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`, scenario `alert_spike_sweep`.

The Critic never sees your self-report and cites specific values.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — add a cooldown_basis to WatchmanConfig, drive from SimulationClock
   -> if still failing, MANDATORY approach change. Tuning the cooldown constant is NOT an approach
      change; moving from interval polling to edge-triggered breach detection on the value stream is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D5 moving < 0.5 AND single_spike_alerts at
                  time_scale = 1e6 still != 1
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: more than 200 alerts in the 600-frame 1e6 run (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.06/alert_timing.json`

A reader finds: the injected spike duration in simulated seconds, the cooldown basis and its value in
simulated seconds, the poll interval in simulated seconds, and one row per `time_scale` giving the
alert count for a single spike, the alert count for a repeating spike, the number of cap hits, and
the step fidelity at that scale — plus the breakpoint cooldown basis. This is the evidence that the
monitoring layer means the same thing at every compression setting.

## 9. Definition of NOT done

- Alerts fire correctly at `time_scale = 1e6` but now fire three times at `time_scale = 1`, because
  the poll interval was shortened rather than rebased.
- The cooldown is sim-time but the poll is still `Instant`-based, so short spikes are still missed
  between samples.
- The breakpoint cooldown remains in executed ticks and the artifact reports
  `breakpoint_cooldown_basis: "Ticks"`.
- The wall-time path is deleted rather than kept as an option, so an operator running an unattended
  real-time simulation loses a rate limit they relied on.
- The 10-alert cap is hit and the run reports `cap_hits: 0` because cap suppression is not counted.
- The alert payload timestamps the breach in wall time, so a reader of the alert cannot locate the
  event in the recording, whose series are indexed by simulation time.
````

---

## docs/PROMPTS/items/G2.07_conservation-invariants-runnable-suite.md

````markdown
---
id: G2.07
title: Conservation invariants as a runnable suite
workload: W3
workload_secondary: [W1]
phase: G2
depends_on: [G2.01, G1.12]
blocks: [G2.14, G5.23]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G2.07/conservation.json
escalation: >
  If energy drift in the frictionless pendulum exceeds 5% over 3600 ticks under the engine's shipped
  SubstepCount(6), STALL. That is a solver-configuration question with consequences for every other
  item in this pack and must be decided, not tuned around.
status: DRAFT
notes: >
  Tier M. The conservation law functions already exist; this item wires them into a scenario suite
  with tolerances rather than writing new physics.
---

## 1. Objective

Four conservation invariants — energy, linear momentum, angular momentum, and mass — are checked
continuously during headless runs of four purpose-built scenarios, each with an analytic reference
and a stated tolerance. One command reports the measured drift for all four and exits non-zero if
any exceeds its bound.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian** (`avian3d`); never Rapier. Units are meter-native. Licence: PolyForm Shield
1.0.0 (source-available).

**Why this matters more than a unit test.** Pack T2 spends the rest of its budget adding models
(spatial electrochemistry, fracture, FEA). Every one of them can be wrong in a way a discharge-curve
fit will not reveal but a conservation check will. A conservation suite is the only validation source
in this pack that needs no external dataset — the reference is a mathematical identity, so it can
never go stale.

**What already exists and must be reused, not rewritten.**
`eustress/crates/common/src/realism/laws/conservation.rs`:

- `mass_conservation_check(initial_mass, current_masses) -> f32` (line 21)
- `total_mechanical_energy(kinetic, potential)` (line 54),
  `energy_conservation_check(initial_energy, current_energy)` (line 61),
  `energy_dissipated(initial, final)` (line 67)
- `total_momentum(masses, velocities) -> Vec3` (line 99),
  `momentum_conservation_check(initial_momentum, current_momentum) -> Vec3` (line 108)
- `center_of_mass` (line 113), `center_of_mass_velocity` (line 128)
- `angular_momentum_point` (line 143), `total_angular_momentum` (line 148),
  `angular_momentum_conservation_check` (line 161)
- a checker type with `initialize` (line 198), `check` (line 219), `all_conserved` (line 271)

**The engine's physics pins**, applied in `eustress/crates/engine/src/main.rs` and mirrored in
`eustress/crates/common/tests/determinism.rs`: `Time::<Fixed>::from_hz(60.0)`, `SubstepCount(6)`,
`avian3d::dynamics::solver::SolverConfig::default()`, `Gravity(Vec3::NEG_Y * 9.80665)`. Your
scenarios must use these exact pins; a conservation number measured under different pins does not
describe the engine anyone ships.

**The four scenarios are fixed.**

1. `pendulum_conservation` — single rigid pendulum, no damping, no friction, released from 60
   degrees. Invariant: total mechanical energy. Analytic reference: `E = m g L (1 - cos theta0)`,
   constant. This scenario already exists in `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`
   from `G2.01` and reports `energy_drift_frac` in
   `docs/PROMPTS/artifacts/G2.01/physics_baseline.json`.
2. `elastic_collision_1d` — two equal-mass bodies, restitution 1.0, head-on, zero gravity, zero
   friction. Invariants: linear momentum and kinetic energy. Analytic reference: velocities exchange.
3. `spinning_body_free` — one dynamic body with initial angular velocity, zero gravity, no contacts.
   Invariant: angular momentum vector magnitude and direction.
4. `mass_ledger` — the fracture-precursor scenario: N bodies whose total mass must equal the initial
   total. Invariant: mass. This is the scenario `G2.14` extends when a body splits.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/common/src/realism/laws/conservation.rs` — additive
- `eustress/crates/common/tests/` — new test files permitted
- `eustress/crates/engine/src/bin/` — a `conservation-suite` binary is permitted
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json` — add the three new scenarios
- `docs/PROMPTS/artifacts/G2.07/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.06/` — frozen
- `eustress/crates/common/src/physics/determinism.rs` and the engine's solver pins — measuring under
  the shipped pins is the point
- `eustress/crates/engine/src/ui/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Adding damping to make energy look conserved, shortening the run below 3600 ticks, raising a
  tolerance, or dropping a scenario are all measurement changes. If the measurement is genuinely
  wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Do not change `SubstepCount` or `SolverConfig` to improve a number. If the shipped configuration
  cannot hold a tolerance, that is the finding — report it and STALL per the front-matter trigger.
- Drift is measured as a **fraction of the initial quantity**, sampled every 60 ticks, and both the
  maximum and the final drift are reported. A quantity that drifts up and back is not conserved.
- Tolerances are fixed `TARGET` values, chosen to be achievable by a 60 Hz / 6-substep impulse solver
  over 60 simulated seconds:
  - energy (pendulum): <= 0.5% max drift
  - linear momentum (elastic collision): <= 0.1% max drift
  - angular momentum (free spin): <= 1.0% max drift in magnitude and <= 1.0 degree in direction
  - mass (ledger): <= 1e-6 relative
- The binary must exit non-zero when any tolerance is exceeded. A reporter that always exits 0 is not
  a gate.

## 5. Exit criterion

### Criterion
All four invariants hold within their stated tolerances over **3600 ticks** at 60 Hz with the shipped
solver pins and the suite binary exits **0**; when every tolerance is tightened by a factor of 10 the
same binary exits **non-zero**.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin conservation-suite
    ./target/release/conservation-suite \
        --recipe ../docs/PROMPTS/harness/recipes/G2_sim_evidence.json \
        --ticks 3600 --sample-every 60 \
        --out ../docs/PROMPTS/artifacts/G2.07/conservation.json
    echo "EXIT_PASS=$?"
    ./target/release/conservation-suite \
        --recipe ../docs/PROMPTS/harness/recipes/G2_sim_evidence.json \
        --ticks 3600 --sample-every 60 --tolerance-scale 0.1 \
        --out ../docs/PROMPTS/artifacts/G2.07/conservation_tightened.json
    echo "EXIT_FAIL=$?"

Expected output shape (`conservation.json`):

    {
      "ticks": 3600, "hz": 60, "substeps": 6, "sample_every": 60,
      "invariants": {
        "energy_pendulum":            {"tolerance": 0.005, "max_drift": 0.0000, "final_drift": 0.0000, "pass": true},
        "linear_momentum_collision":  {"tolerance": 0.001, "max_drift": 0.0000, "final_drift": 0.0000, "pass": true},
        "angular_momentum_free_spin": {"tolerance": 0.010, "max_drift": 0.0000, "max_angle_deg": 0.00,  "pass": true},
        "mass_ledger":                {"tolerance": 1e-6,  "max_drift": 0.0000, "final_drift": 0.0000, "pass": true}
      },
      "all_pass": true
    }

Pass condition:

    EXIT_PASS == 0 AND EXIT_FAIL != 0
    AND conservation.json ."all_pass" == true
    AND every invariant's max_drift <= its tolerance
    AND conservation.json ."ticks" == 3600 AND ."substeps" == 6

The second invocation is not optional: a gate that cannot fail is not a gate, and `EXIT_FAIL != 0` is
how that is proven.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**.

D5 will ask whether a physicist would accept these four checks as covering the solver's failure
modes. Prioritise: reporting max drift and not just final drift, sampling densely enough that a
transient spike is visible, and stating the analytic reference for each scenario in the artifact so a
reader can verify the initial quantity by hand. Capture recipe:
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json`.

The Critic never sees your self-report and must cite a specific value.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — scenario construction plus per-sample checks via conservation.rs
   -> if still failing, MANDATORY approach change. Adjusting a scenario's initial conditions is NOT
      an approach change; moving from contact-based scenarios to constraint/joint-based ones to
      isolate solver dissipation is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same invariant failing and its max_drift
                  moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: pendulum energy drift > 5% under the shipped pins (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.07/conservation.json`

A reader finds: tick count, rate, substep count, sampling interval, and for each of the four
invariants its tolerance, maximum drift, final drift, and pass flag — plus an `all_pass` boolean.
Alongside it, `conservation_tightened.json` demonstrating the suite fails when it should.

This artifact is what `G2.14` (fracture) and `G5.23` (regression gate) build on: a fracture event
that violates mass or momentum conservation gets caught here rather than discovered by a customer.

## 9. Definition of NOT done

- Energy is conserved because the pendulum was given damping "for stability".
- The suite reports pass with `max_drift` absent and only `final_drift` present, hiding a transient.
- The tightened-tolerance run also exits 0, proving the binary never fails.
- Angular momentum magnitude is checked but direction is not, so a body that precesses under solver
  error scores clean.
- Scenarios are run with `SubstepCount` raised above 6 to reduce drift. The measurement must describe
  the shipped configuration.
- `mass_ledger` passes trivially because no body count ever changes, and the scenario is not
  structured so `G2.14` can extend it with a split event.
````

---

## docs/PROMPTS/items/G2.08_chemistry-agnostic-0d-cell-validated-against-public-data.md

````markdown
---
id: G2.08
title: Chemistry-agnostic 0-D cell model validated against a public dataset
workload: W1
workload_secondary: [W3]
phase: G2
depends_on: [G2.01, G1.12]
blocks: [G2.09, G5.22, G2.32]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G2.08/cell_validation_0d.json
escalation: >
  If no public discharge dataset with a redistributable licence can be located and archived, STALL
  before writing any model code. A validation item whose reference cannot be shipped with the repo
  has no evidence and the whole G2.08-G2.13 ladder loses its foundation.
status: DRAFT
notes: >
  Tier L. This is the first rung of the V-Cell electrochemistry ladder. It deliberately validates
  against a Li-ion dataset rather than Na-S, because public Na-S cell data is scarce and the laws
  module already claims to be chemistry-agnostic - so validating on a different chemistry tests both
  the model and that claim at once.
---

## 1. Objective

The existing lumped 0-D cell model is parameterised from an archived public dataset rather than from
constants hard-coded in the tick system, and its constant-current discharge curve is compared to
that dataset with a measured RMS voltage error under 150 mV at 0.5C. Cycle counting and capacity
fade produce non-zero, physically ordered values. The comparison rig, the digitised reference points,
and the residuals are committed.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**The honest current state.** `docs/AUDIT/19_REALISM_PHYSICS.md` P4 records: V-Cell "Nernst +
Butler-Volmer are **lumped 0-D models** (no spatial electrochemistry / ion transport) — validation
gap vs. real cells now flagged." There is no validation of any kind today: no dataset, no error
metric, no residuals. The model has never been compared to a measured cell.

**The law functions are already correct and chemistry-agnostic.**
`eustress/crates/common/src/realism/laws/electrochemistry.rs` (436 lines) provides
`nernst_potential`, `thermal_voltage`, `butler_volmer_current`, `butler_volmer_symmetric`,
`tafel_overpotential`, `exchange_current_density`, `ohmic_overpotential`, `electrolyte_asr`,
`cell_resistance_from_asr`, `terminal_voltage`, `round_trip_efficiency`, `arrhenius_conductivity`,
`nernst_einstein_diffusivity`, `nernst_planck_flux`, `ohmic_heat`, `reaction_heat`, `entropic_heat`,
`total_heat_generation`, `steady_state_temp_rise`, `capacity_retention_power_law`,
`cycles_to_retention`, `state_of_charge`, `c_rate`, `monroe_newman_critical_current`,
`dendrite_risk`. **Do not rewrite these.** The module's own header states they are
"Chemistry-agnostic implementations of core electrochemical principles" — this item tests that claim.

**The defects are in the tick system, not the laws.**
`eustress/crates/engine/src/simulation/electrochemistry.rs`, function `electrochemical_tick`:

1. **Cycle counting never increments.** Section 9 computes
   `let cycle_fraction = charge_delta_ah.abs() / (2.0 * effective_capacity);` then
   `let new_cycles = echem_state.cycle_count as f32 + cycle_fraction;` then
   `echem_state.cycle_count = new_cycles as u32;`. `cycle_count` is `u32`; a sub-unit fraction added
   to an integer and cast back to `u32` truncates to the same integer. `cycle_count` is therefore
   permanently 0, which makes `capacity_retention_power_law(0, …)` return 1.0 permanently. Capacity
   fade is dead code.
2. **Open-circuit voltage is a toy.** Section 1 uses `activity_ratio = (1.0 - soc) / soc` fed to
   `nernst_potential`. That produces a symmetric logit shape, not a real cell OCV curve. It is the
   single largest source of discharge-curve error and cannot be fixed by tuning.
3. **Kinetic parameters are hard-coded per-tick literals, not per-entity data.** Section 3 contains
   `let j0 = 50.0_f32; // A/m2` and `let electrode_area = 0.03_f32; // ~300 cm2 = 0.03 m2`. A
   chemistry-agnostic model cannot have its exchange current density baked into the loop.
4. **Diffusion overpotential is hard-coded zero.** Section 4 calls
   `echem::terminal_voltage(ocv, eta_ohmic, eta_ct, 0.0, is_discharge)` with the comment "no
   diffusion overpotential for now". That is `G2.09`'s subject; leave the zero in place here but
   make the parameter reachable.
5. **`ionic_conductivity` is a field nobody reads.**
   `eustress/crates/common/src/realism/particles/components.rs` line 511 declares
   `pub ionic_conductivity: f32` on `ElectrochemicalState`; grep the tick — it is never used.
   `internal_resistance` is a constant with no temperature dependence even though
   `echem::arrhenius_conductivity` exists. That is `G2.11`'s subject; here, just make the parameter
   set explicit.

`ElectrochemicalState` (`components.rs` line 497) fields: `voltage`, `terminal_voltage`,
`capacity_ah`, `soc`, `current`, `internal_resistance`, `ionic_conductivity`, `cycle_count`,
`c_rate`, `capacity_retention`, `heat_generation`, `dendrite_risk`. Defaults (line 524):
`capacity_ah: 202.5`, `soc: 1.0`, `internal_resistance: 0.001`, `voltage: 2.23`.

**Choosing the reference dataset.** You must select, verify, and archive one public
constant-current discharge dataset for a single cell. Candidate sources to evaluate — **verify each
one yourself before citing it; do not assume anything about its contents or licence**:

- The NASA Prognostics Center of Excellence battery data repository (Li-ion 18650 cycling).
- The Oxford Battery Degradation Dataset (pouch-cell cycling, University of Oxford).
- The parameter set and validation curves published with Chen et al., "Development of Experimental
  Techniques for Parameterization of Multi-scale Lithium-ion Battery Models", Journal of The
  Electrochemical Society 167 080534 (2020) — the LG M50 cell, widely redistributed as the
  `Chen2020` parameter set in the open-source PyBaMM project.

Requirements the chosen source must meet, and which the artifact must record:

- Publicly downloadable without registration behind a paywall.
- A licence that permits redistributing the digitised points inside this repository. Record the
  licence string in the artifact. If it does not permit redistribution, record the URL and the
  retrieval date and store a checksum of the original file rather than the file.
- At least **50 usable (SOC or capacity, voltage) points** on a constant-current discharge at a
  stated C-rate and a stated temperature.
- A stated nominal capacity so C-rate can be reproduced.

**Prerequisite already satisfied.** `G2.01` built `sim-evidence`, the recipe
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json` with the `vcell_discharge_0p5c` scenario, and
`docs/PROMPTS/artifacts/G2.01/physics_baseline.json` recording that scenario's `final_soc`,
`final_terminal_v`, and wall time.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/src/simulation/electrochemistry.rs`
- `eustress/crates/common/src/realism/particles/components.rs` — additive fields on
  `ElectrochemicalState` and a new `CellParameters` component
- `eustress/crates/common/src/realism/laws/electrochemistry.rs` — additive only; do not change the
  signature or behaviour of an existing function
- `eustress/crates/common/src/realism/constants.rs` — additive chemistry blocks
- `eustress/crates/engine/src/bin/` — a `cell-validate` binary is permitted
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json` — add `cell_validation_ccd`
- `docs/PROMPTS/artifacts/G2.08/` — including the archived reference points

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.07/` — frozen
- `eustress/crates/common/src/simulation/clock.rs` — time compression is `G2.04`/`G2.05`
- `eustress/crates/engine/src/ui/` — no panel work
- `eustress/crates/common/src/physics/` — no Avian changes

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Trimming reference points near the knees, restricting the SOC comparison window, switching from
  RMS to a percentile, or fitting the model to the same points used for validation are all
  measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- **No fitting on the test set.** Parameters may be taken from the source publication or fitted on a
  disjoint C-rate. State in the artifact which C-rate was used for parameterisation and which for
  validation, and they must differ.
- Chemistry parameters move out of the tick loop and into data: a `CellParameters` component or
  resource carrying at minimum nominal capacity, electrode area, exchange current density, transfer
  coefficients, series resistance, an OCV representation, and the entropy coefficient. After this
  item there must be **zero** chemistry literals inside `electrochemical_tick`.
- The OCV representation must be a tabulated or piecewise curve as a function of SOC, interpolated
  with the existing `eustress/crates/common/src/realism/numerics/interpolation.rs`. Keep
  `nernst_potential` available for chemistries that genuinely want it.
- Fix cycle counting by storing accumulated equivalent cycles as an `f32` (or `f64`) and deriving the
  integer count for display, so `capacity_retention_power_law` receives a real argument.
- Do not touch diffusion overpotential or temperature dependence. Those are `G2.09` and `G2.11`, and
  taking them here removes the headroom those items need to demonstrate improvement.

## 5. Exit criterion

### Criterion
RMS voltage error between the simulated constant-current discharge and the archived reference
points, over at least **50 reference points** at the validation C-rate, is **<= 150 mV**; and after a
simulated 10 full equivalent cycles the reported `cycle_count` is **>= 9 and <= 11** with
`capacity_retention` strictly less than 1.0.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin cell-validate
    ./target/release/cell-validate \
        --reference ../docs/PROMPTS/artifacts/G2.08/reference_points.csv \
        --params    ../docs/PROMPTS/artifacts/G2.08/cell_parameters.json \
        --c-rate 0.5 \
        --cycle-check-equivalent-cycles 10 \
        --out ../docs/PROMPTS/artifacts/G2.08/cell_validation_0d.json
    echo "EXIT_VAL=$?"

Expected output shape (`cell_validation_0d.json`):

    {
      "model": "0D_lumped",
      "reference": {
        "source": "<publication or repository name>",
        "url": "https://…",
        "retrieved": "2026-08-06",
        "licence": "<licence string>",
        "sha256": "sha256:…",
        "cell_nominal_capacity_ah": 0.0,
        "temperature_c": 25.0,
        "validation_c_rate": 0.5,
        "parameterisation_c_rate": 1.0,
        "point_count": 0
      },
      "error": {"rms_v": 0.000, "max_abs_v": 0.000, "mean_signed_v": 0.000},
      "residuals": [{"soc": 0.90, "ref_v": 0.000, "sim_v": 0.000, "err_v": 0.000}],
      "cycling": {"equivalent_cycles_simulated": 10.0, "reported_cycle_count": 0, "capacity_retention": 1.0},
      "chemistry_literals_in_tick": 0
    }

Pass condition:

    EXIT_VAL == 0
    AND ."reference".point_count >= 50
    AND ."error".rms_v <= 0.150
    AND ."cycling".reported_cycle_count >= 9 AND <= 11
    AND ."cycling".capacity_retention < 1.0
    AND ."chemistry_literals_in_tick" == 0
    AND ."reference".validation_c_rate != ."reference".parameterisation_c_rate

Read `rms_v` and `reported_cycle_count`. The existence of `cell_validation_0d.json` is not a pass.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**.

D5 will ask whether an electrochemist reading the residual series would accept this as a validated
0-D model. Prioritise: residuals that are small in the plateau and honestly large at the knees (that
is what 0-D lumped does, and pretending otherwise reads as a fit rather than a model); a clearly
stated provenance block for the reference data; and a `mean_signed_v` near zero, because a large
signed bias means the OCV curve is offset rather than the kinetics being wrong. Capture recipe:
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json`, scenario `cell_validation_ccd`.

The Critic never sees your self-report and must cite a specific value.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — tabulated OCV from the source + parameters lifted from the publication
   -> if still failing, MANDATORY approach change. Nudging series resistance is NOT an approach
      change; moving from published parameters to a least-squares fit on a disjoint C-rate is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D5 moving < 0.5 AND rms_v moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: no redistributable public dataset located (see front matter) — STALL before
                   writing model code, not after
```

The stall packet must request exactly one of: LOWER the RMS floor to a stated value with the stated
consequence for `G2.09`/`G2.10`; FUND approach D; DEFER behind a named item; KILL.

## 8. Artifact

`docs/PROMPTS/artifacts/G2.08/cell_validation_0d.json`

A reader finds: the model label, a full reference-data provenance block (source, URL, retrieval date,
licence, SHA-256, nominal capacity, temperature, both C-rates, point count), the three error metrics,
the full per-point residual series, the cycling check results, and a count of chemistry literals
remaining in the tick loop. Alongside it: `reference_points.csv` (the digitised reference) and
`cell_parameters.json` (the parameter set).

`error.rms_v` from this file is the number `G2.09` must beat by at least a factor of two. Every
later rung of the ladder cites it.

## 9. Definition of NOT done

- RMS clears 150 mV because the parameters were fitted on the same curve used for validation.
  `parameterisation_c_rate` and `validation_c_rate` differ for exactly this reason.
- The OCV is still `(1 - soc) / soc` fed to `nernst_potential` and the error was reduced by inflating
  series resistance to flatten the curve. Check `mean_signed_v` and the residual shape.
- `cycle_count` now increments because it is incremented by 1 every N ticks rather than from charge
  throughput, so it no longer means "equivalent full cycles".
- `capacity_retention` drops below 1.0 but so fast that the cell is dead in 50 cycles, because
  `alpha` and `beta` were left at the hard-coded `0.00005` / `0.5` while the chemistry changed.
- Chemistry literals move from `electrochemical_tick` into a `const` block in the same file and
  `chemistry_literals_in_tick` is reported as 0 on a technicality. The parameters must be reachable
  as component or resource data an agent can set with `set_sim_value`.
- The reference dataset is a screenshot digitised by eye with 12 points and the artifact claims 50.
````

---

## docs/PROMPTS/items/G2.09_single-particle-model-solid-phase-diffusion.md

````markdown
---
id: G2.09
title: Single-particle model with solid-phase diffusion and diffusion overpotential
workload: W1
workload_secondary: [W3]
phase: G2
depends_on: [G2.08, G1.12]
blocks: [G2.10]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G2.09/cell_validation_spm.json
escalation: >
  If the radial diffusion solve costs more than 0.5 ms per cell per tick at 20 radial nodes, STALL.
  A per-cell cost that high makes any multi-cell pack simulation impossible and the correct response
  is a decision about the discretisation, not a smaller node count chosen to hide the cost.
status: DRAFT
notes: >
  Second rung of the V-Cell ladder. The analytic short-time diffusion check is what stops this item
  from being "the curve fits better now" - it validates the solver independently of the cell data.
---

## 1. Objective

Solid-phase lithium (or sodium) transport is solved as 1-D radial Fickian diffusion inside a
representative particle per electrode, producing a surface-concentration-driven diffusion
overpotential that replaces the hard-coded zero in the terminal-voltage calculation. The solver is
validated twice: against the analytic solution for diffusion in a sphere, and against the same public
discharge dataset used in `G2.08`, at two C-rates.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**Where the ladder stands.** `docs/AUDIT/19_REALISM_PHYSICS.md` records the V-Cell model as "lumped
0-D … no spatial electrochemistry / ion transport". `G2.08` kept it 0-D but made it parameterised and
validated, and recorded its error in `docs/PROMPTS/artifacts/G2.08/cell_validation_0d.json` as
`error.rms_v`. Read that number before you start; it is your baseline and you must beat it by a
factor of two.

**The exact line you are replacing.** In `eustress/crates/engine/src/simulation/electrochemistry.rs`,
`electrochemical_tick` section 4:

```rust
echem_state.terminal_voltage = echem::terminal_voltage(
    ocv, eta_ohmic, eta_ct, 0.0, // no diffusion overpotential for now
    is_discharge,
);
```

The fourth argument is `eta_diff`. `G2.08` made it reachable; this item computes it.

**What the laws module already gives you.**
`eustress/crates/common/src/realism/laws/electrochemistry.rs` provides
`nernst_einstein_diffusivity(conductivity, concentration, z, temperature)` and
`nernst_planck_flux(diffusivity, concentration, conc_gradient, potential_gradient, z, temperature)`.
Solid-phase diffusion needs only Fick's second law in spherical coordinates; the Nernst-Planck form
is for the electrolyte and belongs to `G2.10`.

**Integrators available — reuse them.** `eustress/crates/common/src/realism/numerics/ode/` has
`euler.rs`, `runge_kutta.rs`, `verlet.rs`, `implicit.rs` (BDF).
`eustress/crates/common/src/realism/numerics/interpolation.rs` is what `G2.08` used for the OCV
table. Radial diffusion at realistic diffusivities is stiff at the surface; the implicit module
exists for this reason.

**The analytic reference.** For a sphere of radius `R` with uniform initial concentration and a
constant flux `j` applied at the surface from `t = 0`, the surface concentration for short times
(`Fo = D t / R^2 << 1`) approaches the semi-infinite result
`c_s(t) - c_0 = (2 j / F) * sqrt(t / (pi * D))`. This is a closed-form check that needs no dataset
and cannot go stale. Use it as the primary solver validation; the discharge-curve fit is secondary.

**Prerequisites already satisfied.** `G2.08` produced `cell_parameters.json` (including electrode
area and nominal capacity) and `reference_points.csv` with its provenance block. Reuse both — do not
re-source the dataset. `G2.08` also removed chemistry literals from the tick loop, so particle radius
and solid diffusivity belong in the same parameter structure.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/src/simulation/electrochemistry.rs`
- `eustress/crates/common/src/realism/laws/electrochemistry.rs` — additive only
- `eustress/crates/common/src/realism/particles/components.rs` — additive
- `eustress/crates/common/src/realism/numerics/` — additive
- `eustress/crates/engine/src/bin/` — extend `cell-validate`
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`
- `docs/PROMPTS/artifacts/G2.09/`

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.08/` — frozen, including `reference_points.csv`
- Electrolyte transport — that is `G2.10`; adding it here makes `G2.10` unmeasurable
- Temperature dependence of transport — that is `G2.11`
- `eustress/crates/engine/src/ui/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Re-digitising the reference, dropping the 1C validation point, changing the analytic tolerance, or
  reducing the node count to make the timing bound pass are all measurement changes. If the
  measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Two electrodes, one representative particle each, at least **20 radial nodes**. Record the node
  count in the artifact.
- The analytic check must be run at a Fourier number range that actually probes the short-time
  regime: sample `Fo` in `[1e-4, 1e-2]` with at least 20 points.
- Do not couple to the electrolyte. In an SPM the electrolyte is assumed uniform; that assumption is
  what `G2.10` removes, and keeping it here is what makes `G2.10`'s improvement attributable.
- Mass must be conserved in the particle: the integral of concentration over the sphere must track
  the integrated surface flux. Check it and report the closure error.
- The 0-D path must remain selectable, so `G2.08`'s number stays reproducible at this commit.

## 5. Exit criterion

### Criterion
Maximum relative error of simulated surface concentration versus the analytic short-time solution is
**<= 2%** across the sampled Fourier range; and discharge-curve RMS voltage error versus the `G2.08`
reference dataset is **<= 60 mV at 1C and <= 40 mV at 0.5C**, both at least a factor of two better
than `G2.08`'s recorded `error.rms_v`.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin cell-validate
    ./target/release/cell-validate \
        --model spm --radial-nodes 20 \
        --analytic-check sphere_short_time --fourier-range 1e-4,1e-2 --fourier-samples 20 \
        --reference ../docs/PROMPTS/artifacts/G2.08/reference_points.csv \
        --params    ../docs/PROMPTS/artifacts/G2.09/cell_parameters_spm.json \
        --c-rates 0.5,1.0 \
        --baseline  ../docs/PROMPTS/artifacts/G2.08/cell_validation_0d.json \
        --out ../docs/PROMPTS/artifacts/G2.09/cell_validation_spm.json
    echo "EXIT_SPM=$?"

Expected output shape:

    {
      "model": "SPM", "radial_nodes": 20, "integrator": "BDF1",
      "analytic": {
        "case": "sphere_short_time_constant_flux",
        "fourier_range": [1e-4, 1e-2], "samples": 20,
        "max_rel_error": 0.000, "pass": true
      },
      "discharge": [
        {"c_rate": 0.5, "rms_v": 0.000, "max_abs_v": 0.000, "point_count": 0},
        {"c_rate": 1.0, "rms_v": 0.000, "max_abs_v": 0.000, "point_count": 0}
      ],
      "baseline_0d_rms_v": 0.000,
      "improvement_factor_at_0p5c": 0.00,
      "particle_mass_closure_error": 0.0000,
      "solve_ms_per_cell_per_tick": 0.00
    }

Pass condition:

    EXIT_SPM == 0
    AND ."analytic".max_rel_error <= 0.02
    AND discharge[c_rate=1.0].rms_v <= 0.060
    AND discharge[c_rate=0.5].rms_v <= 0.040
    AND ."improvement_factor_at_0p5c" >= 2.0
    AND ."particle_mass_closure_error" <= 0.001
    AND ."solve_ms_per_cell_per_tick" <= 0.5
    AND ."radial_nodes" >= 20

Read the emitted errors. Do not infer success from the JSON existing.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**.

D5 will ask whether the improvement is physical rather than cosmetic. Prioritise: the diffusion
overpotential growing with C-rate in the correct direction and magnitude; end-of-discharge voltage
collapse appearing at the right SOC because surface concentration saturates, not because a fit
parameter was tuned; and reporting the particle mass-closure error, which is the check that separates
a working solver from a plausible-looking one. Capture recipe:
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json`.

The Critic never sees your self-report and must cite a specific value.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — uniform radial finite-volume grid with an implicit (BDF) step
   -> if still failing, MANDATORY approach change. Adding nodes is NOT an approach change; moving
      from a uniform grid to a surface-refined grid, or to a polynomial/eigenfunction approximation
      of the particle, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D5 moving < 0.5 AND rms_v at 1C moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: solve_ms_per_cell_per_tick > 0.5 at 20 nodes (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.09/cell_validation_spm.json`

A reader finds: the model label, radial node count and integrator, the analytic sphere check with its
Fourier range and maximum relative error, discharge RMS and maximum error at both C-rates with point
counts, the `G2.08` baseline RMS and the computed improvement factor, the particle mass-closure
error, and the per-cell per-tick solve cost. Alongside it: `cell_parameters_spm.json` and the
analytic-comparison series.

## 9. Definition of NOT done

- The discharge fit improves but the analytic sphere check is absent or fails. Fitting a curve is not
  solving a PDE, and the analytic case is the only check here that cannot be gamed.
- 1C passes and 0.5C regresses relative to `G2.08`. Both bounds are simultaneous.
- `improvement_factor_at_0p5c` clears 2.0 because the `G2.08` baseline was regenerated with worse
  parameters at this commit. The baseline file is frozen and must be read, not recomputed.
- The node count is dropped to 5 so `solve_ms_per_cell_per_tick` clears 0.5. The node floor and the
  timing bound are both binding.
- Electrolyte concentration effects are quietly added to make 1C fit, which is `G2.10`'s work and
  destroys `G2.10`'s ability to show an improvement.
- `particle_mass_closure_error` is not reported, so a solver that leaks lithium looks fine on the
  voltage curve for the first 80% of discharge.
````

---

## docs/PROMPTS/items/G2.10_p2d-electrolyte-transport.md

````markdown
---
id: G2.10
title: P2D electrolyte transport (Doyle-Fuller-Newman)
workload: W1
workload_secondary: [W3]
phase: G2
depends_on: [G2.09, G1.12]
blocks: [G2.11, G2.12]
tier: XL
token_envelope: 1200000
wallclock_envelope: 5d
max_builds: 20
critic_gate: [D5, D6]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G2.10/cell_validation_p2d.json
escalation: >
  If the coupled electrolyte-plus-particle solve fails to converge (Newton residual not decreasing)
  at 2C on more than 1% of timesteps, STALL. A model that silently falls back to the previous
  timestep's solution is worse than no model, because its outputs still look plausible.
status: DRAFT
notes: >
  Tier XL: this is a new subsystem - a coupled nonlinear PDE solve across three regions - not a
  change to an existing one. Third rung of the V-Cell ladder and the item that retires the
  "no spatial electrochemistry" finding in docs/AUDIT/19_REALISM_PHYSICS.md.
---

## 1. Objective

Electrolyte concentration and potential are solved as 1-D fields through the anode, separator, and
cathode, coupled to the per-electrode particle diffusion from `G2.09` through Butler-Volmer kinetics.
The resulting pseudo-two-dimensional model reproduces the reference cell's discharge curve at 0.5C,
1C, and 2C within 30 mV RMS, and at 2C it reduces RMS error by at least 40% relative to the SPM —
the regime where electrolyte limitation is the dominant physics.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**The finding this item retires.** `docs/AUDIT/19_REALISM_PHYSICS.md` P4: V-Cell "Nernst +
Butler-Volmer are **lumped 0-D models** (no spatial electrochemistry / ion transport)". Feature 5
risk R5.1: "Simplified model — lumped 1-D RC thermal, no spatial gradients." After this item,
spatial ion transport exists in the through-thickness direction and the audit line must be updated
by whoever closes the item — with the measured error bound, not with an adjective.

**Where the ladder stands.**
- `docs/PROMPTS/artifacts/G2.08/cell_validation_0d.json` — 0-D lumped, `error.rms_v` at 0.5C.
- `docs/PROMPTS/artifacts/G2.09/cell_validation_spm.json` — SPM with radial solid diffusion, RMS at
  0.5C and 1C, plus the analytic sphere check and `solve_ms_per_cell_per_tick`.
Both files are frozen. Read them; do not regenerate them.

**What exists to build on.**
- `eustress/crates/common/src/realism/laws/electrochemistry.rs` — `butler_volmer_current(j0, eta,
  alpha_a, alpha_c, temperature)`, `butler_volmer_symmetric`, `exchange_current_density(k0, c_ox,
  c_red, alpha)`, `nernst_planck_flux(diffusivity, concentration, conc_gradient,
  potential_gradient, z, temperature)`, `nernst_einstein_diffusivity`, `electrolyte_asr(thickness,
  ionic_conductivity)`, `cell_resistance_from_asr`, `terminal_voltage(ocv, eta_ohmic, eta_ct,
  eta_diff, is_discharge)`. **Do not rewrite these.** `nernst_planck_flux` is exactly the electrolyte
  flux law you need.
- `eustress/crates/common/src/realism/numerics/ode/implicit.rs` — BDF. A DFN solve is a coupled
  differential-algebraic system; explicit stepping will not hold at 2C.
- `eustress/crates/common/src/realism/symbolic/` — `solver.rs`, `nonlinear.rs`, `resolver.rs`,
  `causal.rs`, `codegen.rs`, `expressions.rs`. `docs/AUDIT/19_REALISM_PHYSICS.md` P4 records
  Symbolica as **partially wired** (a `use symbolica::atom::Atom` in `causal.rs` plus a feature flag);
  do not assume a working symbolic solver. If you use `nonlinear.rs`, verify it first and say so.
- `eustress/crates/engine/src/simulation/electrochemistry.rs` — the tick system that owns the
  per-frame integration and publishes `battery.*` keys to `SimValuesResource` (function
  `publish_echem_to_sim_values`).

**The physics you are implementing, stated so it is unambiguous.** Three regions along `x`: negative
electrode (thickness `L_n`), separator (`L_s`), positive electrode (`L_p`). Two electrolyte fields —
salt concentration `c_e(x,t)` and potential `phi_e(x,t)`. Two solid fields — potential `phi_s(x,t)`
in each electrode, and the `G2.09` radial concentration `c_s(r,x,t)` at each electrode node (this is
the "pseudo-two-dimensional" part). Local current density couples them through Butler-Volmer with
overpotential `eta = phi_s - phi_e - U(c_s_surface)`. Charge conservation ties the integrated local
current to the applied cell current.

**Prerequisites already satisfied.** `G2.09` delivered the radial particle solver and the analytic
sphere validation. `G2.08` delivered `reference_points.csv` with its provenance and licence block,
and `cell_parameters.json`. All three are frozen inputs.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`, not `cargo check` — a coupled solver that fails to converge still type-checks.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/common/src/realism/laws/electrochemistry.rs` — additive only
- `eustress/crates/common/src/realism/particles/` — a new `p2d` module is permitted
- `eustress/crates/common/src/realism/numerics/` — additive linear-algebra or Newton helpers
- `eustress/crates/engine/src/simulation/electrochemistry.rs`
- `eustress/crates/engine/src/bin/` — extend `cell-validate`
- `eustress/crates/common/Cargo.toml` — a sparse-linear-algebra dependency is permitted with a
  one-line justification in the result block
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`
- `docs/PROMPTS/artifacts/G2.10/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.09/` — frozen, including `reference_points.csv`
- Temperature dependence of transport properties — that is `G2.11`
- Mesh/timestep convergence study — that is `G2.12`, deliberately separated so this item cannot
  self-certify its own discretisation
- `eustress/crates/engine/src/ui/`
- `eustress/crates/common/src/physics/` — no Avian changes

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Dropping the 2C case, re-digitising the reference, widening the SOC comparison window, or reporting
  a median instead of RMS are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The solver must **report non-convergence, never silently accept it**. Every timestep records its
  Newton iteration count and final residual; the artifact reports the fraction of timesteps that hit
  the iteration cap. A fallback to the previous solution without recording it is the failure this
  item's escalation trigger exists to catch.
- Minimum discretisation: at least **10 nodes per region** (30 total through-thickness) and the
  `G2.09` radial node count at each electrode node. Record both.
- The SPM path from `G2.09` and the 0-D path from `G2.08` must remain selectable at this commit, so
  all three rungs are reproducible from one binary. That is also how `improvement_vs_spm` is
  computed honestly.
- Charge conservation is a hard invariant: the integral of local current density over each electrode
  must equal the applied cell current to within 0.1% every timestep. Report the worst closure error.
- Isothermal only. Every transport property is evaluated at the reference temperature stated in
  `G2.08`'s provenance block. `G2.11` removes that assumption.

## 5. Exit criterion

### Criterion
Discharge-curve RMS voltage error versus the `G2.08` reference dataset is **<= 30 mV at each of
0.5C, 1C, and 2C**; RMS at 2C is at least **40% lower** than the SPM value recorded in
`docs/PROMPTS/artifacts/G2.09/cell_validation_spm.json`; and the fraction of timesteps failing to
converge is **0**.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin cell-validate
    ./target/release/cell-validate \
        --model p2d \
        --nodes-per-region 10 --radial-nodes 20 \
        --reference ../docs/PROMPTS/artifacts/G2.08/reference_points.csv \
        --params    ../docs/PROMPTS/artifacts/G2.10/cell_parameters_p2d.json \
        --c-rates 0.5,1.0,2.0 \
        --baseline-spm ../docs/PROMPTS/artifacts/G2.09/cell_validation_spm.json \
        --baseline-0d  ../docs/PROMPTS/artifacts/G2.08/cell_validation_0d.json \
        --out ../docs/PROMPTS/artifacts/G2.10/cell_validation_p2d.json
    echo "EXIT_P2D=$?"

Expected output shape:

    {
      "model": "P2D_DFN",
      "discretisation": {"nodes_negative": 10, "nodes_separator": 10, "nodes_positive": 10, "radial_nodes": 20},
      "solver": {"scheme": "BDF1 + Newton", "max_newton_iters": 25,
                 "nonconverged_timestep_fraction": 0.0, "worst_final_residual": 0.0},
      "discharge": [
        {"c_rate": 0.5, "rms_v": 0.000, "max_abs_v": 0.000, "point_count": 0},
        {"c_rate": 1.0, "rms_v": 0.000, "max_abs_v": 0.000, "point_count": 0},
        {"c_rate": 2.0, "rms_v": 0.000, "max_abs_v": 0.000, "point_count": 0}
      ],
      "baselines": {"spm_rms_v_at_2c": 0.000, "zero_d_rms_v_at_0p5c": 0.000},
      "improvement_vs_spm_at_2c": 0.00,
      "charge_closure_worst_rel_error": 0.0000,
      "electrolyte_depletion_min_c_e_frac_at_2c": 0.00,
      "solve_ms_per_cell_per_tick": 0.00
    }

Pass condition:

    EXIT_P2D == 0
    AND every discharge[*].rms_v <= 0.030
    AND ."improvement_vs_spm_at_2c" >= 0.40
    AND ."solver".nonconverged_timestep_fraction == 0.0
    AND ."charge_closure_worst_rel_error" <= 0.001
    AND ."discretisation".nodes_negative >= 10 AND .nodes_separator >= 10 AND .nodes_positive >= 10

Read the emitted RMS values and the non-convergence fraction. File existence is not a pass.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)** and **D6 (overall coherence)**, floor
**8.0 each**. The mean is irrelevant; either below 8.0 fails the item.

D5 will ask whether an electrochemist would recognise the internal state as a DFN solution and not
just a better fit. Prioritise: an electrolyte concentration profile that depletes at the electrode
furthest from the separator and does so more at 2C than at 0.5C
(`electrolyte_depletion_min_c_e_frac_at_2c` is in the artifact for this reason); a local current
density distribution that is non-uniform through the electrode thickness; and honest reporting of the
Newton residual.

D6 is gated because a P2D model that lives beside a 0-D thermal model, a 0-D degradation model, and a
hard-coded dendrite risk term is three unrelated fidelity levels in one component. State in the
result block which parts of `ElectrochemicalState` are now spatially resolved and which are not, so a
reader is not misled about what the number covers.

Capture recipe: `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`. The Critic never sees your
self-report and must cite a specific value.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — finite-volume discretisation, fully coupled Newton on the whole system
   -> if still failing, MANDATORY approach change. Refining the mesh is NOT an approach change;
      moving from a monolithic Newton solve to an operator-split (Gauss-Seidel between electrolyte
      and particles) scheme is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND rms_v
                  at 2C moving < 5%
  - Budget      : 1.8M tokens or 30 builds consumed (150% of the XL envelope)
  - Item-specific: nonconverged_timestep_fraction > 0.01 at 2C (see front matter)
```

The stall packet must request exactly one of: LOWER the RMS floor to a stated value with the stated
consequence for `G2.11` and `G2.13`; FUND approach D; DEFER behind a named item; KILL with a
statement of what the platform loses (concretely: the "spatial electrochemistry" gap in
`docs/AUDIT/19_REALISM_PHYSICS.md` stays open).

## 8. Artifact

`docs/PROMPTS/artifacts/G2.10/cell_validation_p2d.json`

A reader finds: the model label; the full discretisation (nodes per region and radial nodes); the
solver scheme, iteration cap, non-converged timestep fraction, and worst residual; RMS and maximum
error at all three C-rates with point counts; the frozen SPM and 0-D baselines and the computed
improvement factor; the worst charge-closure error; the minimum electrolyte concentration fraction
reached at 2C; and the per-cell per-tick solve cost. Alongside it: `cell_parameters_p2d.json` and the
spatial field snapshots at 10%, 50%, and 90% depth of discharge at 2C.

## 9. Definition of NOT done

- All three RMS bounds clear but `improvement_vs_spm_at_2c` is below 0.40, meaning the electrolyte
  model contributed nothing and the gain came from re-tuning parameters. Both conditions bind.
- `nonconverged_timestep_fraction` is 0.0 because non-convergence is not detected, only silently
  accepted. Report the Newton iteration histogram alongside it so this is visible.
- The electrolyte fields are solved but `electrolyte_depletion_min_c_e_frac_at_2c` is 1.00, i.e. the
  concentration never moves — the transport coefficients are so large the field is uniform and the
  model is an SPM with extra cost.
- Node counts are set to 3 per region to make the solve converge and the artifact reports them
  honestly but the floor of 10 is missed.
- The 0-D and SPM code paths are deleted, so `G2.08` and `G2.09` can no longer be reproduced at this
  commit and the improvement factors cannot be re-derived by a stranger.
- Temperature dependence is added to make 2C fit. That is `G2.11`'s subject and removes its headroom.
````

---

## docs/PROMPTS/items/G2.11_thermal-coupling-temperature-dependent-transport.md

````markdown
---
id: G2.11
title: Thermal coupling with temperature-dependent transport, validated on a temperature sweep
workload: W1
workload_secondary: [W3]
phase: G2
depends_on: [G2.10, G1.12]
blocks: [G2.13]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G2.11/thermal_coupling.json
escalation: >
  If the coupled electro-thermal solve produces a temperature that rises without bound at 2C on any
  tested ambient, STALL immediately. A runaway that is a numerical artifact rather than physics is
  the most dangerous possible output of this subsystem, because thermal runaway is exactly what a
  customer would use it to predict.
status: DRAFT
notes: >
  Fourth rung of the V-Cell ladder. This is the item that retires the "ElectrochemicalState and
  ThermodynamicState are decoupled" finding - and the honest version of that finding is narrower
  than the audit line suggests, which the body states precisely.
---

## 1. Objective

Transport and kinetic properties depend on temperature through Arrhenius relations, cell heat
generation drives a thermal state, and the thermal state feeds back into the electrochemistry within
one timestep. The coupled model reproduces measured discharge capacity across an ambient-temperature
sweep to within 5%, and predicted surface temperature rise at 2C to within 3 K of the cited measured
value.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**The finding, stated precisely.** `docs/AUDIT/19_REALISM_PHYSICS.md` P4 records: "Particle ECS
`ElectrochemicalState` + `ThermodynamicState` are **decoupled** (no thermal-effect-on-reaction-rate
coupling)." Read the source before you act on that. In
`eustress/crates/engine/src/simulation/electrochemistry.rs`, `electrochemical_tick`:

- section 1 reads `let temperature = thermo.as_ref().map(|t| t.temperature).unwrap_or(298.15);` and
  passes it into `echem::nernst_potential(...)`;
- section 3 passes the same temperature into `echem::tafel_overpotential(...)`;
- section 7 writes heat back: `let thermal_mass = 0.45 * 900.0;` then
  `thermo_state.temperature += echem_state.heat_generation * dt / thermal_mass;` followed by passive
  cooling with `let r_thermal = 2.0_f32;` toward `let ambient = 298.15_f32;`.

So a two-way coupling exists at the lumped level. What is genuinely missing is narrower and more
consequential:

1. **Transport properties are temperature-independent.** `internal_resistance` is a constant field
   read straight from `ElectrochemicalState`; `echem::arrhenius_conductivity(sigma0, e_act,
   temperature)` exists in `eustress/crates/common/src/realism/laws/electrochemistry.rs` and is never
   called. Solid and electrolyte diffusivities from `G2.09` and `G2.10` likewise have no temperature
   dependence.
2. **Exchange current density is temperature-independent.** After `G2.08` it is a parameter rather
   than the literal `let j0 = 50.0_f32;` that used to sit in the loop, but it still has no Arrhenius
   term.
3. **Thermal parameters are hard-coded literals.** `0.45 * 900.0` (mass times specific heat) and
   `r_thermal = 2.0` are baked into the tick for one specific cell. Ambient is hard-coded 298.15 K,
   and `thermo_state.temperature = thermo_state.temperature.max(ambient)` clamps the cell so it can
   never be colder than 25 C — which makes any sub-ambient sweep point meaningless.
4. **Entropic heat uses a single chemistry constant.** Section 6 calls
   `echem::entropic_heat(temperature, current, constants::na_s::ENTROPY_COEFFICIENT)`, so the
   entropy coefficient is not per-parameter-set and is not a function of SOC.

**Where the ladder stands.** `docs/PROMPTS/artifacts/G2.10/cell_validation_p2d.json` records the
isothermal P2D model's RMS at 0.5C, 1C, and 2C, its discretisation, and its solver convergence
statistics. It is frozen. `G2.08`'s `reference_points.csv` and its provenance block, including the
reference temperature, are also frozen.

**The temperature sweep reference.** You must extend the `G2.08` reference archive with discharge
data at **at least three ambient temperatures spanning at least 40 K** (for example 0 C, 25 C,
45 C), from the same public source where possible. The same provenance requirements apply: public
URL, retrieval date, licence permitting redistribution of the digitised points (or a checksum if it
does not), and at least 30 points per temperature. If the surface-temperature-rise measurement comes
from a different publication than the discharge curves, cite both separately.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/src/simulation/electrochemistry.rs`
- `eustress/crates/common/src/realism/laws/electrochemistry.rs` — additive only
- `eustress/crates/common/src/realism/particles/components.rs` — `ThermodynamicState` and the
  parameter structures
- `eustress/crates/common/src/realism/thermal_conduction.rs` — read it first; it provides
  `ThermalContact`, `ThermalConductionConfig`, `thermal_conduction_system`,
  `auto_thermal_contacts_system`, `ThermalConductionPlugin`. Reuse rather than duplicating.
- `eustress/crates/engine/src/bin/` — extend `cell-validate`
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`
- `docs/PROMPTS/artifacts/G2.11/`

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.10/` — frozen
- The P2D spatial discretisation itself — you may evaluate its coefficients at a temperature; you may
  not change its node layout. That is `G2.12`.
- `eustress/crates/engine/src/ui/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Dropping the coldest sweep point, narrowing the temperature span below 40 K, relaxing the 3 K
  surface-rise bound, or comparing predicted capacity against a model rather than against measured
  data are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Remove the `.max(ambient)` clamp on cell temperature. A cell in a 0 C chamber must be able to sit
  at 0 C. Leaving that clamp makes the cold sweep point untestable.
- Thermal parameters (mass, specific heat, thermal resistance, ambient) become data, not literals.
  After this item there must be **zero** thermal literals in `electrochemical_tick`.
- Every Arrhenius term must state its activation energy in the parameter file with a source. An
  activation energy chosen to make a curve fit, with no citation, fails D5 regardless of the number.
- Coupling must be within-timestep: the temperature used to evaluate transport at step `n` is the
  temperature resulting from step `n-1`'s heat at minimum, and the artifact must say which scheme
  (explicit lag or fully coupled) was used.
- The isothermal path must remain selectable, so `G2.10`'s numbers stay reproducible at this commit.

## 5. Exit criterion

### Criterion
Predicted discharge capacity at each of at least three ambient temperatures spanning at least 40 K is
within **5%** of the measured value; predicted peak surface temperature rise at 2C is within **3 K**
of the cited measured value; and the isothermal 25 C RMS is no worse than `G2.10`'s recorded value
plus 5 mV.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin cell-validate
    ./target/release/cell-validate \
        --model p2d --thermal coupled \
        --reference-sweep ../docs/PROMPTS/artifacts/G2.11/reference_temperature_sweep.csv \
        --reference-thermal ../docs/PROMPTS/artifacts/G2.11/reference_surface_temp_2c.csv \
        --params ../docs/PROMPTS/artifacts/G2.11/cell_parameters_thermal.json \
        --ambients-c 0,25,45 --c-rates 0.5,2.0 \
        --baseline-p2d ../docs/PROMPTS/artifacts/G2.10/cell_validation_p2d.json \
        --out ../docs/PROMPTS/artifacts/G2.11/thermal_coupling.json
    echo "EXIT_THERM=$?"

Expected output shape:

    {
      "coupling_scheme": "explicit_lag_one_step",
      "thermal_literals_in_tick": 0,
      "arrhenius_terms": [
        {"property": "electrolyte_conductivity", "e_act_j_per_mol": 0.0, "source": "…"},
        {"property": "solid_diffusivity",        "e_act_j_per_mol": 0.0, "source": "…"},
        {"property": "exchange_current_density", "e_act_j_per_mol": 0.0, "source": "…"}
      ],
      "capacity_sweep": [
        {"ambient_c": 0.0,  "measured_capacity_ah": 0.0, "simulated_capacity_ah": 0.0, "rel_error": 0.000},
        {"ambient_c": 25.0, "measured_capacity_ah": 0.0, "simulated_capacity_ah": 0.0, "rel_error": 0.000},
        {"ambient_c": 45.0, "measured_capacity_ah": 0.0, "simulated_capacity_ah": 0.0, "rel_error": 0.000}
      ],
      "temperature_span_k": 45.0,
      "surface_temp_rise_2c": {"measured_k": 0.0, "simulated_k": 0.0, "abs_error_k": 0.0},
      "isothermal_25c_rms_v": 0.000,
      "baseline_p2d_rms_v_at_25c": 0.000,
      "runaway_detected": false
    }

Pass condition:

    EXIT_THERM == 0
    AND every capacity_sweep[*].rel_error <= 0.05
    AND ."temperature_span_k" >= 40.0
    AND ."surface_temp_rise_2c".abs_error_k <= 3.0
    AND ."isothermal_25c_rms_v" <= ."baseline_p2d_rms_v_at_25c" + 0.005
    AND ."thermal_literals_in_tick" == 0
    AND ."runaway_detected" == false

Read the relative errors and the temperature error. File existence is not a pass.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**.

D5 will ask whether a battery engineer would trust this to predict thermal behaviour. Prioritise:
capacity falling at low temperature for the right reason (transport slowing, visible in the
overpotential breakdown, not a fitted capacity multiplier); the temperature trace showing the
characteristic rise-then-plateau as generation balances cooling; and every activation energy carrying
a citation. An unsourced activation energy is the single fastest way to fail this dimension. Capture
recipe: `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`.

The Critic never sees your self-report and must cite a specific value.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — Arrhenius scaling on transport + lumped thermal with data-driven params
   -> if still failing, MANDATORY approach change. Re-fitting an activation energy is NOT an approach
      change; moving from a lumped thermal node to a multi-node thermal model with a jelly-roll
      conduction path (via realism/thermal_conduction.rs) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D5 moving < 0.5 AND the worst capacity rel_error
                  moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: unbounded temperature rise at 2C on any ambient (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.11/thermal_coupling.json`

A reader finds: the coupling scheme, a zero count of thermal literals remaining in the tick, every
Arrhenius term with its activation energy and source, the capacity sweep with measured and simulated
values and relative errors at each ambient, the temperature span covered, the 2C surface-temperature
rise comparison, the isothermal 25 C RMS against the frozen `G2.10` baseline, and a runaway flag.
Alongside it: the two reference CSVs with their provenance blocks and
`cell_parameters_thermal.json`.

This artifact is the evidence that retires "no thermal-effect-on-reaction-rate coupling". Whoever
closes the item updates `docs/AUDIT/19_REALISM_PHYSICS.md` with the measured bounds — not with an
adjective, and with no changelog residue in the document body.

## 9. Definition of NOT done

- Capacity matches at all three ambients because a temperature-dependent capacity multiplier was
  fitted directly, rather than emerging from transport slowing down.
- The `.max(ambient)` clamp survives, so the 0 C point is simulated at 25 C and passes for the wrong
  reason. Grep the tick for `.max(` before claiming this item.
- Activation energies appear in the parameter file with `"source": "estimated"`.
- The 2C surface-temperature rise matches because `r_thermal` was fitted to that single measurement,
  and the same `r_thermal` produces a nonsensical result at 0.5C. Report both C-rates.
- The isothermal path is deleted, so `G2.10`'s frozen RMS cannot be reproduced at this commit.
- `runaway_detected` is false because runaway detection was never implemented, rather than because no
  runaway occurred.
````

---

## docs/PROMPTS/items/G2.12_p2d-numerical-convergence-and-charge-conservation.md

````markdown
---
id: G2.12
title: P2D numerical convergence and charge conservation
workload: W3
workload_secondary: [W1]
phase: G2
depends_on: [G2.10]
blocks: [G2.13, G5.23]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G2.12/p2d_convergence.json
escalation: >
  If the observed spatial order of convergence is below 0.5 (i.e. refining the mesh barely changes
  the answer in the wrong way, or changes it erratically), STALL. That signature means the
  discretisation is inconsistent, and no amount of refinement will fix it.
status: DRAFT
notes: >
  Deliberately separated from G2.10 so the model cannot certify its own discretisation. Tier M -
  it adds no physics, only a study.
---

## 1. Objective

The P2D model's discretisation is shown to converge: halving the through-thickness node spacing
changes terminal voltage by at most 2 mV, halving the timestep changes it by at most 2 mV, the
observed spatial order of convergence is at least 1.0, and total charge closes to within 0.1% over a
full discharge. The mesh and timestep shipped by default are justified by this study rather than
chosen by feel.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**Why a separate item.** `G2.10` built the pseudo-two-dimensional electrolyte-plus-particle model and
validated it against measured discharge curves at 0.5C, 1C, and 2C. A model can fit measured data
while being under-resolved — the discretisation error and the parameter error cancel. This item is
the independent check, and it is deliberately not run by the same agent that chose the mesh.

**What is frozen and must be read, not regenerated.**
`docs/PROMPTS/artifacts/G2.10/cell_validation_p2d.json` records the shipped discretisation
(`nodes_negative`, `nodes_separator`, `nodes_positive`, `radial_nodes`), the solver scheme and its
non-convergence fraction, the RMS errors at all three C-rates, and
`charge_closure_worst_rel_error`. `docs/PROMPTS/artifacts/G2.08/reference_points.csv` is the
reference dataset with its provenance and licence block.

**The definitions, stated so there is no ambiguity.**

- **Grid convergence.** Run the model at node counts `N`, `2N`, `4N` per region with everything else
  fixed. The observed order `p` is
  `p = log2( |V_N - V_2N| / |V_2N - V_4N| )`, evaluated on the terminal voltage at a fixed depth of
  discharge (50%). Report `p` and the pairwise differences.
- **Timestep convergence.** The same procedure with the timestep halved twice at fixed mesh.
- **Charge closure.** The integral of applied current over the discharge, minus the change in total
  lithium (or sodium) inventory across both electrodes expressed in ampere-hours, divided by the
  nominal capacity.

**Where the code is.** The model lives in the `p2d` module under
`eustress/crates/common/src/realism/particles/` and is driven by
`eustress/crates/engine/src/simulation/electrochemistry.rs`. The validation binary is
`cell-validate` in `eustress/crates/engine/src/bin/`.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/src/bin/` — extend `cell-validate` with a `--convergence-study` mode
- `eustress/crates/common/src/realism/particles/` — **only** to expose mesh and timestep as
  parameters if they are not already; no change to the discretisation scheme
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`
- `docs/PROMPTS/artifacts/G2.12/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.11/` — frozen
- The P2D discretisation scheme and the physics. If the study shows the scheme is inconsistent, that
  is a finding and a STALL, not a licence to rewrite `G2.10`.
- `eustress/crates/engine/src/ui/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Evaluating convergence at a depth of discharge where the curve is flattest, using a coarser
  starting `N` so the differences look small, or reporting only the finest-mesh result are all
  measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The convergence study must start from the **shipped** node count recorded in `G2.10`'s artifact,
  not from a coarser one chosen to make refinement look dramatic.
- Evaluate at 2C. Convergence at 0.5C proves nothing, because at low rate the gradients that the mesh
  resolves are nearly absent.
- If the study shows the shipped mesh is too coarse, the correct outcome is to raise the shipped
  default and re-run `G2.10`'s validation command to confirm the RMS bounds still hold — report both
  numbers. Do not raise the mesh without re-checking the fit.
- Charge closure is measured over a **full** discharge from SOC 1.0 to the cutoff voltage, not over a
  window.

## 5. Exit criterion

### Criterion
At 2C and 50% depth of discharge: `|V_2N - V_4N| <= 2.0 mV`, `|V_dt/2 - V_dt/4| <= 2.0 mV`, observed
spatial order of convergence `p >= 1.0`, and full-discharge charge closure `<= 0.1%`.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin cell-validate
    ./target/release/cell-validate \
        --convergence-study \
        --shipped-config ../docs/PROMPTS/artifacts/G2.10/cell_validation_p2d.json \
        --c-rate 2.0 --evaluate-at-dod 0.5 \
        --mesh-refinements 3 --timestep-refinements 3 \
        --params ../docs/PROMPTS/artifacts/G2.10/cell_parameters_p2d.json \
        --out ../docs/PROMPTS/artifacts/G2.12/p2d_convergence.json
    echo "EXIT_CONV=$?"

Expected output shape:

    {
      "c_rate": 2.0, "evaluated_at_dod": 0.5,
      "shipped_nodes_per_region": 10, "shipped_timestep_s": 0.0,
      "mesh": [
        {"nodes_per_region": 10, "terminal_v": 0.000000},
        {"nodes_per_region": 20, "terminal_v": 0.000000},
        {"nodes_per_region": 40, "terminal_v": 0.000000}
      ],
      "mesh_diff_N_2N_v": 0.000000,
      "mesh_diff_2N_4N_v": 0.000000,
      "observed_spatial_order": 0.00,
      "timestep": [
        {"dt_s": 0.0, "terminal_v": 0.000000},
        {"dt_s": 0.0, "terminal_v": 0.000000},
        {"dt_s": 0.0, "terminal_v": 0.000000}
      ],
      "timestep_diff_half_quarter_v": 0.000000,
      "charge_closure_rel_error_full_discharge": 0.00000,
      "shipped_config_adequate": true
    }

Pass condition:

    EXIT_CONV == 0
    AND ."mesh_diff_2N_4N_v" <= 0.002
    AND ."timestep_diff_half_quarter_v" <= 0.002
    AND ."observed_spatial_order" >= 1.0
    AND ."charge_closure_rel_error_full_discharge" <= 0.001
    AND ."c_rate" == 2.0

Read the emitted differences and the order. File existence is not a pass.

## 6. Critic gate

`critic_gate: []`. Convergence is arithmetic: the differences either shrink at the expected rate or
they do not, and there is no perceptual component. The mechanical criterion carries all of the
weight, which is why it constrains four independent quantities — two differences, an order, and a
conservation closure — rather than one.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — uniform refinement of the shipped mesh and timestep
   -> if still failing, MANDATORY approach change. Adding one more refinement level is NOT an
      approach change; switching the refinement study to a manufactured-solution (MMS) test, where
      the exact answer is known by construction, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with observed_spatial_order moving < 0.1
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: observed_spatial_order < 0.5 (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.12/p2d_convergence.json`

A reader finds: the C-rate and depth of discharge at which convergence was evaluated, the shipped
node count and timestep, the three-level mesh sequence with terminal voltages, the two pairwise
differences, the observed spatial order, the three-level timestep sequence with its difference, the
full-discharge charge-closure error, and a boolean stating whether the shipped configuration is
adequate. If it is not, the artifact also records the raised configuration and the re-run `G2.10`
RMS values.

This is the file that lets a stranger say the P2D result is a solution to the equations rather than a
property of the grid.

## 9. Definition of NOT done

- Convergence is demonstrated at 0.5C, where the gradients the mesh exists to resolve are absent.
- `observed_spatial_order` is reported but computed from the wrong pair of differences, so a
  first-order scheme reports second-order convergence.
- The study starts at 3 nodes per region rather than the shipped 10, making the refinement look
  decisive while saying nothing about what ships.
- The mesh is found inadequate, is raised, and `G2.10`'s discharge validation is not re-run, so
  nobody knows whether the published RMS bounds still hold at the shipped configuration.
- Charge closure is measured over the SOC 0.9-0.1 window rather than the full discharge, excluding
  the end-of-discharge region where inventory bookkeeping errors concentrate.
- The timestep study is run at a fixed mesh that is finer than the shipped one, so the two studies
  describe a configuration nobody uses.
````

---

## docs/PROMPTS/items/G2.13_p2d-under-time-compression-performance-envelope.md

````markdown
---
id: G2.13
title: P2D under time compression — performance envelope with the error bound held
workload: W1
workload_secondary: [W6]
phase: G2
depends_on: [G2.11, G2.12, G2.05, G1.12]
blocks: []
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G2.13/p2d_compression_envelope.json
escalation: >
  If holding the 5 mV compression error bound at any usable time_scale requires per-cell cost above
  2.0 ms per tick, STALL. At that cost a 100-cell pack cannot run faster than real time and the
  compression story does not exist for the P2D model, which is a scope decision, not a tuning one.
status: DRAFT
notes: >
  Final rung of the V-Cell ladder. It is the item that converts the model into a claim the business
  can make: "N simulated hours per wall-clock minute, at M mV of error."
---

## 1. Objective

The thermally coupled P2D model runs under time compression with a stated, measured error bound, and
the supported compression envelope is published: for each tested `time_scale`, the RMS terminal
voltage deviation from the uncompressed reference, the wall-clock time for a full discharge, and the
number of cells that can be simulated concurrently at that scale.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**What the ladder has established, all frozen — read, do not regenerate.**
- `docs/PROMPTS/artifacts/G2.05/compression_error.json` — the compression error bound for the *0-D*
  model, its `supported_max_scale_at_5mv`, and the per-scale wall times.
- `docs/PROMPTS/artifacts/G2.10/cell_validation_p2d.json` — the isothermal P2D discretisation, solver
  statistics, RMS at 0.5C / 1C / 2C, and `solve_ms_per_cell_per_tick`.
- `docs/PROMPTS/artifacts/G2.11/thermal_coupling.json` — the coupled electro-thermal model's capacity
  sweep and surface-temperature validation.
- `docs/PROMPTS/artifacts/G2.12/p2d_convergence.json` — the mesh and timestep that are numerically
  justified, and whether the shipped configuration is adequate.

**Why the compression story changes for P2D.** `G2.05` bounded compression error for a model whose
state is a handful of scalars. P2D carries `3 x nodes_per_region` electrolyte unknowns plus
`radial_nodes` per electrode node per particle, solved implicitly with Newton iterations. Larger
timesteps are cheaper per simulated second but harder to converge, and the balance point is an
empirical question this item answers.

**The mechanism you are working within.** `SimulationClock::advance` in
`eustress/crates/common/src/simulation/clock.rs` lines 78-102 advances `simulation_time_s` by the
full compressed delta and caps executed ticks at `max_ticks_per_frame` (`CONFIG DEFAULT` = 10),
zeroing the accumulator on saturation. `G2.04` instrumented that as `sim.step_fidelity`,
`sim.dropped_sim_seconds`, `sim.saturated_frames`. `G2.05` changed the integration so the error is
bounded rather than the steps silently dropped. Your P2D solve must sit inside whatever mechanism
`G2.05` shipped — read `eustress/crates/engine/src/simulation/electrochemistry.rs` and
`eustress/crates/engine/src/simulation/plugin.rs` to see what that is at this commit, rather than
assuming.

**The business claim this produces.** `docs/development/SIMULATION_SYSTEM.md` documents presets up to
`BATTERY_CYCLE_TEST = 7.2e6x`. That preset is a `CONFIG DEFAULT` and has never been validated for any
model. After this item, the only compression figure this project may publish for the cell model is
the one in your artifact.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/src/simulation/electrochemistry.rs`
- `eustress/crates/common/src/realism/particles/` — the `p2d` module's stepping strategy only
- `eustress/crates/common/src/realism/numerics/` — additive
- `eustress/crates/engine/src/bin/` — extend `cell-validate` and `sim-evidence`
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`
- `docs/PROMPTS/artifacts/G2.13/`

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.12/` — frozen
- The P2D physics and the thermal coupling. This item changes *how the model is stepped in time*, not
  what it models. Changing the physics invalidates `G2.10` and `G2.11`.
- The mesh justified by `G2.12`. If you coarsen it, `G2.12`'s convergence result no longer applies.
- `eustress/crates/engine/src/ui/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Coarsening the mesh, loosening the Newton tolerance, narrowing the SOC comparison window, or
  comparing against a compressed reference are all measurement changes. If the measurement is
  genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The reference is a `time_scale = 1` run of the same model at the same commit with the `G2.12`
  mesh. Nothing else.
- Comparison is at fixed SOC points, 400 samples across SOC 0.9 to 0.1, matching `G2.05`'s protocol
  so the two numbers are comparable.
- Report both **isothermal** and **thermally coupled** compression error. Thermal feedback introduces
  a slow mode that large timesteps handle differently from the electrochemical fast modes, and
  collapsing the two hides that.
- Multi-cell scaling must be measured, not extrapolated: actually run 1, 8, and 64 cells and report
  the wall time for each.
- If adaptive timestepping is used, report the timestep histogram. A model that quietly reverts to
  tiny steps has not compressed anything, and the wall time will show it.

## 5. Exit criterion

### Criterion
At the highest `time_scale` reported as supported, RMS terminal-voltage deviation from the
`time_scale = 1` reference over 400 SOC samples is **<= 5.0 mV** in both isothermal and coupled
modes, the full 0.5C discharge completes in **less wall time than the reference**, the supported
scale is **>= 100**, and `solve_ms_per_cell_per_tick` is **<= 2.0 ms**.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin cell-validate
    ./target/release/cell-validate \
        --model p2d --compression-envelope \
        --mesh-from ../docs/PROMPTS/artifacts/G2.12/p2d_convergence.json \
        --params ../docs/PROMPTS/artifacts/G2.11/cell_parameters_thermal.json \
        --reference-scale 1 --scales 10,100,1000,10000 \
        --thermal-modes isothermal,coupled \
        --soc-window 0.9,0.1 --soc-samples 400 \
        --cell-counts 1,8,64 \
        --out ../docs/PROMPTS/artifacts/G2.13/p2d_compression_envelope.json
    echo "EXIT_ENV=$?"

Expected output shape:

    {
      "mesh": {"nodes_per_region": 10, "radial_nodes": 20},
      "reference_wall_s": 0.0,
      "points": [
        {"time_scale": 10.0,    "rms_isothermal_v": 0.000, "rms_coupled_v": 0.000, "wall_s": 0.0, "newton_iters_mean": 0.0, "step_fidelity": 1.000},
        {"time_scale": 100.0,   "rms_isothermal_v": 0.000, "rms_coupled_v": 0.000, "wall_s": 0.0, "newton_iters_mean": 0.0, "step_fidelity": 1.000},
        {"time_scale": 1000.0,  "rms_isothermal_v": 0.000, "rms_coupled_v": 0.000, "wall_s": 0.0, "newton_iters_mean": 0.0, "step_fidelity": 0.000},
        {"time_scale": 10000.0, "rms_isothermal_v": 0.000, "rms_coupled_v": 0.000, "wall_s": 0.0, "newton_iters_mean": 0.0, "step_fidelity": 0.000}
      ],
      "supported_max_scale_at_5mv": 100.0,
      "solve_ms_per_cell_per_tick": 0.00,
      "multi_cell": [
        {"cells": 1,  "wall_s_full_discharge": 0.0},
        {"cells": 8,  "wall_s_full_discharge": 0.0},
        {"cells": 64, "wall_s_full_discharge": 0.0}
      ],
      "timestep_histogram": {"bins_s": [], "counts": []},
      "published_claim": "N simulated hours per wall-clock minute at <= 5 mV, single cell"
    }

Pass condition:

    EXIT_ENV == 0
    AND ."supported_max_scale_at_5mv" >= 100.0
    AND at time_scale == supported_max_scale_at_5mv:
        rms_isothermal_v <= 0.005 AND rms_coupled_v <= 0.005 AND wall_s < reference_wall_s
    AND ."solve_ms_per_cell_per_tick" <= 2.0
    AND ."multi_cell" has entries for 1, 8, and 64 cells with non-null wall times

Read the emitted RMS values and wall times. File existence is not a pass.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**.

D5 will ask whether the published compression claim is honest. Prioritise: reporting the coupled-mode
error separately, since thermal feedback is where large steps go wrong; the Newton iteration mean
rising with `time_scale` in a way a numerical analyst would expect; and the timestep histogram,
because it is the only way a reader can tell whether "compression" means larger steps or the same
steps executed faster. A `supported_max_scale_at_5mv` that is much lower than
`docs/development/SIMULATION_SYSTEM.md`'s `7.2e6x` preset is a correct and valuable outcome — report
it plainly. Capture recipe: `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`.

The Critic never sees your self-report and must cite a specific value.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — larger implicit steps with Newton, error-controlled step selection
   -> if still failing, MANDATORY approach change. Loosening the Newton tolerance is NOT an approach
      change; moving to a multirate scheme that steps the thermal and degradation modes at a coarser
      rate than the electrochemical modes is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D5 moving < 0.5 AND supported_max_scale_at_5mv
                  unchanged
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: solve_ms_per_cell_per_tick > 2.0 at the G2.12 mesh (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.13/p2d_compression_envelope.json`

A reader finds: the mesh used and where it came from, the uncompressed reference wall time, one row
per `time_scale` giving isothermal and coupled RMS deviation, wall time, mean Newton iterations, and
step fidelity; the supported maximum scale at the 5 mV bound; the per-cell per-tick solve cost; the
measured multi-cell wall times at 1, 8, and 64 cells; the timestep histogram; and the single sentence
this project is permitted to publish about cell-simulation speed.

## 9. Definition of NOT done

- The bound holds isothermally and fails in coupled mode, and only the isothermal number is
  published.
- `supported_max_scale_at_5mv` clears 100 because the mesh was coarsened below the `G2.12`-justified
  configuration, so the compressed run is being compared against a reference that no longer
  represents the validated model.
- Multi-cell wall times are extrapolated from the single-cell number rather than measured. The
  command runs 1, 8, and 64 cells for exactly this reason.
- Wall time at the supported scale exceeds the reference. Nothing was compressed.
- The timestep histogram shows the adaptive controller pinned at the minimum step for 90% of the run,
  meaning the speed-up came from somewhere other than compression and is not reproducible.
- The result block cites `7.2e6x` from `docs/development/SIMULATION_SYSTEM.md` as an achieved figure.
  It is a `CONFIG DEFAULT` preset, and the only validated figure is the one this artifact measures.
````

---

## docs/PROMPTS/items/G2.14_fracture-to-avian-rigid-body-split.md

````markdown
---
id: G2.14
title: Fracture to Avian — a crack criterion drives a real rigid-body split
workload: W1
workload_secondary: [W3]
phase: G2
depends_on: [G2.07, G1.07, G1.12]
blocks: []
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D5, D6]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G2.14/fracture_conservation.json
escalation: >
  If a split event produces a non-manifold or zero-volume fragment that Avian accepts as a collider,
  STALL. A degenerate collider is a source of solver blow-ups that will surface far from this item
  and be attributed to something else.
status: DRAFT
notes: >
  Tier L. The mesh-splitting geometry already exists; the missing piece is the criterion-to-body
  pipeline and the conservation accounting across the split.
---

## 1. Objective

When the mode-I stress intensity factor at a crack exceeds the material's fracture toughness, the
affected body splits into two Avian dynamic rigid bodies along the crack plane. Total mass is
conserved to within 1%, total linear momentum to within 2%, and the split event is recorded in the
simulation recording with the criterion values that triggered it.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian** (`avian3d`); never Rapier. Units are meter-native. Licence: PolyForm Shield
1.0.0 (source-available).

**The honest current state.** `docs/AUDIT/19_REALISM_PHYSICS.md` P4 records: "Fracture mechanics
`fracture_mesh.rs` exists but **no integration path to Avian** — visualisation only today." The same
document's Feature 8 status is red, and its open question Q19.6 is: "Fracture coupling with Avian
rigid bodies (when does a part break apart)?" This item answers Q19.6.

**What exists — reuse it, do not rewrite it.**

- `eustress/crates/common/src/realism/deformation/fracture_mesh.rs` — `MeshSplitResult { positive:
  Option<Mesh>, negative: Option<Mesh>, cut_vertices: Vec<Vec3>, success: bool }` and
  `split_mesh_by_plane(mesh, plane_origin, plane_normal) -> MeshSplitResult`. It classifies vertices
  against the plane and returns the two halves. This is the geometry half and it works.
- `eustress/crates/common/src/realism/materials/fracture.rs` — the criterion half:
  `stress_intensity_mode_i(stress, crack_length, geometry_factor)` (line 128),
  `stress_intensity_mode_ii` (133), `stress_intensity_mode_iii` (138),
  `finite_width_center` (151), `finite_width_edge` (161),
  `check_griffith_fracture(stress_intensity, fracture_toughness) -> bool` (173),
  `griffith_critical_stress` (178), `critical_crack_length` (186),
  `energy_release_rate(stress_intensity, material, plane_strain)` (194),
  `paris_law(delta_k, c, m)` (216), plus a `Crack` type with `new(position, direction,
  initial_length)` (95), `tip_position()` (107), `propagate(delta_length)` (112), and a damage
  accumulator with `add_crack`, `total_crack_length`, `has_critical_crack(critical_length)`,
  `accumulate_damage`, `record_stress`.
- `eustress/crates/common/src/realism/materials/properties.rs` — `MaterialProperties`, the source of
  fracture toughness.
- `eustress/crates/common/src/realism/deformation/` also has `components.rs`, `systems.rs`,
  `vertex.rs`, `gpu_deform.rs`.

**What is missing.** Nothing consumes `check_griffith_fracture` to despawn one Avian body and spawn
two. There is no mass or inertia recomputation for the fragments, no velocity inheritance, and no
recorded event.

**Prerequisite already satisfied.** `G2.07` built the conservation suite
(`eustress/crates/engine/src/bin/conservation-suite`) with four invariants, including the
`mass_ledger` scenario built specifically so this item can extend it with a split event. Its
tolerances and reporting shape are in `docs/PROMPTS/artifacts/G2.07/conservation.json`, frozen. The
conservation law functions are in `eustress/crates/common/src/realism/laws/conservation.rs`.

**Physics pins in force**: `Time::<Fixed>::from_hz(60.0)`, `SubstepCount(6)`,
`SolverConfig::default()`, `Gravity(Vec3::NEG_Y * 9.80665)`.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`, not `cargo check` — a split that produces a degenerate collider type-checks fine and
panics at runtime.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/common/src/realism/deformation/` — all files
- `eustress/crates/common/src/realism/materials/fracture.rs` — additive
- `eustress/crates/engine/src/physics/` — a `fracture.rs` bridging module is permitted
- `eustress/crates/engine/src/bin/` — extend `conservation-suite`
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json` — add `fracture_split`
- `docs/PROMPTS/artifacts/G2.14/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.13/` — frozen
- `eustress/crates/common/src/physics/` and the engine's solver pins
- `eustress/crates/engine/src/ui/`
- Continuum stress solving. This item takes a stress field as input (uniform or analytically
  prescribed); computing it from an FEA solve is `G2.15`.
- `eustress/crates/engine/src/simulation/plugin.rs` — owned by `G1.07` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Widening the mass tolerance, excluding the split frame from the conservation window, or reporting
  conservation before the split rather than across it are all measurement changes. If the measurement
  is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Conservation is measured **across** the split event: the sample immediately before and the sample
  immediately after must both be in the window, and the artifact must report the frame index of the
  split.
- Fragment mass must be computed from fragment volume and material density, not assigned as half the
  parent mass. Report both fragment masses and their sum.
- Both fragments inherit the parent's linear and angular velocity at the split point, plus the
  rigid-body velocity contribution from their offset centres of mass. State the formula used.
- Reject degenerate fragments before spawning: a fragment with volume below a stated threshold, or
  with a non-manifold boundary, must be either merged back or discarded with its mass accounted for
  in the ledger. A discarded fragment whose mass is not accounted for fails the mass invariant, which
  is the intended behaviour.
- The split must be recorded as a `SimulationEvent` in the recording
  (`eustress/crates/common/src/simulation/recorder.rs`) with the stress intensity, toughness, crack
  length, and plane at the moment of the split.

## 5. Exit criterion

### Criterion
In the `fracture_split` scenario, a body under a stress that pushes mode-I stress intensity above
material toughness splits into exactly 2 Avian dynamic bodies; total mass drift across the split is
**<= 1%**, total linear momentum drift **<= 2%**, both fragments have non-zero volume and a valid
collider, and the split is present in the recording as an event.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin conservation-suite
    ./target/release/conservation-suite \
        --recipe ../docs/PROMPTS/harness/recipes/G2_sim_evidence.json \
        --scenario fracture_split \
        --ticks 1200 --sample-every 1 \
        --out ../docs/PROMPTS/artifacts/G2.14/fracture_conservation.json
    echo "EXIT_FRAC=$?"

Expected output shape:

    {
      "scenario": "fracture_split", "ticks": 1200,
      "split": {
        "occurred": true, "frame": 0,
        "k_i_pa_sqrt_m": 0.0, "k_ic_pa_sqrt_m": 0.0,
        "crack_length_m": 0.0, "plane_normal": [0.0, 0.0, 0.0],
        "recorded_as_event": true
      },
      "bodies": {"before": 1, "after": 2, "degenerate_rejected": 0},
      "fragments": [
        {"volume_m3": 0.0, "mass_kg": 0.0, "collider_valid": true},
        {"volume_m3": 0.0, "mass_kg": 0.0, "collider_valid": true}
      ],
      "conservation_across_split": {
        "mass_before_kg": 0.0, "mass_after_kg": 0.0, "mass_drift": 0.0000,
        "momentum_before": [0.0,0.0,0.0], "momentum_after": [0.0,0.0,0.0], "momentum_drift": 0.0000
      },
      "baseline_g207_all_pass": true
    }

Pass condition:

    EXIT_FRAC == 0
    AND ."split".occurred == true AND ."split".recorded_as_event == true
    AND ."bodies".after == 2
    AND every fragment.volume_m3 > 0 AND every fragment.collider_valid == true
    AND ."conservation_across_split".mass_drift <= 0.01
    AND ."conservation_across_split".momentum_drift <= 0.02
    AND ."baseline_g207_all_pass" == true

The final condition re-runs `G2.07`'s four invariants unchanged: a fracture implementation that
breaks the pendulum's energy conservation has broken more than it fixed.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)** and **D6 (overall coherence)**, floor
**8.0 each**. Either below 8.0 fails the item.

D5 will ask whether the split looks like fracture rather than like a deletion followed by two spawns.
Prioritise: fragments that separate along the crack plane with the velocity the parent had, no
instantaneous velocity injection at the split frame, and criterion values recorded at the split so a
reader can verify `K_I > K_Ic` by arithmetic.

D6 is gated because a fracture event that teleports fragments, resets their sleep state, or drops
them through the floor reads as a different system bolted on. The split must be continuous with the
simulation on both sides of it.

Capture recipe: `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`, scenario `fracture_split`. The
Critic never sees your self-report and must cite a specific value or frame.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — despawn parent, spawn two bodies from split_mesh_by_plane output
   -> if still failing, MANDATORY approach change. Adjusting the degenerate-volume threshold is NOT
      an approach change; moving from despawn-and-respawn to keeping the parent entity as one
      fragment and spawning only the second is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  momentum_drift moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a zero-volume or non-manifold fragment accepted as a collider (front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.14/fracture_conservation.json`

A reader finds: the scenario and tick count; the split event with its frame index, mode-I stress
intensity, material toughness, crack length, plane normal, and whether it was recorded as a
simulation event; body counts before and after plus rejected degenerate fragments; per-fragment
volume, mass, and collider validity; mass and momentum before and after the split with their drifts;
and confirmation that `G2.07`'s four invariants still pass unchanged.

## 9. Definition of NOT done

- Mass is conserved because each fragment was assigned half the parent mass regardless of geometry.
  Fragment masses must come from volume times density and will differ.
- Momentum drift clears 2% because the measurement window starts after the split frame.
- The split fires on a stress threshold rather than on `check_griffith_fracture`, so crack length and
  geometry factor play no part and the criterion is not fracture mechanics.
- Two bodies exist afterwards but one has a zero-volume collider that Avian silently accepts,
  producing a solver blow-up several hundred ticks later.
- The split event is logged to stdout but not written into the recording, so `compare_runs` and any
  downstream analysis cannot see it.
- `G2.07`'s pendulum or elastic-collision invariants regress and the result block does not mention
  it.
````

---

## docs/PROMPTS/items/G2.15_beam-fea-validated-against-euler-bernoulli.md

````markdown
---
id: G2.15
title: Beam FEA validated against the closed-form Euler-Bernoulli solution
workload: W1
workload_secondary: [W3]
phase: G2
depends_on: [G2.01, G1.12]
blocks: []
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G2.15/beam_fea_validation.json
escalation: >
  If tip deflection does not converge monotonically toward the analytic value as elements are added,
  STALL. Non-monotone convergence in a linear beam element means the element formulation or the
  assembly is wrong, and refining further will not reveal which.
status: DRAFT
notes: >
  The closed-form beam functions already exist and are the reference. This item builds the assembled
  solver that generalises beyond the handful of textbook load cases those functions cover.
---

## 1. Objective

A 1-D Euler-Bernoulli beam finite-element solver assembles and solves arbitrary combinations of
supports and loads, and reproduces the closed-form solutions already in the codebase: tip deflection
of an end-loaded cantilever within 0.5% at 20 elements, mid-span deflection of a uniformly loaded
simply supported beam within 0.5%, and the first natural frequency of a cantilever within 2%.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**The honest current state.** `eustress/crates/common/src/realism/structures/` contains `beams.rs`,
`columns.rs`, `composites.rs`, `fatigue.rs`, `mod.rs`. `beams.rs` provides closed-form results for a
fixed catalogue of load cases and nothing that assembles them:

- `bending_stress(moment, y, i_moment)` (line 27), `shear_stress(shear, q_first_moment, i_moment,
  width)` (41)
- `beam_deflection_cantilever_end_load(p, length, e, i, x)` (62),
  `beam_deflection_cantilever_max(p, length, e, i)` (73),
  `beam_deflection_simply_supported_center(p, length, e, i)` (84),
  `beam_deflection_udl_simply_supported(w, length, e, i)` (97)
- `max_moment_cantilever_end(p, length)` (113), `max_moment_simply_supported_center(p, length)`
  (120), `max_moment_udl(w, length)` (127)
- `section_modulus(i_moment, c_max)` (140), `moment_of_area_rectangle(b, h)` (153),
  `moment_of_area_circle(r)` (159), `moment_of_area_hollow_circle(r_outer, r_inner)` (166),
  `moment_of_area_i_beam(b, h, t_web, t_flange)` (185)
- `natural_frequency_cantilever(e, i, rho_lin, length)` (213),
  `natural_frequency_simply_supported(e, i, rho_lin, length)` (228)

These functions **are your reference solutions**. Do not modify them; a validation whose reference
was edited by the same agent is worthless. Their existence is also why this item has a real analytic
target rather than a hand-waved one.

**What is missing.** There is no stiffness-matrix assembly, no boundary-condition application, no
linear solve, and therefore no way to analyse a beam with supports or loads outside the catalogue.
That is the gap a structural engineer notices within one minute of opening the tool.

**Why the analytic reference is the right validation source.** Euler-Bernoulli cantilever tip
deflection under an end load is `delta = P L^3 / (3 E I)` exactly; the uniformly loaded simply
supported mid-span deflection is `5 w L^4 / (384 E I)` exactly; the first natural frequency of a
cantilever is `f1 = (1.875104^2 / (2 pi L^2)) sqrt(E I / rho_lin)`. These are identities, not
measurements — they need no dataset, no licence, and they cannot go stale.

**Prerequisite already satisfied.** `G2.01` built `sim-evidence` and the recipe
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json`, and recorded the host block in
`docs/PROMPTS/artifacts/G2.01/physics_baseline.json`.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/common/src/realism/structures/` — a new `fea.rs` module; `mod.rs` for
  registration
- `eustress/crates/common/src/realism/numerics/` — additive linear-algebra helpers
- `eustress/crates/common/Cargo.toml` — a dense/sparse linear-algebra dependency is permitted with a
  one-line justification in the result block
- `eustress/crates/engine/src/bin/` — a `beam-validate` binary is permitted
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`
- `docs/PROMPTS/artifacts/G2.15/`

### Out of scope — do not edit
- `eustress/crates/common/src/realism/structures/beams.rs` — **frozen**; it is the reference
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.14/` — frozen
- `eustress/crates/common/src/physics/` — no Avian changes; this item is a static solve
- `eustress/crates/engine/src/ui/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Editing the reference functions in `beams.rs`, evaluating deflection at a point other than the
  stated one, comparing at 200 elements when the criterion says 20, or dropping the frequency case
  are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Use standard Hermite cubic beam elements with two nodes and two degrees of freedom per node
  (transverse displacement and rotation). State the element type in the artifact.
- Report a convergence sequence at 2, 5, 10, 20, and 40 elements for the cantilever case, so a reader
  can see the approach to the analytic value. A single number at 20 elements does not demonstrate a
  working solver.
- The natural frequency case requires a consistent mass matrix. A lumped mass matrix will not reach
  2% on the first mode at reasonable element counts; if you use one, say so and expect to miss the
  bound.
- The solver must accept at least three support types (fixed, pinned, roller) and at least two load
  types (point load, uniformly distributed load). Demonstrate one case outside the closed-form
  catalogue — for example a propped cantilever — and report its result even though there is no
  catalogue reference for it.
- Units are meter-native throughout: metres, newtons, pascals, kilograms.

## 5. Exit criterion

### Criterion
At **20 elements**: cantilever end-load tip deflection within **0.5%** of
`beam_deflection_cantilever_max`, uniformly loaded simply supported mid-span deflection within
**0.5%** of `beam_deflection_udl_simply_supported`, and first natural frequency within **2%** of
`natural_frequency_cantilever`; and the cantilever error sequence over 2, 5, 10, 20, 40 elements is
monotonically decreasing.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin beam-validate
    ./target/release/beam-validate \
        --element-counts 2,5,10,20,40 \
        --cases cantilever_end_load,udl_simply_supported,cantilever_first_mode,propped_cantilever \
        --out ../docs/PROMPTS/artifacts/G2.15/beam_fea_validation.json
    echo "EXIT_BEAM=$?"

Expected output shape:

    {
      "element_type": "Hermite_cubic_2node_4dof", "mass_matrix": "consistent",
      "material": {"e_pa": 2.0e11, "rho_lin_kg_per_m": 0.0},
      "section": {"kind": "rectangle", "b_m": 0.0, "h_m": 0.0, "i_m4": 0.0},
      "cases": {
        "cantilever_end_load": {
          "analytic_m": 0.000000, "at_20_elements_m": 0.000000, "rel_error_at_20": 0.0000,
          "convergence": [
            {"elements": 2,  "rel_error": 0.0000},
            {"elements": 5,  "rel_error": 0.0000},
            {"elements": 10, "rel_error": 0.0000},
            {"elements": 20, "rel_error": 0.0000},
            {"elements": 40, "rel_error": 0.0000}
          ],
          "monotone": true
        },
        "udl_simply_supported":  {"analytic_m": 0.000000, "at_20_elements_m": 0.000000, "rel_error_at_20": 0.0000},
        "cantilever_first_mode": {"analytic_hz": 0.0000,  "at_20_elements_hz": 0.0000,  "rel_error_at_20": 0.0000},
        "propped_cantilever":    {"tip_reaction_n": 0.0, "max_moment_nm": 0.0, "no_catalogue_reference": true}
      }
    }

Pass condition:

    EXIT_BEAM == 0
    AND cases.cantilever_end_load.rel_error_at_20 <= 0.005
    AND cases.udl_simply_supported.rel_error_at_20 <= 0.005
    AND cases.cantilever_first_mode.rel_error_at_20 <= 0.02
    AND cases.cantilever_end_load.monotone == true
    AND cases.propped_cantilever.max_moment_nm is non-null

Read the emitted relative errors. File existence is not a pass.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**.

D5 will ask whether a structural engineer would use this. Prioritise: the convergence sequence, which
is the single most informative thing in the artifact; reporting the section properties and material
so a reader can recompute the analytic value by hand; and the out-of-catalogue propped-cantilever
case, which is the evidence that the solver assembles rather than looks up. Capture recipe:
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json`.

The Critic never sees your self-report and must cite a specific value.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — Hermite cubic elements, direct dense solve, consistent mass matrix
   -> if still failing, MANDATORY approach change. Adding elements is NOT an approach change;
      switching the eigenvalue extraction from a dense symmetric solve to subspace iteration, or
      switching element formulation to Timoshenko, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D5 moving < 0.5 AND rel_error_at_20 for the
                  cantilever moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: non-monotone convergence in the cantilever sequence (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.15/beam_fea_validation.json`

A reader finds: the element type and mass-matrix formulation; the material and section properties
used; for each catalogue case the analytic value, the 20-element FEA value, and the relative error;
the full convergence sequence with a monotonicity flag for the cantilever; and the out-of-catalogue
propped-cantilever result. This is the evidence that Eustress can analyse a structure rather than
look one up.

## 9. Definition of NOT done

- The errors clear their bounds because `beams.rs` was edited so the "analytic" value matches the
  FEA result. That file is frozen for exactly this reason.
- Deflection converges but the first natural frequency is off by 15% because a lumped mass matrix was
  used and the artifact does not say so.
- The convergence sequence is reported only at 20 and 40 elements, so monotonicity cannot be judged.
- The propped-cantilever case is omitted, leaving no evidence the solver does anything the closed-form
  catalogue could not already do.
- Section properties are hard-coded in the binary rather than reported, so a reader cannot recompute
  the analytic value independently.
- Units are mixed — a section given in millimetres against a length in metres. Units are meter-native
  and the artifact must show consistent SI throughout.
````

---

## docs/PROMPTS/items/G2.16_reactor-control-loop-step-response.md

````markdown
---
id: G2.16
title: Reactor control loop — measured step response with overshoot and settling bounds
workload: W1
workload_secondary: [W3]
phase: G2
depends_on: [G2.03, G1.12]
blocks: []
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G2.16/control_step_response.json
escalation: >
  If rebasing the controller onto simulation time makes the loop unstable at any tested time_scale,
  STALL rather than retuning the gains inside this item. Gain selection for a compressed-time plant
  is a controls design decision with safety implications and must be made explicitly.
status: DRAFT
notes: >
  The three-loop PID already exists and is prior art the rest of the platform should reuse. This item
  measures it properly and fixes the wall-clock dt defect that makes it wrong under compression.
---

## 1. Objective

The reactor's three-loop controller is driven by simulation time rather than wall-clock time, and its
closed-loop step response is measured: for a 50% to 80% power demand step, overshoot is at most 10%
and the output settles inside a 5% band within 120 simulated seconds, with the same result at
`time_scale = 1` and `time_scale = 100`.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**The controller that exists.** `eustress/crates/common/src/realism/nuclear/systems.rs`, function
`update_ai_controller_system` (section 6, from line ~132). It runs three PID loops on an
`ArcReactorAIController`:

```rust
let dt = time.delta_secs().min(0.05);
…
ai.power_pid.setpoint = batt.load_demand_watts;
let rod_correction = if matches!(ai.mode, ReactorControlMode::PowerFollow) {
    ai.power_pid.update(conv.electrical_output_watts, dt)
} else { 0.0 };
let reactivity_correction = ai.reactivity_pid.update(kinetics.neutron_population, dt);
let total_rod_delta = rod_correction + reactivity_correction;
rods.bank_a_pct = (rods.bank_a_pct + total_rod_delta * 0.5).clamp(0.0, 100.0);
rods.bank_b_pct = (rods.bank_b_pct + total_rod_delta * 0.5).clamp(0.0, 100.0);
let flow_correction = ai.thermal_pid.update(thermal.core_temp_celsius, dt);
thermal.coolant_flow_pct = (thermal.coolant_flow_pct - flow_correction).clamp(10.0, 100.0);
```

**The defect.** `dt` is `time.delta_secs()` — Bevy's **wall-clock** frame delta, clamped to 50 ms. It
is not multiplied by `SimulationClock::time_scale`. So under time compression the plant advances by
`wall_delta * time_scale` simulated seconds while the controller integrates and differentiates as if
only `wall_delta` had passed. The integral term is therefore under-accumulated by the compression
factor and the derivative term over-weighted by it. At `time_scale = 100` the controller is
effectively operating on a plant 100x faster than it believes.

**The controller implementation.** `eustress/crates/common/src/realism/control/pid.rs` —
`PidController` with `kp`, `ki`, `kd`, `setpoint`, `output_min`, `output_max`,
`anti_windup_limit` (integral clamped to `[-limit/ki, limit/ki]` when `ki != 0`), `integral`,
`prev_error`, `prev_measured`, `derivative_on_measurement`, `enabled`, `output`, and
`update(measured, dt)`. Also in `eustress/crates/common/src/realism/control/`: `discrete.rs`,
`frequency.rs`, `state_space.rs`.

**The successor design that must keep working.**
`eustress/crates/common/src/realism/nuclear/control_law.rs` defines `FeedforwardCoefficients`
(`a_rod`, `b_rod`, `c_flow`, `d_flow`, `kp_neutron_trim`, `rod_min_safe_pct`, `rod_max_safe_pct`,
`flow_min_safe_pct`) intended to replace the PID once fitted. Its doc comment says coefficients are
loaded from `docs/arc1/feedforward_coefficients.toml` — **that path does not exist in this
repository**; the struct ships with an analytical first guess. Do not create that file as part of
this item; do not depend on it.

**Prerequisite already satisfied.** `G2.03` proved the headless engine stack reruns byte-identically
over 3600 ticks, and built the `headless-determinism` driver. That is what makes a settling-time
measurement meaningful: the same step produces the same trajectory every time, so overshoot is a
property of the controller and not of scheduling jitter.

**Definitions, fixed.** Overshoot is `(peak - final) / (final - initial)` for the step. Settling time
is the first simulated time after the step from which the output remains inside `final +/- 5%` for the
remainder of the window. Both are measured on `electrical_output_watts`.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/common/src/realism/nuclear/systems.rs`
- `eustress/crates/common/src/realism/control/pid.rs` — additive only
- `eustress/crates/engine/src/bin/` — a `control-step` binary is permitted
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json` — add `reactor_power_step`
- `docs/PROMPTS/artifacts/G2.16/`

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.15/` — frozen
- `eustress/crates/common/src/realism/nuclear/control_law.rs` — the feedforward successor is out of
  scope; do not create `docs/arc1/feedforward_coefficients.toml`
- `eustress/crates/common/src/simulation/clock.rs`
- `eustress/crates/engine/src/ui/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Shrinking the step size, lengthening the settling window, widening the settling band beyond 5%, or
  measuring settling on a filtered signal are all measurement changes. If the measurement is genuinely
  wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The controller `dt` must be simulation seconds. Derive it from `SimulationClock` — do not multiply
  the wall delta by `time_scale` in the control system itself, because that duplicates logic the
  clock already owns and will diverge from `G2.05`'s stepping. Read
  `eustress/crates/common/src/simulation/clock.rs` and use what it exposes.
- Keep the existing gains. If the loop cannot meet the bounds with the shipped gains at simulation
  time, that is the finding — report it and STALL rather than retuning. Retuning is the front-matter
  escalation.
- Anti-windup must remain effective after the change: report the peak integral value at both time
  scales and confirm it stays inside the configured clamp.
- The safety clamps (`rods` to `[0, 100]`, `coolant_flow_pct` to `[10, 100]`) stay. A controller that
  meets the bound only by exceeding a clamp has not met it.

## 5. Exit criterion

### Criterion
For a 50% to 80% power-demand step: overshoot **<= 10%** and 5%-band settling time **<= 120 simulated
seconds**, at both `time_scale = 1` and `time_scale = 100`, with the two settling times differing by
**<= 10%** of each other.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin control-step
    ./target/release/control-step \
        --recipe ../docs/PROMPTS/harness/recipes/G2_sim_evidence.json \
        --scenario reactor_power_step \
        --step-from-pct 50 --step-to-pct 80 \
        --window-sim-seconds 600 \
        --scales 1,100 \
        --out ../docs/PROMPTS/artifacts/G2.16/control_step_response.json
    echo "EXIT_STEP=$?"

Expected output shape:

    {
      "step": {"from_pct": 50, "to_pct": 80}, "window_sim_seconds": 600,
      "settling_band_pct": 5.0,
      "controller_dt_source": "SimulationClock",
      "gains": {"power": {"kp":0.0,"ki":0.0,"kd":0.0},
                "reactivity": {"kp":0.0,"ki":0.0,"kd":0.0},
                "thermal": {"kp":0.0,"ki":0.0,"kd":0.0}},
      "points": [
        {"time_scale": 1.0,   "overshoot": 0.000, "settling_sim_s": 0.0, "steady_state_error_pct": 0.00, "peak_integral": 0.0, "anti_windup_ok": true, "clamps_hit": 0},
        {"time_scale": 100.0, "overshoot": 0.000, "settling_sim_s": 0.0, "steady_state_error_pct": 0.00, "peak_integral": 0.0, "anti_windup_ok": true, "clamps_hit": 0}
      ],
      "settling_time_ratio": 1.00
    }

Pass condition:

    EXIT_STEP == 0
    AND every point.overshoot <= 0.10
    AND every point.settling_sim_s <= 120.0
    AND ."settling_time_ratio" >= 0.90 AND <= 1.10
    AND every point.anti_windup_ok == true
    AND ."controller_dt_source" == "SimulationClock"

Read the overshoot and settling values. File existence is not a pass.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**.

D5 will ask whether a controls engineer would accept this loop. Prioritise: a step response whose
shape is physically plausible for a reactor (rod motion leading power, coolant flow responding to
core temperature with a lag), the two time scales producing visually superimposable trajectories when
plotted against simulated time, and honest reporting of steady-state error, which a PID with a
clamped integral can retain. Capture recipe:
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json`, scenario `reactor_power_step`.

The Critic never sees your self-report and must cite a specific value.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — rebase controller dt onto SimulationClock, measure, report
   -> if still failing, MANDATORY approach change. Changing the clamp on dt is NOT an approach
      change; moving from a per-frame continuous-time PID to the discrete controller in
      realism/control/discrete.rs, stepped at a fixed simulation-time rate, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D5 moving < 0.5 AND settling_sim_s at
                  time_scale = 100 moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: instability at any tested time_scale (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.16/control_step_response.json`

A reader finds: the step definition and measurement window, the settling band, the source of the
controller timestep, the three PID gain sets as shipped, and for each time scale the overshoot,
settling time in simulated seconds, steady-state error, peak integral value, anti-windup status, and
clamp-hit count — plus the ratio of the two settling times. Alongside it: the recorded response
trajectories at both scales.

This artifact is also the reference the rest of the platform reuses: it is the only measured
closed-loop response in the repository, and any other control loop should be compared against its
shape.

## 9. Definition of NOT done

- The bounds are met by retuning the gains. The gains block exists in the artifact so a reader can
  confirm the shipped values were used.
- `settling_time_ratio` is 1.00 because both runs were executed at `time_scale = 1` and the label was
  changed.
- Overshoot clears 10% because the step was reduced from 30 percentage points to 5.
- The controller now uses simulation time but `anti_windup_ok` is false at `time_scale = 100`, and
  the result block does not mention it. Larger effective `dt` per update is exactly what pushes an
  integrator past its clamp.
- A clamp on rod position or coolant flow is hit during settling and `clamps_hit` is reported as 0
  because clamp saturation is not counted.
- `docs/arc1/feedforward_coefficients.toml` is created to make the successor path load. That file is
  out of scope and inventing it fabricates fitted coefficients nobody measured.
````

---

## docs/PROMPTS/items/G2.17_transient-conduction-vs-analytic-semi-infinite-slab.md

````markdown
---
id: G2.17
title: Transient conduction validated against the analytic semi-infinite slab
workload: W3
workload_secondary: [W1]
phase: G2
depends_on: [G2.01]
blocks: [G5.23]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G2.17/conduction_validation.json
escalation: >
  If holding the 2% error bound requires a per-frame temperature change larger than the shipped
  max_delta_t_per_frame clamp of 50 K, STALL. That clamp exists to prevent instability and silently
  raising it converts a visible error into an invisible one.
status: DRAFT
notes: >
  Tier M. Pure numerics against a closed-form solution - no dataset, no licence, no perceptual
  judgement, so critic_gate is empty and the criterion is correspondingly tight.
---

## 1. Objective

The engine's transient heat conduction produces temperatures within 2% of the closed-form
semi-infinite-slab solution across Fourier numbers from 0.01 to 1.0, at three depths, and the
shipped stability clamp is shown not to be silently truncating the solution in the validated range.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**What exists.** `eustress/crates/common/src/realism/thermal_conduction.rs`:

- `ThermalContact { entity_a, entity_b, contact_area, contact_thickness }` (line 26)
- `ThermalConductionConfig { enabled, max_delta_t_per_frame, min_delta_t_threshold,
  auto_contact_radius, auto_detect_contacts }` (line 43), with `Default` giving
  `max_delta_t_per_frame: 50.0`, `min_delta_t_threshold: 0.01`, `auto_contact_radius: 0.5`,
  `auto_detect_contacts: true` (all `CONFIG DEFAULT`)
- `thermal_conduction_system` (line 79) — per-frame heat transfer for each `ThermalContact` using
  Fourier's law `Q = k_eff * A * dT / L`, with `k_eff` the harmonic mean of the two materials'
  conductivities, then `dT = Q * dt / (m * c_p)`
- `auto_thermal_contacts_system` (line 158), `ThermalConductionPlugin` (line 230)

**What has never been done.** There is no comparison of this system against any reference. It is a
plausible discretisation with no error bound.

**The analytic reference.** A semi-infinite solid initially at uniform `T_i`, with its surface held
at `T_s` from `t = 0`, has
`(T(x,t) - T_s) / (T_i - T_s) = erf( x / (2 sqrt(alpha t)) )`,
where `alpha = k / (rho c_p)`. Expressed with the Fourier number `Fo = alpha t / L^2` for a chosen
reference length `L`, this is a closed-form identity requiring no dataset and no licence. Implement
`erf` yourself (a rational approximation accurate to better than 1e-7 is sufficient — state which)
or take it from an existing dependency, and record which in the artifact.

**Discretisation.** The chain of `ThermalContact` entities is a 1-D conduction path. Build the slab
as `N >= 50` nodes in a line so the spatial truncation error is small compared to the 2% bound, and
apply the fixed surface temperature by holding node 0.

**Prerequisite already satisfied.** `G2.01` built `sim-evidence` and the recipe
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json`, and recorded the host block in
`docs/PROMPTS/artifacts/G2.01/physics_baseline.json`.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/common/src/realism/thermal_conduction.rs`
- `eustress/crates/common/src/realism/numerics/` — additive (`erf` and friends)
- `eustress/crates/common/tests/` — new test files permitted
- `eustress/crates/engine/src/bin/` — a `conduction-validate` binary is permitted
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`
- `docs/PROMPTS/artifacts/G2.17/`

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.16/` — frozen
- `eustress/crates/engine/src/simulation/electrochemistry.rs` — cell thermal coupling is `G2.11`
- `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/ui/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Narrowing the Fourier range, sampling only at the deepest probe where the response is smallest,
  reducing the node count, or comparing against a numerically integrated "reference" instead of the
  closed-form `erf` are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Probe at three depths corresponding to `x / (2 sqrt(alpha t_end))` of approximately 0.25, 0.75, and
  1.5, so the comparison spans the steep, mid, and tail regions of the error function.
- Relative error is measured against the temperature *difference* `(T - T_s) / (T_i - T_s)`, not
  against absolute kelvin, so the bound does not become trivially easy by choosing a large `T_i`.
- Report whether `max_delta_t_per_frame` clamped any step during the validated run. If it did, the
  run is not a valid validation of the underlying scheme and the artifact must say so.
- Do not raise `max_delta_t_per_frame` to avoid the clamp. Reduce the timestep instead, and report
  the timestep used.

## 5. Exit criterion

### Criterion
Maximum relative error against the closed-form `erf` solution is **<= 2%** across Fourier numbers
from 0.01 to 1.0 at all three probe depths, with **at least 20 Fourier samples**, at least 50 nodes,
and **zero** clamp activations.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin conduction-validate
    ./target/release/conduction-validate \
        --nodes 50 \
        --fourier-range 0.01,1.0 --fourier-samples 20 \
        --probe-eta 0.25,0.75,1.5 \
        --out ../docs/PROMPTS/artifacts/G2.17/conduction_validation.json
    echo "EXIT_COND=$?"

Expected output shape:

    {
      "nodes": 50, "timestep_s": 0.0, "alpha_m2_per_s": 0.0,
      "erf_implementation": "<name and stated accuracy>",
      "fourier_range": [0.01, 1.0], "fourier_samples": 20,
      "probes": [
        {"eta": 0.25, "max_rel_error": 0.0000, "mean_rel_error": 0.0000},
        {"eta": 0.75, "max_rel_error": 0.0000, "mean_rel_error": 0.0000},
        {"eta": 1.50, "max_rel_error": 0.0000, "mean_rel_error": 0.0000}
      ],
      "overall_max_rel_error": 0.0000,
      "clamp_activations": 0,
      "max_delta_t_per_frame_config": 50.0
    }

Pass condition:

    EXIT_COND == 0
    AND ."overall_max_rel_error" <= 0.02
    AND ."clamp_activations" == 0
    AND ."nodes" >= 50
    AND ."fourier_samples" >= 20
    AND every probe.max_rel_error <= 0.02

Read `overall_max_rel_error` and `clamp_activations`. File existence is not a pass.

## 6. Critic gate

`critic_gate: []`. The reference is a closed-form identity, so the comparison is arithmetic and there
is nothing for a blinded evaluator to judge. The mechanical criterion is correspondingly tight: it
constrains the error at three separate depths, the sample density, the node count, and the clamp
count, so it cannot be satisfied by a favourable choice of probe or a coarse sweep.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — explicit conduction on a 50-node chain with a stability-limited step
   -> if still failing, MANDATORY approach change. Halving the timestep again is NOT an approach
      change; moving from the explicit per-frame update to an implicit (Crank-Nicolson or BDF) step
      via realism/numerics/ode/implicit.rs is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with overall_max_rel_error moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: clamp activation required to stay stable (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.17/conduction_validation.json`

A reader finds: the node count, timestep, and thermal diffusivity used; the `erf` implementation and
its stated accuracy; the Fourier range and sample count; per-probe maximum and mean relative error at
three depths; the overall maximum; the clamp activation count; and the configured
`max_delta_t_per_frame`. This is the evidence that heat moves through the substrate at the rate
physics says it does.

## 9. Definition of NOT done

- The error clears 2% at the deepest probe (where the temperature barely changes and relative error
  is measured against a near-zero difference) and fails at the shallowest. All three probes bind.
- `clamp_activations` is 0 because clamping is not counted, not because it did not happen.
- The reference `erf` is itself computed by numerically integrating the same discretisation, making
  the comparison circular.
- Node count is reduced to 10 and the artifact reports the reduced count honestly but the floor of 50
  is missed.
- `max_delta_t_per_frame` is raised from 50 K to 5000 K so the explicit scheme survives a large
  timestep. That is a measurement change dressed as a configuration change.
- The relative error is computed against absolute temperature in kelvin, so a 300 K baseline makes a
  30 K error look like 10%.
````

---

## docs/PROMPTS/items/G2.18_kernel-law-rune-parity.md

````markdown
---
id: G2.18
title: Kernel-law declaration surface — a Rune-declared law equals the native kernel
workload: W5
workload_secondary: [W1]
phase: G2
depends_on: [G2.01]
blocks: []
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G2.18/law_parity.json
escalation: >
  If any domain module fails to build at runtime (realism_law_modules logs an error and skips it),
  STALL and name the module. A silently skipped module means a whole physics domain is unavailable
  to scripts while the engine reports no failure.
status: DRAFT
notes: >
  W5 primary: this is the extension surface - the mechanism by which someone outside the company
  declares a first-principles law without touching Rust. Under PolyForm Shield, scripts and plugins
  built with the substrate are permitted at no cost, which is what makes this the right W5 item.
---

## 1. Objective

Every realism law function exposed to Rune returns the same value as the native Rust kernel function
it wraps, to within 1e-9 relative, across a fixed input grid; every domain module builds without
being skipped; and a custom law declared in a Rune script drives an ECS component through a headless
run, proving the surface is usable end to end rather than merely present.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. **Slint is Rust** — `.slint` files
compile to Rust — though no UI is involved here. Licence: **PolyForm Shield 1.0.0**; say
**source-available**, never open source. Under that licence, plugins, Rune and Luau scripts, and
end-products built *with* the substrate are permitted at no cost and royalty-free — which is exactly
the surface this item hardens.

**What exists.** `eustress/crates/common/src/realism/scripting/laws/mod.rs` exposes the realism
kernel's stateless law functions to Rune under `eustress::realism::<domain>::<fn>`. Its own header
gives the usage shape:

```rune
use eustress::realism::electrical;
use eustress::realism::chemistry;
let i = electrical::ohm_current(12.0, 4.0);          // 3.0 A
let k = chemistry::arrhenius_rate(1.0e8, 50000.0, 298.15);
```

`realism_law_modules()` builds thirteen modules — `electrical`, `chemistry`, `thermocycles`,
`structures`, `propulsion`, `optics`, `acoustics`, `nuclear`, `plasma`, `control`, `numerics`,
`mechanics`, `thermodynamics` — each from a `create_module()` in the correspondingly named file under
`eustress/crates/common/src/realism/scripting/laws/`. The bindings are documented as "f64 wrappers
over the f32 kernel laws (f64-native kernels pass through unchanged)".

**The failure mode this item catches.** From `realism_law_modules()`:

```rust
for (name, build) in builders {
    match build() {
        Ok(m) => modules.push(m),
        Err(e) => error!("Failed to build realism::{} Rune module: {:?}", name, e),
    }
}
```

A module that fails to build is **logged and skipped**, not propagated. An entire physics domain can
be missing from every script in the product while the engine starts normally. Nothing tests that all
thirteen are present.

**The second failure mode.** The bindings widen f32 kernels to f64. A wrapper that transposes
arguments, drops one, or applies a unit conversion produces plausible numbers that differ from the
native function — and nothing compares them.

**A relevant precedent from this repository's history.** `#[rune(item = ::eustress)]` is mandatory on
Rune-exposed constructors; without it they compile and then silently do not exist at the call site.
Assume nothing about registration; assert it.

**Prerequisite already satisfied.** `G2.01` built the `sim-evidence` binary and recorded the host
block in `docs/PROMPTS/artifacts/G2.01/physics_baseline.json`.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`, not `cargo check` — a Rune module that fails to register type-checks perfectly.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/common/src/realism/scripting/laws/` — all files
- `eustress/crates/common/src/realism/scripting/` — `api.rs`, `bindings.rs`, `mod.rs`
- `eustress/crates/common/tests/` — new test files permitted
- `eustress/crates/engine/src/bin/` — a `law-parity` binary is permitted
- `docs/PROMPTS/artifacts/G2.18/` — including the demonstration Rune script

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.17/` — frozen
- The native kernel law functions under `eustress/crates/common/src/realism/laws/` and the domain
  modules under `eustress/crates/common/src/realism/{chemistry,control,electrical,fluids,materials,
  nuclear,plasma,propulsion,structures,thermocycles}/` — **frozen**; they are the reference. If a
  binding disagrees with the kernel, the binding is wrong.
- `eustress/crates/engine/src/ui/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Loosening the 1e-9 relative tolerance, shrinking the input grid, excluding a domain, or excluding a
  function that fails are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Coverage floor: at least **30 law functions across at least 6 domains**. List every function tested
  in the artifact by domain and name. A function that cannot be tested because its inputs are not
  scalars must be listed as `untestable` with a one-line reason, and counts toward neither the
  numerator nor the denominator.
- The input grid must include boundary values the kernels guard against: zero, negative, and very
  large arguments. Several kernels early-return a sentinel (for example
  `electrolyte_asr` returns `f32::INFINITY` when conductivity is zero). The binding must return the
  same sentinel, and the comparison must handle non-finite values explicitly rather than skipping
  them.
- Tolerance is 1e-9 **relative**, computed in f64 after widening the f32 kernel result. Where the
  kernel is f32-native, the widened value is the reference — you are testing the binding, not f32.
- `realism_law_modules()` must be changed so a build failure is **surfaced**, not just logged. Return
  the failures alongside the modules, or provide a companion function that reports them. Do not make
  it panic.
- The end-to-end demonstration must be a script that declares a law, is loaded at runtime, and whose
  output changes an ECS component value observable in a headless recording. A script that only prints
  does not demonstrate the surface.

## 5. Exit criterion

### Criterion
All **13** domain modules build with zero skips; at least **30** law functions across at least **6**
domains match their native kernel to within **1e-9 relative** (or produce the identical non-finite
sentinel) at every point of the input grid, with **zero** mismatches; and a Rune-declared custom law
changes a named ECS component value in a headless run.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin law-parity
    ./target/release/law-parity \
        --grid-points 0,-1,1e-6,0.5,1.0,2.0,298.15,1e6 \
        --min-functions 30 --min-domains 6 \
        --demo-script ../docs/PROMPTS/artifacts/G2.18/custom_law_demo.rune \
        --demo-component-key "custom.law.output" \
        --out ../docs/PROMPTS/artifacts/G2.18/law_parity.json
    echo "EXIT_PARITY=$?"

Expected output shape:

    {
      "modules": {"expected": 13, "built": 13, "skipped": [], "skip_count": 0},
      "coverage": {"functions_tested": 0, "domains_tested": 0, "untestable": []},
      "tolerance_rel": 1e-9,
      "grid_points": [0.0, -1.0, 1e-6, 0.5, 1.0, 2.0, 298.15, 1e6],
      "mismatches": [],
      "mismatch_count": 0,
      "nonfinite_sentinel_agreements": 0,
      "demo": {
        "script": "custom_law_demo.rune",
        "component_key": "custom.law.output",
        "value_before": 0.0, "value_after": 0.0, "changed": true,
        "recording_contains_series": true
      }
    }

Pass condition:

    EXIT_PARITY == 0
    AND ."modules".skip_count == 0 AND ."modules".built == 13
    AND ."coverage".functions_tested >= 30 AND ."coverage".domains_tested >= 6
    AND ."mismatch_count" == 0
    AND ."demo".changed == true AND ."demo".recording_contains_series == true

Read `mismatch_count` and `skip_count`. File existence is not a pass.

## 6. Critic gate

`critic_gate: []`. Numeric parity to 1e-9 is a comparison, not a judgement. The mechanical criterion
replaces the Critic and is deliberately multi-part: module count, skip count, function coverage,
domain coverage, mismatch count, and a live end-to-end demonstration. The last of those is the part
that cannot be satisfied by a unit test — it requires the script surface to actually work in a
running headless engine.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — reflect over the module list, drive each binding from a generated grid
   -> if still failing, MANDATORY approach change. Adding grid points is NOT an approach change;
      moving from hand-written per-function comparisons to a macro that generates both the binding
      and its parity test from one declaration is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with mismatch_count moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: any domain module skipped at runtime (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.18/law_parity.json`

A reader finds: expected versus built module counts with the names of any skipped modules; the number
of functions and domains covered plus an explicit list of untestable functions with reasons; the
tolerance and the input grid; every mismatch with its domain, function, inputs, kernel value, and
binding value; the mismatch count; the count of agreeing non-finite sentinels; and the end-to-end
demonstration result. Alongside it: `custom_law_demo.rune`, the script a third party can copy.

This is the W5 evidence: someone outside the company can declare a first-principles law in a script,
and its numbers are the same as the ones the Rust kernel produces.

## 9. Definition of NOT done

- Parity passes because functions that disagreed were moved to the `untestable` list without a reason
  that survives scrutiny.
- All 13 modules are reported as built because the count comes from the builder table length rather
  than from the successfully constructed modules.
- Non-finite sentinels are skipped rather than compared, so a binding that returns 0.0 where the
  kernel returns infinity passes.
- The demonstration script prints a value but no ECS component changes, so nothing proves the surface
  reaches simulation state.
- The tolerance is relaxed to 1e-6 "because f32". The kernel result is widened to f64 first; the test
  is of the binding, and 1e-9 relative is achievable for a wrapper that does nothing but widen.
- Coverage is 30 functions all drawn from two domains.
````

---

## docs/PROMPTS/items/G5.20_avian-throughput-curve.md

````markdown
---
id: G5.20
title: Avian throughput curve — measured ms/step versus body count
workload: W1
workload_secondary: [W3]
phase: G5
depends_on: [G2.01]
blocks: [G5.21]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G5.20/avian_throughput_curve.json
escalation: >
  If the bench cannot reach 50 000 dynamic bodies without exhausting memory on the harness machine,
  STALL and report the ceiling reached with its memory figure. Do not substitute static bodies to
  inflate the count.
status: DRAFT
notes: >
  Cheap, high-value. Replaces a config default that is currently being read as a measurement in
  docs/AUDIT/05_SPACE_STREAMING.md.
---

## 1. Objective

A measured curve of Avian physics step cost against dynamic body count exists, spanning 1 000 to at
least 50 000 bodies, separating transient and steady-state regimes, and stating the body count at
which the mean step cost crosses 16.7 ms on the harness machine. The number this project quotes for
physics scale becomes a measurement rather than a configuration default.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian** (`avian3d`); never Rapier. Units are meter-native. Licence: PolyForm Shield
1.0.0 (source-available).

**The number that must be replaced.** `docs/AUDIT/05_SPACE_STREAMING.md` line 23 carries a
2.10-million-entity figure. That value is an `active_cap` **configuration default**, not a
measurement — nobody has run it. Any external reader who takes it as a benchmark has been misled, and
this item is what makes the honest number available.

**What already exists — extend it, do not replace it.**
`eustress/benches/instance-capacity/src/bin/avian_physics_bench.rs`. From its own header:

- `STEPS` physics steps per scenario; 600 steps at 60 Hz is 10 simulated seconds
- reports **overall** mean ms/step across all steps, **early** mean (first 100 steps — bodies
  actively falling and colliding), and **steady-state** mean (last 100 steps — bodies at rest or
  asleep), plus `awake_at_end`
- two scenarios, `falling` and `static_heavy`
- driven with a fixed-timestep deterministic driver; `avian3d` feature set `3d, f32, parry-f32,
  parallel`
- run with `cargo run --release --bin avian-physics-bench` from
  `eustress/benches/instance-capacity`

The neighbouring `eustress/benches/instance-capacity/src/main.rs` is the instance-capacity bench
(`cargo run --release --bin instance-capacity`), which doubles N exponentially and reports a named
`StopReason`. Reuse that doubling-with-stop-reason pattern.

**Prerequisite already satisfied.** `G2.01` recorded `avian_falling` and `avian_static_heavy` at a
single body count in `docs/PROMPTS/artifacts/G2.01/physics_baseline.json`, together with the host
block (OS, CPU model, rustc version). That host block defines "the harness machine" and your curve
must carry the same block, because ms/step is meaningless without it.

**Physics pins in force:** `Time::<Fixed>::from_hz(60.0)`, `SubstepCount(6)`,
`SolverConfig::default()`, `Gravity(Vec3::NEG_Y * 9.80665)`.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`. The bench crate is small and builds far faster than the engine.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/benches/instance-capacity/src/bin/avian_physics_bench.rs`
- `eustress/benches/instance-capacity/src/` — new bench binaries permitted
- `eustress/benches/instance-capacity/Cargo.toml`
- `docs/PROMPTS/artifacts/G5.20/`

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G2.18/` — frozen
- `eustress/crates/engine/` and `eustress/crates/common/` — this item measures; it does not optimise
- The solver pins. Measuring under the shipped configuration is the point.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Reducing `SubstepCount` to make the curve flatter, counting sleeping bodies as active, substituting
  static bodies for dynamic ones, or dropping the largest N are all measurement changes. If the
  measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Keep the existing three-figure reporting: overall, early, and steady-state mean ms/step. The early
  window is where a buyer's worst case lives; a curve that reports only the steady-state mean is
  reporting the number after everything fell asleep.
- Report `awake_at_end` at every N. A steady-state figure taken when 95% of bodies are asleep must be
  legible as such.
- Run each N at least 3 times and report the median plus the spread. A single timing on a desktop OS
  is noise.
- Report peak resident memory at each N, so the ceiling in the escalation trigger is grounded.
- Do not thread-pin or otherwise tune the machine. The curve must describe the shipped configuration
  on an ordinary machine.

## 5. Exit criterion

### Criterion
The curve covers at least **6 body counts** from 1 000 to at least **50 000** dynamic bodies, each
with 3 repeats, and reports a `crossing_16_7ms_body_count` derived by interpolation from the measured
`early_ms` series — or explicitly `null` with the maximum measured `early_ms` if the curve never
crosses.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress/benches/instance-capacity
    cargo run --release --bin avian-physics-bench -- \
        --sweep 1000,5000,10000,25000,50000,100000 \
        --repeats 3 --steps 600 \
        --scenarios falling,static_heavy \
        --host-block ../../../docs/PROMPTS/artifacts/G2.01/physics_baseline.json \
        --out ../../../docs/PROMPTS/artifacts/G5.20/avian_throughput_curve.json
    echo "EXIT_SWEEP=$?"

Expected output shape:

    {
      "host": {"os": "windows", "cpu": "<model string>", "rustc": "1.xx.x"},
      "pins": {"hz": 60, "substeps": 6, "gravity_m_s2": 9.80665},
      "avian_features": "3d, f32, parry-f32, parallel",
      "steps_per_run": 600, "repeats": 3,
      "scenarios": {
        "falling": [
          {"n_dynamic": 1000,   "overall_ms": {"median": 0.00, "min": 0.00, "max": 0.00},
                                "early_ms":   {"median": 0.00, "min": 0.00, "max": 0.00},
                                "steady_ms":  {"median": 0.00, "min": 0.00, "max": 0.00},
                                "awake_at_end": 0, "peak_rss_mb": 0.0}
        ],
        "static_heavy": []
      },
      "crossing_16_7ms_body_count": 0,
      "max_n_reached": 0,
      "stop_reason": "completed"
    }

Pass condition:

    EXIT_SWEEP == 0
    AND scenarios.falling has >= 6 entries
    AND ."max_n_reached" >= 50000
    AND every entry has non-null early_ms.median, steady_ms.median, awake_at_end, peak_rss_mb
    AND ."crossing_16_7ms_body_count" is a number OR (it is null AND max early_ms.median < 16.7)
    AND ."host".cpu equals the cpu recorded in G2.01's physics_baseline.json

Read the emitted series. File existence is not a pass.

## 6. Critic gate

`critic_gate: []`. A throughput curve is a measurement, not a judgement. The mechanical criterion
carries the weight and is deliberately multi-part — sweep breadth, repeat count, three timing
regimes, awake counts, memory, and host agreement — because the failure mode here is a number that is
technically true and practically misleading.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — extend the existing bench with a sweep loop and repeat medians
   -> if still failing, MANDATORY approach change. Adding one more N is NOT an approach change;
      moving from one process per sweep to one process per N (to isolate allocator state and memory
      growth) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with max_n_reached unchanged
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: memory exhaustion below 50 000 dynamic bodies (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G5.20/avian_throughput_curve.json`

A reader finds: the host block (matching `G2.01`), the solver pins and the `avian3d` feature set,
steps per run and repeat count, and for each scenario a row per body count giving median, minimum and
maximum overall / early / steady-state ms per step, the awake body count at the end, and peak
resident memory — plus the interpolated 16.7 ms crossing point, the maximum N reached, and a stop
reason.

This file replaces the 2.10-million figure in `docs/AUDIT/05_SPACE_STREAMING.md` as the number this
project quotes for physics scale. Whoever closes the item updates that document to cite this artifact,
with no changelog residue in the document body.

## 9. Definition of NOT done

- The curve reports only `steady_ms`, which at large N is dominated by sleeping bodies and understates
  the cost of an active scene by an order of magnitude.
- 100 000 bodies are reached by spawning static colliders, which do not exercise the solver. The
  sweep axis is `n_dynamic`.
- Each N is run once, so the medians are single samples and the min/max columns are copies of it.
- `crossing_16_7ms_body_count` is extrapolated beyond the measured range rather than interpolated
  within it, and the artifact does not say so.
- The host block differs from `G2.01`'s, so the throughput curve and the physics baseline describe
  two different machines and cannot be read together.
- `awake_at_end` is omitted, making it impossible to tell whether a fast steady-state figure means a
  fast solver or a sleeping scene.
````

---

## docs/PROMPTS/items/G5.21_frame-time-distribution-under-physics-load.md

````markdown
---
id: G5.21
title: Frame-time distribution under physics load
workload: W1
workload_secondary: [W3]
phase: G5
depends_on: [G5.20, G1.08]
blocks: []
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G5.21/frame_time_distribution.json
escalation: >
  If the p99 frame time is dominated by a single system that performs synchronous disk I/O on the
  main thread, STALL and name the system with path:line. Moving persistence off the frame path is a
  substantial change with correctness implications for WorldDb and must be funded as its own item.
status: DRAFT
notes: >
  Tier L: measurement is cheap but every diagnostic cycle costs a full engine build, and the fix -
  if one is needed - lands in the engine crate.
---

## 1. Objective

The engine's frame-time distribution under a physics load equal to half the `G5.20` 16.7 ms crossing
point is measured over 3600 consecutive frames: p50, p95, p99, and maximum are reported, p99 is at
most twice p50, and no frame exceeds 100 ms. Where the bound is missed, the responsible system is
named from profiler evidence rather than guessed at.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian** (`avian3d`); never Rapier. Units are meter-native. **Slint is Rust** — the
studio UI compiles to Rust and is part of the frame. Licence: PolyForm Shield 1.0.0
(source-available).

**What `G5.20` established.** `docs/PROMPTS/artifacts/G5.20/avian_throughput_curve.json` (frozen)
contains the measured ms/step versus dynamic-body-count curve for the `falling` and `static_heavy`
scenarios, in three regimes (overall, early, steady-state), with awake counts and peak memory, plus
`crossing_16_7ms_body_count`. Your test load is **half** that crossing body count, so the physics step
alone consumes roughly half the frame budget and everything else in the frame has to fit in the rest.
Read the number; do not choose your own.

**The instrumentation that already exists — use it, do not build another.**
`eustress/crates/engine/src/profiler.rs` and `eustress/crates/engine/src/frame_diagnostics.rs` are
always compiled. From `profiler.rs`'s own header:

- `EUSTRESS_PROFILE` — set to any non-empty value to arm capture (line 60)
- `EUSTRESS_PROFILE_FRAMES` — window length in frames, default 120 (line 61)
- outputs `eustress_profile.txt`, a ranked table of total ms per system over the window (line 27),
  and `eustress_profile.svg`, an `inferno` flamegraph (line 31), plus
  `eustress_profile_phases.txt` (line 93), all written to the working directory
- with `EUSTRESS_PROFILE` unset the layer is dormant and costs one marker system per frame

A finer per-system trace is available behind the cargo feature `profiling` (which enables
`bevy/trace` and forces a full Bevy rebuild — use a separate `--target-dir` if you reach for it).

**A prior measurement that is directly relevant.**
`docs/development/BENCHMARK_VS_ENGINE_AUDIT.md` records a benchmark at 8 000 entities running at
5 406 FPS against the engine at 10 000 entities running at roughly 45 FPS — a 120x gap — with roughly
10 000 draw calls, and one-second stutters traced to `write_instance_changes_system` performing
20 000 synchronous disk operations. That document names several root causes and marks some fixed.
Read it before you start; it is the most likely explanation for a p99 failure, and it is also the
reason this item's escalation trigger is worded the way it is.

**Related physics-side machinery.** `eustress/crates/engine/src/physics/collider_streaming.rs` (476
lines) governs which colliders are resident. If the load you construct causes collider churn, that
churn is part of the frame cost and must be reported, not eliminated.

**Definitions, fixed.** Frame time is wall-clock time between successive frame starts, sampled by
`frame_diagnostics.rs`. Percentiles are over the full 3600-frame window with no warm-up exclusion
beyond the first 120 frames, which are discarded to let load and asset residency settle. Report the
discard count explicitly.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/src/bin/` — a `frame-distribution` driver binary is permitted
- `eustress/crates/engine/src/physics/` — only if a specific hot system is identified and the fix is
  local and named in the result block
- `docs/PROMPTS/artifacts/G5.21/`

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G5.20/` — frozen
- `eustress/crates/engine/src/ui/slint_ui.rs` — at 23 103 lines it is the largest file in the
  repository and a change there is its own item with its own review
- `eustress/crates/engine/src/space/world_db_plugin.rs` and the WorldDb write path — if this is the
  bottleneck, that is the front-matter escalation, not a licence to change persistence
- The solver pins
- `eustress/crates/engine/src/frame_diagnostics.rs` — owned by `G1.08` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- `eustress/crates/engine/src/profiler.rs` — owned by `G1.08` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Reducing the body count below half the `G5.20` crossing point, discarding more than the first 120
  frames, excluding "outlier" frames, capping the frame rate so the distribution narrows, or
  measuring only the physics step instead of the whole frame are all measurement changes. If the
  measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The run must be windowed and headless-capable where possible, but a frame-time distribution that
  excludes rendering does not describe what a user experiences. State clearly in the artifact which
  mode was measured and, if headless, that the figure is a lower bound.
- Report the top 10 systems by total time from `eustress_profile.txt` alongside the distribution.
  A p99 number with no attribution cannot be acted on and will not satisfy this item.
- Do not "fix" the distribution by disabling a subsystem. If disabling one is what reveals the cause,
  report the delta as a diagnostic and put the subsystem back.
- Run three separate 3600-frame sessions and report all three distributions. A single session on a
  desktop OS will contain background-process noise, and three sessions make it distinguishable from a
  real engine stall.

## 5. Exit criterion

### Criterion
Over 3600 frames (after discarding the first 120) at a dynamic-body count equal to half the `G5.20`
`crossing_16_7ms_body_count`: **p99 <= 2.0 x p50**, **max frame time <= 100 ms**, and **zero** frames
above 250 ms, in each of three independent sessions.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin frame-distribution
    EUSTRESS_PROFILE=1 EUSTRESS_PROFILE_FRAMES=3600 \
    ./target/release/frame-distribution \
        --load-from ../docs/PROMPTS/artifacts/G5.20/avian_throughput_curve.json \
        --load-fraction 0.5 \
        --frames 3600 --discard-first 120 --sessions 3 \
        --profile-out ../docs/PROMPTS/artifacts/G5.21/ \
        --out ../docs/PROMPTS/artifacts/G5.21/frame_time_distribution.json
    echo "EXIT_FRAME=$?"

Expected output shape:

    {
      "mode": "windowed",
      "n_dynamic": 0, "derived_from_crossing_body_count": 0, "load_fraction": 0.5,
      "frames_per_session": 3600, "discarded_first": 120, "sessions": 3,
      "distributions": [
        {"session": 1, "p50_ms": 0.00, "p95_ms": 0.00, "p99_ms": 0.00, "max_ms": 0.00,
         "frames_over_100ms": 0, "frames_over_250ms": 0, "p99_over_p50": 0.00},
        {"session": 2, "…": null},
        {"session": 3, "…": null}
      ],
      "top_systems_by_total_ms": [
        {"rank": 1, "system": "<name>", "total_ms": 0.0, "share": 0.00}
      ],
      "profiler_artifacts": ["eustress_profile.txt", "eustress_profile.svg", "eustress_profile_phases.txt"]
    }

Pass condition:

    EXIT_FRAME == 0
    AND every distribution.p99_over_p50 <= 2.0
    AND every distribution.max_ms <= 100.0
    AND every distribution.frames_over_250ms == 0
    AND ."sessions" == 3 AND ."frames_per_session" == 3600
    AND ."top_systems_by_total_ms" has >= 10 entries

Read the emitted percentiles. File existence is not a pass.

## 6. Critic gate

`critic_gate: []`. Frame-time distribution is entirely numeric, and the visual half of temporal
stability — whether the image holds together while the camera moves — belongs to the render pack, not
here. The mechanical criterion encodes the numeric floors directly and is multi-part (ratio, maximum,
tail count, session count, attribution depth) so it cannot be satisfied by one lucky run.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — measure, attribute via eustress_profile.txt, fix the top local offender
   -> if still failing, MANDATORY approach change. Re-running for a better sample is NOT an approach
      change; moving from the ranked-table profiler to the `profiling` cargo feature with bevy/trace
      (separate --target-dir) to get per-system spans is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with p99_over_p50 moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: p99 dominated by a system doing synchronous disk I/O on the main thread
                   (see front matter) — name it with path:line and stop
```

## 8. Artifact

`docs/PROMPTS/artifacts/G5.21/frame_time_distribution.json`

A reader finds: the mode measured (windowed or headless), the dynamic body count and the `G5.20`
crossing point it was derived from, frames per session and the discard count, three independent
distributions each with p50 / p95 / p99 / max, tail counts above 100 ms and 250 ms, and the p99-to-p50
ratio; the top ten systems by total time with their share of the window; and the profiler artifact
filenames. The `eustress_profile.txt` and `eustress_profile.svg` files are archived alongside.

## 9. Definition of NOT done

- The ratio clears 2.0 because the load was set well below half the `G5.20` crossing point. The load
  is derived from the frozen artifact for exactly this reason.
- The distribution is measured headless and reported without saying so, so the number excludes
  rendering and is quoted as if it described the studio.
- One session passes and two fail, and only the passing session is reported. All three bind.
- Warm-up discard is raised from 120 to 1200 frames to remove a load-time stall that a user would
  actually experience.
- `top_systems_by_total_ms` is populated by hand from an assumption rather than parsed from
  `eustress_profile.txt`, so the attribution is unverifiable.
- A subsystem is disabled to pass and is not re-enabled, so the measured configuration is not one that
  ships.
````

---

## docs/PROMPTS/items/G5.22_multi-variant-experiment-throughput.md

````markdown
---
id: G5.22
title: Multi-variant experiment throughput with a noise floor that makes ranking meaningful
workload: W6
workload_secondary: [W1]
phase: G5
depends_on: [G2.03, G2.08, G7.08, G1.12]
blocks: []
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G2_sim_evidence.json
artifact: docs/PROMPTS/artifacts/G5.22/variant_sweep.json
escalation: >
  If the measured run-to-run noise floor for the ranking metric is not zero despite G2.03 having
  proved byte-identical reruns, STALL. A non-zero noise floor in a deterministic system means the
  experiment harness is injecting variability - wall-clock seeding, timestamps, or thread ordering -
  and ranking results on top of that is worthless.
status: DRAFT
notes: >
  W6 primary: the operator's throughput per wall-clock hour is the thing being measured. It depends
  on G2.03 because a deterministic substrate is what makes a small measured delta attributable to the
  intervention rather than to noise.
---

## 1. Objective

Eight parameter variants of the validated cell model run in one batch, each producing a saved
experiment result, and `compare_runs` ranks them on a declared metric. The run-to-run noise floor is
measured (by running one variant twice) and the winning variant's margin over the runner-up exceeds
that noise floor by at least a factor of three. Total wall time for the batch is recorded, so the
operator's throughput becomes a number.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian**; never Rapier. Units are meter-native. Licence: PolyForm Shield 1.0.0
(source-available).

**The tools that exist — use them; do not build a parallel harness.**

- `run_experiment` — `eustress/crates/tools/src/simulation_tools.rs` line 1567. Its declared input
  schema: required `name` (string) and `duration_s` (number); optional `description` (string),
  `sim_values` (object of string to number), `time_scale` (number, default 1.0), `create_branch`
  (boolean, default false, creates `exp/<name>-<timestamp>`), `timeout_s` (number, default 300). It
  applies the sim-value overrides, runs for `duration_s` of simulated time at `time_scale`, polls to
  completion, computes stats, and saves a structured result under
  `<universe_root>/.eustress/experiments/`. `requires_approval: true`; streams to
  `workshop.simulation.experiment`.
- `compare_runs` — same file, line 1760. Required `run_a`, `run_b` (experiment file name, or the
  shortcuts `latest` / `latest-1`); optional `higher_is_better` (array of metric keys). Loads both
  JSON files from `<universe_root>/.eustress/experiments/` and reports per-metric deltas.
- `list_experiments` — same file, line 1889.
- `set_sim_value` — same file, line 115. Queues the write to
  `<universe>/.eustress/sim-commands.jsonl`; the engine drains it on the next sim tick.
- `feedback_diff` — `eustress/crates/tools/src/git_tools.rs` line 467. **This is a git diff, not a
  telemetry diff.** Do not use it to compare experiment outputs; `compare_runs` is the tool for that.

**What is missing.** `compare_runs` compares exactly two runs. There is no batch mode, no ranking
across N variants, and no notion of a noise floor — so today an agent can produce a ranking that is
indistinguishable from a coin flip and nothing in the system says so.

**Why the noise floor is the crux.** `G2.03` proved that the headless engine stack reruns
byte-identically over 3600 ticks (`docs/PROMPTS/artifacts/G2.03/headless_determinism.json`, frozen).
If that holds, the same variant run twice must produce an identical metric and the noise floor is
exactly zero — which makes any non-zero margin significant. If the noise floor is *not* zero, the
experiment harness is adding variability that `G2.03` excluded, and the front-matter escalation
fires. Measuring it is therefore both a sanity check on this item and a regression check on `G2.03`.

**The variants.** Eight parameter sets of the `G2.08`-validated cell model, varying at least two
independent parameters over at least three levels each (so the sweep is not one-dimensional). The
ranking metric must be a physically meaningful scalar the model already produces — for example
delivered energy in watt-hours over a fixed discharge, or round-trip efficiency. Declare it and its
direction in the artifact. `docs/PROMPTS/artifacts/G2.08/cell_validation_0d.json` (frozen) records
the validated parameter set and its error bound; every variant must stay inside the range over which
the model was validated, and the artifact must state that range.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/mcp-server/src/` — registration of a new tool, if one is added
- `eustress/crates/engine/src/bin/` — a `variant-sweep` driver binary is permitted
- `docs/PROMPTS/harness/recipes/G2_sim_evidence.json`
- `docs/PROMPTS/artifacts/G5.22/`

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G5.21/` — frozen
- The cell physics. This item ranks variants of a validated model; changing the model invalidates
  `G2.08`.
- `eustress/crates/engine/src/ui/` — no panel work
- `eustress/crates/tools/src/git_tools.rs`
- `eustress/crates/tools/src/simulation_tools.rs` — owned by `G7.08` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Choosing variants whose spread is artificially large, ranking on a metric with no physical meaning,
  measuring the noise floor on a different scenario than the variants, or reporting the batch wall
  time excluding setup are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The noise floor is measured by running **one** of the eight variants **twice**, unchanged, and
  taking the absolute difference in the ranking metric. Report which variant and both values.
- Every variant must stay inside the parameter range over which `G2.08` validated the model. A
  variant outside that range produces a number the model has no licence to predict; if you include
  one for contrast, flag it `extrapolated: true` and exclude it from the ranking.
- Batch wall time is measured end to end, including scaffolding and teardown, because that is what
  the operator actually spends.
- Do not use `create_branch: true` for the sweep. Eight experiment branches are a git-hygiene problem
  and none of the results depend on them.
- The ranking must be reproducible: running the batch twice must produce the same ordering. Report
  whether it did.

## 5. Exit criterion

### Criterion
Eight variants complete and are ranked on a declared metric; the measured noise floor is **0.0**; the
winner's margin over the runner-up is at least **3x** the noise floor **or**, when the noise floor is
exactly zero, is strictly greater than zero and at least **1%** of the winner's metric value; the
ranking is identical across two batch runs; and batch wall time is recorded.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin variant-sweep
    ./target/release/variant-sweep \
        --recipe ../docs/PROMPTS/harness/recipes/G2_sim_evidence.json \
        --variants ../docs/PROMPTS/artifacts/G5.22/variants.json \
        --metric delivered_energy_wh --higher-is-better \
        --validated-range-from ../docs/PROMPTS/artifacts/G2.08/cell_validation_0d.json \
        --noise-floor-variant v03 --repeat-batches 2 \
        --out ../docs/PROMPTS/artifacts/G5.22/variant_sweep.json
    echo "EXIT_SWEEP=$?"

Expected output shape:

    {
      "metric": "delivered_energy_wh", "direction": "higher_is_better",
      "variant_count": 8, "parameters_varied": ["…", "…"], "levels_per_parameter": 3,
      "batch_wall_s": 0.0, "experiments_dir": "<universe>/.eustress/experiments",
      "noise_floor": {"variant": "v03", "run_1": 0.000000, "run_2": 0.000000, "abs_delta": 0.000000},
      "ranking": [
        {"rank": 1, "variant": "v05", "metric": 0.000000, "extrapolated": false, "experiment_file": "…json"},
        {"rank": 2, "variant": "v02", "metric": 0.000000, "extrapolated": false, "experiment_file": "…json"}
      ],
      "winner_margin": 0.000000,
      "margin_over_noise_floor": null,
      "margin_as_fraction_of_winner": 0.0000,
      "ranking_stable_across_batches": true
    }

Pass condition:

    EXIT_SWEEP == 0
    AND ."variant_count" == 8
    AND ."noise_floor".abs_delta == 0.0
    AND ."winner_margin" > 0.0
    AND ."margin_as_fraction_of_winner" >= 0.01
    AND ."ranking_stable_across_batches" == true
    AND ."batch_wall_s" > 0.0
    AND no ranked entry has extrapolated == true

Read the noise floor and the margin. File existence is not a pass.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**.

D5 will ask whether the ranking means anything. Prioritise: a stated noise floor measured the same way
as the variants; a winner whose advantage is explicable from the physics (a parameter change that
should raise delivered energy, raising it) rather than merely numerically largest; and honest flagging
of any variant that sits outside the range over which the model was validated. A ranking presented
without a noise floor is the exact failure this dimension exists to catch. Capture recipe:
`docs/PROMPTS/harness/recipes/G2_sim_evidence.json`.

The Critic never sees your self-report and must cite a specific value.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — batch driver invoking the existing run_experiment path per variant
   -> if still failing, MANDATORY approach change. Adding variants is NOT an approach change; moving
      from sequential per-variant engine sessions to one session that resets state between variants
      (or vice versa, to isolate cross-variant contamination) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with ranking_stable_across_batches still false
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: non-zero noise floor despite G2.03 (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G5.22/variant_sweep.json`

A reader finds: the ranking metric and its direction; the variant count, which parameters were varied
and over how many levels; total batch wall time and the experiments directory; the noise-floor
measurement with the repeated variant and both values; the full ranking with each variant's metric,
extrapolation flag, and saved experiment filename; the winner's margin in absolute and fractional
terms; and whether the ordering was stable across two batches. Alongside it: `variants.json`, the
input parameter sets.

This is the W6 evidence: the operator can state how many parameter variants they can evaluate per
wall-clock hour, and how large a difference the system can actually resolve.

## 9. Definition of NOT done

- The winner is declared with no noise floor measured, so the margin cannot be interpreted.
- The noise floor is measured on a different, shorter scenario than the variants, so it understates
  the variability of the actual runs.
- Eight variants vary a single parameter over eight levels, so the sweep says nothing about
  interaction and the "multi-variant" claim is one-dimensional.
- The winning variant sits outside `G2.08`'s validated parameter range and is ranked anyway.
- `batch_wall_s` measures only the summed `duration_s` of the runs rather than actual wall clock,
  so the operator-throughput number is fiction.
- `compare_runs` is bypassed in favour of an ad-hoc comparison, so the shipped tool is still
  unproven and the item improves nothing a user can reach.
````

---

## docs/PROMPTS/items/G5.23_physics-regression-gate-binary.md

````markdown
---
id: G5.23
title: Physics regression gate binary that can actually fail
workload: W3
workload_secondary: [W6]
phase: G5
depends_on: [G2.02, G2.07, G2.12, G2.17, G1.03]
blocks: []
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G5.23/physics_gate.json
escalation: >
  If any constituent gate cannot be run without a windowed session or a GPU, STALL and name it. The
  point of this binary is that it runs where nobody is watching; a gate that needs a desktop is not
  a gate.
status: DRAFT
notes: >
  Deliberately does NOT touch .github/workflows - CI is a standing out-of-scope entry for every item
  in this library. This produces the single command a later, human-approved CI change would invoke.
---

## 1. Objective

One command runs every numeric gate produced by pack T2, compares each measured value against the
threshold recorded in its frozen artifact, prints a pass/fail table, and exits 0 only if all pass. A
deliberately injected regression makes it exit non-zero, proving the gate can fail. The command needs
no window, no GPU, and no human.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — never a game engine.
Physics is **Avian** (`avian3d`); never Rapier. Units are meter-native. Licence: PolyForm Shield
1.0.0 (source-available).

**Why this exists.** `.github/workflows/ci.yml` has three jobs: `cargo deny --config ../deny.toml
check advisories bans sources`; naga WGSL validation (which skips any file containing `naga_oil`
directives, meaning every real Bevy shader); and a `cargo tree -p eustress-engine -e normal,build`
grep asserting `eustress-data` is present. `.github/workflows/linux-engine.yml` runs a single
`cargo check --package eustress-engine`. `.github/workflows/release.yml` runs
`cargo build --release --package eustress-engine` per platform. **There is no `cargo test`, no
clippy, and no workspace build anywhere in CI**, while approximately 2 061 `#[test]` functions exist
across `eustress/crates/`. An enterprise buyer reads those files.

You are **not** permitted to change CI — that is a standing out-of-scope entry for every item in this
library, and wiring a gate into CI is a human decision. What you are producing is the single command
such a change would invoke, proven to work and proven to fail.

**The gates you are aggregating, all with frozen artifacts.**

| Gate | Source item | Command shape | Threshold source |
|---|---|---|---|
| Avian determinism | `G2.02` | `cargo test -p eustress-common --features physics --test determinism -- --test-threads=1` | `docs/PROMPTS/artifacts/G2.02/determinism_gate.json` `golden_hash` |
| Conservation invariants | `G2.07` | `conservation-suite --ticks 3600 --sample-every 60` | `docs/PROMPTS/artifacts/G2.07/conservation.json` per-invariant `tolerance` |
| P2D convergence | `G2.12` | `cell-validate --convergence-study …` | `docs/PROMPTS/artifacts/G2.12/p2d_convergence.json` |
| Transient conduction | `G2.17` | `conduction-validate --nodes 50 …` | `docs/PROMPTS/artifacts/G2.17/conduction_validation.json` `overall_max_rel_error` |

Read each artifact for its exact field names before wiring it; the shapes above are summaries, and
each item's own prompt is the authority on its output.

**Note on `physics` not being a default feature.** `eustress/crates/common/Cargo.toml` line 123 has
`default = ["model-import", "geotiff", "streaming", "units_v1"]` and line 125 `physics =
["avian3d"]`. The determinism test is `#![cfg(feature = "physics")]`, so the `--features physics`
flag in the command above is load-bearing. A gate that drops it runs an empty test and exits 0.

**Headless constraint.** `eustress/crates/engine/src/bin/headless.rs` is `MinimalPlugins` +
`ScheduleRunnerPlugin` — no winit, no GPU. Its documented limits: no gltf loader is registered, and
`ai_camera` / `viewport.capture` need the unbuilt `--render gpu` tier
(`docs/architecture/HEADLESS_RUNTIME.md` line 269, P6, still `new`). Every gate in this binary must
run inside those limits.

**Build reality.** 10-15 minute builds, one at a time, never killed mid-compile. Validate with
`cargo run`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/src/bin/` — a `physics-gate` binary
- `eustress/crates/engine/Cargo.toml` — `[[bin]]` entry only
- `docs/PROMPTS/artifacts/G5.23/`
- A `docs/PROMPTS/harness/gates/physics_gate_thresholds.json` file collecting the thresholds read
  from the frozen artifacts, so the gate has one place to look

### Out of scope — do not edit
- **Anything under `.github/workflows/`** — never modify CI to make a gate pass, and never wire this
  binary into CI as part of this item
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G2.01/` through `/G5.22/` — **frozen**; the gate reads them and must never
  write them
- The constituent gates' own implementations. If one of them fails, that is a regression to report,
  not a gate to loosen.
- `eustress/crates/engine/src/ui/`
- Any existing entry in `eustress/crates/engine/Cargo.toml` — the file is owned by `G1.03`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Copying a threshold with a wider value than its frozen artifact records, skipping a gate that is
  slow, marking a gate `advisory` so its failure does not affect the exit code, or catching a panic
  and reporting a pass are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Every threshold in `physics_gate_thresholds.json` must be traceable: each entry records the source
  artifact path and the field it came from. A threshold with no provenance is a number someone made
  up.
- The binary must run every gate even after one fails, so a single run reports the full picture, and
  must exit non-zero if any failed.
- Total runtime must be under 15 minutes on the `G2.01` host, excluding compilation. Report the
  measured runtime. If a gate is too slow, report it rather than shortening it — the correct fix is a
  faster gate, not a smaller one.
- Output must be machine-readable JSON **and** a human-readable table on stdout. A wall of JSON is
  not a gate report someone will read at 2 a.m.
- No network access. Every gate runs from what is in the repository.

## 5. Exit criterion

### Criterion
`physics-gate` runs all **four** constituent gates, exits **0** on an unmodified tree, and exits
**non-zero** when invoked with `--inject-regression <gate>` for each of the four gates in turn — four
separate non-zero exits, one per gate.

### Measurement

Command:

    cd E:/Workspace/EustressEngine/eustress
    cargo build --release --bin physics-gate
    ./target/release/physics-gate \
        --thresholds ../docs/PROMPTS/harness/gates/physics_gate_thresholds.json \
        --out ../docs/PROMPTS/artifacts/G5.23/physics_gate.json
    echo "EXIT_CLEAN=$?"
    for g in determinism conservation p2d_convergence conduction; do
      ./target/release/physics-gate \
        --thresholds ../docs/PROMPTS/harness/gates/physics_gate_thresholds.json \
        --inject-regression "$g" \
        --out "../docs/PROMPTS/artifacts/G5.23/physics_gate_injected_$g.json"
      echo "EXIT_INJECT_$g=$?"
    done

Expected output shape (`physics_gate.json`):

    {
      "host": {"os": "windows", "cpu": "<model string>", "rustc": "1.xx.x"},
      "commit": "…", "tree_dirty": false,
      "total_runtime_s": 0.0,
      "gates": [
        {"name": "determinism",     "measured": "0x…",  "threshold": "0x…",  "comparator": "==",  "pass": true, "runtime_s": 0.0, "threshold_source": "docs/PROMPTS/artifacts/G2.02/determinism_gate.json#golden_hash"},
        {"name": "conservation",    "measured": 0.0000, "threshold": 0.0050, "comparator": "<=",  "pass": true, "runtime_s": 0.0, "threshold_source": "docs/PROMPTS/artifacts/G2.07/conservation.json#invariants.energy_pendulum.tolerance"},
        {"name": "p2d_convergence", "measured": 0.0000, "threshold": 0.0020, "comparator": "<=",  "pass": true, "runtime_s": 0.0, "threshold_source": "docs/PROMPTS/artifacts/G2.12/p2d_convergence.json#mesh_diff_2N_4N_v"},
        {"name": "conduction",      "measured": 0.0000, "threshold": 0.0200, "comparator": "<=",  "pass": true, "runtime_s": 0.0, "threshold_source": "docs/PROMPTS/artifacts/G2.17/conduction_validation.json#overall_max_rel_error"}
      ],
      "all_pass": true,
      "gates_run": 4, "gates_skipped": 0,
      "requires_gpu": false, "requires_window": false, "requires_network": false
    }

Pass condition:

    EXIT_CLEAN == 0
    AND EXIT_INJECT_determinism != 0
    AND EXIT_INJECT_conservation != 0
    AND EXIT_INJECT_p2d_convergence != 0
    AND EXIT_INJECT_conduction != 0
    AND physics_gate.json ."gates_run" == 4 AND ."gates_skipped" == 0
    AND ."all_pass" == true
    AND ."total_runtime_s" <= 900
    AND ."requires_gpu" == false AND ."requires_window" == false AND ."requires_network" == false
    AND every gate has a non-empty threshold_source

Grep every one of the five exit-code markers. A gate binary that exits 0 unconditionally satisfies
`EXIT_CLEAN` alone, which is precisely why the four injected-regression exits are part of the pass
condition.

## 6. Critic gate

`critic_gate: []`. This is a mechanical item: five exit codes and a JSON report. The replacement for
Critic judgement is the injection matrix — four separate deliberate regressions, each of which must
produce a non-zero exit. That is a far harder thing to fake than a blinded score.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — subprocess each constituent gate, parse its JSON, compare to threshold
   -> if still failing, MANDATORY approach change. Adjusting a parse is NOT an approach change;
      moving from subprocess-and-parse to linking the gate logic directly into one binary (removing
      the JSON round trip) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same injected regression failing to produce a
                  non-zero exit
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: any gate requires a window or a GPU (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G5.23/physics_gate.json`

A reader finds: the host block, commit and dirty flag, total runtime, and one row per gate giving its
name, measured value, threshold, comparator, pass flag, runtime, and the artifact path plus field the
threshold came from — followed by an overall pass flag, gates-run and gates-skipped counts, and three
booleans confirming the gate needs no GPU, no window, and no network. Alongside it: the four
injected-regression reports, and `docs/PROMPTS/harness/gates/physics_gate_thresholds.json`.

This file is the answer to "how would we know if physics regressed?" and it is the one artifact in
pack T2 an outside reviewer will ask for first.

## 9. Definition of NOT done

- The binary exits 0 cleanly but also exits 0 under every injected regression, so it has never failed
  and never will.
- One gate is marked `advisory` so its failure does not affect the exit code, and the report still
  says `all_pass: true`.
- Thresholds are hard-coded in the binary with no `threshold_source`, so nobody can tell whether they
  match the frozen artifacts or were loosened.
- The determinism gate is invoked without `--features physics`, compiling the test to nothing and
  passing vacuously.
- A gate is skipped because it is slow and `gates_skipped` is reported as 0.
- The item wires the binary into `.github/workflows/ci.yml`. CI is out of scope for every item in
  this library, and that change is a human decision made with the runtime figure this artifact
  provides.
````

---

## Pack close-out

**What this pack does not cover, deliberately.**

- **CI wiring.** Every item here treats `.github/workflows/` as out of scope. `G5.23` produces the
  command a CI change would invoke and measures its runtime; the decision to run it in CI is a human
  one.
- **Rendering, capture bundles, and any perceptual dimension other than D5.** Frame *time* is here
  (`G5.21`); frame *appearance* is not.
- **Studio UI.** `eustress/crates/engine/src/ui/slint_ui.rs` is out of scope in every item.
- **Anything requiring a GPU or a desktop session.** `eustress/crates/engine/src/bin/headless.rs` is
  `MinimalPlugins` + `ScheduleRunnerPlugin`, and `--render gpu` (`docs/architecture/HEADLESS_RUNTIME.md`
  line 269, P6) is unstarted. Every gate in this pack runs without one.

**Documents that must be updated when items close**, by whoever closes them, written so they read as
though always correct — no changelog residue in the document body:

| Item | Document | What changes |
|---|---|---|
| `G2.03` | `docs/architecture/HEADLESS_RUNTIME.md` line 294 | the determinism gate becomes a passed gate citing `docs/PROMPTS/artifacts/G2.03/headless_determinism.json` |
| `G2.06` | `docs/AUDIT/11_SIMULATION_DEBUGGER.md` Feature 3 R3.1 | wall-time cooldown finding resolved, citing the measured alert counts |
| `G2.10` | `docs/AUDIT/19_REALISM_PHYSICS.md` P4 | "lumped 0-D, no spatial electrochemistry" replaced by the measured RMS bound |
| `G2.11` | `docs/AUDIT/19_REALISM_PHYSICS.md` P4 | the decoupling finding replaced by the measured capacity-sweep and temperature-rise bounds |
| `G2.14` | `docs/AUDIT/19_REALISM_PHYSICS.md` Feature 8 and Q19.6 | fracture-to-Avian integration path exists, with its conservation bounds |
| `G5.20` | `docs/AUDIT/05_SPACE_STREAMING.md` line 23 | the 2.10M config default replaced by the measured throughput curve |

**A note on the V-Cell ladder for whoever sequences the work.** `G2.08` through `G2.13` are six items
and roughly half this pack's total budget. They are sequenced deliberately: each rung's error bound
is tighter than the last and is only defensible because the previous rung's number was measured on
the same rig against the same archived reference. Running them out of order, or skipping `G2.12`
because `G2.10` "already fits the data", produces a model that matches a curve without solving the
equations — which is the failure mode the whole ladder exists to prevent.
